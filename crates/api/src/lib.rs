mod client;
mod models;
#[cfg(test)]
mod tests;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Sender, channel};

pub use client::{API_BASE, Client, Error, Result, Shelf, Tokens};
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
    pub done: Done,
}

impl Job {
    pub fn new(call: Call, done: impl FnOnce(Result<Reply>) + Send + 'static) -> Self {
        Job {
            call,
            done: Box::new(done),
        }
    }
}

impl Client {
    pub fn run(&self, call: Call) -> Result<Reply> {
        Ok(match call {
            Call::Storefront => Reply::Storefront(self.storefront()?),
            Call::Search(term) => Reply::Search(self.search(&term)?),
            Call::Album(id) => Reply::Detail(self.album(&id)?),
            Call::Playlist(id) => Reply::Detail(self.playlist(&id)?),
            Call::Library(shelf) => Reply::Page(self.library(shelf)?),
            Call::PlaylistTracks(id) => Reply::Page(self.playlist_tracks(&id)?),
            Call::Next(next) => Reply::Page(self.next(&next)?),
        })
    }
}

pub fn worker(client: Client) -> Sender<Job> {
    let (tx, rx) = channel::<Job>();
    std::thread::Builder::new()
        .name("apple-api".into())
        .spawn(move || {
            for job in rx {
                let ran = catch_unwind(AssertUnwindSafe(|| client.run(job.call)));
                (job.done)(
                    ran.unwrap_or_else(|_| Err(Error::Decode("the API call panicked".into()))),
                );
            }
        })
        .expect("spawn apple-api thread");
    tx
}
