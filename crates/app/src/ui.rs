use bramley_engine::{PlayState, Snapshot};
use egui::{Color32, RichText};

use crate::playback::{Action, Banner};

const ACCENT: Color32 = Color32::from_rgb(250, 45, 72);

fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn banner(ui: &mut egui::Ui, banner: &Banner) {
    let text = match banner {
        Banner::SignInNeeded => {
            "Sign in needed: Bramley has no Apple Music sign-in it can use.".to_owned()
        }
        Banner::Error(e) => format!("Playback error: {e}"),
    };
    egui::Frame::new()
        .fill(ACCENT)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(Color32::WHITE).strong());
        });
}

pub fn now_playing(ui: &mut egui::Ui, shown: &Snapshot, started: bool, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(260.0);
            let title = shown.title.as_deref().unwrap_or("Nothing playing");
            ui.label(RichText::new(title).strong().size(15.0));
            ui.label(RichText::new(shown.artist.as_deref().unwrap_or("")).weak());
        });
        ui.add_enabled_ui(started, |ui| {
            if ui.button(RichText::new("⏮").size(18.0)).clicked() {
                actions.push(Action::Previous);
            }
            let playing = matches!(shown.state, PlayState::Playing | PlayState::Loading);
            let (glyph, action) = if playing {
                ("⏸", Action::Pause)
            } else {
                ("⏵", Action::Play)
            };
            if ui.button(RichText::new(glyph).size(22.0)).clicked() {
                actions.push(action);
            }
            if ui.button(RichText::new("⏭").size(18.0)).clicked() {
                actions.push(Action::Next);
            }
            let duration = shown.duration.unwrap_or(0.0);
            let mut position = shown.position.unwrap_or(0.0).min(duration);
            ui.label(clock(position));
            ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
            let slider =
                egui::Slider::new(&mut position, 0.0..=duration.max(1.0)).show_value(false);
            let response = ui.add_enabled(duration > 0.0, slider);
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                actions.push(Action::Seek(position));
            }
            ui.label(clock(duration));
        });
    });
}
