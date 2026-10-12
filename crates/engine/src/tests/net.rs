use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use super::element::{engine, run};
use crate::net::forbidden_request_header;

#[test]
fn forbidden_request_headers_match_the_fetch_spec_list_plus_user_agent() {
    let forbidden = "Origin User-Agent Referer Cookie Host accept-charset Accept-Encoding \
        Access-Control-Request-Headers Access-Control-Request-Method Access-Control-Request-Private-Network \
        Connection Content-Length Cookie2 Date DNT Expect Keep-Alive Set-Cookie TE Trailer \
        Transfer-Encoding Upgrade Via Proxy-Authorization Sec-Fetch-Mode";
    for name in forbidden.split_whitespace().chain([" origin "]) {
        assert!(forbidden_request_header(name, "x"), "{name}");
    }
    assert!(forbidden_request_header(
        "X-HTTP-Method-Override",
        "get, trace"
    ));
    assert!(forbidden_request_header("X-Method-Override", "CONNECT"));
    for (name, value) in [
        ("Authorization", "Bearer x"),
        ("Content-Type", "application/json"),
        ("Music-User-Token", "x"),
        ("X-HTTP-Method-Override", "PATCH"),
        ("Accept", "audio/mp4"),
    ] {
        assert!(!forbidden_request_header(name, value), "{name}");
    }
}

#[test]
fn fetch_and_xhr_drop_forbidden_headers_before_they_leave_the_process() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..2 {
            let (mut sock, _) = listener.accept().unwrap();
            let mut head = String::new();
            let mut r = BufReader::new(sock.try_clone().unwrap());
            while r.read_line(&mut head).unwrap() > 2 && !head.ends_with("\r\n\r\n") {}
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            seen.push(head.to_ascii_lowercase());
        }
        seen
    });
    let (mut e, sink) = engine();
    let headers = r#"{ Origin: "https:\/\/music.apple.com", "User-Agent": "Mozilla/5.0 Chrome/141", Referer: "https:\/\/music.apple.com/", Cookie: "itre=1", "Sec-Fetch-Site": "same-site", "X-Kept": "1" }"#;
    let log = run(
        &mut e,
        &sink,
        &format!(
            r#"
            const url = "http:\/\/127.0.0.1:{port}/";
            L("fetch:" + (await fetch(url, {{ headers: {headers} }})).status);
            const xhr = new XMLHttpRequest();
            xhr.open("GET", url);
            for (const [k, v] of Object.entries({headers})) xhr.setRequestHeader(k, v);
            await new Promise(r => {{ xhr.onloadend = r; xhr.send(); }});
            L("xhr:" + xhr.status);
        "#
        ),
    );
    assert_eq!(log, ["fetch:200", "xhr:200"]);
    for request in server.join().unwrap() {
        assert!(request.contains("x-kept: 1"), "{request}");
        for leaked in [
            "origin:",
            "referer:",
            "cookie:",
            "sec-fetch-site",
            "music.apple.com",
            "mozilla",
            "chrome",
        ] {
            assert!(
                !request.contains(leaked),
                "{leaked} reached the wire:\n{request}"
            );
        }
    }
}

