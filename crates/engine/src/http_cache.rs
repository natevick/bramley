use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::net::{HttpRequest, HttpResponse};

const MAX_ENTRY_BYTES: usize = 512 << 10;
const MAX_TOTAL_BYTES: usize = 2 << 20;
const MAX_ENTRIES: usize = 256;
const PERSONAL_REQUEST_HEADERS: [&str; 4] = [
    "authorization",
    "media-user-token",
    "music-user-token",
    "x-apple-music-user-token",
];

struct Stored {
    response: HttpResponse,
    vary: Vec<(String, Option<String>)>,
    fresh_until: Instant,
    stored_at: Instant,
    bytes: usize,
}

#[derive(Default)]
struct State {
    stored: HashMap<String, Stored>,
    pending: HashSet<String>,
    bytes: usize,
}

#[derive(Default)]
pub struct HttpCache {
    state: Mutex<State>,
    landed: Condvar,
}

fn field(headers: &[(String, String)], name: &str) -> Option<String> {
    let lines: Vec<&str> = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .collect();
    (!lines.is_empty()).then(|| lines.join(", "))
}

fn directives(cache_control: &str) -> HashMap<String, Option<String>> {
    cache_control
        .split(',')
        .filter_map(|d| {
            let mut parts = d.trim().splitn(2, '=');
            let name = parts.next()?.trim().to_ascii_lowercase();
            (!name.is_empty()).then(|| {
                (
                    name,
                    parts.next().map(|v| v.trim().trim_matches('"').to_owned()),
                )
            })
        })
        .collect()
}

fn request_forbids_cache(req: &HttpRequest) -> bool {
    let cc = directives(&field(&req.headers, "cache-control").unwrap_or_default());
    let pragma = field(&req.headers, "pragma").unwrap_or_default();
    cc.contains_key("no-cache")
        || cc.contains_key("no-store")
        || cc.get("max-age").is_some_and(|v| v.as_deref() == Some("0"))
        || pragma.to_ascii_lowercase().contains("no-cache")
        || req
            .headers
            .iter()
            .any(|(k, _)| k.to_ascii_lowercase().starts_with("if-"))
}

pub fn lookup_allowed(req: &HttpRequest) -> bool {
    req.use_cache
        && req.method.eq_ignore_ascii_case("GET")
        && req.body.is_none()
        && field(&req.headers, "range").is_none()
        && !request_forbids_cache(req)
}

fn freshness(req: &HttpRequest, resp: &HttpResponse) -> Option<Duration> {
    if resp.error.is_some() || resp.status != 200 || resp.body.len() > MAX_ENTRY_BYTES {
        return None;
    }
    let cc = directives(&field(&resp.headers, "cache-control")?);
    if cc.contains_key("no-store") || cc.contains_key("no-cache") {
        return None;
    }
    let personal = PERSONAL_REQUEST_HEADERS
        .iter()
        .any(|h| field(&req.headers, h).is_some());
    if personal && !cc.contains_key("public") {
        return None;
    }
    let max_age: u64 = cc.get("max-age")?.as_deref()?.parse().ok()?;
    let age: u64 = field(&resp.headers, "age")
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(0);
    max_age
        .checked_sub(age)
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
}

fn vary_of(req: &HttpRequest, resp: &HttpResponse) -> Option<Vec<(String, Option<String>)>> {
    let Some(vary) = field(&resp.headers, "vary") else {
        return Some(Vec::new());
    };
    vary.split(',')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .map(|n| (n != "*").then(|| (n.clone(), field(&req.headers, &n))))
        .collect()
}

fn entry_bytes(key: &str, resp: &HttpResponse) -> usize {
    key.len()
        + resp.url.len()
        + resp.body.len()
        + resp
            .headers
            .iter()
            .map(|(k, v)| k.len() + v.len())
            .sum::<usize>()
}

impl HttpCache {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.state.lock().unwrap().stored.len()
    }

    pub fn clear(&self) {
        let mut st = self.state.lock().unwrap();
        st.stored.clear();
        st.bytes = 0;
    }

    pub fn fetch(
        &self,
        req: &HttpRequest,
        network: impl FnOnce(&HttpRequest) -> HttpResponse,
    ) -> HttpResponse {
        if !lookup_allowed(req) {
            return network(req);
        }
        let key = req.url.clone();
        let deadline = Instant::now() + Duration::from_millis(req.timeout_ms);
        let mut st = self.state.lock().unwrap();
        loop {
            let now = Instant::now();
            if let Some(hit) = st.stored.get(&key).filter(|s| {
                s.fresh_until > now && s.vary.iter().all(|(n, v)| field(&req.headers, n) == *v)
            }) {
                return HttpResponse {
                    id: req.id,
                    ..hit.response.clone()
                };
            }
            if !st.pending.contains(&key) {
                break;
            }
            if now >= deadline {
                drop(st);
                return network(req);
            }
            st = self.landed.wait_timeout(st, deadline - now).unwrap().0;
        }
        st.pending.insert(key.clone());
        drop(st);
        let pending = Pending {
            cache: self,
            key: &key,
        };
        let resp = network(req);
        if let Some((lifetime, vary)) = freshness(req, &resp).zip(vary_of(req, &resp)) {
            self.store(&key, &resp, lifetime, vary);
        }
        drop(pending);
        resp
    }

    fn store(
        &self,
        key: &str,
        resp: &HttpResponse,
        lifetime: Duration,
        vary: Vec<(String, Option<String>)>,
    ) {
        let now = Instant::now();
        let bytes = entry_bytes(key, resp);
        let mut st = self.state.lock().unwrap();
        let expired: Vec<String> = st
            .stored
            .iter()
            .filter(|(k, s)| s.fresh_until <= now || k.as_str() == key)
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired {
            let gone = st.stored.remove(&k).unwrap();
            st.bytes -= gone.bytes;
        }
        while st.bytes + bytes > MAX_TOTAL_BYTES || st.stored.len() >= MAX_ENTRIES {
            let Some(oldest) = st
                .stored
                .iter()
                .min_by_key(|(_, s)| s.stored_at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            let gone = st.stored.remove(&oldest).unwrap();
            st.bytes -= gone.bytes;
        }
        st.bytes += bytes;
        st.stored.insert(
            key.to_owned(),
            Stored {
                response: resp.clone(),
                vary,
                fresh_until: now + lifetime,
                stored_at: now,
                bytes,
            },
        );
    }
}

struct Pending<'a> {
    cache: &'a HttpCache,
    key: &'a str,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.cache.state.lock().unwrap().pending.remove(self.key);
        self.cache.landed.notify_all();
    }
}
