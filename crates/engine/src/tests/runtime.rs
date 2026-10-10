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
        let config = crate::EngineConfig {
            truncate_scripts: truncate,
            ..Default::default()
        };
        let mut e = crate::Engine::new(config).unwrap();
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
