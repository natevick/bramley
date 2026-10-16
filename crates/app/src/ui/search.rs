use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bramley_api::{Call, Error, Item, Job, Kind, Reply, SearchResults};
use egui::{Color32, ColorImage, Key, RichText, TextureHandle, WidgetType};
use serde_json::{Value, json};

use super::labeled;
use super::nav::{Nav, Route, Shell};
use crate::covers::{self, Tally, allowed_art, art_url};
use crate::playback::Action;

pub const DEBOUNCE: Duration = Duration::from_millis(250);
pub const DEADLINE: Duration = Duration::from_secs(10);
const ART_PT: f32 = 40.0;
const RECENT_PT: f32 = 96.0;

type Found = (u64, bramley_api::Result<Reply>);
type Fetched = (u64, String, ColorImage);
type Wanted = (u64, String, String, u32);

#[derive(Debug, PartialEq)]
pub enum State {
    Idle,
    Loading(Instant),
    Empty,
    Results(SearchResults),
    Failed(String, String),
}

#[derive(Debug, PartialEq)]
enum Recent {
    Unasked,
    Asked,
    Albums(Vec<Item>),
}

pub struct Search {
    pending: Option<(String, Instant, Option<Instant>)>,
    seq: u64,
    pub(super) state: State,
    asked: Option<Instant>,
    took: Option<Duration>,
    tx: Sender<Found>,
    rx: Receiver<Found>,
    pub(super) art: Art,
    recent: Recent,
    recent_tx: Sender<bramley_api::Result<Reply>>,
    recent_rx: Receiver<bramley_api::Result<Reply>>,
    recent_art: Art,
    #[cfg(feature = "dev")]
    pub click_first_song: bool,
    #[cfg(feature = "dev")]
    pub click_first_playlist: bool,
    #[cfg(feature = "dev")]
    pub typed_once_signed_in: Option<String>,
}

impl Search {
    pub fn new(art_prefix: Option<String>) -> Self {
        let (tx, rx) = channel();
        let (recent_tx, recent_rx) = channel();
        Search {
            pending: None,
            seq: 0,
            state: State::Idle,
            asked: None,
            took: None,
            tx,
            rx,
            recent: Recent::Unasked,
            recent_tx,
            recent_rx,
            recent_art: Art::new(art_prefix.clone(), RECENT_PT),
            art: Art::new(art_prefix, ART_PT),
            #[cfg(feature = "dev")]
            click_first_song: false,
            #[cfg(feature = "dev")]
            click_first_playlist: false,
            #[cfg(feature = "dev")]
            typed_once_signed_in: None,
        }
    }

    pub fn edited(&mut self, text: &str, now: Instant) {
        self.pending = Some((text.trim().to_owned(), now + DEBOUNCE, None));
    }

    pub fn enter(&mut self, text: &str, now: Instant) {
        self.pending = Some((text.trim().to_owned(), now, None));
    }

    pub fn enter_by(&mut self, text: &str, now: Instant, limit: Instant) {
        self.pending = Some((text.trim().to_owned(), now, Some(limit)));
    }

    pub fn wait(&self, now: Instant) -> Option<Duration> {
        let (_, due, _) = self.pending.as_ref()?;
        Some(due.saturating_duration_since(now))
    }

