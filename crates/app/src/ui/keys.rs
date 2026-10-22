use std::collections::HashSet;

use egui::{Context, Event, Id, Key, KeyboardShortcut, Modifiers, RawInput};

use super::cursor::{self, Step};
use crate::playback::Action;
use Do::*;
use Gate::{Once, WhenIdle};
use Key::{ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Enter, PageDown, PageUp};
use bramley_engine::Snapshot;

const UP: [(Key, Modifiers); 1] = [(ArrowUp, COMMAND)];
const DOWN: [(Key, Modifiers); 1] = [(ArrowDown, COMMAND)];

#[derive(Clone, Copy, PartialEq)]
pub enum Gate {
    Always,
    WhenIdle,
    Once,
    Letters,
    Repeat,
}

#[derive(Clone, Copy)]
pub enum Intent {
    Region,
    Cursor,
    Dismiss,
    Activate,
    Back,
    Letters,
    Help,
    Shortcut,
    Do(Do),
}

#[derive(Clone, Copy)]
pub enum Do {
    Next,
    Previous,
    Seek(f64),
    Volume(i8),
    PlayNext,
    QueueAdd,
}

impl Do {
    pub fn action(self, shown: &Snapshot, volume: u8, song: Option<String>) -> Option<Action> {
        match self {
            Self::Next => Some(Action::Next),
            Self::Previous => Some(Action::Previous),
            Self::Seek(by) => {
                let end = shown.duration.filter(|d| *d > 0.0)?;
                let to = shown.position.unwrap_or(0.0) + by;
                Some(Action::Seek(to.clamp(0.0, end)))
            }
            Self::Volume(by) => {
                let to = volume.saturating_add_signed(by).min(100);
                Some(Action::Volume(f32::from(to) / 100.0))
            }
            Self::PlayNext => song.map(Action::PlayNext),
            Self::QueueAdd => song.map(Action::QueueAdd),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Platforms {
    All,
    NotMac,
    Mac,
}

impl Platforms {
    pub fn here(self) -> bool {
        match self {
            Self::All => true,
            Self::NotMac => !cfg!(target_os = "macos"),
            Self::Mac => cfg!(target_os = "macos"),
        }
    }
}

pub struct Binding {
    pub keys: &'static [(Key, Modifiers)],
    pub gate: Gate,
    pub intent: Intent,
    pub help: &'static str,
    pub platforms: Platforms,
}

const CURSOR_KEYS: [(Key, Modifiers); 8] = [
    (Key::ArrowUp, Modifiers::NONE),
    (Key::ArrowDown, Modifiers::NONE),
    (Key::ArrowLeft, Modifiers::NONE),
    (Key::ArrowRight, Modifiers::NONE),
    (Key::Home, Modifiers::NONE),
    (Key::End, Modifiers::NONE),
    (Key::PageUp, Modifiers::NONE),
    (Key::PageDown, Modifiers::NONE),
];

const fn bind(
    keys: &'static [(Key, Modifiers)],
    gate: Gate,
    intent: Do,
    help: &'static str,
) -> Binding {
    let platforms = match intent {
        Do::Volume(_) => Platforms::NotMac,
        _ => Platforms::All,
    };
    Binding {
        keys,
        gate,
        intent: Intent::Do(intent),
        help,
        platforms,
    }
}

const fn row(keys: &'static [(Key, Modifiers)], intent: Intent, help: &'static str) -> Binding {
    let (gate, platforms) = (Gate::Always, Platforms::All);
    Binding {
        keys,
        gate,
        intent,
        help,
        platforms,
    }
}

const LETTERS: &str = "hjklgG/";

const NONE: Modifiers = Modifiers::NONE;
const COMMAND: Modifiers = Modifiers::COMMAND;
const SHIFT: Modifiers = Modifiers::SHIFT;

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: &[(Key::Tab, Modifiers::NONE), (Key::Tab, Modifiers::SHIFT)],
        gate: Gate::Always,
        intent: Intent::Region,
        help: "Next or previous region",
        platforms: Platforms::All,
    },
    Binding {
        keys: &CURSOR_KEYS,
        gate: Gate::WhenIdle,
        intent: Intent::Cursor,
        help: "Move the cursor",
        platforms: Platforms::All,
    },
    Binding {
        keys: &[(Key::Escape, Modifiers::NONE)],
        gate: Gate::WhenIdle,
        intent: Intent::Dismiss,
        help: "Leave, close or go back",
        platforms: Platforms::All,
    },
    Binding {
        keys: &[(Key::Enter, Modifiers::NONE)],
        gate: Gate::Repeat,
        intent: Intent::Activate,
        help: "Activate",
        platforms: Platforms::All,
    },
    Binding {
        keys: &[(Key::Backspace, Modifiers::NONE)],
        gate: Gate::WhenIdle,
        intent: Intent::Back,
        help: "Back",
        platforms: Platforms::All,
    },
    Binding {
        keys: &[(Key::ArrowLeft, Modifiers::ALT)],
        gate: Gate::WhenIdle,
        intent: Intent::Back,
        help: "Back",
        platforms: Platforms::NotMac,
    },
    Binding {
        keys: &[(Key::OpenBracket, Modifiers::COMMAND)],
        gate: Gate::WhenIdle,
        intent: Intent::Back,
        help: "Back",
        platforms: Platforms::Mac,
    },
    bind(&[(ArrowRight, COMMAND)], Once, Next, "Next track"),
    bind(&[(ArrowLeft, COMMAND)], Once, Previous, "Previous track"),
    bind(&[(ArrowRight, SHIFT)], WhenIdle, Seek(10.0), "Position"),
    bind(&[(ArrowLeft, SHIFT)], WhenIdle, Seek(-10.0), "Position"),
    bind(&UP, WhenIdle, Volume(5), "Volume"),
    bind(&DOWN, WhenIdle, Volume(-5), "Volume"),
    Binding {
        keys: &[],
        gate: Gate::Letters,
        intent: Intent::Letters,
        help: "Move like the arrows; g g and G go to the ends; / searches",
        platforms: Platforms::All,
    },
    bind(&[(Enter, SHIFT)], Once, PlayNext, "Play next"),
    bind(&[(Enter, COMMAND)], Once, QueueAdd, "Add to queue"),
    row(&[(Key::L, COMMAND)], Intent::Shortcut, "Focus search"),
    row(&[(Key::Space, NONE)], Intent::Shortcut, "Play or pause"),
    Binding {
        keys: &[],
        gate: Gate::WhenIdle,
        intent: Intent::Help,
        help: "Show or hide this help",
        platforms: Platforms::All,
    },
];

fn texts(intent: Intent) -> &'static str {
    match intent {
        Intent::Do(Next) => ">",
        Intent::Do(Previous) => "<",
        Intent::Do(Volume(by)) if by > 0 => "=",
        Intent::Do(Volume(_)) => "-",
        Intent::Letters => LETTERS,
        Intent::Help => "?",
        _ => "",
    }
}

