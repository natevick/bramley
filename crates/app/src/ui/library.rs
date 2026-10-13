use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use bramley_api::{Call, Error, Item, Job, Reply, Shelf};
use bramley_engine::{Queue, Snapshot};
use egui::{ColorImage, TextureHandle, WidgetType};
use serde_json::{Value, json};

use super::labeled;
use super::nav::{Edge, Nav, Route};
use crate::covers::{self, Tally, allowed_art, art_url};
use crate::playback::Action;

#[cfg(test)]
mod tests;

pub const ROW_PT: f32 = 40.0;
const CELL_PT: f32 = 140.0;
pub const MARGIN: usize = 20;
pub const ART_CAP: usize = 96;
pub const QUEUE_AHEAD: usize = 100;
pub const TOP_UP_AT: usize = 10;
pub const QUEUE_CAP: usize = 1000;
pub const TOP_UP_BATCH: usize = 16;
pub const TOP_UP_RETRIES: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(60),
];
pub const DEADLINE: Duration = Duration::from_secs(15);
pub const RETRY: Duration = Duration::from_secs(5);
pub const ART_RETRY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Playlists,
    Albums,
    Songs,
    Tracks,
    Artists,
    ArtistAlbums,
    ArtistSongs,
    ArtistName,
}

const LISTS: [List; 8] = [
    List::Playlists,
    List::Albums,
    List::Songs,
    List::Tracks,
    List::Artists,
    List::ArtistAlbums,
    List::ArtistSongs,
    List::ArtistName,
];

#[derive(Default)]
pub struct Paged {
    pub items: Vec<Item>,
    next: Option<String>,
    total: Option<u32>,
    pages: usize,
    busy_until: Option<Instant>,
    asked: u64,
    wanted: bool,
    failed: bool,
    read: usize,
    skipped: usize,
}

impl Paged {
    fn call(&self, list: List, owner: Option<String>) -> Option<Call> {
        if let Some(next) = &self.next {
            return Some(Call::Next(next.clone()));
        }
        match list {
            List::ArtistSongs => owner.map(Call::AlbumTracks),
            _ if self.pages > 0 => None,
            List::Playlists => Some(Call::Library(Shelf::Playlists)),
            List::Albums => Some(Call::Library(Shelf::Albums)),
            List::Songs => Some(Call::Library(Shelf::Songs)),
            List::Artists => Some(Call::Library(Shelf::Artists)),
            List::Tracks => owner.map(Call::PlaylistTracks),
            List::ArtistAlbums => owner.map(Call::ArtistAlbums),
            List::ArtistName => owner.map(Call::LibraryArtist),
        }
    }

    fn idle(&self, now: Instant) -> bool {
        self.wanted && self.busy_until.is_none_or(|until| now >= until)
    }

    fn complete(&self) -> bool {
        self.pages > 0 && self.next.is_none()
    }
}

type Found = (u64, List, bramley_api::Result<Reply>);

struct Mailbox(Sender<Found>, Receiver<Found>);

impl Default for Mailbox {
    fn default() -> Self {
        let (tx, rx) = channel();
        Mailbox(tx, rx)
    }
}

struct Follow {
    list: List,
    owner: String,
    next: usize,
    batch: Vec<String>,
    previous: Vec<String>,
    generation: Option<u64>,
    tries: usize,
    sent: Option<Instant>,
}

#[derive(Default)]
pub struct Library {
    lists: [Paged; 8],
    follow: Option<Follow>,
    pub top_up_failed: bool,
    playlist: String,
    artist: String,
    sent: u64,
    mail: Mailbox,
    pub art: Art,
    built: (usize, usize, usize),
    pub denied: bool,
    #[cfg(feature = "dev")]
    pub click_last: bool,
    #[cfg(feature = "dev")]
    pub click_row: Option<usize>,
}

impl Library {
    pub fn new(prefix: Option<String>) -> Self {
        let art = Art {
            prefix,
            ..Art::default()
        };
        Library {
            art,
            ..Library::default()
        }
    }

    pub fn list(&self, list: List) -> &Paged {
        &self.lists[list as usize]
    }

    fn owner(&self, list: List) -> &str {
        match list {
            List::Tracks => &self.playlist,
            List::ArtistAlbums | List::ArtistSongs | List::ArtistName => &self.artist,
            _ => "",
        }
    }

