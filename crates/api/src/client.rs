use std::hash::{BuildHasher, RandomState};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bramley_engine::{redact, user_agent};
use serde::de::DeserializeOwned;

use crate::models::{Detail, Envelope, Item, Page, SearchResults, Searched, WithTracks};
use crate::store::{Store, digest};

pub const API_BASE: &str = "https://api.music.apple.com";
const MAX_BODY_BYTES: u64 = 2 << 20;
const MAX_PAGES: usize = 200;
const RETRY_BUDGET: Duration = Duration::from_secs(1);
const MAX_RETRIES: u32 = 2;
const RECEIVE_RESPONSE: Duration = Duration::from_secs(10);
const MAX_RETRY_AFTER_S: u64 = 3600;
const UNREADABLE_RETRY_AFTER_S: u64 = 60;
const SEARCH_TYPES: &str = "songs,albums,artists,playlists";
const SEARCH_LIMIT: &str = "10";
const LIBRARY_LIMIT: &str = "100";
const CATALOG_ARTIST_LIMIT: &str = "25";
const LIBRARY: &str = "/v1/me/library/";
const NO_RELATED: &str = "No related resources found for ";

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    DeveloperToken,
    SignInNeeded,
    RateLimited(u64),
    Refused(String),
    TooLarge,
    Http(u16, String),
    Network(String),
    Decode(String),
    Config(String),
    Superseded,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::DeveloperToken => f.write_str("Bramley's developer token was rejected"),
            Error::SignInNeeded => f.write_str("sign in needed"),
            Error::Superseded => f.write_str("superseded by a newer search"),
            Error::RateLimited(s) => write!(f, "Apple Music is rate limiting; retry in {s} s"),
            Error::Refused(why) => write!(f, "request refused: {why}"),
            Error::TooLarge => write!(f, "response over {MAX_BODY_BYTES} bytes"),
            Error::Http(status, why) => write!(f, "HTTP {status}: {why}"),
            Error::Network(why) | Error::Decode(why) | Error::Config(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Tokens {
    pub developer: String,
    pub user: Option<String>,
}

type TokenSource = Arc<dyn Fn() -> std::result::Result<Tokens, String> + Send + Sync>;
type Renew = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shelf {
    Playlists,
    Albums,
    Songs,
    Artists,
}

type Storefront = Arc<Mutex<Option<(u64, String)>>>;

#[derive(Clone)]
pub struct Client {
    agent: Arc<Mutex<ureq::Agent>>,
    base: String,
    tokens: TokenSource,
    renew: Renew,
    storefront: Storefront,
    hasher: RandomState,
    cache: Option<Arc<Store>>,
}

pub(crate) struct Scoped {
    store: Arc<Store>,
    scope: String,
    epoch: u64,
}

impl Scoped {
    fn read(&self, request: &str) -> Option<Vec<u8>> {
        self.store.get(&["api", &self.scope, request])
    }

    fn write(&self, request: &str, body: &[u8]) {
        self.store
            .put(&["api", &self.scope, request], body, self.epoch);
    }

    fn forget(&self, request: &str) {
        self.store.remove(&["api", &self.scope, request]);
    }
}

pub struct Session<'a> {
    client: &'a Client,
    pub(crate) tokens: Tokens,
    user_key: Option<u64>,
    pub(crate) offline: bool,
    cached: Option<Scoped>,
}

impl Client {
    pub fn new(
        tokens: impl Fn() -> std::result::Result<Tokens, String> + Send + Sync + 'static,
    ) -> Self {
        Self::at(API_BASE, tokens)
    }

    #[cfg(feature = "loopback")]
    pub fn loopback(
        port: u16,
        tokens: impl Fn() -> std::result::Result<Tokens, String> + Send + Sync + 'static,
    ) -> Self {
        Self::at(&format!("http://127.0.0.1:{port}"), tokens)
    }

