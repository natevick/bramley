pub mod content;
pub mod nav;
pub mod player_bar;
pub mod queue_panel;
pub mod search;
pub mod sidebar;
#[cfg(test)]
mod tests;

use egui::{Color32, Key, Modifiers, RichText, WidgetInfo, WidgetType};

use crate::playback::{Action, Banner};
use nav::Nav;

const ACCENT: Color32 = Color32::from_rgb(250, 45, 72);
const ACCENT_ON_LIGHT: Color32 = Color32::from_rgb(200, 25, 50);

pub fn banner(ui: &mut egui::Ui, banner: &Banner) -> bool {
    let text = match banner {
        Banner::SignInNeeded => {
            "Sign in needed: Bramley has no Apple Music sign-in it can use.".to_owned()
        }
        Banner::SigningIn(url) => {
            format!("Finish signing in to Apple Music in your browser: {url}")
        }
        Banner::Error(e) => format!("Playback error: {e}"),
    };
    let mut sign_in = false;
    egui::Frame::new()
        .fill(ACCENT)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(text).color(Color32::WHITE).strong());
                sign_in = *banner == Banner::SignInNeeded && ui.button("Sign in").clicked();
            });
        });
    sign_in
}

pub fn storage(ui: &mut egui::Ui, mode: &str) {
    let text = match mode {
        "keyring" => "Sign-in kept in the system keyring",
        _ => "Sign-in kept in a private file: this system has no keyring service",
    };
    ui.label(RichText::new(text).weak());
}

pub fn notice(ui: &mut egui::Ui, notice: &str) {
    let color = if ui.visuals().dark_mode {
        ACCENT
    } else {
        ACCENT_ON_LIGHT
    };
    ui.label(RichText::new(notice).color(color).strong());
}

pub fn labeled(response: egui::Response, kind: WidgetType, label: &str) -> egui::Response {
    let enabled = response.enabled();
    response.widget_info(|| WidgetInfo::labeled(kind, enabled, label));
    response
}

pub fn shortcuts(
    ctx: &egui::Context,
    toggle: Option<Action>,
    actions: &mut Vec<Action>,
    navs: &mut Vec<Nav>,
) {
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::L)) {
        navs.push(Nav::FocusSearch);
    }
    let Some(toggle) = toggle.filter(|_| !ctx.text_edit_focused()) else {
        return;
    };
    let fresh = |e: &egui::Event| {
        matches!(
            e,
            egui::Event::Key {
                key: Key::Space,
                pressed: true,
                repeat: false,
                ..
            }
        )
    };
    let fresh = ctx.input(|i| i.events.iter().any(fresh));
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space)) && fresh {
        actions.push(toggle);
    }
}
