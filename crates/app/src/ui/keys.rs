use std::collections::HashSet;

use egui::{Context, Event, Id, Key, Modifiers, RawInput};

#[derive(Clone, Copy, PartialEq)]
pub enum Gate {
    Always,
    WhenIdle,
}

pub enum Intent {
    Region,
    Cursor,
    Dismiss,
}

#[expect(dead_code)]
pub struct Binding {
    pub keys: &'static [(Key, Modifiers)],
    pub gate: Gate,
    pub intent: Intent,
    pub help: &'static str,
    pub on_macos: bool,
}

const ARROWS: [(Key, Modifiers); 4] = [
    (Key::ArrowUp, Modifiers::NONE),
    (Key::ArrowDown, Modifiers::NONE),
    (Key::ArrowLeft, Modifiers::NONE),
    (Key::ArrowRight, Modifiers::NONE),
];

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: &[(Key::Tab, Modifiers::NONE), (Key::Tab, Modifiers::SHIFT)],
        gate: Gate::Always,
        intent: Intent::Region,
        help: "Next or previous region",
        on_macos: true,
    },
    Binding {
        keys: &ARROWS,
        gate: Gate::WhenIdle,
        intent: Intent::Cursor,
        help: "Move the cursor",
        on_macos: true,
    },
    Binding {
        keys: &[(Key::Escape, Modifiers::NONE)],
        gate: Gate::WhenIdle,
        intent: Intent::Dismiss,
        help: "Leave, close or go back",
        on_macos: true,
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
        modifiers,
        ..
    } = event
    else {
        return false;
    };
    (idle || binding.gate == Gate::Always)
        && (binding.on_macos || !cfg!(target_os = "macos"))
        && binding
            .keys
            .iter()
            .any(|&(k, m)| k == *key && modifiers.matches_exact(m))
}

pub fn take(ctx: &Context, input: &mut RawInput) {
    let idle = idle(ctx);
    input
        .events
        .retain(|e| !BINDINGS.iter().any(|b| takes(b, e, idle)));
}