    fn owner_for(&self, list: List) -> Option<String> {
        match list {
            List::ArtistSongs => self.unread_album(),
            _ => Some(self.owner(list).to_owned()),
        }
    }

    fn due(&self, list: List, now: Instant) -> bool {
        let paged = self.list(list);
        paged.idle(now) && paged.call(list, self.owner_for(list)).is_some()
    }

    fn more(&self, list: List) -> bool {
        match list {
            List::ArtistSongs => {
                !self.list(List::ArtistAlbums).complete()
                    || self.unread_album().is_some()
                    || self.list(list).next.is_some()
            }
            _ => !self.list(list).complete(),
        }
    }

    fn unread_album(&self) -> Option<String> {
        let albums = self.list(List::ArtistAlbums);
        let read = self.list(List::ArtistSongs).read;
        let album = albums.items.get(read).filter(|_| albums.complete());
        album.map(|album| album.id.clone())
    }

    pub fn reset(&mut self) {
        self.lists = Default::default();
        self.follow = None;
        self.denied = false;
        self.art.clear();
    }

    pub fn tick(
        &mut self,
        now: Instant,
        signed_in: bool,
        wake: impl Fn() + Clone + Send + 'static,
    ) -> Vec<Job> {
        if !signed_in {
            let used = self
                .lists
                .iter()
                .any(|p| p.pages > 0 || p.busy_until.is_some());
            if used || self.denied {
                self.reset();
            }
            return Vec::new();
        }
        if self.denied {
            return Vec::new();
        }
        let mut jobs = Vec::new();
        for list in LISTS {
            let owner = self.owner_for(list);
            let paged = &mut self.lists[list as usize];
            let Some(call) = paged.call(list, owner).filter(|_| paged.idle(now)) else {
                continue;
            };
            self.sent += 1;
            (paged.wanted, paged.busy_until, paged.asked) =
                (false, Some(now + DEADLINE), self.sent);
            let (tx, wake, asked) = (self.mail.0.clone(), wake.clone(), self.sent);
            jobs.push(Job::new(call, now + DEADLINE, move |found| {
                let _ = tx.send((asked, list, found));
                wake();
            }));
        }
        jobs
    }

    pub fn receive(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        while let Ok((asked, list, found)) = self.mail.1.try_recv() {
            any = true;
            let paged = &mut self.lists[list as usize];
            if paged.asked != asked {
                continue;
            }
            paged.asked = 0;
            match found {
                Ok(Reply::Page(mut page)) => {
                    paged.items.append(&mut page.items);
                    if list == List::ArtistSongs {
                        paged.read += usize::from(page.next.is_none());
                    } else {
                        paged.total = page.total.or(paged.total);
                    }
                    (paged.next, paged.busy_until, paged.failed) = (page.next, None, false);
                    paged.wanted = false;
                    paged.pages += 1;
                }
                Err(Error::SignInNeeded) => {
                    (paged.busy_until, paged.failed, self.denied) = (None, true, true);
                }
                Err(e) if list == List::ArtistSongs && permanent(&e) => {
                    (paged.next, paged.busy_until) = (None, None);
                    (paged.read, paged.skipped) = (paged.read + 1, paged.skipped + 1);
                }
                failed => {
                    let wait = match failed {
                        Err(Error::RateLimited(s)) => RETRY.max(Duration::from_secs(s)),
                        _ => RETRY,
                    };
                    (paged.busy_until, paged.failed) = (Some(Instant::now() + wait), true);
                    ctx.request_repaint_after(wait);
                }
            }
        }
        self.art.receive(ctx) | any
    }

    pub fn status(&self) -> Value {
        let list = |p: &Paged| {
            json!({ "loaded": p.items.len(), "total": p.total, "pages": p.pages,
                    "more": p.next.is_some(), "failed": p.failed })
        };
        let [
            playlists,
            albums,
            songs,
            tracks,
            artists,
            artist_albums,
            mut artist_songs,
            _,
        ] = self.lists.each_ref().map(list);
        artist_songs["skipped"] = json!(self.list(List::ArtistSongs).skipped);
        artist_songs["more"] = json!(self.more(List::ArtistSongs));
        let (built, built_max, shown_to) = self.built;
        json!({ "playlists": playlists, "albums": albums, "songs": songs, "tracks": tracks,
                "artists": artists, "artistAlbums": artist_albums, "artistSongs": artist_songs,
                "rowsBuilt": built, "rowsBuiltMax": built_max, "shownTo": shown_to,
                "artTextures": self.art.textures.len(), "artDecoded": self.art.decoded })
    }
}