#[test]
fn sequential_requests_to_one_host_reuse_one_connection() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            count.fetch_add(1, Ordering::SeqCst);
            let mut sock = sock.unwrap();
            std::thread::spawn(move || {
                let mut r = BufReader::new(sock.try_clone().unwrap());
                loop {
                    let mut head = String::new();
                    while r.read_line(&mut head).unwrap_or(0) > 2 && !head.ends_with("\r\n\r\n") {}
                    if head.is_empty()
                        || sock
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                            .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &format!(
            "for (let i = 0; i < 3; i++) L(await (await fetch('http://127.0.0.1:{port}/' + i)).text());"
        ),
    );
    assert_eq!(log, ["ok", "ok", "ok"]);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

fn get(url: &str, headers: &[(&str, &str)]) -> crate::net::HttpRequest {
    crate::net::HttpRequest {
        id: 7,
        method: "GET".into(),
        url: url.into(),
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: None,
        timeout_ms: 1000,
        use_cache: true,
    }
}

fn reply(headers: &[(&str, &str)]) -> crate::net::HttpResponse {
    crate::net::HttpResponse {
        id: 0,
        status: 200,
        status_text: "OK".into(),
        url: "u".into(),
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: b"#EXTM3U".to_vec(),
        error: None,
    }
}

fn network_calls(
    cache: &crate::http_cache::HttpCache,
    req: &crate::net::HttpRequest,
    resp: &crate::net::HttpResponse,
    times: usize,
) -> usize {
    let calls = std::cell::Cell::new(0);
    for _ in 0..times {
        let out = cache.fetch(req, |r| {
            calls.set(calls.get() + 1);
            crate::net::HttpResponse {
                id: r.id,
                ..resp.clone()
            }
        });
        assert_eq!((out.id, out.body.as_slice()), (req.id, &b"#EXTM3U"[..]));
    }
    calls.get()
}

#[test]
fn a_fresh_get_is_served_from_the_http_cache_and_everything_else_goes_to_the_network() {
    use crate::http_cache::HttpCache;
    let fresh = [("Cache-Control", "public, max-age=600")];
    type Case<'a> = (
        &'a str,
        crate::net::HttpRequest,
        &'a [(&'a str, &'a str)],
        usize,
    );
    let cases: &[Case] = &[
        ("fresh max-age", get("http://h/a", &[]), &fresh, 1),
        (
            "no-store",
            get("http://h/a", &[]),
            &[("Cache-Control", "no-store, max-age=600")],
            3,
        ),
        (
            "no-cache",
            get("http://h/a", &[]),
            &[("Cache-Control", "no-cache, max-age=600")],
            3,
        ),
        (
            "no max-age",
            get("http://h/a", &[]),
            &[("Cache-Control", "public")],
            3,
        ),
        (
            "Age uses up the lifetime",
            get("http://h/a", &[]),
            &[("Cache-Control", "max-age=600"), ("Age", "600")],
            3,
        ),
        (
            "Vary: *",
            get("http://h/a", &[]),
            &[("Cache-Control", "max-age=600"), ("Vary", "*")],
            3,
        ),
        (
            "range request",
            get("http://h/a", &[("Range", "bytes=0-9")]),
            &fresh,
            3,
        ),
        (
            "authorization without public",
            get("http://h/a", &[("Authorization", "Bearer x")]),
            &[("Cache-Control", "max-age=600")],
            3,
        ),
        (
            "authorization with public",
            get("http://h/a", &[("Authorization", "Bearer x")]),
            &fresh,
            1,
        ),
        (
            "a second Cache-Control line says no-store",
            get("http://h/a", &[]),
            &[
                ("Cache-Control", "max-age=600"),
                ("Cache-Control", "no-store"),
            ],
            3,
        ),
        (
            "request no-cache",
            get("http://h/a", &[("Cache-Control", "no-cache")]),
            &fresh,
            3,
        ),
        (
            "request no-store",
            get("http://h/a", &[("Cache-Control", "no-store")]),
            &fresh,
            3,
        ),
        (
            "request max-age=0",
            get("http://h/a", &[("Cache-Control", "max-age=0")]),
            &fresh,
            3,
        ),
        (
            "request Pragma: no-cache",
            get("http://h/a", &[("Pragma", "no-cache")]),
            &fresh,
            3,
        ),
        (
            "conditional request",
            get("http://h/a", &[("If-None-Match", "\"x\"")]),
            &fresh,
            3,
        ),
        (
            "music user token without public",
            get("http://h/a", &[("Music-User-Token", "t")]),
            &[("Cache-Control", "max-age=600")],
            3,
        ),
        (
            "x-apple music user token without public",
            get("http://h/a", &[("X-Apple-Music-User-Token", "t")]),
            &[("Cache-Control", "max-age=600")],
            3,
        ),
        (
            "media user token without public",
            get("http://h/a", &[("Media-User-Token", "t")]),
            &[("Cache-Control", "max-age=600")],
            3,
        ),
    ];
    for (name, req, headers, expected) in cases {
        assert_eq!(
            network_calls(&HttpCache::default(), req, &reply(headers), 3),
            *expected,
            "{name}"
        );
    }
    let mut no_store_mode = get("http://h/a", &[]);
    no_store_mode.use_cache = false;
    assert_eq!(
        network_calls(&HttpCache::default(), &no_store_mode, &reply(&fresh), 3),
        3,
        "fetch cache mode other than default"
    );
    let mut not_200 = reply(&fresh);
    not_200.status = 206;
    let calls = std::cell::Cell::new(0);
    let cache = HttpCache::default();
    for _ in 0..2 {
        cache.fetch(&get("http://h/a", &[]), |_| {
            calls.set(calls.get() + 1);
            not_200.clone()
        });
    }
    assert_eq!(calls.get(), 2, "only 200 responses are stored");
}

#[test]
fn a_vary_header_keys_the_entry_on_the_request_header_it_names() {
    let cache = crate::http_cache::HttpCache::default();
    let resp = reply(&[
        ("Cache-Control", "max-age=600"),
        ("Vary", "Accept-Language"),
    ]);
    let en = get("http://h/v", &[("Accept-Language", "en")]);
    let fr = get("http://h/v", &[("Accept-Language", "fr")]);
    assert_eq!(network_calls(&cache, &en, &resp, 2), 1);
    assert_eq!(network_calls(&cache, &fr, &resp, 1), 1);
}

#[test]
fn concurrent_identical_cacheable_fetches_share_one_network_request() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let mut sock = sock.unwrap();
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(sock.try_clone().unwrap());
                loop {
                    let mut head = String::new();
                    while r.read_line(&mut head).unwrap_or(0) > 2 && !head.ends_with("\r\n\r\n") {}
                    if head.is_empty() {
                        return;
                    }
                    seen.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let reply = b"HTTP/1.1 200 OK\r\nCache-Control: public, max-age=600\r\nContent-Length: 7\r\n\r\n#EXTM3U";
                    if sock.write_all(reply).is_err() {
                        return;
                    }
                }
            });
        }
    });
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &format!(
            r#"
            const url = "http:\/\/127.0.0.1:{port}/list.m3u8";
            const bodies = await Promise.all([fetch(url), fetch(url)].map(p => p.then(r => r.text())));
            L(bodies.join(","));
            L(await (await fetch(url, {{ cache: "no-store" }})).text());
        "#
        ),
    );
    assert_eq!(log, ["#EXTM3U,#EXTM3U", "#EXTM3U"]);
    assert_eq!(
        requests.load(Ordering::SeqCst),
        2,
        "two concurrent GETs plus one no-store GET"
    );
}

