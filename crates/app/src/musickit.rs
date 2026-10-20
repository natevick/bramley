use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bramley_engine::Tokens;

const HELD_FOR: Duration = Duration::from_secs(3600);
const RENEW_EVERY: Duration = Duration::from_secs(60);
pub const NOT_AUTHORIZED: &str = "Apple Music access is not authorized for Bramley";
const STATUS_UNAVAILABLE: &str = "macOS did not report Bramley's Apple Music access";
const TOKENS_UNAVAILABLE: &str = "Apple Music did not give Bramley its tokens";
const DENIED: &str = "Apple Music access is denied for Bramley; allow it in System Settings > Privacy & Security > Media & Apple Music";
const RESTRICTED: &str = "Apple Music access is restricted on this Mac";
const UNANSWERED: &str = "Apple Music did not answer Bramley's sign-in request";

type Authorization = Box<dyn Fn(bool) -> Result<String, String> + Send + Sync>;
type Fetch = Box<dyn Fn(bool) -> Result<Tokens, String> + Send + Sync>;

#[derive(Clone)]
struct Pair {
    developer: String,
    user: String,
}

#[derive(Default)]
struct Held {
    pair: Option<(Instant, Pair)>,
    renewed: Option<Instant>,
    failure: Option<String>,
}

pub struct Session {
    authorization: Authorization,
    fetch: Fetch,
    held: Mutex<Held>,
}

fn lock(held: &Mutex<Held>) -> MutexGuard<'_, Held> {
    held.lock().unwrap_or_else(|e| e.into_inner())
}

fn refusal(status: &str) -> Option<&'static str> {
    match status {
        "authorized" => None,
        "denied" => Some(DENIED),
        "restricted" => Some(RESTRICTED),
        _ => Some(UNANSWERED),
    }
}

impl Session {
    #[cfg(any(test, target_os = "macos"))]
    pub fn new(
        authorization: impl Fn(bool) -> Result<String, String> + Send + Sync + 'static,
        fetch: impl Fn(bool) -> Result<Tokens, String> + Send + Sync + 'static,
    ) -> Self {
        Session {
            authorization: Box::new(authorization),
            fetch: Box::new(fetch),
            held: Mutex::default(),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn system() -> Self {
        Session::new(
            |ask| bramley_macos_player::authorization(ask).map_err(|e| e.to_string()),
            |fresh| bramley_macos_player::tokens(fresh).map_err(|e| e.to_string()),
        )
    }

    pub fn authorized(&self) -> Result<bool, &'static str> {
        let status = (self.authorization)(false).map_err(|_| STATUS_UNAVAILABLE)?;
        Ok(refusal(&status).is_none())
    }

    pub fn request(&self) -> Result<bool, &'static str> {
        let status = (self.authorization)(true).map_err(|_| UNANSWERED)?;
        lock(&self.held).pair = None;
        refusal(&status).map_or(Ok(true), Err)
    }

    pub fn forget(&self) {
        lock(&self.held).pair = None;
    }

    fn pair(&self, fresh: bool) -> Result<Pair, String> {
        if !self.authorized()? {
            return Err(NOT_AUTHORIZED.into());
        }
        let held = lock(&self.held).pair.clone();
        let usable = held.filter(|(at, _)| !fresh && at.elapsed() < HELD_FOR);
        if let Some((_, pair)) = usable {
            return Ok(pair);
        }
        let fetched = (self.fetch)(fresh);
        let mut held = lock(&self.held);
        held.failure = fetched.as_ref().err().cloned();
        let tokens = fetched?;
        let pair = Pair {
            developer: tokens.developer,
            user: tokens.user,
        };
        held.pair = Some((Instant::now(), pair.clone()));
        Ok(pair)
    }

    pub fn developer(&self) -> Result<String, String> {
        self.pair(false).map(|pair| pair.developer)
    }

    pub fn user(&self) -> Result<Option<String>, &'static str> {
        if !self.authorized()? {
            return Ok(None);
        }
        let pair = self.pair(false).map_err(|_| TOKENS_UNAVAILABLE)?;
        Ok(Some(pair.user))
    }

    pub fn peek(&self) -> Option<String> {
        let held = lock(&self.held);
        held.pair.as_ref().map(|(_, pair)| pair.developer.clone())
    }

    pub fn failure(&self) -> Option<String> {
        lock(&self.held).failure.clone()
    }

    pub fn renew(&self, rejected: &str) -> bool {
        {
            let mut held = lock(&self.held);
            let newer = held
                .pair
                .as_ref()
                .is_some_and(|(_, p)| p.developer != rejected);
            let recent = held.renewed.is_some_and(|at| at.elapsed() < RENEW_EVERY);
            if newer || recent {
                return newer;
            }
            held.renewed = Some(Instant::now());
        }
        self.pair(true).is_ok_and(|pair| pair.developer != rejected)
    }
}

#[cfg(test)]
mod tests;