impl Library {
    pub(crate) fn play_from(&mut self, list: List, index: usize) -> Option<Action> {
        let action = play(list, &self.list(list).items, index);
        self.follow = match &action {
            Some(Action::PlayIn(Queue::Songs(ids), _)) => Some(Follow {
                list,
                owner: self.owner(list).to_owned(),
                next: index + ids.len(),
                batch: ids.clone(),
                previous: Vec::new(),
                generation: None,
                tries: 0,
                sent: None,
            }),
            _ => None,
        };
        action
    }

    pub fn started(&mut self, generation: u64) {
        if let Some(follow) = self.follow.as_mut() {
            follow.generation.get_or_insert(generation);
        }
    }

    pub fn top_up(
        &mut self,
        snap: &Snapshot,
        (generation, settled): (u64, bool),
        now: Instant,
    ) -> Option<Action> {
        let follow = self.follow.as_ref()?;
        let moved = follow.owner != self.owner(follow.list);
        let follow = self.follow.as_mut()?;
        if follow.generation != Some(generation) {
            if follow.generation.is_some() {
                self.follow = None;
            }
            return None;
        }
        let at = snap.queue_position?;
        let (length, tail) = (snap.queue_length, &snap.queue.last()?.id);
        let ends = at + snap.queue.len() == length && length <= at + 1 + TOP_UP_AT;
        if !ends || !settled {
            return None;
        }
        if follow.previous.contains(tail) {
            let Some(wait) = TOP_UP_RETRIES.get(follow.tries) else {
                (self.follow, self.top_up_failed) = (None, true);
                return None;
            };
            if follow.sent.is_some_and(|sent| now < sent + *wait) {
                return None;
            }
            (follow.tries, follow.sent) = (follow.tries + 1, Some(now));
            return Some(Action::TopUp(follow.batch.clone()));
        }
        if !follow.batch.contains(tail) {
            return None;
        }
        let paged = &mut self.lists[follow.list as usize];
        let cursor = paged.items.get(follow.next - 1).map(|item| &item.id);
        let room = QUEUE_CAP.saturating_sub(length);
        let end = paged.items.len().min(follow.next + TOP_UP_BATCH.min(room));
        if moved || cursor != follow.batch.last() || room == 0 {
            self.follow = None;
            return None;
        }
        if end == follow.next {
            paged.wanted = true;
            return None;
        }
        let batch = paged.items[follow.next..end]
            .iter()
            .map(|item| item.id.clone());
        follow.previous = std::mem::replace(&mut follow.batch, batch.collect());
        (follow.next, follow.tries, follow.sent) = (end, 0, Some(now));
        Some(Action::TopUp(follow.batch.clone()))
    }
}

fn permanent(e: &Error) -> bool {
    match e {
        Error::Http(status, _) => (400..500).contains(status) && *status != 429,
        Error::Decode(_) | Error::Refused(_) | Error::TooLarge => true,
        _ => false,
    }
}

pub fn play(list: List, items: &[Item], index: usize) -> Option<Action> {
    let item = items.get(index)?;
    Some(match list {
        List::Playlists | List::Artists | List::ArtistName => return None,
        List::Albums | List::ArtistAlbums => Action::PlayIn(Queue::Album(item.id.clone()), 0),
        List::Songs | List::Tracks | List::ArtistSongs => {
            let ahead = &items[index..items.len().min(index + QUEUE_AHEAD)];
            let ids = ahead.iter().map(|song| song.id.clone()).collect();
            Action::PlayIn(Queue::Songs(ids), 0)
        }
    })
}

type Want = (String, String, u32);
type Queued = Arc<(Mutex<(Vec<Want>, HashSet<String>)>, Condvar)>;
type Decoded = (String, Option<ColorImage>);

#[derive(Default)]
pub struct Art {
    prefix: Option<String>,
    textures: HashMap<String, (TextureHandle, u64)>,
    frame: u64,
    wanted: Vec<Want>,
    recent: HashSet<String>,
    failed: HashMap<String, Instant>,
    drawn: usize,
    shown: usize,
    queue: Option<(Queued, Receiver<Decoded>)>,
    decoded: usize,
}

