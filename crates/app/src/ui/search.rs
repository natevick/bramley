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

type Found = (u64, bramley_api::Result<Reply>);
type Fetched = (u64, String, ColorImage);
type Wanted = (u64, String, String, u32);

#[derive(Debug, PartialEq)]
pub enum State {
    Idle,
    Loading(Instant),
    Empty,
    Results(SearchResults),
    Failed(String),
}

pub struct Search {
    pending: Option<(String, Instant)>,
    seq: u64,
    pub(super) state: State,
    asked: Option<Instant>,
    took: Option<Duration>,
    tx: Sender<Found>,
    rx: Receiver<Found>,
    pub(super) art: Art,
    #[cfg(feature = "dev")]
    pub click_first_song: bool,
}

impl Search {
    pub fn new(art_prefix: Option<String>) -> Self {
        let (tx, rx) = channel();
        Search {
            pending: None,
            seq: 0,
            state: State::Idle,
            asked: None,
            took: None,
            tx,
            rx,
            art: Art::new(art_prefix),
            #[cfg(feature = "dev")]
            click_first_song: false,
        }
    }

    pub fn edited(&mut self, text: &str, now: Instant) {
        self.pending = Some((text.trim().to_owned(), now + DEBOUNCE));
    }

    pub fn enter(&mut self, text: &str, now: Instant) {
        self.pending = Some((text.trim().to_owned(), now));
    }

    pub fn wait(&self, now: Instant) -> Option<Duration> {
        let (_, due) = self.pending.as_ref()?;
        Some(due.saturating_duration_since(now))
    }

    pub fn tick(
        &mut self,
        now: Instant,
        signed_in: bool,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Job> {
        if !signed_in {
            if self.state != State::Idle {
                self.replace(State::Idle);
            }
            return None;
        }
        if matches!(self.state, State::Loading(deadline) if now > deadline) {
            self.state = State::Failed("the search timed out".into());
        }
        let (term, _) = self.pending.take_if(|(_, due)| *due <= now)?;
        if term.is_empty() {
            self.replace(State::Idle);
            return None;
        }
        self.replace(State::Loading(now + DEADLINE));
        (self.asked, self.took) = (Some(now), None);
        let (tx, seq) = (self.tx.clone(), self.seq);
        Some(Job::new(Call::Search(term), now + DEADLINE, move |found| {
            let _ = tx.send((seq, found));
            wake();
        }))
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
                Ok(_) => State::Failed("the search could not run".into()),
                Err(e) => State::Failed(cause(&e)),
            };
        }
        self.art.receive(ctx) | any
    }

    pub fn status(&self) -> Value {
        let (state, results) = match &self.state {
            State::Idle => ("idle", 0),
            State::Loading(_) => ("loading", 0),
            State::Empty => ("empty", 0),
            State::Results(r) => ("results", sections(r).iter().map(|(_, s)| s.len()).sum()),
            State::Failed(_) => ("error", 0),
        };
        let ms = self.took.map(|t| t.as_millis() as u64);
        let art = self.art.textures.values().flatten().count();
        json!({ "state": state, "results": results, "ms": ms, "art": art })
    }
}

pub fn cause(e: &Error) -> String {
    match e {
        Error::DeveloperToken | Error::SignInNeeded | Error::RateLimited(_) => e.to_string(),
        Error::Http(status, _) => format!("Apple Music answered HTTP {status}"),
        Error::Network(_) => "Apple Music could not be reached".into(),
        _ => "the search could not run".into(),
    }
}

pub fn sections(r: &SearchResults) -> Vec<(&'static str, &[Item])> {
    let all = [
        ("Songs", &r.songs),
        ("Albums", &r.albums),
        ("Artists", &r.artists),
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
        _ => {}
    }
}

pub struct Art {
    prefix: Option<String>,
    current: Arc<AtomicU64>,
    pub(super) tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
    jobs: Option<Sender<Wanted>>,
    textures: HashMap<String, Option<TextureHandle>>,
}

impl Art {
    fn new(prefix: Option<String>) -> Self {
        let (tx, rx) = channel();
        Art {
            prefix,
            current: Arc::default(),
            tx,
            rx,
            jobs: None,
            textures: HashMap::new(),
        }
    }

    pub(super) fn set(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    fn clear(&mut self) {
        self.current.fetch_add(1, Ordering::SeqCst);
        self.textures.clear();
    }

    fn receive(&mut self, ctx: &egui::Context) -> bool {
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

    fn want(&mut self, ctx: &egui::Context, item: &Item) {
        let template = item
            .artwork
            .as_deref()
            .filter(|_| item.kind != Kind::Artists);
        let fresh = !self.textures.contains_key(&item.id);
        let Some(template) = template.filter(|_| fresh) else {
            return;
        };
        self.textures.insert(item.id.clone(), None);
        let px = covers::px(ART_PT, ctx.pixels_per_point());
        let url = art_url(template, px).filter(|url| allowed_art(url, self.prefix.as_deref()));
        if self.jobs.is_none() {
            let (current, done) = (self.current.clone(), self.tx.clone());
            self.jobs = fetcher(current, done, ctx.clone());
        }
        if let (Some(jobs), Some(url)) = (&self.jobs, url) {
            let _ = jobs.send((self.set(), item.id.clone(), url, px));
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
        let (agent, tally) = (covers::agent(), Tally::default());
        let live = queue
            .into_iter()
            .filter(|job| job.0 == current.load(Ordering::SeqCst));
        for (set, key, url, px) in live {
            let image = catch_unwind(AssertUnwindSafe(|| covers::cover(&agent, &url, px, &tally)));
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
        State::Idle => return,
        State::Loading(_) => return drop(ui.spinner()),
        State::Empty => return drop(ui.label(RichText::new("No results.").weak())),
        State::Failed(why) => return super::notice(ui, &format!("Search failed: {why}")),
        State::Results(results) => results,
    };
    #[cfg(feature = "dev")]
    if search.click_first_song
        && let Some(song) = results.songs.items.first()
    {
        search.click_first_song = false;
        clicked(song, actions, navs);
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (title, items) in sections(results) {
            ui.label(RichText::new(title).strong().size(15.0));
            for item in items {
                row(ui, item, &mut search.art, (actions, navs));
            }
            ui.add_space(8.0);
        }
    });
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
        _ => text.clone(),
    };
    let size = egui::vec2(ART_PT, ART_PT);
    let image = match art.textures.get(&item.id) {
        Some(Some(texture)) => egui::Image::new(texture),
        _ => egui::Image::new((egui::TextureId::default(), size)).tint(Color32::TRANSPARENT),
    };
    let button = egui::Button::image_and_text(image.fit_to_exact_size(size), text).frame(false);
    let enabled = item.kind != Kind::Artists;
    let response = labeled(ui.add_enabled(enabled, button), WidgetType::Button, &label);
    if ui.is_rect_visible(response.rect) {
        art.want(ui.ctx(), item);
    }
    if response.clicked() {
        clicked(item, emit.0, emit.1);
    }
}
