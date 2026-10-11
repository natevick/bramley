use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bramley_api::{Call, Item, Job, Kind, Reply};
use bramley_engine::Queue;
use egui::{RichText, WidgetType};
use serde_json::{Value, json};

use super::labeled;
use super::nav::Route;
use super::player_bar::clock;
use super::search::{Art, cause};
use crate::playback::Action;

#[cfg(test)]
mod tests;

pub const DEADLINE: Duration = Duration::from_secs(10);
pub const MAX_TRACKS: usize = 1000;
const ART_PT: f32 = 160.0;
const ROW_PT: f32 = 26.0;

type Found = (u64, bramley_api::Result<Reply>);

#[derive(Debug, PartialEq)]
pub struct Model {
    pub item: Item,
    pub tracks: Vec<Item>,
    pub at: Vec<usize>,
    pub seen: usize,
    pub next: Option<String>,
    pub broken: bool,
}

impl Model {
    pub fn new(detail: bramley_api::Detail) -> Self {
        let mut model = Model {
            item: detail.item,
            tracks: Vec::new(),
            at: Vec::new(),
            seen: 0,
            next: None,
            broken: false,
        };
        model.add(detail.tracks);
        model
    }

    pub fn add(&mut self, page: bramley_api::Page) {
        let first = self.seen;
        self.seen += page.items.len();
        let songs = page.items.into_iter().enumerate();
        for (n, track) in songs.filter(|(_, t)| t.kind == Kind::Songs) {
            self.tracks.push(track);
            self.at.push(first + n);
        }
        self.tracks.truncate(MAX_TRACKS);
        self.at.truncate(MAX_TRACKS);
        self.next = page.next.filter(|_| self.tracks.len() < MAX_TRACKS);
    }

    pub fn summary(&self) -> String {
        let n = self.tracks.len();
        if self.next.is_some() || self.broken {
            return format!("{n}+ songs");
        }
        let ms: u64 = self.tracks.iter().filter_map(|t| t.duration_ms).sum();
        let minutes = (ms + 30_000) / 60_000;
        let length = match minutes / 60 {
            0 => format!("{minutes} min"),
            hours => format!("{hours} hr {} min", minutes % 60),
        };
        let songs = if n == 1 { "song" } else { "songs" };
        format!("{n} {songs}, {length}")
    }
}

#[derive(Debug, PartialEq)]
pub enum State {
    Idle,
    Loading(Instant),
    Loaded(Box<Model>),
    Failed(String),
}

pub fn container(route: &Route) -> Option<Queue> {
    match route {
        Route::Album(id) => Some(Queue::Album(id.clone())),
        Route::Playlist(id) if !id.starts_with("p.") => Some(Queue::Playlist(id.clone())),
        _ => None,
    }
}

pub fn play_from(open: &Queue, model: &Model, index: usize) -> Option<Action> {
    Some(Action::PlayIn(open.clone(), *model.at.get(index)?))
}

pub struct Detail {
    open: Option<Queue>,
    seq: u64,
    pub(super) state: State,
    more: Option<Instant>,
    tx: Sender<Found>,
    rx: Receiver<Found>,
    pub(super) art: Art,
    #[cfg(feature = "dev")]
    pub click_track: Option<usize>,
}

impl Detail {
    pub fn new(art_prefix: Option<String>) -> Self {
        let (tx, rx) = channel();
        Detail {
            open: None,
            seq: 0,
            state: State::Idle,
            more: None,
            tx,
            rx,
            art: Art::new(art_prefix, ART_PT),
            #[cfg(feature = "dev")]
            click_track: None,
        }
    }

    fn replace(&mut self, open: Option<Queue>, state: State) {
        (self.open, self.state, self.more) = (open, state, None);
        self.seq += 1;
        self.art.clear();
    }

    fn job(&self, call: Call, deadline: Instant, wake: impl Fn() + Send + 'static) -> Job {
        let (tx, seq) = (self.tx.clone(), self.seq);
        Job::new(call, deadline, move |found| {
            let _ = tx.send((seq, found));
            wake();
        })
    }