#[test]
fn a_vary_split_over_two_header_lines_still_keys_on_every_named_header() {
    let cache = crate::http_cache::HttpCache::default();
    let resp = reply(&[
        ("Cache-Control", "public, max-age=600"),
        ("Vary", "Accept-Encoding"),
        ("Vary", "Media-User-Token"),
    ]);
    let a = get("http://h/v2", &[("Media-User-Token", "user-a")]);
    let b = get("http://h/v2", &[("Media-User-Token", "user-b")]);
    assert_eq!(network_calls(&cache, &a, &resp, 2), 1);
    assert_eq!(
        network_calls(&cache, &b, &resp, 1),
        1,
        "user B was served user A's response"
    );
}

#[test]
fn clearing_empties_the_cache_and_the_entry_count_and_expiry_are_bounded() {
    let cache = crate::http_cache::HttpCache::default();
    let fresh = reply(&[("Cache-Control", "max-age=600")]);
    for i in 0..300 {
        network_calls(&cache, &get(&format!("http://h/{i}"), &[]), &fresh, 1);
    }
    assert!(cache.len() <= 256, "{} entries", cache.len());
    cache.clear();
    assert_eq!(network_calls(&cache, &get("http://h/0", &[]), &fresh, 1), 1);
    let short = reply(&[("Cache-Control", "max-age=601"), ("Age", "600")]);
    cache.clear();
    for i in 0..5 {
        network_calls(&cache, &get(&format!("http://h/short{i}"), &[]), &short, 1);
    }
    std::thread::sleep(std::time::Duration::from_millis(1100));
    network_calls(&cache, &get("http://h/after", &[]), &fresh, 1);
    assert_eq!(cache.len(), 1, "expired entries are purged on insert");
}

