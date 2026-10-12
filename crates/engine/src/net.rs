use std::sync::mpsc::Sender;
use std::sync::{LazyLock, Mutex};
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

struct Landing {
    done: Sender<Completion>,
    completion: Option<Completion>,
}

impl Drop for Landing {
    fn drop(&mut self) {
        if let Some(c) = self.completion.take() {
            let _ = self.done.send(c);
        }
    }
}

const PANICKED: &str = "the request thread panicked";

pub fn spawn(req: HttpRequest, done: Sender<Completion>) {
    std::thread::Builder::new()
        .name("http".into())
        .spawn(move || {
            let mut landing = Landing {
                done,
                completion: Some(Completion::Http(failed(&req, PANICKED.into()))),
            };
            landing.completion = Some(Completion::Http(CACHE.fetch(&req, perform)));
        })
        .expect("spawn http thread");
}

pub fn spawn_script(id: u32, url: String, done: Sender<Completion>) {
    std::thread::Builder::new()
        .name("script".into())
        .spawn(move || {
            let mut landing = Landing {
                done,
                completion: Some(Completion::Script {
                    id,
                    url: url.clone(),
                    result: Err(PANICKED.into()),
                }),
            };
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
            landing.completion = Some(Completion::Script { id, url, result });
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
static POOL: Mutex<Option<ureq::Agent>> = Mutex::new(None);
const RECEIVE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn clear_cache() {
    CACHE.clear();
}

fn agent(pooled: bool) -> ureq::Agent {
    let build = |idle: usize| -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(crate::user_agent())
            .max_idle_connections(idle)
            .build()
            .into()
    };
    if !pooled {
        return build(0);
    }
    POOL.lock()
        .unwrap()
        .get_or_insert_with(|| build(10))
        .clone()
}

fn attempt(req: &HttpRequest, agent: &ureq::Agent) -> Result<HttpResponse, String> {
    let timeout = Duration::from_millis(req.timeout_ms.max(1000));
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
    fn send<S: ureq::AsSendBody>(
        agent: &ureq::Agent,
        request: ureq::http::Request<S>,
        timeout: Duration,
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        let request = agent
            .configure_request(request)
            .timeout_global(Some(timeout))
            .timeout_recv_response(Some(timeout.min(RECEIVE_RESPONSE_TIMEOUT)))
            .build();
        agent.run(request)
    }
    let sent = match &req.body {
        Some(b) => builder.body(b.clone()).map(|r| send(agent, r, timeout)),
        None => builder.body(()).map(|r| send(agent, r, timeout)),
    };
    let mut resp = sent
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let body = resp
        .body_mut()
        .with_config()
        .limit(256 << 20)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok(HttpResponse {
        id: req.id,
        status: resp.status().as_u16(),
        status_text: resp.status().canonical_reason().unwrap_or("").to_string(),
        url: resp.get_uri().to_string(),
        headers: resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect(),
        body,
        error: None,
    })
}

fn failed(req: &HttpRequest, error: String) -> HttpResponse {
    HttpResponse {
        id: req.id,
        status: 0,
        status_text: String::new(),
        url: req.url.clone(),
        headers: Vec::new(),
        body: Vec::new(),
        error: Some(error),
    }
}

pub(crate) fn perform(req: &HttpRequest) -> HttpResponse {
    let idempotent = matches!(req.method.to_ascii_uppercase().as_str(), "GET" | "HEAD");
    let result = attempt(req, &agent(true)).or_else(|e| {
        *POOL.lock().unwrap() = None;
        if idempotent {
            attempt(req, &agent(false))
        } else {
            Err(e)
        }
    });
    result.unwrap_or_else(|e| failed(req, e))
}
