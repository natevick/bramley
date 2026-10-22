pub mod content;
pub mod cursor;
pub mod detail;
pub mod keys;
pub mod library;
pub mod nav;
pub mod player_bar;
pub mod queue_panel;
pub mod search;
pub mod sidebar;
#[cfg(test)]
pub(crate) mod tests;

use egui::{Color32, Key, Modifiers, RichText, WidgetInfo, WidgetType};

use crate::playback::{Action, Banner};
use crate::theme::Palette;
use nav::Nav;

pub const MIN_WINDOW: [f32; 2] = [360.0, 480.0];
pub const COMPACT_PT: f32 = 640.0;
pub const CONTENT_PANEL_MIN_PT: f32 = 336.0;
pub const QUEUE_PT: f32 = 260.0;
pub const DRAWER_PT: f32 = 260.0;
const OVERLAY_MARGIN_PT: f32 = 8.0;

pub fn overlay(
    ui: &egui::Ui,
    name: &str,
    (rect, fill): (egui::Rect, Color32),
    add: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let area = egui::Area::new(egui::Id::new(name))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false);
    area.show(ui.ctx(), |ui| {
        ui.set_min_size(rect.size());
        ui.painter().rect_filled(rect, 0.0, fill);
        let inner = egui::UiBuilder::new().max_rect(rect.shrink(OVERLAY_MARGIN_PT));
        ui.scope_builder(inner, add);
    })
    .response
}

pub fn banner(ui: &mut egui::Ui, banner: &Banner, offer_sign_in: bool) -> bool {
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
        .fill(Palette::of(ui.ctx()).banner())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(text).color(Palette::ON_BANNER).strong());
                sign_in = offer_sign_in && ui.button("Sign in").clicked();
            });
        });
    sign_in
}

pub fn notice(ui: &mut egui::Ui, notice: &str) {
    let color = Palette::of(ui.ctx()).notice();
    ui.label(RichText::new(notice).color(color).strong());
}

pub fn track_menu(response: &egui::Response, id: &str, actions: &mut Vec<Action>) {
    if response.has_focus() {
        keys::focused_song(&response.ctx, id);
    }
    response.context_menu(|ui| {
        if ui.button("Play next").clicked() {
            actions.push(Action::PlayNext(id.to_owned()));
        }
        if ui.button("Add to queue").clicked() {
            actions.push(Action::QueueAdd(id.to_owned()));
        }
    });
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
