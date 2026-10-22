use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use bramley_api::store::{API_CAP, ART_CAP, Store, cache_dir, write_private};
use bramley_api::warm::{Limiter, Report, Sizes, Stop, run};
use fastframe_instance::Slot;

use crate::covers::{self, Refused, Tally};
use crate::credentials::{Credentials, keyring_locked};
use crate::devtoken::{self, DeveloperToken};
use crate::ui::library::{CELL_PT, ROW_PT};

#[cfg(test)]
mod tests;

const BUDGET: usize = 60;
const PACE: Duration = Duration::from_secs(1);
const SCALE_FILE: &str = "scale";
const LOCK_FILE: &str = "warm.lock";
const STATE_FILE: &str = "warm.state";
const BACKOFF_FIRST_MS: u64 = 5 * 60_000;
const BACKOFF_MAX_MS: u64 = 60 * 60_000;
static LAST_SCALE: AtomicU32 = AtomicU32::new(0);

#[cfg(unix)]
pub fn running(slot: &Slot) -> bool {
    std::os::unix::net::UnixStream::connect(slot.dir().join(crate::CHANNEL_FILE)).is_ok()
}

#[cfg(not(unix))]
pub fn running(_: &Slot) -> bool {
    true
}

pub fn remember_scale(scale: f32) {
    if LAST_SCALE.swap(scale.to_bits(), Ordering::Relaxed) != scale.to_bits() {
        let saved =
            cache_dir().map(|root| std::fs::write(root.join(SCALE_FILE), scale.to_string()));
        drop(saved);
    }
}

pub(crate) fn recorded_scale(root: &Path) -> Option<f32> {
    let text = std::fs::read_to_string(root.join(SCALE_FILE)).ok()?;
    text.trim()
        .parse()
        .ok()
        .filter(|scale: &f32| scale.is_finite() && *scale > 0.0)
}

pub(crate) fn sizes(scale: f32) -> Sizes {
    let px = |points| covers::px(points, scale);
    Sizes {
        cell: px(CELL_PT),
        row: px(ROW_PT),
        home: covers::home_px(scale),
    }
}

pub(crate) fn lock(root: &Path) -> Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options
        .open(root.join(LOCK_FILE))
        .context("the warm lock")?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e).context("the warm lock"),
    }
}

pub fn fetcher(
    store: Arc<Store>,
    limiter: Arc<Limiter>,
    prefix: Option<String>,
) -> impl FnMut(&str, u32) {
    let (mut agent, tally) = (
        covers::agent(),
        Tally::warming(store.clone(), limiter.clone()),
    );
    move |url, px| {
        let cached = store.age(&covers::art_key(url)).is_some();
        if cached || !covers::allowed_art(url, prefix.as_deref()) || limiter.spend(false).is_err() {
            return;
        }
        let Err(e) = covers::cover(&mut agent, url, px, &tally) else {
            return;
        };
        match e.downcast_ref::<Refused>() {
            Some(refused) => limiter.observe(refused.status, refused.wait),
            None if e.is::<ureq::Error>() => limiter.stop(Stop::Network),
            None => {}
        }
    }
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn state(root: &Path) -> Option<(String, u64, u32)> {
    let text = std::fs::read_to_string(root.join(STATE_FILE)).ok()?;
    let mut words = text.split_whitespace();
    let kind = words.next()?.to_owned();
    Some((
        kind,
        words.next()?.parse().ok()?,
        words.next()?.parse().ok()?,
    ))
}

pub(crate) fn paused(root: &Path, now: u64) -> Option<Stop> {
    let (kind, at, _) = state(root)?;
    match kind.as_str() {
        "refused" => {
            let started = std::fs::metadata(root.join(SCALE_FILE)).ok();
            let started = started.and_then(|meta| meta.modified().ok()).map(millis);
            started
                .is_none_or(|started| started <= at)
                .then_some(Stop::Refused)
        }
        "backoff" => (now < at).then_some(Stop::Backoff),
        _ => None,
    }
}

pub(crate) fn record(root: &Path, report: &Report, wait: Duration, now: u64) {
    let file = root.join(STATE_FILE);
    let failures = state(root)
        .filter(|(kind, ..)| kind == "backoff")
        .map_or(0, |s| s.2);
    let line = match report.stop {
        Stop::Refused => format!("refused {now} 0"),
        Stop::RateLimited | Stop::ServerError => {
            let failures = failures + 1;
            let delay = BACKOFF_FIRST_MS << (failures - 1).min(10);
            let delay = delay.min(BACKOFF_MAX_MS).max(wait.as_millis() as u64);
            format!("backoff {} {failures}", now + delay)
        }
        Stop::Complete | Stop::Budget => {
            drop(std::fs::remove_file(file));
            return;
        }
        _ => return,
    };
    drop(write_private(&file, line.as_bytes()));
}

fn idle(stop: Stop) -> Result<Report> {
    Ok(Report {
        stop,
        ..Report::default()
    })
}

fn warm(args: &[String], slot: Result<Slot>) -> Result<Report> {
    if args != ["warm"] {
        bail!("usage: bramley warm");
    }
    let slot = slot?;
    if running(&slot) {
        return idle(Stop::AppRunning);
    }
    let config = crate::config_dir()?;
    let developer = DeveloperToken::system(devtoken::URL.into(), &config);
    let user = Credentials::system(&config, developer);
    if keyring_locked() {
        return idle(Stop::Keyring);
    }
    start(user, slot, cache_dir().context("no cache directory")?)
}

pub(crate) fn start(user: Credentials, slot: Slot, root: PathBuf) -> Result<Report> {
    match user.load() {
        Ok(Some(_)) => {}
        Ok(None) => return idle(Stop::NoToken),
        Err(_) => return idle(Stop::Keyring),
    }
    let (art, api) = (
        Store::open(&root, "art", ART_CAP)?,
        Store::open(&root, "api", API_CAP)?,
    );
    let Some(_lock) = lock(&root)? else {
        return idle(Stop::Busy);
    };
    let now = millis(SystemTime::now());
    if let Some(stop) = paused(&root, now) {
        return idle(stop);
    }
    let halt = move || running(&slot);
    let limiter = Arc::new(Limiter::new(BUDGET, PACE, halt));
    let tokens = move || crate::api_tokens(&user);
    let client = bramley_api::Client::new(tokens)
        .caching(Arc::new(api))
        .warming(limiter.clone());
    let scale = recorded_scale(&root).map(sizes);
    let mut art = fetcher(Arc::new(art), limiter.clone(), None);
    let report = run(&client, &limiter, scale, &mut art);
    record(&root, &report, limiter.retry_after(), now);
    Ok(report)
}

pub fn command(args: &[String], slot: Result<Slot>) -> i32 {
    match warm(args, slot) {
        Ok(report) => {
            println!("bramley warm: {}", report.line());
            0
        }
        Err(e) => {
            eprintln!("bramley warm: {e:#}");
            1
        }
    }
}
