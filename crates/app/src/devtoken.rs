use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::musickit::Session;

pub const URL: &str = "https://natevick.github.io/bramley/developer-token.json";
const MAX_BYTES: u64 = 64 << 10;
const MAX_TOKEN: usize = 8 << 10;
const DAY: u64 = 86_400;
const RENEW_DAYS: u64 = 30;
const WARN_DAYS: u64 = 14;
const UNAVAILABLE: &str = "Bramley's developer token is unavailable";
const BACKOFF_S: [u64; 3] = [5, 30, 60];
const REFETCH_EVERY: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_secs(1);

struct Published {
    token: String,
    exp: u64,
}

#[derive(Default)]
struct Health {
    source: Option<&'static str>,
    days_left: Option<u64>,
    warning: Option<String>,
    error: Option<String>,
}

#[derive(Default)]
struct Attempts {
    failures: usize,
    due: Option<Instant>,
    cause: Option<String>,
    refetched: Option<Instant>,
}

#[derive(Clone)]
pub struct DeveloperToken {
    url: Option<String>,
    cache: PathBuf,
    config: PathBuf,
    agent: ureq::Agent,
    attempts: Arc<Mutex<Attempts>>,
    health: Arc<Mutex<Health>>,
    musickit: Option<Arc<Session>>,
}

fn now() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |d| d.as_secs())
}

