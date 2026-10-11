use bramley_engine::Snapshot;
use egui::{RichText, WidgetType};

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
            ui.label(RichText::new(title).strong().size(15.0));
            ui.label(RichText::new(shown.artist.as_deref().unwrap_or("")).weak());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.selectable_label(queue_open, "Queue").clicked() {
                navs.push(Nav::ToggleQueue);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                controls(ui, shown, started, actions);
            });
        });
    });
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
