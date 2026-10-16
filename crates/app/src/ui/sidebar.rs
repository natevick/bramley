use egui::collapsing_header::paint_default_icon;
use egui::{RichText, Sense, WidgetType, vec2};

use super::labeled;
use super::library::Library;
use super::nav::{Nav, Route};

const ENTRY_PT: f32 = 28.0;
const FOOTER_PT: f32 = 90.0;
const CHEVRON_PT: f32 = 24.0;

fn entry(ui: &mut egui::Ui, current: &Route, route: Route, navs: &mut Vec<Nav>) -> egui::Response {
    let size = [ui.available_width(), ENTRY_PT];
    let item = egui::Button::selectable(*current == route, route.title());
    let response = ui.add_sized(size, item);
    if response.clicked() {
        navs.push(Nav::Go(route));
    }
    response
}

fn chevron(ui: &mut egui::Ui, link: &egui::Response, collapsed: &mut bool) {
    let rect =
        egui::Rect::from_min_max(link.rect.right_top() - vec2(CHEVRON_PT, 0.0), link.rect.max);
    let toggle = ui.interact(rect, link.id.with("fold"), Sense::click());
    if toggle.clicked() {
        *collapsed = !*collapsed;
    }
    let label = if *collapsed {
        "Expand playlists"
    } else {
        "Collapse playlists"
    };
    let toggle = labeled(toggle, WidgetType::Button, label);
    let icon = egui::Rect::from_center_size(
        rect.center(),
        vec2(ui.spacing().icon_width, ui.spacing().icon_width) * 0.6,
    );
    paint_default_icon(
        ui,
        if *collapsed { 0.0 } else { 1.0 },
        &toggle.with_new_rect(icon),
    );
}

pub fn show(ui: &mut egui::Ui, current: &Route, library: &mut Library, navs: &mut Vec<Nav>) {
    ui.add_space(10.0);
    ui.heading("Bramley");
    ui.add_space(10.0);
    for route in [Route::Home, Route::Search] {
        entry(ui, current, route, navs);
    }
    ui.separator();
    ui.label(RichText::new("Your Library").strong());
    for route in [Route::Artists, Route::Albums, Route::Songs] {
        entry(ui, current, route, navs);
    }
    let link = entry(ui, current, Route::Playlists, navs);
    chevron(ui, &link, &mut library.playlists_collapsed);
    if !library.playlists_collapsed {
        let height = ui.available_height() - FOOTER_PT;
        ui.indent("sidebar-playlists-nest", |ui| {
            super::library::sidebar(ui, library, current, navs, height)
        });
    }
}

pub fn account(ui: &mut egui::Ui, signed_in: bool) -> bool {
    let mut sign_out = false;
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(10.0);
        sign_out = signed_in && ui.button("Sign out").clicked();
        if signed_in {
            ui.label(RichText::new("Signed in to Apple Music").weak());
        }
    });
    sign_out
}