impl Art {
    fn clear(&mut self) {
        (self.textures, self.wanted, self.recent, self.failed) = Default::default();
        if let Some((queued, _)) = &self.queue {
            queued.0.lock().unwrap().0.clear();
        }
    }

    fn get(&mut self, ctx: &egui::Context, item: &Item, pt: f32) -> Option<TextureHandle> {
        self.drawn += 1;
        if let Some((texture, used)) = self.textures.get_mut(&item.id) {
            *used = self.frame;
            return Some(texture.clone());
        }
        if self
            .failed
            .get(&item.id)
            .is_some_and(|until| Instant::now() < *until)
        {
            return None;
        }
        let px = covers::px(pt, ctx.pixels_per_point());
        let url = art_url(item.artwork.as_deref()?, px)?;
        if allowed_art(&url, self.prefix.as_deref()) {
            self.wanted.push((item.id.clone(), url, px));
        }
        None
    }

    pub fn flush(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        self.shown = std::mem::take(&mut self.drawn);
        self.recent = self.wanted.iter().map(|(key, ..)| key.clone()).collect();
        if self.wanted.is_empty() && self.queue.is_none() {
            return;
        }
        let (queued, _) = self.queue.get_or_insert_with(|| fetcher(ctx.clone()));
        let mut jobs = queued.0.lock().unwrap();
        let (queue, busy) = &mut *jobs;
        *queue = std::mem::take(&mut self.wanted);
        queue.retain(|(key, ..)| !busy.contains(key));
        queued.1.notify_one();
    }

    fn receive(&mut self, ctx: &egui::Context) -> bool {
        let Some((queued, rx)) = &self.queue else {
            return false;
        };
        let decoded: Vec<_> = rx.try_iter().collect();
        let mut jobs = queued.0.lock().unwrap();
        jobs.1
            .retain(|key| !decoded.iter().any(|(done, _)| done == key));
        drop(jobs);
        let any = !decoded.is_empty();
        self.decoded += decoded.len();
        for (key, image) in decoded {
            let Some(image) = image else {
                self.failed.insert(key, Instant::now() + ART_RETRY);
                continue;
            };
            if self.recent.contains(&key) {
                let texture = ctx.load_texture(format!("library-{key}"), image, Default::default());
                self.insert(key, texture);
            }
        }
        any
    }

    pub fn insert(&mut self, key: String, texture: TextureHandle) {
        self.textures.insert(key, (texture, self.frame));
        let cap = ART_CAP.max(self.shown * 3 / 2);
        while self.textures.len() > cap {
            let oldest = self.textures.iter().min_by_key(|(_, (_, used))| *used);
            let (oldest, used) = oldest
                .map(|(key, (_, used))| (key.clone(), *used))
                .expect("a texture");
            if used + 1 >= self.frame {
                break;
            }
            self.textures.remove(&oldest);
        }
    }
}

fn fetcher(waker: egui::Context) -> (Queued, Receiver<Decoded>) {
    let (queued, (tx, rx)) = (Queued::default(), channel());
    let jobs = queued.clone();
    let fetch = move || {
        let (mut agent, tally) = (covers::agent(), Tally::default());
        loop {
            let mut waiting = jobs
                .1
                .wait_while(jobs.0.lock().unwrap(), |q| q.0.is_empty())
                .unwrap();
            let (key, url, px) = waiting.0.pop().expect("a queued cover");
            waiting.1.insert(key.clone());
            drop(waiting);
            let image = covers::cover(&mut agent, &url, px, &tally).ok();
            if tx.send((key, image)).is_err() {
                return;
            }
            waker.request_repaint();
        }
    };
    let _ = std::thread::Builder::new()
        .name("library-art".into())
        .spawn(fetch);
    (queued, rx)
}