pub fn shown(binding: &Binding) -> String {
    match binding.intent {
        Intent::Do(Seek(by)) => format!("Seek {by:+} s"),
        Intent::Do(Volume(by)) => format!("Volume {by:+}%"),
        _ => binding.help.to_owned(),
    }
}

pub fn modal(ctx: &Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(Id::new("keys-help"), open));
}

pub fn help(ui: &mut egui::Ui) {
    const TITLE: &str = "Keyboard shortcuts";
    let role = |node: &mut egui::accesskit::Node| {
        node.set_role(egui::accesskit::Role::Dialog);
        node.set_label(TITLE);
    };
    ui.ctx().accesskit_node_builder(ui.unique_id(), role);
    ui.heading(TITLE);
    egui::ScrollArea::vertical().show(ui, |ui| {
        let by = ui.data_mut(|d| d.remove_temp::<f32>(Id::new("keys-scroll")));
        let jump = egui::style::ScrollAnimation::none();
        ui.scroll_with_delta_animation(egui::vec2(0.0, by.unwrap_or_default()), jump);
        for b in BINDINGS.iter().filter(|b| b.platforms.here()) {
            let keys = b
                .keys
                .iter()
                .map(|&(k, m)| ui.ctx().format_shortcut(&KeyboardShortcut::new(m, k)));
            let letters = texts(b.intent).chars().map(String::from);
            let chord = keys.chain(letters).collect::<Vec<_>>().join(", ");
            ui.label(egui::RichText::new(chord).strong());
            ui.label(shown(b));
        }
    });
}

fn sliders() -> Id {
    Id::new("keys-sliders")
}

pub fn mark_slider(ctx: &Context, id: Id) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<HashSet<Id>>(sliders())
            .insert(id)
    });
}

fn idle(ctx: &Context) -> bool {
    let marked: HashSet<Id> = ctx.data(|d| d.get_temp(sliders())).unwrap_or_default();
    let on_slider = ctx
        .memory(|m| m.focused())
        .is_some_and(|id| marked.contains(&id));
    !(ctx.text_edit_focused() || on_slider || egui::Popup::is_any_open(ctx))
}

fn takes(binding: &Binding, event: &Event, (idle, typing): (bool, bool), repeating: bool) -> bool {
    let text = |t: &str| t.len() == 1 && texts(binding.intent).contains(t);
    let hit = match event {
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            let chord = |&(k, m): &(Key, Modifiers)| k == *key && modifiers.matches_exact(m);
            binding.keys.iter().any(chord)
        }
        Event::Text(t) => text(t),
        _ => return false,
    };
    let open = match binding.gate {
        Gate::Always => true,
        Gate::WhenIdle | Once => idle,
        Gate::Letters => idle && !typing,
        Gate::Repeat => repeating,
    };
    open && hit && binding.platforms.here() && !matches!(binding.intent, Intent::Shortcut)
}

