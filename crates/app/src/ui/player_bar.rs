use std::time::Duration;

use bramley_engine::{Repeat, Snapshot};
use egui::{RichText, WidgetInfo, WidgetType};

use super::labeled;
use super::nav::Nav;
use crate::playback::{Action, play_or_pause};

const VOLUME_EVERY: f64 = 0.1;

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
    volume: u8,
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
            if cfg!(not(target_os = "macos")) {
                volume_slider(ui, volume, actions);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                controls(ui, shown, started, actions);
            });
        });
    });
}

fn volume_slider(ui: &mut egui::Ui, current: u8, actions: &mut Vec<Action>) {
    let id = ui.id().with("volume");
    let held: Option<(u8, u8, f64)> = ui.data(|d| d.get_temp(id));
    let mut percent = held.map_or(current, |(shown, ..)| shown);
    let slider = egui::Slider::new(&mut percent, 0..=100).show_value(false);
    let response = ui.scope(|ui| {
        ui.spacing_mut().slider_width = 100.0;
        ui.add(slider)
    });
    let response = response.inner.on_hover_text(format!("{percent}%"));
    let enabled = response.enabled();
    response.widget_info(|| WidgetInfo::slider(enabled, f64::from(percent), "Volume"));
    ui.label("🔈");
    let now = ui.input(|i| i.time);
    let (mut sent, mut at) = held.map_or((current, f64::NEG_INFINITY), |(_, sent, at)| (sent, at));
    if percent != sent && (response.drag_stopped() || now - at >= VOLUME_EVERY) {
        actions.push(Action::Volume(f32::from(percent) / 100.0));
        (sent, at) = (percent, now);
    }
    if percent != sent {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(VOLUME_EVERY));
    }
    if percent != sent || response.dragged() {
        ui.data_mut(|d| d.insert_temp(id, (percent, sent, at)));
    } else {
        ui.data_mut(|d| d.remove_temp::<(u8, u8, f64)>(id));
    }
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
