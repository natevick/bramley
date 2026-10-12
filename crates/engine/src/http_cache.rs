use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::net::{HttpRequest, HttpResponse};

const MAX_ENTRY_BYTES: usize = 512 << 10;
const MAX_TOTAL_BYTES: usize = 2 << 20;

struct Stored {
    response: HttpResponse,
    vary: Vec<(String, Option<String>)>,
    fresh_until: Instant,
    stored_at: Instant,
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

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
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

pub fn lookup_allowed(req: &HttpRequest) -> bool {
    req.use_cache
        && req.method.eq_ignore_ascii_case("GET")
        && req.body.is_none()
        && header(&req.headers, "range").is_none()
}

fn freshness(req: &HttpRequest, resp: &HttpResponse) -> Option<Duration> {
    if resp.error.is_some() || resp.status != 200 || resp.body.len() > MAX_ENTRY_BYTES {
        return None;
    }
    let cc = directives(header(&resp.headers, "cache-control")?);
    if cc.contains_key("no-store") || cc.contains_key("no-cache") {
        return None;
    }
    if header(&req.headers, "authorization").is_some() && !cc.contains_key("public") {
        return None;
    }
    let max_age: u64 = cc.get("max-age")?.as_deref()?.parse().ok()?;
    let age: u64 = header(&resp.headers, "age")
        .and_then(|a| a.trim().parse().ok())
        .unwrap_or(0);
    max_age
        .checked_sub(age)
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
}

fn vary_of(req: &HttpRequest, resp: &HttpResponse) -> Option<Vec<(String, Option<String>)>> {
    let Some(vary) = header(&resp.headers, "vary") else {
        return Some(Vec::new());
    };
    vary.split(',')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .map(|n| (n != "*").then(|| (n.clone(), header(&req.headers, &n).map(str::to_owned))))
        .collect()
}

impl HttpCache {
    pub fn fetch(
        &self,
        req: &HttpRequest,
        network: impl FnOnce(&HttpRequest) -> HttpResponse,
    ) -> HttpResponse {
        if !lookup_allowed(req) {
            return network(req);
        }
        let key = req.url.clone();
        let mut st = self.state.lock().unwrap();
        loop {
            let now = Instant::now();
            if let Some(hit) = st.stored.get(&key).filter(|s| {
                s.fresh_until > now
                    && s.vary
                        .iter()
                        .all(|(n, v)| header(&req.headers, n) == v.as_deref())
            }) {
                return HttpResponse {
                    id: req.id,
                    ..hit.response.clone()
                };
            }
            if !st.pending.contains(&key) {
                break;
            }
            st = self.landed.wait(st).unwrap();
        }
        st.pending.insert(key.clone());
        drop(st);
        let pending = Pending {
            cache: self,
            key: &key,
        };
        let resp = network(req);
        let entry = freshness(req, &resp).zip(vary_of(req, &resp));
        if let Some((lifetime, vary)) = entry {
            let now = Instant::now();
            let mut st = self.state.lock().unwrap();
            if let Some(old) = st.stored.remove(&key) {
                st.bytes -= old.response.body.len();
            }
            while st.bytes + resp.body.len() > MAX_TOTAL_BYTES {
                let Some(oldest) = st
                    .stored
                    .iter()
                    .min_by_key(|(_, s)| s.stored_at)
                    .map(|(k, _)| k.clone())
                else {
                    break;
                };
                let gone = st.stored.remove(&oldest).unwrap();
                st.bytes -= gone.response.body.len();
            }
            st.bytes += resp.body.len();
            st.stored.insert(
                key.clone(),
                Stored {
                    response: resp.clone(),
                    vary,
                    fresh_until: now + lifetime,
                    stored_at: now,
                },
            );
        }
        drop(pending);
        resp
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
