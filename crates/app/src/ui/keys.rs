use std::collections::HashSet;

use egui::{Context, Event, Id, Key, Modifiers, RawInput};

use super::cursor::{self, Step};
use crate::playback::Action;
use Do::*;
use Gate::{Once, WhenIdle};
use Key::{ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Enter, Equals, Minus};
use bramley_engine::Snapshot;

const UP: [(Key, Modifiers); 2] = [(Equals, Modifiers::NONE), (ArrowUp, COMMAND)];
const DOWN: [(Key, Modifiers); 2] = [(Minus, Modifiers::NONE), (ArrowDown, COMMAND)];

#[derive(Clone, Copy, PartialEq)]
pub enum Gate {
    Always,
    WhenIdle,
    Once,
    Repeat,
}

#[derive(Clone, Copy)]
pub enum Intent {
    Region,
    Cursor,
    Dismiss,
    Activate,
    Back,
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

#[cfg_attr(not(test), expect(dead_code))]
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
    bind(&[(Enter, SHIFT)], Once, PlayNext, "Play next"),
    bind(&[(Enter, COMMAND)], Once, QueueAdd, "Add to queue"),
];

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

fn takes(binding: &Binding, event: &Event, idle: bool) -> bool {
    let text = |t: &str| match binding.intent {
        Intent::Do(Next) => t == ">",
        Intent::Do(Previous) => t == "<",
        _ => false,
    };
    let (hit, repeat) = match event {
        Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } => {
            let chord = |&(k, m): &(Key, Modifiers)| k == *key && modifiers.matches_exact(m);
            (binding.keys.iter().any(chord), *repeat)
        }
        Event::Text(t) => (text(t), false),
        _ => return false,
    };
    let open = match binding.gate {
        Gate::Always => true,
        Gate::WhenIdle | Once => idle,
        Gate::Repeat => repeat,
    };
    open && hit && binding.platforms.here()
}

fn step(binding: &Binding, key: Key, modifiers: Modifiers) -> Option<Step> {
    match binding.intent {
        Intent::Region if modifiers.shift => Some(Step::Prev),
        Intent::Region => Some(Step::Next),
        Intent::Cursor => Step::of(key),
        Intent::Dismiss => Some(Step::Escape),
        Intent::Back => Some(Step::Back),
        Intent::Activate | Intent::Do(_) => None,
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

pub fn take(ctx: &Context, input: &mut RawInput) {
    let idle = idle(ctx);
    let mut repeating = false;
    input.events.retain(|event| {
        if let Event::Key { repeat, .. } = event {
            repeating = *repeat;
        }
        let Some(binding) = BINDINGS.iter().find(|b| takes(b, event, idle)) else {
            return true;
        };
        log::debug!("took {event:?}");
        match (binding.intent, event) {
            (Intent::Do(_), _) if binding.gate == Once && repeating => {}
            (Intent::Do(d), _) => {
                ctx.data_mut(|data| data.get_temp_mut_or_default::<Vec<Do>>(Id::NULL).push(d))
            }
            (_, Event::Key { key, modifiers, .. }) => {
                cursor::push_all(ctx, step(binding, *key, *modifiers))
            }
            _ => {}
        }
        false
    });
}
