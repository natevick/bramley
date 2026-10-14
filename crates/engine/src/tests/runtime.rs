use super::element::{engine, run};
use super::in_child;

#[cfg(target_os = "linux")]
#[test]
fn random_values_never_fall_back_to_zeros_when_no_file_can_be_opened() {
    let test = "tests::runtime::random_values_never_fall_back_to_zeros_when_no_file_can_be_opened";
    let child = in_child(test, &["prlimit", "--nofile=64:64"], || {
        let (mut e, sink) = engine();
        let held: Vec<std::fs::File> = std::iter::from_fn(|| std::fs::File::open("/dev/null").ok())
            .take(64)
            .collect();
        assert!(
            std::fs::File::open("/dev/urandom").is_err(),
            "{} fds held",
            held.len()
        );
        let log = run(
            &mut e,
            &sink,
            r#"
            const b = crypto.getRandomValues(new Uint8Array(32));
            L(b.some(x => x !== 0) && crypto.randomUUID() !== crypto.randomUUID() ? "random" : "zeros");
        "#,
        );
        assert_eq!(log, ["random"]);
    });
    if let Some(c) = child {
        assert!(c.status.success(), "{:?}\n{}", c.status, c.stderr);
    }
}

fn serve(responses: usize, body: &'static str) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        (0..responses)
            .map(|_| {
                let (mut sock, _) = listener.accept().unwrap();
                let mut head = String::new();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                while r.read_line(&mut head).unwrap() > 2 && !head.ends_with("\r\n\r\n") {}
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                sock.write_all(reply.as_bytes()).unwrap();
                head
            })
            .collect()
    });
    (port, server)
}

#[test]
fn scripts_are_hashed_as_fetched_and_truncation_happens_in_memory_only() {
    let body = "globalThis.loaded = ['whole', 'script'];";
    let (port, server) = serve(2, body);
    let whole = "7bc9f8a048f0e862488c6ab3c26eb8c19118f79f836608d914c71467892e01dc";
    for truncate in [false, true] {
        let mut e = crate::Engine::new(crate::EngineConfig::default()).unwrap();
        if truncate {
            e.sabotage_truncate_scripts();
        }
        e.eval_script("load.js", &format!("const s = document.createElement('script'); s.src = 'http://127.0.0.1:{port}/a.js'; s.onload = s.onerror = ev => {{ globalThis.outcome = ev.type; }}; document.head.appendChild(s);")).unwrap();
        for _ in 0..2000 {
            e.tick();
            if e.eval_json("globalThis.outcome").unwrap() != serde_json::Value::Null {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let log = e.status()["scripts"][1].clone();
        assert_eq!(log["fetchedSha256"], whole, "{log}");
        assert_eq!(log["fetchedBytes"], body.len());
        let (outcome, evaluated) = if truncate {
            ("error", body.len() / 2)
        } else {
            ("load", body.len())
        };
        assert_eq!(e.eval_json("globalThis.outcome").unwrap(), outcome);
        assert_eq!(log["evaluatedBytes"], evaluated);
        assert_eq!(log["evaluatedSha256"] == whole, !truncate, "{log}");
    }
    let ua = format!("user-agent: {}", crate::user_agent()).to_ascii_lowercase();
    for request in server.join().unwrap() {
        assert!(request.to_ascii_lowercase().contains(&ua), "{request}");
    }
}

#[test]
fn navigator_reports_bramleys_own_user_agent_and_origin() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        "L(navigator.userAgent); L(location.origin); L(origin);",
    );
    assert_eq!(
        log,
        [
            crate::user_agent().as_str(),
            "app://bramley",
            "app://bramley"
        ]
    );
    assert!(crate::user_agent().starts_with("Bramley/"));
}

#[test]
fn redact_removes_known_secrets_and_anything_jwt_shaped() {
    let secrets = ["user-token-value".to_string(), String::new()];
    let text = "a user-token-value b eyJhbGciOi.eyJpc3MiOiJYIn0.c2lnbmF0dXJl, eyJnot-a-jwt c";
    assert_eq!(
        crate::redact(text, &secrets),
        "a <redacted> b <jwt>, eyJnot-a-jwt c"
    );
}

#[test]
fn redact_catches_cut_and_percent_encoded_secrets() {
    let secret = "abcdefghijklmnop+qrstu/vwxyz=".to_string();
    let cut = format!("tail {}", &secret[..20]);
    assert_eq!(
        crate::redact(&cut, std::slice::from_ref(&secret)),
        "tail <redacted>"
    );
    let encoded = "q=abcdefghijklmnop%2Bqrstu%2Fvwxyz%3D&x";
    assert_eq!(crate::redact(encoded, &[secret]), "q=<redacted>&x");
}

#[test]
fn failing_platform_callbacks_print_redacted_errors() {
    let test = "tests::runtime::failing_platform_callbacks_print_redacted_errors";
    let child = in_child(test, &[], || {
        let config = crate::EngineConfig {
            redact: vec!["s3cr3t-user-token".into()],
            ..Default::default()
        };
        let e = crate::Engine::new(config).unwrap();
        e.eval_script(
            "t.js",
            "__platform.statusJson = () => { throw new Error('leaked s3cr3t-user-token'); };",
        )
        .unwrap();
        e.status();
    });
    if let Some(c) = child {
        assert!(c.status.success(), "{}", c.stderr);
        assert!(c.stderr.contains("statusJson failed"), "{}", c.stderr);
        assert!(!c.stderr.contains("s3cr3t"), "{}", c.stderr);
    }
}

