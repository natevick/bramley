use std::cmp::Reverse;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::client::{Client, Error, Session, Shelf};
use crate::models::{Item, Kind, Page, art_url};

const REFRESH: Duration = Duration::from_secs(6 * 3600);
const LISTS: Duration = Duration::from_secs(30 * 60);
const VISIBLE: usize = 24;
const RESERVE: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Stop {
    #[default]
    Complete,
    Budget,
    RateLimited,
    ServerError,
    Refused,
    AppRunning,
    Network,
    Backoff,
    NoToken,
    Keyring,
    Busy,
}

impl Stop {
    pub fn name(self) -> &'static str {
        match self {
            Stop::Complete => "complete",
            Stop::Budget => "budget",
            Stop::RateLimited => "rate-limited",
            Stop::ServerError => "server-error",
            Stop::Refused => "refused",
            Stop::AppRunning => "app-running",
            Stop::Network => "network",
            Stop::Backoff => "backoff",
            Stop::NoToken => "no-token",
            Stop::Keyring => "keyring",
            Stop::Busy => "busy",
        }
    }
}

struct Spent {
    left: usize,
    requests: usize,
    last: Option<Instant>,
    stop: Option<Stop>,
    wait: Duration,
}

pub struct Limiter {
    spent: Mutex<Spent>,
    pace: Duration,
    halt: Box<dyn Fn() -> bool + Send + Sync>,
}

impl Limiter {
    pub fn new(
        budget: usize,
        pace: Duration,
        halt: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        let spent = Spent {
            left: budget,
            requests: 0,
            last: None,
            stop: None,
            wait: Duration::ZERO,
        };
        Limiter {
            spent: Mutex::new(spent),
            pace,
            halt: Box::new(halt),
        }
    }

    fn spent(&self) -> std::sync::MutexGuard<'_, Spent> {
        self.spent.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn halted(&self, spent: &mut Spent) -> Result<(), Stop> {
        let halt = spent
            .stop
            .or_else(|| (self.halt)().then_some(Stop::AppRunning));
        match halt {
            Some(stop) => Err(*spent.stop.insert(stop)),
            None => Ok(()),
        }
    }

    pub fn check(&self) -> Result<(), Stop> {
        self.halted(&mut self.spent())
    }

    pub fn spend(&self, paced: bool) -> Result<(), Stop> {
        let mut spent = self.spent();
        if let Some(last) = spent.last.filter(|_| paced) {
            std::thread::sleep(self.pace.saturating_sub(last.elapsed()));
        }
        self.halted(&mut spent)?;
        if spent.left == 0 {
            return Err(*spent.stop.insert(Stop::Budget));
        }
        spent.left -= 1;
        spent.requests += 1;
        if paced {
            spent.last = Some(Instant::now());
        }
        Ok(())
    }

    pub fn observe(&self, status: u16, wait: Duration) {
        match status {
            429 => self.stop(Stop::RateLimited),
            500..=599 => self.stop(Stop::ServerError),
            401 | 403 => self.stop(Stop::Refused),
            _ => return,
        }
        self.spent().wait = wait;
    }

    pub fn stop(&self, stop: Stop) {
        self.spent().stop.get_or_insert(stop);
    }

    pub fn stopped(&self) -> Option<Stop> {
        self.spent().stop
    }

    pub fn requests(&self) -> usize {
        self.spent().requests
    }

    pub fn retry_after(&self) -> Duration {
        self.spent().wait
    }

    fn remaining(&self) -> usize {
        self.spent().left
    }
}

#[derive(Clone, Copy)]
pub struct Sizes {
    pub cell: u32,
    pub row: u32,
    pub home: u32,
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub requests: usize,
    pub skipped: usize,
    pub stop: Stop,
}

impl Report {
    pub fn line(&self) -> String {
        let Report {
            requests,
            skipped,
            stop,
        } = self;
        let stop = stop.name();
        format!("requests={requests} skipped={skipped} stopped={stop}")
    }
}

type Fetch<'a> = &'a mut dyn FnMut(&str, u32);