    pub(crate) fn at(
        base: &str,
        tokens: impl Fn() -> std::result::Result<Tokens, String> + Send + Sync + 'static,
    ) -> Self {
        Client {
            agent: Arc::new(Mutex::new(agent(true))),
            base: base.to_owned(),
            tokens: Arc::new(tokens),
            renew: Arc::new(|_| false),
            storefront: Storefront::default(),
            hasher: RandomState::new(),
            cache: None,
        }
    }

    pub fn caching(mut self, store: Arc<Store>) -> Self {
        self.cache = Some(store);
        self
    }

    pub fn renewing(mut self, renew: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.renew = Arc::new(renew);
        self
    }

    pub(crate) fn renew(&self, rejected: &str) -> bool {
        (self.renew)(rejected)
    }

    pub fn session(&self) -> Result<Session<'_>> {
        let tokens = (self.tokens)().map_err(Error::Config)?;
        let user_key = tokens.user.as_ref().map(|u| self.hasher.hash_one(u));
        if user_key.is_none() {
            *self.storefront.lock().unwrap() = None;
        }
        let scoped = |store: &Arc<Store>| {
            let user = tokens.user.as_deref()?;
            Some(Scoped {
                store: store.clone(),
                scope: digest(&[user]),
                epoch: store.epoch(),
            })
        };
        let cached = self.cache.as_ref().and_then(scoped);
        Ok(Session {
            client: self,
            tokens,
            user_key,
            offline: false,
            cached,
        })
    }
}

