use std::time::Duration;

use bramley_engine::{Repeat, Snapshot};
use egui::{RichText, WidgetInfo, WidgetType};

use super::labeled;
use super::nav::Nav;
use crate::playback::{Action, play_or_pause};

const VOLUME_EVERY: f64 = 0.1;
const VOLUME_SLIDER_PT: f32 = 100.0;
const VOLUME_SLIDER_FROM_PT: f32 = 800.0;
const TITLE_MIN_PT: f32 = 120.0;
const TITLE_MAX_PT: f32 = 220.0;
const TITLE_SHARE: f32 = 0.2;
const ROOMY_RESERVE_PT: f32 = 60.0;

pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn glyph(ui: &mut egui::Ui, text: &str, size: f32, label: &str) -> bool {
    let response = ui.button(RichText::new(text).size(size));
    labeled(response, WidgetType::Button, label).clicked()
}

fn queue_button(ui: &mut egui::Ui, open: bool, navs: &mut Vec<Nav>) {
    if ui.selectable_label(open, "Queue").clicked() {
        navs.push(Nav::ToggleQueue);
    }
}

fn title_column(ui: &mut egui::Ui, shown: &Snapshot, width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width.max(0.0));
        let title = shown.title.as_deref().unwrap_or("Nothing playing");
        ui.add(egui::Label::new(RichText::new(title).strong().size(15.0)).truncate());
        let artist = RichText::new(shown.artist.as_deref().unwrap_or("")).weak();
        ui.add(egui::Label::new(artist).truncate());
    });
}

pub fn show(
    ui: &mut egui::Ui,
    shown: &Snapshot,
    started: bool,
    volume: u8,
    (queue_open, compact): (bool, bool),
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    let width = ui.available_width();
    if compact {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                queue_button(ui, queue_open, navs);
                title_column(ui, shown, ui.available_width());
            });
        });
    }
    ui.horizontal(|ui| {
        if !compact {
            title_column(
                ui,
                shown,
                (width * TITLE_SHARE).clamp(TITLE_MIN_PT, TITLE_MAX_PT),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !compact {
                queue_button(ui, queue_open, navs);
            }
            ui.add_enabled_ui(started, |ui| modes(ui, shown, actions));
            let inline = !compact && width >= VOLUME_SLIDER_FROM_PT;
            if cfg!(not(target_os = "macos")) {
                volume_control(ui, volume, actions, inline);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                controls(ui, shown, started, actions, inline);
            });
        });
    });
}

fn volume_slider(ui: &mut egui::Ui, percent: &mut u8) -> egui::Response {
    let slider = egui::Slider::new(percent, 0..=100).show_value(false);
    let response = ui.scope(|ui| {
        ui.spacing_mut().slider_width = VOLUME_SLIDER_PT;
        ui.add(slider)
    });
    let shown = *percent;
    super::keys::mark_slider(ui.ctx(), response.inner.id);
    let response = response.inner.on_hover_text(format!("{shown}%"));
    let enabled = response.enabled();
    response.widget_info(|| WidgetInfo::slider(enabled, f64::from(shown), "Volume"));
    response
}

fn volume_control(ui: &mut egui::Ui, current: u8, actions: &mut Vec<Action>, inline: bool) {
    let id = ui.id().with("volume");
    let held: Option<(u8, u8, f64)> = ui.data(|d| d.get_temp(id));
    let mut percent = held.map_or(current, |(shown, ..)| shown);
    let response = if inline {
        let response = volume_slider(ui, &mut percent);
        ui.label("🔈");
        Some(response)
    } else {
        let icon = labeled(ui.button("🔈"), WidgetType::Button, "Volume menu");
        egui::Popup::from_toggle_button_response(&icon)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| volume_slider(ui, &mut percent))
            .map(|popup| popup.inner)
    };
    let (stopped, dragging) = response
        .as_ref()
        .map_or((false, false), |r| (r.drag_stopped(), r.dragged()));
    let now = ui.input(|i| i.time);
    let (mut sent, mut at) = held.map_or((current, f64::NEG_INFINITY), |(_, sent, at)| (sent, at));
    if percent != sent && (stopped || now - at >= VOLUME_EVERY) {
        actions.push(Action::Volume(f32::from(percent) / 100.0));
        (sent, at) = (percent, now);
    }
    if percent != sent {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(VOLUME_EVERY));
    }
    if percent != sent || dragging {
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

fn controls(
    ui: &mut egui::Ui,
    shown: &Snapshot,
    started: bool,
    actions: &mut Vec<Action>,
    roomy: bool,
) {
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
        let font = egui::TextStyle::Body.resolve(ui.style());
        let end = ui
            .painter()
            .layout_no_wrap(clock(duration), font, egui::Color32::PLACEHOLDER);
        let clocks = if roomy {
            ROOMY_RESERVE_PT
        } else {
            end.size().x + ui.spacing().item_spacing.x
        };
        let room = ui.available_width() - clocks;
        ui.spacing_mut().slider_width = room.max(0.0);
        let slider = egui::Slider::new(&mut position, 0.0..=duration.max(1.0)).show_value(false);
        let response = labeled(
            ui.add_enabled(duration > 0.0, slider),
            WidgetType::Slider,
            "Position",
        );
        super::keys::mark_slider(ui.ctx(), response.id);
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            actions.push(Action::Seek(position));
        }
        ui.label(clock(duration));
    });
}