fn tile(
    ui: &mut egui::Ui,
    art: &mut Art,
    item: &Item,
    (cell, width): (bool, f32),
    verb: &str,
) -> egui::Response {
    let pt = if cell { CELL_PT } else { ROW_PT };
    let size = egui::vec2(pt, pt);
    let blank = egui::Rect::from_min_max(egui::epaint::WHITE_UV, egui::epaint::WHITE_UV);
    let image = match art.get(ui.ctx(), item, pt) {
        Some(texture) => egui::Image::new(&texture),
        None => egui::Image::new((egui::TextureId::default(), size))
            .uv(blank)
            .tint(ui.visuals().faint_bg_color),
    }
    .fit_to_exact_size(size);
    let name = item.name.as_deref().unwrap_or("Untitled");
    let text = match (&item.artist, cell) {
        (Some(artist), false) => format!("{name} · {artist}"),
        _ => name.to_owned(),
    };
    let response = if cell {
        let shown = ui.vertical(|ui| {
            let button = ui.add(egui::Button::image(image).frame(false));
            ui.add_sized(
                [pt, 16.0],
                egui::Label::new(egui::RichText::new(&text).small()).truncate(),
            );
            button
        });
        shown.inner
    } else {
        let button = egui::Button::image_and_text(image, text.as_str())
            .right_text("")
            .frame(false)
            .truncate();
        ui.add_sized([width, pt + 4.0], button)
    };
    labeled(response, WidgetType::Button, &format!("{verb} {text}"))
}

pub fn show(
    ui: &mut egui::Ui,
    lib: &mut Library,
    (route, scroll): (&Route, &mut Option<Edge>),
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    lib.built.0 = 0;
    let (list, verb) = match route {
        Route::Artists => (List::Artists, "Open artist"),
        Route::Artist(id) => return artist(ui, lib, (id, scroll), (actions, navs)),
        Route::Playlists => (List::Playlists, "Open playlist"),
        Route::Albums => (List::Albums, "Play album"),
        Route::Songs => (List::Songs, "Play"),
        Route::Playlist(id) if lib.playlist != *id => {
            lib.playlist.clone_from(id);
            lib.lists[List::Tracks as usize] = Paged::default();
            (List::Tracks, "Play")
        }
        _ => (List::Tracks, "Play"),
    };
    view(
        ui,
        lib,
        (list, verb),
        (f32::INFINITY, scroll),
        (actions, navs),
    );
}

fn artist(
    ui: &mut egui::Ui,
    lib: &mut Library,
    (id, scroll): (&str, &mut Option<Edge>),
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    if lib.artist != id {
        lib.artist = id.to_owned();
        lib.lists[List::ArtistAlbums as usize] = Paged::default();
        lib.lists[List::ArtistSongs as usize] = Paged::default();
        lib.lists[List::ArtistName as usize] = Paged::default();
    }
    let listed = lib.list(List::Artists).items.iter().find(|a| a.id == id);
    let named = listed.or(lib.list(List::ArtistName).items.first());
    match named.and_then(|a| a.name.as_deref()) {
        Some(name) => drop(ui.label(egui::RichText::new(name).heading())),
        None => lib.lists[List::ArtistName as usize].wanted = true,
    }
    let skipped = lib.list(List::ArtistSongs).skipped;
    if skipped > 0 {
        let albums = if skipped == 1 { "album" } else { "albums" };
        super::notice(ui, &format!("{skipped} {albums} couldn't be read."));
    }
    let albums_pt = 2.0 * (CELL_PT + 30.0);
    view(
        ui,
        lib,
        (List::ArtistAlbums, "Play album"),
        (albums_pt, &mut None),
        (actions, navs),
    );
    ui.separator();
    view(
        ui,
        lib,
        (List::ArtistSongs, "Play"),
        (f32::INFINITY, scroll),
        (actions, navs),
    );
    if lib.list(List::ArtistSongs).wanted && !lib.list(List::ArtistAlbums).complete() {
        lib.lists[List::ArtistAlbums as usize].wanted = true;
    }
}