impl Session<'_> {
    pub(crate) fn cached(&self) -> bool {
        self.cached.is_some()
    }

    pub fn storefront(&self) -> Result<String> {
        let cached = self.client.storefront.lock().unwrap().clone();
        if let Some((_, id)) = cached.filter(|(key, _)| Some(*key) == self.user_key) {
            return Ok(id);
        }
        if self.offline {
            return self.hint().ok_or_else(not_cached);
        }
        let found: Envelope<Item> = self.json("/v1/me/storefront", &[])?;
        let id = found.data.into_iter().next().map(|s| s.id);
        let id = id.filter(|id| segment(id));
        let id = id.ok_or_else(|| Error::Decode("no usable storefront".into()))?;
        if let (Some(c), Some(user)) = (&self.cached, &self.tokens.user) {
            c.store.remember_storefront(user, &id, c.epoch);
        }
        *self.client.storefront.lock().unwrap() = self.user_key.map(|key| (key, id.clone()));
        Ok(id)
    }

    fn hint(&self) -> Option<String> {
        let (c, user) = (self.cached.as_ref()?, self.tokens.user.as_ref()?);
        c.store.storefront(user)
    }

    pub fn search(&self, term: &str) -> Result<SearchResults> {
        let path = format!("/v1/catalog/{}/search", self.storefront()?);
        let query = [
            ("term", term),
            ("types", SEARCH_TYPES),
            ("limit", SEARCH_LIMIT),
        ];
        Ok(self.json::<Searched>(&path, &query)?.results)
    }

    pub fn album(&self, id: &str) -> Result<Detail> {
        self.detail("albums", id)
    }

    pub fn playlist(&self, id: &str) -> Result<Detail> {
        self.detail("playlists", id)
    }

    fn detail(&self, kind: &str, id: &str) -> Result<Detail> {
        let id = id_segment(id)?;
        let path = format!("/v1/catalog/{}/{kind}/{id}", self.storefront()?);
        let found: Envelope<WithTracks> = self.json(&path, &[])?;
        let first = found.data.into_iter().next();
        let found = first.ok_or_else(|| Error::Decode(format!("no {kind} in the response")))?;
        Ok(Detail {
            item: found.item,
            tracks: found.relationships.tracks,
        })
    }

    pub fn library(&self, shelf: Shelf) -> Result<Page> {
        let shelf = format!("{shelf:?}").to_lowercase();
        self.json(&format!("{LIBRARY}{shelf}"), &[("limit", LIBRARY_LIMIT)])
    }

    pub fn playlist_tracks(&self, id: &str) -> Result<Page> {
        self.related("playlists", id, "tracks")
    }

    pub fn library_artist(&self, id: &str) -> Result<Page> {
        self.json(&format!("{LIBRARY}artists/{}", id_segment(id)?), &[])
    }

    pub fn album_tracks(&self, id: &str) -> Result<Page> {
        self.related("albums", id, "tracks")
    }

    pub fn artist_albums(&self, id: &str) -> Result<Page> {
        self.related("artists", id, "albums")
    }

    pub fn catalog_artist_albums(&self, id: &str) -> Result<Page> {
        let path = format!(
            "/v1/catalog/{}/artists/{}/albums",
            self.storefront()?,
            id_segment(id)?
        );
        self.json(&path, &[("limit", CATALOG_ARTIST_LIMIT)])
    }

    pub fn recently_played(&self) -> Result<Page> {
        self.json("/v1/me/recent/played", &[])
    }

    fn related(&self, kind: &str, id: &str, relationship: &str) -> Result<Page> {
        let path = format!("{LIBRARY}{kind}/{}/{relationship}", id_segment(id)?);
        match self.json(&path, &[("limit", LIBRARY_LIMIT)]) {
            Err(Error::Http(404, why)) if why.strip_prefix(NO_RELATED) == Some(relationship) => {
                Ok(Page {
                    total: Some(0),
                    ..Page::default()
                })
            }
            page => page,
        }
    }

    pub fn next(&self, next: &str) -> Result<Page> {
        let unlimited = next.starts_with(LIBRARY) && !next.contains("limit=");
        let limit = [("limit", LIBRARY_LIMIT)];
        let query: &[(&str, &str)] = if unlimited { &limit } else { &[] };
        self.json(next, query)
    }

    pub fn all(&self, mut page: Page) -> Result<(Vec<Item>, bool)> {
        let mut items = std::mem::take(&mut page.items);
        for _ in 0..MAX_PAGES {
            let Some(next) = page.next.take() else {
                return Ok((items, false));
            };
            page = self.next(&next)?;
            items.append(&mut page.items);
        }
        Ok((items, page.next.is_some()))
    }

    fn json<T: DeserializeOwned>(&self, target: &str, query: &[(&str, &str)]) -> Result<T> {
        let body = self.get(target, query)?;
        serde_json::from_slice(&body).map_err(|e| {
            let at = format!("line {} column {}", e.line(), e.column());
            Error::Decode(format!("unreadable response at {at}"))
        })
    }

    fn get(&self, target: &str, query: &[(&str, &str)]) -> Result<Vec<u8>> {
        let path = target.split('?').next().unwrap_or_default();
        if !allowed(path) {
            return Err(Error::Refused(format!(
                "{path:?} is not an allowed API path"
            )));
        }
        let pairs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let request = format!("{target}?{}", pairs.join("&"));
        let cached = self.cached.as_ref().filter(|_| browse(path));
        if self.offline {
            return cached.and_then(|c| c.read(&request)).ok_or_else(not_cached);
        }
        let tokens = &self.tokens;
        let me = path.starts_with("/v1/me/");
        let user = match me {
            true => Some(tokens.user.as_deref().ok_or(Error::SignInNeeded)?),
            false => None,
        };
        let secrets = [
            tokens.developer.clone(),
            tokens.user.clone().unwrap_or_default(),
        ];
        let hide = |e: ureq::Error| match e {
            ureq::Error::BodyExceedsLimit(_) => Error::TooLarge,
            e => Error::Network(redact(&e.to_string(), &secrets)),
        };
        let (mut budget, mut retries) = (RETRY_BUDGET, 0);
        let bearer = format!("Bearer {}", tokens.developer);
        let send = |agent: &ureq::Agent| {
            let mut request = agent.get(format!("{}{target}", self.client.base));
            request = request.header("Authorization", &bearer);
            if let Some(user) = user {
                request = request.header("Music-User-Token", user);
            }
            for (key, value) in query {
                request = request.query(*key, *value);
            }
            request.call()
        };
        loop {
            let pooled = self.client.agent.lock().unwrap().clone();
            let called = send(&pooled).or_else(|_| {
                *self.client.agent.lock().unwrap() = agent(true);
                send(&agent(false))
            });
            let mut response = called.map_err(hide)?;
            let status = response.status().as_u16();
            let wait = retry_after(response.headers());
            let mut body = Vec::new();
            let reader = response
                .body_mut()
                .with_config()
                .limit(MAX_BODY_BYTES)
                .reader();
            reader
                .take(MAX_BODY_BYTES + 1)
                .read_to_end(&mut body)
                .map_err(|e| hide(ureq::Error::from(e)))?;
            if body.len() as u64 > MAX_BODY_BYTES {
                return Err(Error::TooLarge);
            }
            match status {
                200 => {
                    if let Some(c) = cached {
                        c.write(&request, &body);
                    }
                    return Ok(body);
                }
                401 | 403 => *self.client.storefront.lock().unwrap() = None,
                _ => {}
            }
            match status {
                401 => return Err(Error::DeveloperToken),
                403 if me => {
                    if let Some(c) = cached {
                        c.forget(&request);
                    }
                    return Err(Error::SignInNeeded);
                }
                429 if wait <= budget && retries < MAX_RETRIES => {
                    (budget, retries) = (budget - wait, retries + 1);
                    std::thread::sleep(wait);
                }
                429 => return Err(Error::RateLimited(wait.as_secs())),
                300..=399 => {
                    return Err(Error::Refused(format!(
                        "redirect (HTTP {status}) not followed"
                    )));
                }
                _ => return Err(Error::Http(status, redact(&apple_detail(&body), &secrets))),
            }
        }
    }
}

