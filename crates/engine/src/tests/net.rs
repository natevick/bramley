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