fn view(
    ui: &mut egui::Ui,
    lib: &mut Library,
    (list, verb): (List, &str),
    (max_height, scroll): (f32, &mut Option<Edge>),
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    if lib.list(list).failed {
        super::notice(ui, "Your library could not load; retrying.");
    }
    let albums = matches!(list, List::Albums | List::ArtistAlbums);
    let columns = if albums {
        ((ui.available_width() / (CELL_PT + 8.0)) as usize).max(1)
    } else {
        1
    };
    let (row_pt, width) = if albums {
        (CELL_PT + 22.0, CELL_PT)
    } else {
        (ROW_PT + 4.0, ui.available_width())
    };
    let loaded = lib.list(list).items.len();
    let mut area = egui::ScrollArea::vertical()
        .id_salt(list as usize)
        .max_height(max_height)
        .auto_shrink([false, max_height.is_finite()]);
    if ui.input(|i| i.smooth_scroll_delta.y != 0.0) {
        *scroll = None;
    }
    let end = loaded.div_ceil(columns) as f32 * (row_pt + ui.spacing().item_spacing.y);
    match *scroll {
        Some(Edge::Top) => (area, *scroll) = (area.vertical_scroll_offset(0.0), None),
        Some(Edge::End) => {
            area = area.vertical_scroll_offset(end);
            if !lib.more(list) && lib.built.2 == loaded {
                *scroll = None;
            }
        }
        None => {}
    }
    #[cfg(feature = "dev")]
    let hooked = list != List::ArtistAlbums;
    let mut hit = None;
    area.show_rows(ui, row_pt, loaded.div_ceil(columns), |ui, rows| {
        let range = rows.start * columns..(rows.end * columns).min(loaded);
        lib.lists[list as usize].wanted |= range.end + MARGIN >= loaded;
        let mut built = 0;
        for row in rows {
            ui.horizontal(|ui| {
                for index in (row * columns..(row + 1) * columns).filter(|i| *i < loaded) {
                    let item = &lib.lists[list as usize].items[index];
                    built += 1;
                    let response = match list {
                        List::Artists => name_row(ui, item, width, verb),
                        _ => tile(ui, &mut lib.art, item, (albums, width), verb),
                    };
                    if response.clicked() {
                        hit = Some(index);
                    }
                    if matches!(list, List::Songs | List::Tracks | List::ArtistSongs) {
                        super::track_menu(&response, &item.id, actions);
                    }
                }
            });
        }
        let total = lib.built.0 + built;
        lib.built = (total, lib.built.1.max(total), range.end);
    });
    if scroll.is_some() && lib.due(list, Instant::now()) {
        ui.ctx().request_repaint();
    }
    #[cfg(feature = "dev")]
    if hooked
        && lib.click_last
        && lib.list(list).next.is_none()
        && lib.built.2 == loaded
        && loaded > 0
    {
        (lib.click_last, hit) = (false, Some(loaded - 1));
    }
    #[cfg(feature = "dev")]
    if let Some(row) = lib.click_row.filter(|row| *row < loaded)
        && hooked
        && !matches!(list, List::Playlists | List::Artists)
    {
        (lib.click_row, hit) = (None, Some(row));
    }
    let items = &lib.list(list).items;
    match hit {
        Some(index) if list == List::Playlists => {
            navs.push(Nav::Go(Route::Playlist(items[index].id.clone())))
        }
        Some(index) if list == List::Artists => {
            navs.push(Nav::Go(Route::Artist(items[index].id.clone())))
        }
        Some(index) => actions.extend(lib.play_from(list, index)),
        None => {}
    }
}

fn name_row(ui: &mut egui::Ui, item: &Item, width: f32, verb: &str) -> egui::Response {
    let name = item.name.as_deref().unwrap_or("Untitled");
    let row = egui::Button::new(name)
        .right_text("")
        .frame(false)
        .truncate();
    let response = ui.add(row.min_size(egui::vec2(width, ROW_PT + 4.0)));
    labeled(response, WidgetType::Button, &format!("{verb} {name}"))
}

pub fn sidebar(ui: &mut egui::Ui, lib: &mut Library, current: &Route, navs: &mut Vec<Nav>) {
    let height = (ui.available_height() - 90.0).max(28.0);
    let loaded = lib.list(List::Playlists).items.len();
    let area = egui::ScrollArea::vertical()
        .id_salt("sidebar-playlists")
        .max_height(height);
    area.show_rows(ui, 24.0, loaded, |ui, rows| {
        lib.lists[List::Playlists as usize].wanted |= rows.end + MARGIN >= loaded;
        for item in &lib.list(List::Playlists).items[rows] {
            let open = Route::Playlist(item.id.clone());
            let name = item.name.as_deref().unwrap_or("Untitled");
            if ui
                .add_sized(
                    [ui.available_width(), 24.0],
                    egui::Button::selectable(*current == open, name).truncate(),
                )
                .clicked()
            {
                navs.push(Nav::Go(open));
            }
        }
    });
}