fn not_cached() -> Error {
    Error::Network("not cached".into())
}

fn agent(pooled: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(user_agent())
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(RECEIVE_RESPONSE))
        .max_idle_connections(if pooled { 10 } else { 0 })
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

pub fn retry_after(headers: &ureq::http::HeaderMap) -> Duration {
    let seconds = match headers.get("retry-after").map(|v| v.to_str()) {
        None => 1,
        Some(v) => v
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(UNREADABLE_RETRY_AFTER_S),
    };
    Duration::from_secs(seconds.min(MAX_RETRY_AFTER_S))
}

fn apple_detail(body: &[u8]) -> String {
    let body: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let detail = &body["errors"][0];
    let why = detail["detail"].as_str().or(detail["title"].as_str());
    why.unwrap_or("Apple Music returned an error").to_owned()
}

pub fn segment(s: &str) -> bool {
    let id_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    !s.is_empty() && s.chars().all(id_char) && s.chars().any(|c| c != '.')
}

fn id_segment(id: &str) -> Result<&str> {
    match segment(id) {
        true => Ok(id),
        false => Err(Error::Refused(format!("{id:?} is not an id"))),
    }
}

pub(crate) fn browse(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    matches!(
        parts[1..],
        [
            "v1",
            "me",
            "library",
            "playlists" | "albums" | "songs" | "artists"
        ] | ["v1", "me", "library", "artists", _]
            | ["v1", "me", "library", "playlists" | "albums", _, "tracks"]
            | ["v1", "me", "library", "artists", _, "albums"]
            | ["v1", "catalog", _, "artists", _, "albums"]
            | ["v1", "catalog", _, "albums" | "playlists", _]
            | ["v1", "catalog", _, "albums" | "playlists", _, "tracks"]
    )
}

pub(crate) fn allowed(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.first() != Some(&"") || !parts[1..].iter().all(|p| segment(p)) {
        return false;
    }
    matches!(
        parts[1..],
        ["v1", "me", "storefront"]
            | ["v1", "me", "library", "playlists" | "albums" | "songs"]
            | ["v1", "me", "library", "artists"]
            | ["v1", "me", "library", "artists", _]
            | ["v1", "me", "library", "playlists" | "albums", _, "tracks"]
            | ["v1", "me", "library", "artists", _, "albums"]
            | ["v1", "catalog", _, "search"]
            | ["v1", "me", "recent", "played"]
            | ["v1", "catalog", _, "artists", _, "albums"]
            | ["v1", "catalog", _, "albums" | "playlists", _]
            | ["v1", "catalog", _, "albums" | "playlists", _, "tracks"]
    )
}
