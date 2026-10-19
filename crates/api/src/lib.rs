mod client;
mod models;
pub mod store;
#[cfg(test)]
mod tests;
pub mod warm;

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(feature = "loopback")]
#[used]
static LOOPBACK_MARKER: &str = "bramley-unshipped-feature:api/loopback\0";

pub use client::{API_BASE, Client, Error, Result, Session, Shelf, Tokens, retry_after, segment};
pub use models::{Detail, Item, Kind, Page, SearchResults, art_url};

#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Storefront,
    Search(String),
    Album(String),
    Playlist(String),
    Library(Shelf),
    PlaylistTracks(String),
    AlbumTracks(String),
    ArtistAlbums(String),
    CatalogArtistAlbums(String),
    RecentlyPlayed,
    LibraryArtist(String),
    Next(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Storefront(String),
    Search(SearchResults),
    Detail(Detail),
    Page(Page),
}

pub type Done = Box<dyn FnOnce(Result<Reply>) + Send>;
pub type Refresh = Box<dyn FnOnce(Result<Reply>) + Send>;
const MAX_PENDING: usize = 8;

pub struct Job {
    pub call: Call,
    pub deadline: Instant,
    pub done: Done,
    pub supersedable: bool,
    pub refresh: Option<Refresh>,
}

impl Job {
    pub fn new(
        call: Call,
        deadline: Instant,
        done: impl FnOnce(Result<Reply>) + Send + 'static,
    ) -> Self {
        Job {
            call,
            deadline,
            done: Box::new(done),
            supersedable: false,
            refresh: None,
        }
    }

    pub fn revalidating(self, refresh: impl FnOnce(Result<Reply>) + Send + 'static) -> Self {
        Job {
            refresh: Some(Box::new(refresh)),
            ..self
        }
    }

    pub fn superseding(self) -> Self {
        Job {
            supersedable: true,
            ..self
        }
    }
}

impl Client {
    pub fn run(&self, call: Call) -> Result<Reply> {
        let api = self.session()?;
        match api.run(call.clone()) {
            Err(Error::DeveloperToken) if self.renew(&api.tokens.developer) => {
                self.session()?.run(call)
            }
            ran => ran,
        }
    }
}

impl Client {
    fn stale(&self, call: &Call) -> Option<Reply> {
        let mut api = self.session().ok().filter(|api| api.cached())?;
        api.offline = true;
        let found = api.run(call.clone()).ok();
        let counter = if found.is_some() {
            &store::API_HITS
        } else {
            &store::API_MISSES
        };
        counter.fetch_add(1, SeqCst);
        found
    }

    fn revalidate(&self, call: Call, stale: &Reply, refresh: Refresh) {
        store::API_REVALIDATIONS.fetch_add(1, SeqCst);
        match self.run(call) {
            Ok(fresh) if fresh != *stale => refresh(Ok(fresh)),
            Err(Error::SignInNeeded) => refresh(Err(Error::SignInNeeded)),
            _ => {}
        }
    }
}

struct Revalidation {
    call: Call,
    stale: Reply,
    refresh: Refresh,
    deadline: Instant,
}

struct Revalidator {
    tx: Sender<Revalidation>,
    pending: Arc<Mutex<Vec<Call>>>,
}

impl Revalidator {
    fn spawn(client: Client) -> std::io::Result<Self> {
        let (tx, rx) = channel::<Revalidation>();
        let pending: Arc<Mutex<Vec<Call>>> = Arc::default();
        let finished = pending.clone();
        let work = move || {
            for Revalidation {
                call,
                stale,
                refresh,
                deadline,
            } in rx
            {
                if Instant::now() < deadline {
                    let again = call.clone();
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        client.revalidate(again, &stale, refresh)
                    }));
                }
                let mut pending = finished.lock().unwrap_or_else(|e| e.into_inner());
                pending.retain(|queued| *queued != call);
            }
        };
        std::thread::Builder::new()
            .name("api-revalidate".into())
            .spawn(work)?;
        Ok(Revalidator { tx, pending })
    }

    fn submit(&self, job: Revalidation) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.len() < MAX_PENDING && !pending.contains(&job.call) {
            pending.push(job.call.clone());
            let _ = self.tx.send(job);
        }
    }
}

impl Session<'_> {
    fn run(&self, call: Call) -> Result<Reply> {
        Ok(match call {
            Call::Storefront => Reply::Storefront(self.storefront()?),
            Call::Search(term) => Reply::Search(self.search(&term)?),
            Call::Album(id) => Reply::Detail(self.album(&id)?),
            Call::Playlist(id) => Reply::Detail(self.playlist(&id)?),
            Call::Library(shelf) => Reply::Page(self.library(shelf)?),
            Call::PlaylistTracks(id) => Reply::Page(self.playlist_tracks(&id)?),
            Call::AlbumTracks(id) => Reply::Page(self.album_tracks(&id)?),
            Call::ArtistAlbums(id) => Reply::Page(self.artist_albums(&id)?),
            Call::CatalogArtistAlbums(id) => Reply::Page(self.catalog_artist_albums(&id)?),
            Call::RecentlyPlayed => Reply::Page(self.recently_played()?),
            Call::LibraryArtist(id) => Reply::Page(self.library_artist(&id)?),
            Call::Next(next) => Reply::Page(self.next(&next)?),
        })
    }
}

pub fn worker(client: Client) -> std::io::Result<Sender<Job>> {
    let revalidator = Revalidator::spawn(client.clone())?;
    let (tx, rx) = channel::<Job>();
    std::thread::Builder::new()
        .name("apple-api".into())
        .spawn(move || {
            let mut queue = VecDeque::new();
            while let Some(job) = queue.pop_front().or_else(|| rx.recv().ok()) {
                queue.extend(rx.try_iter());
                if job.supersedable && queue.iter().any(|newer| newer.supersedable) {
                    (job.done)(Err(Error::Superseded));
                    continue;
                }
                if Instant::now() >= job.deadline {
                    continue;
                }
                let Job {
                    call,
                    done,
                    refresh,
                    deadline,
                    ..
                } = job;
                let stale = refresh.and_then(|refresh| {
                    let found = catch_unwind(AssertUnwindSafe(|| client.stale(&call)));
                    found.ok().flatten().map(|stale| (stale, refresh))
                });
                if let Some((stale, refresh)) = stale {
                    done(Ok(stale.clone()));
                    revalidator.submit(Revalidation {
                        call,
                        stale,
                        refresh,
                        deadline,
                    });
                    continue;
                }
                let ran = catch_unwind(AssertUnwindSafe(|| client.run(call)));
                let panicked = || Err(Error::Decode("the API call panicked".into()));
                done(ran.unwrap_or_else(|_| panicked()));
            }
        })?;
    Ok(tx)
}
