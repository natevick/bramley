use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use bramley_api::{Call, Item, Job, Reply, Shelf};
use bramley_engine::Queue;
use egui::{ColorImage, TextureHandle, WidgetType};
use serde_json::{Value, json};

use super::labeled;
use super::nav::{Nav, Route};
use crate::covers::{self, Tally, allowed_art, art_url};
use crate::playback::Action;

#[cfg(test)]
mod tests;

pub const ROW_PT: f32 = 40.0;
const CELL_PT: f32 = 140.0;
pub const MARGIN: usize = 20;
pub const ART_CAP: usize = 96;
pub const SONGS_WINDOW: usize = 100;
pub const DEADLINE: Duration = Duration::from_secs(15);
const RETRY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Playlists,
    Albums,
    Songs,
    Tracks,
}

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
}

impl Paged {
    fn call(&self, list: List, playlist: &str) -> Option<Call> {
        if let Some(next) = &self.next {
            return Some(Call::Next(next.clone()));
        }
        (self.pages == 0).then(|| match list {
            List::Playlists => Call::Library(Shelf::Playlists),
            List::Albums => Call::Library(Shelf::Albums),
            List::Songs => Call::Library(Shelf::Songs),
            List::Tracks => Call::PlaylistTracks(playlist.to_owned()),
        })
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

#[derive(Default)]
pub struct Library {
    lists: [Paged; 4],
    playlist: String,
    sent: u64,
    mail: Mailbox,
    pub art: Art,
    built: (usize, usize, usize),
    #[cfg(feature = "dev")]
    pub scroll_end: Option<Instant>,
    #[cfg(feature = "dev")]
    pub click_last: bool,
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

    pub fn reset(&mut self) {
        self.lists = Default::default();
        self.art.clear();
    }

    pub fn tick(
        &mut self,
        now: Instant,
        signed_in: bool,
        wake: impl Fn() + Clone + Send + 'static,
    ) -> Vec<Job> {
        if !signed_in {
            if self
                .lists
                .iter()
                .any(|p| p.pages > 0 || p.busy_until.is_some())
            {
                self.reset();
            }
            return Vec::new();
        }
        let mut jobs = Vec::new();
        for list in [List::Playlists, List::Albums, List::Songs, List::Tracks] {
            let paged = &mut self.lists[list as usize];
            let idle = paged.wanted && paged.busy_until.is_none_or(|until| now >= until);
            let Some(call) = paged.call(list, &self.playlist).filter(|_| idle) else {
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
                    (paged.next, paged.busy_until, paged.failed) = (page.next, None, false);
                    paged.wanted = false;
                    paged.total = page.total.or(paged.total);
                    paged.pages += 1;
                }
                _ => {
                    (paged.busy_until, paged.failed) = (Some(Instant::now() + RETRY), true);
                    ctx.request_repaint_after(RETRY);
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
        let [playlists, albums, songs, tracks] = self.lists.each_ref().map(list);
        let (built, built_max, shown_to) = self.built;
        json!({ "playlists": playlists, "albums": albums, "songs": songs, "tracks": tracks,
                "rowsBuilt": built, "rowsBuiltMax": built_max, "shownTo": shown_to,
                "artTextures": self.art.textures.len(), "artDecoded": self.art.decoded })
    }
}

pub fn play(list: List, items: &[Item], index: usize) -> Option<Action> {
    let item = items.get(index)?;
    Some(match list {
        List::Playlists => return None,
        List::Albums => Action::PlayIn(Queue::Album(item.id.clone()), 0),
        List::Songs | List::Tracks => {
            let from = index.saturating_sub(SONGS_WINDOW / 2);
            let window = &items[from..items.len().min(from + SONGS_WINDOW)];
            let ids = window.iter().map(|song| song.id.clone()).collect();
            Action::PlayIn(Queue::Songs(ids), index - from)
        }
    })
}

type Want = (String, String, u32);
type Queued = Arc<(Mutex<Vec<Want>>, Condvar)>;

#[derive(Default)]
pub struct Art {
    prefix: Option<String>,
    textures: HashMap<String, (TextureHandle, u64)>,
    frame: u64,
    wanted: Vec<Want>,
    recent: HashSet<String>,
    queue: Option<(Queued, Receiver<(String, ColorImage)>)>,
    decoded: usize,
}

impl Art {
    fn clear(&mut self) {
        (self.textures, self.wanted, self.recent) = Default::default();
        if let Some((queued, _)) = &self.queue {
            queued.0.lock().unwrap().clear();
        }
    }

    fn get(&mut self, ctx: &egui::Context, item: &Item, pt: f32) -> Option<TextureHandle> {
        if let Some((texture, used)) = self.textures.get_mut(&item.id) {
            *used = self.frame;
            return Some(texture.clone());
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
        self.recent = self.wanted.iter().map(|(key, ..)| key.clone()).collect();
        if self.wanted.is_empty() && self.queue.is_none() {
            return;
        }
        let (queued, _) = self.queue.get_or_insert_with(|| fetcher(ctx.clone()));
        let mut jobs = queued.0.lock().unwrap();
        *jobs = std::mem::take(&mut self.wanted);
        queued.1.notify_one();
    }

    fn receive(&mut self, ctx: &egui::Context) -> bool {
        let Some((_, rx)) = &self.queue else {
            return false;
        };
        let decoded: Vec<_> = rx.try_iter().collect();
        let any = !decoded.is_empty();
        self.decoded += decoded.len();
        for (key, image) in decoded {
            if self.recent.contains(&key) {
                let texture = ctx.load_texture(format!("library-{key}"), image, Default::default());
                self.insert(key, texture);
            }
        }
        any
    }

    pub fn insert(&mut self, key: String, texture: TextureHandle) {
        self.textures.insert(key, (texture, self.frame));
        while self.textures.len() > ART_CAP {
            let oldest = self.textures.iter().min_by_key(|(_, (_, used))| *used);
            let oldest = oldest
                .map(|(key, _)| key.clone())
                .expect("a texture to evict");
            self.textures.remove(&oldest);
        }
    }
}

fn fetcher(waker: egui::Context) -> (Queued, Receiver<(String, ColorImage)>) {
    let (queued, (tx, rx)) = (Queued::default(), channel());
    let jobs = queued.clone();
    let fetch = move || {
        let (agent, tally) = (covers::agent(), Tally::default());
        loop {
            let waiting = jobs.1.wait_while(jobs.0.lock().unwrap(), |q| q.is_empty());
            let (key, url, px) = waiting.unwrap().pop().expect("a queued cover");
            let Ok(image) = covers::cover(&agent, &url, px, &tally) else {
                continue;
            };
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
) -> bool {
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
            .frame(false)
            .truncate();
        ui.add_sized([width, pt + 4.0], button)
    };
    labeled(response, WidgetType::Button, &format!("{verb} {text}")).clicked()
}

pub fn show(
    ui: &mut egui::Ui,
    lib: &mut Library,
    route: &Route,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    let (list, verb) = match route {
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
    if lib.list(list).failed {
        super::notice(ui, "Your library could not load; retrying.");
    }
    let albums = list == List::Albums;
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
    #[allow(unused_mut)]
    let mut area = egui::ScrollArea::vertical()
        .id_salt(list as usize)
        .auto_shrink(false);
    #[cfg(feature = "dev")]
    if lib.scroll_end.is_some_and(|at| Instant::now() >= at) {
        let rows = loaded.div_ceil(columns) as f32;
        area = area.vertical_scroll_offset(rows * (row_pt + ui.spacing().item_spacing.y));
        ui.ctx().request_repaint();
    }
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
                    if tile(ui, &mut lib.art, item, (albums, width), verb) {
                        hit = Some(index);
                    }
                }
            });
        }
        lib.built = (built, lib.built.1.max(built), range.end);
    });
    #[cfg(feature = "dev")]
    if lib.click_last && lib.list(list).next.is_none() && lib.built.2 == loaded && loaded > 0 {
        (lib.click_last, hit) = (false, Some(loaded - 1));
    }
    let items = &lib.list(list).items;
    match hit {
        Some(index) if list == List::Playlists => {
            navs.push(Nav::Go(Route::Playlist(items[index].id.clone())))
        }
        Some(index) => actions.extend(play(list, items, index)),
        None => {}
    }
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
                    egui::Button::selectable(*current == open, name),
                )
                .clicked()
            {
                navs.push(Nav::Go(open));
            }
        }
    });
}
