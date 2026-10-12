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
