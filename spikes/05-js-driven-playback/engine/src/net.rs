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
}

pub struct HttpResponse {
    pub id: u32,
    pub status: u16,
    pub status_text: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub error: Option<String>,
}

pub fn spawn(req: HttpRequest, done: Sender<Completion>) {
    std::thread::Builder::new()
        .name("http".into())
        .spawn(move || {
            let resp = perform(&req);
            let _ = done.send(Completion::Http(resp));
        })
        .expect("spawn http thread");
}

pub fn spawn_script(id: u32, url: String, done: Sender<Completion>) {
    std::thread::Builder::new()
        .name("script".into())
        .spawn(move || {
            let r = perform(&HttpRequest { id, method: "GET".into(), url: url.clone(), headers: Vec::new(), body: None, timeout_ms: 30_000 });
            let result = if r.error.is_none() && (200..300).contains(&r.status) { Ok(r.body) } else { Err(r.error.unwrap_or_else(|| format!("HTTP {}", r.status))) };
            let _ = done.send(Completion::Script { id, url, result });
        })
        .expect("spawn script thread");
}

pub enum Completion {
    Http(HttpResponse),
    Script { id: u32, url: String, result: Result<Vec<u8>, String> },
}

fn perform(req: &HttpRequest) -> HttpResponse {
    let mut out = HttpResponse { id: req.id, status: 0, status_text: String::new(), url: req.url.clone(), headers: Vec::new(), body: Vec::new(), error: None };
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_millis(req.timeout_ms.max(1000)))).build().into();
    let mut builder = ureq::http::Request::builder().method(req.method.as_str()).uri(req.url.as_str());
    for (k, v) in &req.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let result = match &req.body {
        Some(b) => builder.body(b.clone()).map_err(|e| e.to_string()).and_then(|r| agent.run(r).map_err(|e| e.to_string())),
        None => builder.body(()).map_err(|e| e.to_string()).and_then(|r| agent.run(r).map_err(|e| e.to_string())),
    };
    match result {
        Ok(mut resp) => {
            out.status = resp.status().as_u16();
            out.status_text = resp.status().canonical_reason().unwrap_or("").to_string();
            out.url = resp.get_uri().to_string();
            out.headers = resp.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())).collect();
            match resp.body_mut().with_config().limit(256 << 20).read_to_vec() {
                Ok(b) => out.body = b,
                Err(e) => out.error = Some(e.to_string()),
            }
        }
        Err(e) => out.error = Some(e),
    }
    out
}