    pub fn tick(
        &mut self,
        now: Instant,
        signed_in: bool,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Job> {
        if !signed_in {
            self.pending = None;
            self.recent = Recent::Unasked;
            if self.state != State::Idle {
                self.replace(State::Idle);
            }
            return None;
        }
        #[cfg(feature = "dev")]
        if let Some(term) = self.typed_once_signed_in.take() {
            self.enter(&term, now);
        }
        if matches!(self.state, State::Loading(deadline) if now > deadline) {
            self.replace(State::Failed(
                "the search timed out".into(),
                "Timeout".into(),
            ));
        }
        let (term, _, limit) = self.pending.take_if(|(_, due, _)| *due <= now)?;
        if term.is_empty() {
            self.replace(State::Idle);
            return None;
        }
        self.replace(State::Loading(now + DEADLINE));
        (self.asked, self.took) = (Some(now), None);
        let (tx, seq) = (self.tx.clone(), self.seq);
        let deadline = limit.map_or(now + DEADLINE, |limit| limit.min(now + DEADLINE));
        let job = Job::new(Call::Search(term), deadline, move |found| {
            let _ = tx.send((seq, found));
            wake();
        });
        Some(job.superseding())
    }

    pub fn recent_job(
        &mut self,
        now: Instant,
        wanted: bool,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Job> {
        if !wanted || self.recent != Recent::Unasked {
            return None;
        }
        self.recent = Recent::Asked;
        let tx = self.recent_tx.clone();
        Some(Job::new(
            Call::RecentlyPlayed,
            now + DEADLINE,
            move |found| {
                let _ = tx.send(found);
                wake();
            },
        ))
    }

    fn replace(&mut self, state: State) {
        self.seq += 1;
        self.state = state;
        self.art.clear();
    }

    pub fn receive(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        for (seq, found) in self.rx.try_iter() {
            any = true;
            if seq != self.seq {
                continue;
            }
            self.took = self.asked.map(|at| at.elapsed());
            self.state = match found {
                Ok(Reply::Search(r)) if sections(&r).is_empty() => State::Empty,
                Ok(Reply::Search(r)) => State::Results(r),
                Ok(_) => State::Failed("the search could not run".into(), "Reply".into()),
                Err(e) => State::Failed(cause(&e), variant(&e)),
            };
        }
        for found in self.recent_rx.try_iter() {
            any = true;
            if let (Recent::Asked, Ok(Reply::Page(page))) = (&self.recent, found) {
                let album = |item: &Item| matches!(item.kind, Kind::Albums | Kind::LibraryAlbums);
                let albums = page.items.into_iter().filter(album);
                self.recent = Recent::Albums(albums.collect());
            }
        }
        self.art.receive(ctx) | self.recent_art.receive(ctx) | any
    }

    pub fn status(&self) -> Value {
        let (state, error, found) = match &self.state {
            State::Idle => ("idle", None, None),
            State::Loading(_) => ("loading", None, None),
            State::Empty => ("empty", None, None),
            State::Results(r) => ("results", None, Some(r)),
            State::Failed(_, kind) => ("error", Some(kind), None),
        };
        let count = |page: fn(&SearchResults) -> &bramley_api::Page| {
            found.map_or(0, |r| page(r).items.len())
        };
        let (songs, albums, artists, playlists) = (
            count(|r| &r.songs),
            count(|r| &r.albums),
            count(|r| &r.artists),
            count(|r| &r.playlists),
        );
        let recent = match &self.recent {
            Recent::Albums(albums) => albums.len(),
            _ => 0,
        };
        let ms = self.took.map(|t| t.as_millis() as u64);
        let art = self.art.textures.values().flatten().count();
        json!({ "state": state, "error": error, "results": songs + albums + artists + playlists,
            "songs": songs, "albums": albums, "artists": artists, "playlists": playlists,
            "recent": recent, "ms": ms, "art": art,
            "seq": self.seq })
    }
}

pub fn cause(e: &Error) -> String {
    match e {
        Error::DeveloperToken | Error::SignInNeeded | Error::RateLimited(_) => e.to_string(),
        Error::Http(status, _) => format!("Apple Music answered HTTP {status}"),
        Error::Network(_) => "Apple Music could not be reached".into(),
        Error::Config(why) => why.clone(),
        _ => "the search could not run".into(),
    }
}

pub fn variant(e: &Error) -> String {
    format!("{e:?}")
        .split('(')
        .next()
        .unwrap_or_default()
        .to_owned()
}

pub fn sections(r: &SearchResults) -> Vec<(&'static str, &[Item])> {
    let all = [
        ("Artists", &r.artists),
        ("Songs", &r.songs),
        ("Albums", &r.albums),
        ("Playlists", &r.playlists),
    ];
    let shown = all.into_iter().filter(|(_, page)| !page.items.is_empty());
    shown
        .map(|(title, page)| (title, page.items.as_slice()))
        .collect()
}

pub fn clicked(item: &Item, actions: &mut Vec<Action>, navs: &mut Vec<Nav>) {
    let id = item.id.clone();
    match item.kind {
        Kind::Songs => actions.extend(crate::cli::catalog_id(&id).ok().map(Action::PlaySong)),
        Kind::Albums => navs.push(Nav::Go(Route::Album(id))),
        Kind::Playlists => navs.push(Nav::Go(Route::Playlist(id))),
        Kind::Artists => navs.push(Nav::Go(Route::Artist(id))),
        _ => {}
    }
}

pub(super) fn art_key(item: &Item) -> String {
    format!("{:?}/{}", item.kind, item.id)
}

pub struct Art {
    prefix: Option<String>,
    pt: f32,
    current: Arc<AtomicU64>,
    pub(super) tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
    jobs: Option<Sender<Wanted>>,
    textures: HashMap<String, Option<TextureHandle>>,
}

impl Art {
    pub(super) fn new(prefix: Option<String>, pt: f32) -> Self {
        let (tx, rx) = channel();
        Art {
            prefix,
            pt,
            current: Arc::default(),
            tx,
            rx,
            jobs: None,
            textures: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn wanted(&self) -> usize {
        self.textures.len()
    }

    pub(super) fn set(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    pub(super) fn texture(&self, item: &Item) -> Option<&TextureHandle> {
        self.textures.get(&art_key(item))?.as_ref()
    }

    pub(super) fn clear(&mut self) {
        self.current.fetch_add(1, Ordering::SeqCst);
        self.textures.clear();
    }

    pub(super) fn receive(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        for (set, key, image) in self.rx.try_iter() {
            any = true;
            if set == self.set() {
                let texture = ctx.load_texture(format!("search-{key}"), image, Default::default());
                self.textures.insert(key, Some(texture));
            }
        }
        any
    }

    pub(super) fn want(&mut self, ctx: &egui::Context, item: &Item) {
        let template = item.artwork.as_deref();
        let key = art_key(item);
        let fresh = !self.textures.contains_key(&key);
        let Some(template) = template.filter(|_| fresh) else {
            return;
        };
        self.textures.insert(key.clone(), None);
        let px = covers::px(self.pt, ctx.pixels_per_point());
        let url = art_url(template, px).filter(|url| allowed_art(url, self.prefix.as_deref()));
        if self.jobs.is_none() {
            let (current, done) = (self.current.clone(), self.tx.clone());
            self.jobs = fetcher(current, done, ctx.clone());
        }
        if let (Some(jobs), Some(url)) = (&self.jobs, url) {
            let _ = jobs.send((self.set(), key, url, px));
        }
    }
}

fn fetcher(
    current: Arc<AtomicU64>,
    done: Sender<Fetched>,
    waker: egui::Context,
) -> Option<Sender<Wanted>> {
    let (jobs, queue) = channel::<Wanted>();
    let fetch = move || {
        let (mut agent, tally) = (covers::agent(), Tally::default());
        let live = queue
            .into_iter()
            .filter(|job| job.0 == current.load(Ordering::SeqCst));
        for (set, key, url, px) in live {
            let image = catch_unwind(AssertUnwindSafe(|| {
                covers::cover(&mut agent, &url, px, &tally)
            }));
            if let Ok(Ok(image)) = image {
                let _ = done.send((set, key, image));
                waker.request_repaint();
            }
        }
    };
    let spawned = std::thread::Builder::new()
        .name("search-art".into())
        .spawn(fetch);
    spawned.ok().map(|_| jobs)
}

pub fn show(
    ui: &mut egui::Ui,
    (shell, search): (&mut Shell, &mut Search),
    title: egui::Id,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    let field = egui::TextEdit::singleline(&mut shell.query)
        .id(super::content::search_id())
        .hint_text("Search Apple Music")
        .desired_width(f32::INFINITY);
    let response = ui.add(field).labelled_by(title);
    if std::mem::take(&mut shell.focus_search) {
        response.request_focus();
    }
    if response.changed() {
        search.edited(&shell.query, Instant::now());
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        search.enter(&shell.query, Instant::now());
        ui.ctx().request_repaint();
    }
    ui.add_space(6.0);
    let results = match &search.state {
        State::Idle if shell.query.trim().is_empty() => {
            return recent(ui, (&search.recent, &mut search.recent_art), navs);
        }
        State::Idle => return,
        State::Loading(_) => return drop(ui.spinner()),
        State::Empty => return drop(ui.label(RichText::new("No results.").weak())),
        State::Failed(why, _) => return super::notice(ui, &format!("Search failed: {why}")),
        State::Results(results) => results,
    };
    #[cfg(feature = "dev")]
    if search.click_first_song
        && let Some(song) = results.songs.items.first()
    {
        search.click_first_song = false;
        clicked(song, actions, navs);
    }
    #[cfg(feature = "dev")]
    if search.click_first_playlist
        && let Some(playlist) = results.playlists.items.first()
    {
        search.click_first_playlist = false;
        clicked(playlist, actions, navs);
    }
    results_list(ui, results, &mut search.art, (actions, navs));
}

pub(super) fn results_list(
    ui: &mut egui::Ui,
    results: &SearchResults,
    art: &mut Art,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) -> egui::Rect {
    let area = egui::ScrollArea::vertical().auto_shrink([false, true]);
    area.show(ui, |ui| {
        for (title, items) in sections(results) {
            ui.label(RichText::new(title).strong().size(15.0));
            for item in items {
                row(ui, item, art, (actions, navs));
            }
            ui.add_space(8.0);
        }
    })
    .inner_rect
}

fn recent(ui: &mut egui::Ui, (recent, art): (&Recent, &mut Art), navs: &mut Vec<Nav>) {
    let Recent::Albums(albums) = recent else {
        return;
    };
    ui.label(RichText::new("Recently played").strong().size(15.0));
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for item in albums {
                    let name = item.name.as_deref().unwrap_or("Untitled");
                    let size = egui::vec2(RECENT_PT, RECENT_PT);
                    let button = egui::Button::image(blank_or(art, item, size)).frame(false);
                    let label = format!("Open album {name}");
                    let cover = labeled(ui.add(button), WidgetType::Button, &label);
                    if ui.is_rect_visible(cover.rect) {
                        art.want(ui.ctx(), item);
                    }
                    if cover.clicked() {
                        navs.push(Nav::Go(Route::Album(item.id.clone())));
                    }
                }
            });
        });
}

fn blank_or<'a>(art: &'a Art, item: &Item, size: egui::Vec2) -> egui::Image<'a> {
    match art.texture(item) {
        Some(texture) => egui::Image::new(texture),
        _ => egui::Image::new((egui::TextureId::default(), size)).tint(Color32::TRANSPARENT),
    }
    .fit_to_exact_size(size)
}

fn row(ui: &mut egui::Ui, item: &Item, art: &mut Art, emit: (&mut Vec<Action>, &mut Vec<Nav>)) {
    let name = item.name.as_deref().unwrap_or("Untitled");
    let text = match &item.artist {
        Some(artist) => format!("{name} · {artist}"),
        None => name.to_owned(),
    };
    let label = match item.kind {
        Kind::Songs => format!("Play {text}"),
        Kind::Albums => format!("Open album {text}"),
        Kind::Playlists => format!("Open playlist {text}"),
        Kind::Artists => format!("Open artist {text}"),
        _ => text.clone(),
    };
    let size = egui::vec2(ART_PT, ART_PT);
    let button = egui::Button::image_and_text(blank_or(art, item, size), text).frame(false);
    let response = labeled(ui.add(button), WidgetType::Button, &label);
    if ui.is_rect_visible(response.rect) {
        art.want(ui.ctx(), item);
    }
    if response.clicked() {
        clicked(item, emit.0, emit.1);
    }
    if item.kind == Kind::Songs {
        super::track_menu(&response, &item.id, emit.0);
    }
}