fn parse(body: &[u8]) -> Option<Published> {
    let found: serde_json::Value = serde_json::from_slice(body).ok()?;
    let token = found["token"].as_str()?;
    let exp = found["exp"].as_u64().filter(|exp| *exp > now())?;
    let usable = token.len() <= MAX_TOKEN && token.bytes().all(|b| b.is_ascii_graphic());
    (usable && !token.is_empty()).then(|| Published {
        token: token.to_owned(),
        exp,
    })
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl DeveloperToken {
    pub fn new(url: Option<String>, cache_dir: &Path, config: &Path) -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(bramley_engine::user_agent())
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .into();
        DeveloperToken {
            url,
            cache: cache_dir.join("developer-token.json"),
            config: config.to_owned(),
            agent,
            attempts: Arc::default(),
            health: Arc::default(),
            musickit: None,
        }
    }

    pub fn system(url: String, config: &Path) -> Self {
        let cache = bramley_api::store::cache_dir().unwrap_or_else(|| config.to_owned());
        let developer = DeveloperToken::new(Some(url), &cache, config);
        #[cfg(target_os = "macos")]
        let developer = developer.with_musickit(Arc::new(Session::system()));
        developer
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn with_musickit(mut self, session: Arc<Session>) -> Self {
        self.musickit = Some(session);
        self
    }

    pub fn musickit(&self) -> Option<&Arc<Session>> {
        self.musickit.as_ref()
    }

    fn cached(&self) -> Option<Published> {
        parse(&std::fs::read(&self.cache).ok()?)
    }

    fn fetch(&self, url: &str) -> Result<Published, String> {
        let host = url.parse::<ureq::http::Uri>().ok();
        let host = host.as_ref().and_then(|u| u.host()).unwrap_or(url);
        let unreachable = |why: String| format!("couldn't reach {host}: {why}");
        let mut response = self.agent.get(url).call();
        let response = response.as_mut().map_err(|e| unreachable(e.to_string()))?;
        match response.status().as_u16() {
            200 => {}
            status => return Err(unreachable(format!("HTTP {status}"))),
        }
        let body = response.body_mut().with_config().limit(MAX_BYTES);
        let body = body.read_to_vec().map_err(|e| e.to_string())?;
        let published = parse(&body).ok_or("the published token is malformed or expired")?;
        let dir = self.cache.parent().unwrap_or(Path::new("."));
        let text = String::from_utf8_lossy(&body);
        std::fs::create_dir_all(dir)
            .and_then(|()| bramley_api::store::write_shared(&self.cache, text.as_bytes()))
            .map_err(|e| format!("the token cache could not be written: {e}"))?;
        Ok(published)
    }

    fn note(&self, source: &'static str, found: &Published, failed: Option<&str>) -> String {
        let days = found.exp.saturating_sub(now()) / DAY;
        let warning = match failed {
            Some(why) => Some(format!(
                "Bramley's developer token could not be renewed ({why}); {days} days left"
            )),
            None => (days < WARN_DAYS)
                .then(|| format!("Bramley's developer token expires in {days} days")),
        };
        *lock(&self.health) = Health {
            source: Some(source),
            days_left: Some(days),
            warning,
            error: None,
        };
        found.token.clone()
    }

    pub fn get(&self) -> Result<String, String> {
        if let Some(session) = &self.musickit {
            return session.developer();
        }
        let mut attempts = lock(&self.attempts);
        let cached = self.cached();
        let renew_at = now() + RENEW_DAYS * DAY;
        if let Some(fresh) = cached.as_ref().filter(|c| c.exp >= renew_at) {
            return Ok(self.note("cache", fresh, None));
        }
        let due = attempts.due.is_none_or(|due| Instant::now() >= due);
        let failed = match &self.url {
            Some(url) if due => {
                let fetched = self.fetch(url);
                let failures = fetched.is_err().then_some(attempts.failures + 1);
                let step = BACKOFF_S[failures.unwrap_or(BACKOFF_S.len()).min(BACKOFF_S.len()) - 1];
                attempts.due = Some(Instant::now() + Duration::from_secs(step));
                attempts.failures = failures.unwrap_or(0);
                match fetched {
                    Ok(fetched) => {
                        attempts.cause = None;
                        return Ok(self.note("fetched", &fetched, None));
                    }
                    Err(why) => attempts.cause.insert(why).clone(),
                }
            }
            Some(_) => attempts.cause.clone().unwrap_or_default(),
            None => "no address to fetch it from".into(),
        };
        if let Some(stale) = cached {
            return Ok(self.note("cache", &stale, Some(&failed)));
        }
        if let Ok(token) = crate::read_token(&self.config, "developer-token") {
            *lock(&self.health) = Health {
                source: Some("config"),
                ..Health::default()
            };
            return Ok(token);
        }
        let why = format!("{UNAVAILABLE}: {failed}");
        lock(&self.health).error = Some(why.clone());
        Err(why)
    }

    pub fn wait(&self, cancelled: impl Fn() -> bool) -> Result<String, String> {
        loop {
            let got = self.get();
            if got.is_ok() || cancelled() {
                return got;
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn peek(&self) -> Option<String> {
        if let Some(session) = &self.musickit {
            return session.peek();
        }
        let cached = self.cached().map(|c| c.token);
        cached.or_else(|| crate::read_token(&self.config, "developer-token").ok())
    }

    pub fn refetch(&self, rejected: &str) -> bool {
        if let Some(session) = &self.musickit {
            return session.renew(rejected);
        }
        let mut attempts = lock(&self.attempts);
        if self.cached().is_some_and(|c| c.token != rejected) {
            return true;
        }
        let recent = attempts
            .refetched
            .is_some_and(|at| at.elapsed() < REFETCH_EVERY);
        let Some(url) = self.url.as_ref().filter(|_| !recent) else {
            return false;
        };
        attempts.refetched = Some(Instant::now());
        match self.fetch(url) {
            Ok(fetched) => self.note("fetched", &fetched, None) != rejected,
            Err(_) => false,
        }
    }

    pub fn status(&self) -> serde_json::Value {
        if self.musickit.is_some() {
            return serde_json::json!({"devTokenSource": "musickit"});
        }
        let health = lock(&self.health);
        serde_json::json!({
            "devTokenDaysLeft": health.days_left,
            "devTokenSource": health.source,
            "devTokenWarning": health.warning,
        })
    }

    pub fn warning(&self) -> Option<String> {
        lock(&self.health).warning.clone()
    }

    pub fn missing(&self) -> Option<String> {
        if let Some(session) = &self.musickit {
            return session.failure();
        }
        lock(&self.health).error.clone()
    }
}
