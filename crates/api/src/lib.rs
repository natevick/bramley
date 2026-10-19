mod client;
mod models;
pub mod store;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::{Sender, channel};
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
pub type Refresh = Box<dyn FnOnce(Reply) + Send>;

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

    pub fn revalidating(self, refresh: impl FnOnce(Reply) + Send + 'static) -> Self {
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
        if let Ok(fresh) = self.run(call)
            && fresh != *stale
        {
            refresh(fresh);
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
                    ..
                } = job;
                if let Some(refresh) = refresh
                    && let Some(stale) = client.stale(&call)
                {
                    done(Ok(stale.clone()));
                    let client = client.clone();
                    let revalidate = move || client.revalidate(call, &stale, refresh);
                    let _ = std::thread::Builder::new()
                        .name("api-revalidate".into())
                        .spawn(revalidate);
                    continue;
                }
                let ran = catch_unwind(AssertUnwindSafe(|| client.run(call)));
                let panicked = || Err(Error::Decode("the API call panicked".into()));
                done(ran.unwrap_or_else(|_| panicked()));
            }
        })?;
    Ok(tx)
}