fn leader_then_waiter(
    leader_ms: u64,
    leader_fails: bool,
    waiter_timeout_ms: u64,
) -> (std::time::Duration, bool) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let cache = Arc::new(crate::http_cache::HttpCache::default());
    let mut req = get("http://h/lead", &[]);
    let c = cache.clone();
    let lead = get("http://h/lead", &[]);
    let leader = std::thread::spawn(move || {
        c.fetch(&lead, |r| {
            std::thread::sleep(std::time::Duration::from_millis(leader_ms));
            let mut out = reply(&[("Cache-Control", "max-age=600")]);
            out.id = r.id;
            if leader_fails {
                out.error = Some("connection reset".into());
            }
            out
        })
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    req.timeout_ms = waiter_timeout_ms;
    let went_to_network = AtomicBool::new(false);
    let started = std::time::Instant::now();
    cache.fetch(&req, |r| {
        went_to_network.store(true, Ordering::SeqCst);
        crate::net::HttpResponse {
            id: r.id,
            ..reply(&[])
        }
    });
    let waited = started.elapsed();
    leader.join().unwrap();
    (waited, went_to_network.load(Ordering::SeqCst))
}

#[test]
fn a_coalesced_waiter_gives_up_at_its_own_deadline_and_a_failed_leader_releases_it() {
    let (waited, own_fetch) = leader_then_waiter(2000, false, 300);
    assert!(
        own_fetch && waited < std::time::Duration::from_millis(1000),
        "{waited:?}"
    );
    let (waited, own_fetch) = leader_then_waiter(200, true, 10_000);
    assert!(
        own_fetch && waited < std::time::Duration::from_millis(1000),
        "{waited:?}"
    );
}

fn silent_after_first_reply(
    every_connection: bool,
) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::sync::{Arc, Mutex};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (n, sock) in listener.incoming().enumerate() {
            let mut sock = sock.unwrap();
            let log = log.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(sock.try_clone().unwrap());
                for served in 0.. {
                    let mut head = String::new();
                    while r.read_line(&mut head).unwrap_or(0) > 2 && !head.ends_with("\r\n\r\n") {}
                    if head.is_empty() {
                        return;
                    }
                    log.lock()
                        .unwrap()
                        .push(format!("conn{n} {}", head.lines().next().unwrap_or("")));
                    if (n == 0 || every_connection) && served == 1 {
                        std::thread::sleep(std::time::Duration::from_secs(30));
                        return;
                    }
                    if sock
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    (port, seen)
}

#[test]
fn a_silently_dead_pooled_connection_is_retried_once_for_get_and_never_for_post() {
    for (method, retried) in [("GET", true), ("POST", false)] {
        let (port, seen) = silent_after_first_reply(false);
        let req = |method: &str| crate::net::HttpRequest {
            id: 1,
            method: method.into(),
            url: format!("http://127.0.0.1:{port}/x"),
            headers: Vec::new(),
            body: (method == "POST").then(Vec::new),
            timeout_ms: 1500,
            use_cache: false,
        };
        assert_eq!(crate::net::perform(&req("GET")).status, 200);
        let started = std::time::Instant::now();
        let second = crate::net::perform(&req(method));
        let took = started.elapsed();
        if retried {
            assert_eq!(
                (second.status, second.error.as_deref()),
                (200, None),
                "{method}"
            );
            assert!(
                took >= std::time::Duration::from_millis(1400),
                "{method} never met the dead connection: {took:?}"
            );
        } else {
            assert!(second.error.is_some(), "{method} must not be retried");
        }
        assert!(
            took < std::time::Duration::from_secs(5),
            "{method} waited {took:?}"
        );
        let posts = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("POST"))
            .count();
        assert_eq!(
            posts,
            usize::from(method == "POST"),
            "{:?}",
            seen.lock().unwrap()
        );
        assert_eq!(
            crate::net::perform(&req("GET")).status,
            200,
            "{method}: the next request after the failure"
        );
    }
}

#[test]
fn a_transport_error_drops_the_pool_so_sibling_stale_connections_are_not_reused() {
    let (port, _) = silent_after_first_reply(true);
    let req = move || crate::net::HttpRequest {
        id: 1,
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/y"),
        headers: Vec::new(),
        body: None,
        timeout_ms: 1500,
        use_cache: false,
    };
    let warm: Vec<_> = (0..2)
        .map(|_| std::thread::spawn(move || crate::net::perform(&req()).status))
        .collect();
    assert!(warm.into_iter().all(|t| t.join().unwrap() == 200));
    assert_eq!(crate::net::perform(&req()).status, 200);
    let started = std::time::Instant::now();
    assert_eq!(crate::net::perform(&req()).status, 200);
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1000),
        "reused a stale sibling: {:?}",
        started.elapsed()
    );
}
