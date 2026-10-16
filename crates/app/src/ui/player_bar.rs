use bramley_engine::{Repeat, Snapshot};
use egui::{RichText, WidgetInfo, WidgetType};

use super::labeled;
use super::nav::Nav;
use crate::playback::{Action, play_or_pause};

pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn glyph(ui: &mut egui::Ui, text: &str, size: f32, label: &str) -> bool {
    let response = ui.button(RichText::new(text).size(size));
    labeled(response, WidgetType::Button, label).clicked()
}

pub fn show(
    ui: &mut egui::Ui,
    shown: &Snapshot,
    started: bool,
    queue_open: bool,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(220.0);
            let title = shown.title.as_deref().unwrap_or("Nothing playing");
            ui.add(egui::Label::new(RichText::new(title).strong().size(15.0)).truncate());
            let artist = RichText::new(shown.artist.as_deref().unwrap_or("")).weak();
            ui.add(egui::Label::new(artist).truncate());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.selectable_label(queue_open, "Queue").clicked() {
                navs.push(Nav::ToggleQueue);
            }
            ui.add_enabled_ui(started, |ui| modes(ui, shown, actions));
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                controls(ui, shown, started, actions);
            });
        });
    });
}

fn toggle(ui: &mut egui::Ui, on: bool, text: &str, label: &str) -> bool {
    let response = ui.add(egui::Button::selectable(on, RichText::new(text).size(18.0)));
    let enabled = response.enabled();
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, label));
    response.clicked()
}

fn modes(ui: &mut egui::Ui, shown: &Snapshot, actions: &mut Vec<Action>) {
    let (text, label) = match shown.repeat {
        Repeat::Off => ("🔁", "Repeat off"),
        Repeat::All => ("🔁", "Repeat all"),
        Repeat::One => ("🔂", "Repeat one"),
    };
    if toggle(ui, shown.repeat != Repeat::Off, text, label) {
        actions.push(Action::SetRepeat(shown.repeat.next()));
    }
    if toggle(ui, shown.shuffle, "🔀", "Shuffle") {
        actions.push(Action::SetShuffle(!shown.shuffle));
    }
}

fn controls(ui: &mut egui::Ui, shown: &Snapshot, started: bool, actions: &mut Vec<Action>) {
    ui.add_enabled_ui(started, |ui| {
        if glyph(ui, "⏮", 18.0, "Previous track") {
            actions.push(Action::Previous);
        }
        let action = play_or_pause(shown.state);
        let (text, label) = if action == Action::Pause {
            ("⏸", "Pause")
        } else {
            ("⏵", "Play")
        };
        if glyph(ui, text, 22.0, label) {
            actions.push(action);
        }
        if glyph(ui, "⏭", 18.0, "Next track") {
            actions.push(Action::Next);
        }
        let duration = shown.duration.unwrap_or(0.0);
        let mut position = shown.position.unwrap_or(0.0).min(duration);
        ui.label(clock(position));
        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
        let slider = egui::Slider::new(&mut position, 0.0..=duration.max(1.0)).show_value(false);
        let response = labeled(
            ui.add_enabled(duration > 0.0, slider),
            WidgetType::Slider,
            "Position",
        );
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            actions.push(Action::Seek(position));
        }
        ui.label(clock(duration));
    });
}