struct Pass<'a> {
    limiter: &'a Limiter,
    sizes: Option<Sizes>,
    art: Fetch<'a>,
    seen: HashSet<String>,
    skipped: usize,
}

pub fn run(client: &Client, limiter: &Limiter, sizes: Option<Sizes>, art: Fetch) -> Report {
    let mut pass = Pass {
        limiter,
        sizes,
        art,
        seen: HashSet::new(),
        skipped: 0,
    };
    let ended = pass.all(client);
    Report {
        requests: limiter.requests(),
        skipped: pass.skipped,
        stop: ended.err().unwrap_or_default(),
    }
}

impl Pass<'_> {
    fn call<T>(&mut self, call: impl FnOnce() -> Result<T, Error>) -> Result<Option<T>, Stop> {
        match call() {
            Ok(found) => Ok(Some(found)),
            Err(Error::Network(_) | Error::Config(_)) => {
                Err(self.limiter.stopped().unwrap_or(Stop::Network))
            }
            Err(_) => {
                self.limiter.check()?;
                self.skipped += 1;
                Ok(None)
            }
        }
    }

    fn every(&mut self, api: &Session, first: Option<Page>) -> Result<Vec<Item>, Stop> {
        let Some(first) = first else {
            return Ok(Vec::new());
        };
        let found = self.call(|| api.all(first))?;
        Ok(found.map(|(items, _)| items).unwrap_or_default())
    }

    fn all(&mut self, client: &Client) -> Result<(), Stop> {
        let mut api = client.session().map_err(|_| Stop::Network)?;
        api.reuse = Some(LISTS);
        let albums = self.call(|| api.library(Shelf::Albums))?;
        self.call(|| api.library(Shelf::Artists))?;
        let songs = self.call(|| api.library(Shelf::Songs))?.unwrap_or_default();
        let playlists = self.call(|| api.library(Shelf::Playlists))?;
        let albums = self.every(&api, albums)?;
        let playlists = self.every(&api, playlists)?;
        api.reuse = None;
        let owned =
            |item: &&Item| matches!(item.kind, Kind::LibraryAlbums | Kind::LibraryPlaylists);
        let mut units: Vec<(&Item, Option<Duration>)> = albums
            .iter()
            .chain(&playlists)
            .filter(owned)
            .map(|item| (item, api.tracks_age(tracks_of(item.kind), &item.id)))
            .collect();
        units.retain(|(_, age)| age.is_none_or(|age| age >= REFRESH));
        units.sort_by_key(|(_, age)| Reverse(age.unwrap_or(Duration::MAX)));
        for (item, _) in units {
            self.limiter.check()?;
            if self.limiter.remaining() < RESERVE {
                return Err(Stop::Budget);
            }
            let page = match item.kind {
                Kind::LibraryAlbums => self.call(|| api.album_tracks(&item.id))?,
                _ => self.call(|| api.playlist_tracks(&item.id))?,
            };
            if let Some(page) = page {
                self.covers(&page.items, |sizes| sizes.row)?;
            }
        }
        self.covers(&albums, |sizes| sizes.cell)?;
        self.covers(&albums, |sizes| sizes.home)?;
        self.covers(&playlists, |sizes| sizes.row)?;
        self.covers(&songs.items, |sizes| sizes.row)
    }

    fn covers(&mut self, items: &[Item], size: impl Fn(Sizes) -> u32) -> Result<(), Stop> {
        let Some(px) = self.sizes.map(size) else {
            return Ok(());
        };
        for template in items
            .iter()
            .take(VISIBLE)
            .filter_map(|i| i.artwork.as_deref())
        {
            self.limiter.check()?;
            if let Some(url) = art_url(template, px).filter(|url| self.seen.insert(url.clone())) {
                (self.art)(&url, px);
            }
        }
        self.limiter.check()
    }
}

fn tracks_of(kind: Kind) -> &'static str {
    match kind {
        Kind::LibraryAlbums => "albums",
        _ => "playlists",
    }
}