    pub fn tick(
        &mut self,
        now: Instant,
        signed_in: bool,
        route: &Route,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Job> {
        let want = container(route).filter(|_| signed_in);
        if want != self.open {
            let Some(open) = want else {
                self.replace(None, State::Idle);
                return None;
            };
            let call = match &open {
                Queue::Album(id) => Call::Album(id.clone()),
                Queue::Playlist(id) => Call::Playlist(id.clone()),
                Queue::Songs(_) => return None,
            };
            self.replace(Some(open), State::Loading(now + DEADLINE));
            return Some(self.job(call, now + DEADLINE, wake));
        }
        match &mut self.state {
            State::Loading(deadline) if now > *deadline => {
                self.state = State::Failed("Apple Music did not answer in time".into());
            }
            State::Loaded(model) if model.next.is_some() => {
                if self.more.is_some_and(|deadline| now > deadline) {
                    (model.broken, model.next, self.more) = (true, None, None);
                } else if self.more.is_none() {
                    let next = model.next.clone()?;
                    self.more = Some(now + DEADLINE);
                    return Some(self.job(Call::Next(next), now + DEADLINE, wake));
                }
            }
            _ => {}
        }
        None
    }

    pub fn receive(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        for (seq, found) in self.rx.try_iter() {
            any = true;
            if seq != self.seq {
                continue;
            }
            self.more = None;
            match (&mut self.state, found) {
                (State::Loading(_), Ok(Reply::Detail(detail))) => {
                    self.state = State::Loaded(Box::new(Model::new(detail)));
                }
                (State::Loaded(model), Ok(Reply::Page(page))) => model.add(page),
                (State::Loaded(model), _) => (model.broken, model.next) = (true, None),
                (_, Err(e)) => self.state = State::Failed(cause(&e)),
                _ => self.state = State::Failed("Apple Music sent an unexpected reply".into()),
            }
        }
        self.art.receive(ctx) | any
    }

    pub fn status(&self) -> Value {
        let (state, tracks) = match &self.state {
            State::Idle => ("idle", None),
            State::Loading(_) => ("loading", None),
            State::Loaded(model) => ("loaded", Some(model)),
            State::Failed(_) => ("error", None),
        };
        let ids: Vec<&str> = tracks.map_or(Vec::new(), |m| {
            m.tracks.iter().map(|t| t.id.as_str()).collect()
        });
        let complete = tracks.is_some_and(|m| m.next.is_none() && !m.broken);
        let art = tracks.is_some_and(|m| self.art.texture(&m.item).is_some());
        json!({ "state": state, "tracks": ids.len(), "complete": complete, "art": art, "trackIds": ids })
    }
}

pub fn show(ui: &mut egui::Ui, detail: &mut Detail, actions: &mut Vec<Action>) {
    let (open, model) = match (&detail.open, &detail.state) {
        (Some(open), State::Loaded(model)) => (open, model),
        (_, State::Failed(why)) => return super::notice(ui, &format!("Could not load: {why}")),
        (None, _) => return,
        _ => return drop(ui.spinner()),
    };
    #[cfg(feature = "dev")]
    if let Some(n) = detail.click_track.filter(|n| *n <= model.tracks.len()) {
        detail.click_track = None;
        match n {
            0 => actions.push(Action::Shuffle(open.clone())),
            n => actions.extend(play_from(open, model, n - 1)),
        }
    }
    ui.horizontal(|ui| {
        let size = egui::vec2(ART_PT, ART_PT);
        detail.art.want(ui.ctx(), &model.item);
        match detail.art.texture(&model.item) {
            Some(texture) => ui.add(egui::Image::new(texture).fit_to_exact_size(size)),
            None => ui.allocate_response(size, egui::Sense::hover()),
        };
        ui.vertical(|ui| {
            let title = model.item.name.as_deref().unwrap_or("Untitled");
            ui.heading(RichText::new(title).strong());
            ui.label(model.item.artist.as_deref().unwrap_or(""));
            ui.label(RichText::new(model.summary()).weak());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let play = labeled(ui.button("⏵ Play"), WidgetType::Button, "Play all");
                if play.clicked() {
                    actions.extend(play_from(open, model, 0));
                }
                let shuffle = labeled(ui.button("Shuffle"), WidgetType::Button, "Shuffle");
                if shuffle.clicked() && !model.tracks.is_empty() {
                    actions.push(Action::Shuffle(open.clone()));
                }
            });
        });
    });
    ui.add_space(8.0);
    let rows = model.tracks.len();
    egui::ScrollArea::vertical().show_rows(ui, ROW_PT, rows, |ui, range| {
        for index in range {
            let track = &model.tracks[index];
            let name = track.name.as_deref().unwrap_or("Untitled");
            let length = track
                .duration_ms
                .map_or(String::new(), |ms| clock(ms as f64 / 1000.0));
            let text = format!("{:>3}   {name}   {length}", index + 1);
            let row = egui::Button::new(text).frame(false).truncate();
            let size = [ui.available_width(), ROW_PT];
            let row = ui.add_sized(size, row);
            let row = labeled(row, WidgetType::Button, &format!("Play {name}"));
            if row.clicked() {
                actions.extend(play_from(open, model, index));
            }
        }
    });
}