fn step(binding: &Binding, key: Key, modifiers: Modifiers) -> Option<Step> {
    match binding.intent {
        Intent::Region if modifiers.shift => Some(Step::Prev),
        Intent::Region => Some(Step::Next),
        Intent::Cursor => Step::of(key),
        Intent::Dismiss => Some(Step::Escape),
        Intent::Back => Some(Step::Back),
        _ => None,
    }
}

pub fn focused_song(ctx: &Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::NULL, id.to_owned()));
}

pub fn actions(ctx: &Context, shown: &Snapshot, volume: u8) -> Vec<Action> {
    let song: Option<String> = ctx.data_mut(|d| d.remove_temp(Id::NULL));
    let heard: Vec<Do> = ctx
        .data_mut(|d| d.remove_temp(Id::NULL))
        .unwrap_or_default();
    let act = |d: Do| d.action(shown, volume, song.clone());
    heard.into_iter().filter_map(act).collect()
}

pub fn typing(ctx: &Context, until: f64) {
    ctx.data_mut(|d| d.insert_temp(Id::new("keys-typing"), until));
}

fn letter(ctx: &Context, t: &str, now: f64) -> Vec<Step> {
    let g = Id::new("keys-g");
    let pending = ctx.data_mut(|d| d.remove_temp::<f64>(g));
    match t {
        "h" => vec![Step::Left],
        "j" => vec![Step::Down],
        "k" => vec![Step::Up],
        "l" => vec![Step::Right],
        "G" => vec![Step::Last, Step::Bottom],
        "/" => vec![Step::Search],
        "g" if pending.is_some_and(|at| now - at <= 1.0) => vec![Step::First],
        _ => {
            ctx.data_mut(|d| d.insert_temp(g, now));
            vec![]
        }
    }
}

pub fn take(ctx: &Context, input: &mut RawInput) {
    let now = input.time.unwrap_or(0.0);
    let until: Option<f64> = ctx.data(|d| d.get_temp(Id::new("keys-typing")));
    let idle = (idle(ctx), until.is_some_and(|until| now <= until));
    let modal = ctx.data(|d| d.get_temp(Id::new("keys-help")).unwrap_or(false));
    let held_id = Id::new("keys-held");
    let mut held: HashSet<Key> = ctx.data_mut(|d| d.remove_temp(held_id)).unwrap_or_default();
    let mut repeating = false;
    input.events.retain(|event| {
        match event {
            Event::Key {
                key,
                pressed: true,
                repeat,
                ..
            } => repeating = !held.insert(*key) || *repeat,
            Event::Key { key, .. } => {
                held.remove(key);
                repeating = false;
            }
            Event::WindowFocused(false) => held.clear(),
            _ => {}
        }
        let behind = |key: &Key, modifiers: &Modifiers| match key {
            Enter | Key::Space => true,
            Key::L => modifiers.command,
            _ => false,
        };
        if modal && matches!(event, Event::Key { key, modifiers, .. } if behind(key, modifiers)) {
            return false;
        }
        let Some(binding) = BINDINGS.iter().find(|b| takes(b, event, idle, repeating)) else {
            return true;
        };
        if modal {
            let close = matches!(binding.intent, Intent::Dismiss | Intent::Help);
            cursor::push_all(ctx, close.then_some(Step::Help));
            let by = match event {
                Event::Key { key: ArrowUp, .. } => 40.0,
                Event::Key { key: ArrowDown, .. } => -40.0,
                Event::Key { key: PageUp, .. } => 200.0,
                Event::Key { key: PageDown, .. } => -200.0,
                Event::Text(t) if t == "k" => 40.0,
                Event::Text(t) if t == "j" => -40.0,
                _ => 0.0,
            };
            ctx.data_mut(|d| *d.get_temp_mut_or_default::<f32>(Id::new("keys-scroll")) += by);
            return false;
        }
        log::debug!("took {event:?}");
        match (binding.intent, event) {
            (Intent::Do(_), _) if binding.gate == Once && repeating => {}
            (Intent::Do(d), _) => {
                ctx.data_mut(|data| data.get_temp_mut_or_default::<Vec<Do>>(Id::NULL).push(d))
            }
            (Intent::Help, _) => cursor::push_all(ctx, Some(Step::Help)),
            (Intent::Letters, Event::Text(t)) => {
                letter(ctx, t, now)
                    .into_iter()
                    .for_each(|step| cursor::push_all(ctx, Some(step)));
            }
            (_, Event::Key { key, modifiers, .. }) => {
                cursor::push_all(ctx, step(binding, *key, *modifiers))
            }
            _ => {}
        }
        false
    });
    ctx.data_mut(|d| d.insert_temp(held_id, held));
}
