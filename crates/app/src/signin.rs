use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

pub const LIFETIME: Duration = Duration::from_secs(10 * 60);
const PAGE: &str = include_str!("signin.html");
const MAX_TOKEN: usize = 8 * 1024;
const MAX_REQUEST: u64 = 16 * 1024 + MAX_TOKEN as u64;
const READ_WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

type Reply = (&'static str, String, bool);

pub struct Server {
    pub url: String,
    pub done: Receiver<()>,
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

struct Site {
    host: String,
    home: String,
    post: String,
    page: String,
    used: bool,
}

pub fn start(
    developer: &str,
    lifetime: Duration,
    mut keep: impl FnMut(&str) -> bool + Send + 'static,
) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let host = format!("127.0.0.1:{}", listener.local_addr()?.port());
    let nonce = nonce()?;
    let embedded = serde_json::to_string(developer)?.replace("</", "<\\/");
    let mut site = Site {
        home: format!("/{nonce}/"),
        post: format!("/{nonce}/token"),
        page: PAGE.replace("__DEV_TOKEN__", &embedded),
        used: false,
        host,
    };
    let url = format!("http://{}{}", site.host, site.home);
    let (tx, done) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let halt = stop.clone();
    let deadline = Instant::now() + lifetime;
    std::thread::Builder::new()
        .name("sign-in".into())
        .spawn(move || {
            while !halt.load(Ordering::SeqCst) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if site.answer(stream, &mut keep) {
                            let _ = tx.send(());
                        }
                    }
                    Err(_) => std::thread::sleep(POLL),
                }
            }
        })?;
    Ok(Server { url, done, stop })
}

fn nonce() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| std::io::Error::other("no system randomness"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl Site {
    fn answer(&mut self, stream: TcpStream, keep: &mut impl FnMut(&str) -> bool) -> bool {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(READ_WAIT));
        let Ok(mut out) = stream.try_clone() else {
            return false;
        };
        let mut reader = BufReader::new(stream.take(MAX_REQUEST));
        let bad = ("400 Bad Request", String::new(), false);
        let (status, body, kept) = self.route(&mut reader, keep).unwrap_or(bad);
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = out
            .write_all(head.as_bytes())
            .and_then(|()| out.write_all(body.as_bytes()));
        kept
    }

    fn route(
        &mut self,
        reader: &mut impl BufRead,
        keep: &mut impl FnMut(&str) -> bool,
    ) -> Option<Reply> {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut words = line.split_whitespace();
        let (method, path) = (words.next()?.to_owned(), words.next()?.to_owned());
        let (mut host, mut length) = (None, 0usize);
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).ok()? == 0 || header.trim().is_empty() {
                break;
            }
            let (name, value) = header.split_once(':')?;
            match name.trim().to_ascii_lowercase().as_str() {
                "host" if host.is_none() => host = Some(value.trim().to_owned()),
                "host" => return None,
                "content-length" => length = value.trim().parse().ok()?,
                _ => {}
            }
        }
        if length > MAX_TOKEN {
            return None;
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        let empty = |status| Some((status, String::new(), false));
        if host.as_deref() != Some(self.host.as_str()) {
            return empty("403 Forbidden");
        }
        if path != self.home && path != self.post {
            return empty("404 Not Found");
        }
        if self.used {
            return empty("410 Gone");
        }
        match method.as_str() {
            "GET" if path == self.home => Some(("200 OK", self.page.clone(), false)),
            "POST" if path == self.post => {
                let body = String::from_utf8(body).ok()?;
                let token = body.trim();
                if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
                    return None;
                }
                if !keep(token) {
                    return empty("500 Internal Server Error");
                }
                self.used = true;
                Some(("200 OK", String::new(), true))
            }
            "GET" | "POST" => None,
            _ => empty("405 Method Not Allowed"),
        }
    }
}

#[cfg(not(test))]
pub fn open_browser(url: &str) {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(windows)]
    let mut command = Command::new("rundll32");
    #[cfg(windows)]
    command.arg("url.dll,FileProtocolHandler");
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut command = Command::new("xdg-open");
    let spawned = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}
