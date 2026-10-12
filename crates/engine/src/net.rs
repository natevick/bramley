use std::sync::LazyLock;
use std::sync::mpsc::Sender;
use std::time::Duration;

use ureq::ResponseExt;

pub struct HttpRequest {
    pub id: u32,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: u64,
    pub use_cache: bool,
}

#[derive(Clone)]
pub struct HttpResponse {
    pub id: u32,
    pub status: u16,
    pub status_text: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub error: Option<String>,
}

const FORBIDDEN_REQUEST_HEADERS: [&str; 23] = [
    "accept-charset",
    "accept-encoding",
    "access-control-request-headers",
    "access-control-request-method",
    "access-control-request-private-network",
    "connection",
    "content-length",
    "cookie",
    "cookie2",
    "date",
    "dnt",
    "expect",
    "host",
    "keep-alive",
    "origin",
    "referer",
    "set-cookie",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
    "via",
];

pub fn forbidden_request_header(name: &str, value: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    let overrides_to_forbidden_method = matches!(
        name.as_str(),
        "x-http-method" | "x-http-method-override" | "x-method-override"
    ) && value.split(',').any(|m| {
        ["CONNECT", "TRACE", "TRACK"]
            .iter()
            .any(|f| m.trim().eq_ignore_ascii_case(f))
    });
    FORBIDDEN_REQUEST_HEADERS.contains(&name.as_str())
        || name.starts_with("proxy-")
        || name.starts_with("sec-")
        || overrides_to_forbidden_method
}

pub fn spawn(req: HttpRequest, done: Sender<Completion>) {
    std::thread::Builder::new()
        .name("http".into())
        .spawn(move || {
            let resp = CACHE.fetch(&req, perform);
            let _ = done.send(Completion::Http(resp));
        })
        .expect("spawn http thread");
}

pub fn spawn_script(id: u32, url: String, done: Sender<Completion>) {
    std::thread::Builder::new()
        .name("script".into())
        .spawn(move || {
            let r = perform(&HttpRequest {
                id,
                method: "GET".into(),
                url: url.clone(),
                headers: Vec::new(),
                body: None,
                timeout_ms: 30_000,
                use_cache: false,
            });
            let result = if r.error.is_none() && (200..300).contains(&r.status) {
                Ok(r.body)
            } else {
                Err(r.error.unwrap_or_else(|| format!("HTTP {}", r.status)))
            };
            let _ = done.send(Completion::Script { id, url, result });
        })
        .expect("spawn script thread");
}

pub enum Completion {
    Http(HttpResponse),
    Script {
        id: u32,
        url: String,
        result: Result<Vec<u8>, String>,
    },
}

static CACHE: LazyLock<crate::http_cache::HttpCache> = LazyLock::new(Default::default);

pub fn clear_cache() {
    CACHE.clear();
}

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(crate::user_agent())
        .build()
        .into()
});

fn run<S: ureq::AsSendBody>(
    request: ureq::http::Request<S>,
    timeout: Option<Duration>,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let request = AGENT
        .configure_request(request)
        .timeout_global(timeout)
        .build();
    AGENT.run(request).map_err(|e| e.to_string())
}

fn perform(req: &HttpRequest) -> HttpResponse {
    let mut out = HttpResponse {
        id: req.id,
        status: 0,
        status_text: String::new(),
        url: req.url.clone(),
        headers: Vec::new(),
        body: Vec::new(),
        error: None,
    };
    let timeout = Some(Duration::from_millis(req.timeout_ms.max(1000)));
    let mut builder = ureq::http::Request::builder()
        .method(req.method.as_str())
        .uri(req.url.as_str());
    for (k, v) in req
        .headers
        .iter()
        .filter(|(k, v)| !forbidden_request_header(k, v))
    {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let result = match &req.body {
        Some(b) => builder
            .body(b.clone())
            .map_err(|e| e.to_string())
            .and_then(|r| run(r, timeout)),
        None => builder
            .body(())
            .map_err(|e| e.to_string())
            .and_then(|r| run(r, timeout)),
    };
    match result {
        Ok(mut resp) => {
            out.status = resp.status().as_u16();
            out.status_text = resp.status().canonical_reason().unwrap_or("").to_string();
            out.url = resp.get_uri().to_string();
            out.headers = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            match resp.body_mut().with_config().limit(256 << 20).read_to_vec() {
                Ok(b) => out.body = b,
                Err(e) => out.error = Some(e.to_string()),
            }
        }
        Err(e) => out.error = Some(e),
    }
    out
}