fn first_idle_wait(e: &mut crate::Engine) -> std::time::Duration {
    let busy = std::time::Duration::from_millis(5);
    (0..20)
        .map(|_| e.tick())
        .find(|w| *w > busy)
        .unwrap_or(busy)
}

#[test]
fn a_paused_element_sleeps_until_the_next_timer_and_a_playing_one_keeps_pumping() {
    use std::time::Duration;
    let (mut e, sink) = engine();
    let body = r#"
        const a = fresh();
        audio.src = URL.createObjectURL(a.ms);
        await fill(a.ms, a.opened, bytes);
        await audio.play();
        globalThis.el = audio;
    "#;
    run(&mut e, &sink, &super::element::script(body));
    assert!(
        e.tick() <= Duration::from_millis(5),
        "a playing element must keep the 5 ms pump"
    );
    e.eval_script("pause.js", "el.pause(); setTimeout(() => {}, 400);")
        .unwrap();
    let wait = first_idle_wait(&mut e);
    assert!(
        wait > Duration::from_millis(300) && wait <= Duration::from_millis(400),
        "{wait:?}"
    );
}

#[test]
fn a_request_in_flight_keeps_the_engine_polling_until_it_lands() {
    use std::io::{BufRead, BufReader, Write};
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (mut e, _sink) = engine();
    e.eval_script(
        "fetch.js",
        &format!("fetch('http://127.0.0.1:{port}/').then(r => {{ globalThis.got = r.status; }});"),
    )
    .unwrap();
    assert_eq!(first_idle_wait(&mut e), Duration::from_millis(5));
    let (mut sock, _) = listener.accept().unwrap();
    let mut head = String::new();
    let mut r = BufReader::new(sock.try_clone().unwrap());
    while r.read_line(&mut head).unwrap() > 2 && !head.ends_with("\r\n\r\n") {}
    sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
        .unwrap();
    for _ in 0..2000 {
        if e.eval_json("globalThis.got").unwrap() == 200 {
            break;
        }
        e.tick();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(e.eval_json("globalThis.got").unwrap(), 200);
    assert!(first_idle_wait(&mut e) >= Duration::from_secs(1));
}

#[test]
fn a_task_queued_by_one_completion_runs_before_the_next_completion_is_dispatched() {
    let (port, server) = serve(2, "x");
    let (mut e, _sink) = engine();
    e.eval_script(
        "two.js",
        &format!(
            r#"
            const audio = document.createElement("audio");
            let fired = false, first = true;
            audio.addEventListener("volumechange", () => {{ fired = true; }});
            const landed = () => {{ if (first) {{ first = false; audio.volume = 0.5; L("first"); }} else L("second saw volumechange: " + fired); }};
            for (const path of ["a", "b"]) fetch("http:\/\/127.0.0.1:{port}/" + path).then(landed);
        "#
        ),
    )
    .unwrap();
    server.join().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    for _ in 0..200 {
        e.tick();
        if e.eval_json("log.length").unwrap() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        e.eval_json("log").unwrap(),
        serde_json::json!(["first", "second saw volumechange: true"])
    );
}

#[test]
fn a_task_queued_by_a_media_event_runs_before_the_next_completion_is_dispatched() {
    let (port, server) = serve(1, "x");
    let (mut e, _sink) = engine();
    e.eval_script(
        "play.js",
        r#"
        const audio = document.createElement("audio");
        globalThis.fired = false;
        audio.addEventListener("volumechange", () => { fired = true; });
        audio.addEventListener("play", () => { audio.volume = 0.5; });
        audio.play().catch(() => {});
    "#,
    )
    .unwrap();
    e.tick();
    e.eval_script(
        "fetch.js",
        &format!(
            r#"fetch("http:\/\/127.0.0.1:{port}/").then(() => L("completion saw volumechange: " + fired));"#
        ),
    )
    .unwrap();
    server.join().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    for _ in 0..200 {
        e.tick();
        if e.eval_json("log.length").unwrap() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        e.eval_json("log").unwrap(),
        serde_json::json!(["completion saw volumechange: true"])
    );
}

#[test]
fn an_aborted_request_does_not_keep_the_engine_polling() {
    use std::time::Duration;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (mut e, _sink) = engine();
    e.eval_script("fetch.js", &format!("globalThis.ac = new AbortController(); fetch('http://127.0.0.1:{port}/', {{ signal: ac.signal }}).catch(() => {{}});")).unwrap();
    assert_eq!(first_idle_wait(&mut e), Duration::from_millis(5));
    e.eval_script("abort.js", "ac.abort();").unwrap();
    assert!(first_idle_wait(&mut e) >= Duration::from_secs(1));
    drop(listener);
}

#[cfg(feature = "trace")]
#[test]
fn the_cdm_trace_shows_key_and_message_counts_never_key_ids_or_message_text() {
    use serde_json::json;
    let keys = json!({"type": "keys", "sid": 3, "keys": [{"kid": "00112233445566778899aabbccddeeff", "status": "usable"}]});
    let shown = crate::runtime::trace_meta(&keys).to_string();
    assert!(
        shown.contains(r#""keys":1"#) && !shown.contains("00112233"),
        "{shown}"
    );
    let reject =
        json!({"type": "reject", "promise": 1, "message": "license for Some Song Title refused"});
    let shown = crate::runtime::trace_meta(&reject).to_string();
    assert!(
        shown.contains(r#""message":35"#) && !shown.contains("Song Title"),
        "{shown}"
    );
}
