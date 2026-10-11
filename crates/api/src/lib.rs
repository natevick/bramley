mod client;
mod models;
#[cfg(test)]
mod tests;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Sender, channel};
use std::time::Instant;

pub use client::{API_BASE, Client, Error, Result, Session, Shelf, Tokens};
pub use models::{Detail, Item, Kind, Page, SearchResults, art_url};

#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Storefront,
    Search(String),
    Album(String),
    Playlist(String),
    Library(Shelf),
    PlaylistTracks(String),
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

pub struct Job {
    pub call: Call,
    pub deadline: Instant,
    pub done: Done,
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
        }
    }
}

impl Client {
    pub fn run(&self, call: Call) -> Result<Reply> {
        let api = self.session()?;
        Ok(match call {
            Call::Storefront => Reply::Storefront(api.storefront()?),
            Call::Search(term) => Reply::Search(api.search(&term)?),
            Call::Album(id) => Reply::Detail(api.album(&id)?),
            Call::Playlist(id) => Reply::Detail(api.playlist(&id)?),
            Call::Library(shelf) => Reply::Page(api.library(shelf)?),
            Call::PlaylistTracks(id) => Reply::Page(api.playlist_tracks(&id)?),
            Call::Next(next) => Reply::Page(api.next(&next)?),
        })
    }
}

pub fn worker(client: Client) -> std::io::Result<Sender<Job>> {
    let (tx, rx) = channel::<Job>();
    std::thread::Builder::new()
        .name("apple-api".into())
        .spawn(move || {
            for job in rx.into_iter().filter(|job| Instant::now() < job.deadline) {
                let ran = catch_unwind(AssertUnwindSafe(|| client.run(job.call)));
                let panicked = || Err(Error::Decode("the API call panicked".into()));
                (job.done)(ran.unwrap_or_else(|_| panicked()));
            }
        })?;
    Ok(tx)
}
