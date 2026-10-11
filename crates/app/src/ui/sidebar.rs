use egui::RichText;

use super::nav::{Nav, Route};

fn entry(ui: &mut egui::Ui, current: &Route, route: Route, navs: &mut Vec<Nav>) {
    let size = [ui.available_width(), 28.0];
    let item = egui::Button::selectable(*current == route, route.title());
    if ui.add_sized(size, item).clicked() {
        navs.push(Nav::Go(route));
    }
}

pub fn show(ui: &mut egui::Ui, current: &Route, navs: &mut Vec<Nav>) {
    ui.add_space(10.0);
    ui.heading("Bramley");
    ui.add_space(10.0);
    for route in [Route::Home, Route::Search] {
        entry(ui, current, route, navs);
    }
    ui.separator();
    ui.label(RichText::new("Your Library").strong());
    for route in [Route::Playlists, Route::Albums, Route::Songs] {
        entry(ui, current, route, navs);
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Your playlists will be listed here.").weak());
}

pub fn account(ui: &mut egui::Ui, signed_in: bool, storage: Option<&str>) -> bool {
    let mut sign_out = false;
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(10.0);
        sign_out = signed_in && ui.button("Sign out").clicked();
        if let Some(mode) = storage {
            super::storage(ui, mode);
        }
    });
    sign_out
}
