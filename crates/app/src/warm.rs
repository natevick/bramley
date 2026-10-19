use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bramley_api::store::{Store, cache_dir, write_private};
use bramley_api::warm::{Art, Limiter, Report, Sizes, Stop, run};
use fastframe_instance::Slot;

use crate::covers::{self, COVER_PT, Refused, Tally};
use crate::credentials::{Credentials, keyring_locked};
use crate::devtoken::{self, DeveloperToken};
use crate::ui::library::{CELL_PT, ROW_PT};

#[cfg(test)]
mod tests;

const BUDGET: usize = 60;
const PACE: Duration = Duration::from_secs(1);
const SCALE_FILE: &str = "scale";
const LOCK_FILE: &str = "warm.lock";
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
        let saved = cache_dir()
            .map(|root| write_private(&root.join(SCALE_FILE), scale.to_string().as_bytes()));
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
        home: px(COVER_PT),
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
) -> impl FnMut(&str, u32) -> Art {
    let (mut agent, tally) = (covers::agent(), Tally::caching(Some(store.clone())));
    move |url, px| {
        let cached = store.age(&covers::art_key(url)).is_some();
        if cached || !covers::allowed_art(url, prefix.as_deref()) || limiter.art().is_err() {
            return Art::Skipped;
        }
        let Err(e) = covers::cover(&mut agent, url, px, &tally) else {
            return Art::Cached;
        };
        match e.downcast_ref::<Refused>() {
            Some(refused) => limiter.observe(refused.status),
            None if e.is::<ureq::Error>() => limiter.stop(Stop::Network),
            None => {}
        }
        Art::Skipped
    }
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
    start(user, slot)
}

pub(crate) fn start(user: Credentials, slot: Slot) -> Result<Report> {
    match user.load() {
        Ok(Some(_)) => {}
        Ok(None) => return idle(Stop::NoToken),
        Err(_) => return idle(Stop::Keyring),
    }
    let root = cache_dir().context("no cache directory")?;
    let (Some(art), Some(api)) = covers::open_caches() else {
        bail!("the cache directory is unusable");
    };
    let Some(_lock) = lock(&root)? else {
        return idle(Stop::Busy);
    };
    let halt = move || running(&slot);
    let limiter = Arc::new(Limiter::new(BUDGET, PACE, halt));
    let tokens = move || crate::api_tokens(&user);
    let client = bramley_api::Client::new(tokens)
        .caching(api)
        .warming(limiter.clone());
    let scale = recorded_scale(&root).map(sizes);
    Ok(run(
        &client,
        &limiter,
        scale,
        &mut fetcher(art, limiter.clone(), None),
    ))
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
