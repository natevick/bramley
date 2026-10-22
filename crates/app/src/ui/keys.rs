use std::collections::HashSet;

use egui::{Context, Event, Id, Key, Modifiers, RawInput};

use super::cursor::{self, Step};

#[derive(Clone, Copy, PartialEq)]
pub enum Gate {
    Always,
    WhenIdle,
    Repeat,
}

pub enum Intent {
    Region,
    Cursor,
    Dismiss,
    Activate,
    Back,
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

#[expect(dead_code)]
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
    let Event::Key {
        key,
        pressed: true,
        repeat,
        modifiers,
        ..
    } = event
    else {
        return false;
    };
    let open = match binding.gate {
        Gate::Always => true,
        Gate::WhenIdle => idle,
        Gate::Repeat => *repeat,
    };
    open && binding.platforms.here()
        && binding
            .keys
            .iter()
            .any(|&(k, m)| k == *key && modifiers.matches_exact(m))
}

fn step(binding: &Binding, key: Key, modifiers: Modifiers) -> Option<Step> {
    match binding.intent {
        Intent::Region if modifiers.shift => Some(Step::Prev),
        Intent::Region => Some(Step::Next),
        Intent::Cursor => Step::of(key),
        Intent::Dismiss => Some(Step::Escape),
        Intent::Back => Some(Step::Back),
        Intent::Activate => None,
    }
}

pub fn take(ctx: &Context, input: &mut RawInput) {
    let idle = idle(ctx);
    input.events.retain(|event| {
        let Event::Key { key, modifiers, .. } = event else {
            return true;
        };
        let Some(binding) = BINDINGS.iter().find(|b| takes(b, event, idle)) else {
            return true;
        };
        log::debug!("took {key:?}");
        cursor::push_all(ctx, step(binding, *key, *modifiers));
        false
    });
}
