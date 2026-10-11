use egui::{RichText, WidgetType};

use super::labeled;
use super::library::{self, Library};
use super::nav::{Nav, Route, Shell};
use super::search::{self, Search};
use crate::covers::Grid;
use crate::playback::Action;

pub fn search_id() -> egui::Id {
    egui::Id::new("bramley-search")
}

pub fn show(
    ui: &mut egui::Ui,
    (shell, search, library): (&mut Shell, &mut Search, &mut Library),
    covers: &Grid,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
    let title = ui.horizontal(|ui| {
        let back = ui.add_enabled(
            shell.can_go_back(),
            egui::Button::new(RichText::new("⬅").size(16.0)),
        );
        if labeled(back, WidgetType::Button, "Back").clicked() {
            navs.push(Nav::Back);
        }
        ui.heading(shell.route.title()).id
    });
    ui.add_space(6.0);
    match &shell.route {
        Route::Home => covers.show(ui),
        Route::Search => search::show(ui, (shell, search), title.inner, (actions, navs)),
        Route::Playlists | Route::Albums | Route::Songs => {
            library::show(ui, library, &shell.route, (actions, navs))
        }
        Route::Playlist(id) if id.starts_with("p.") => {
            library::show(ui, library, &shell.route, (actions, navs))
        }
        route => {
            let what = route.title().to_lowercase();
            ui.label(RichText::new(format!("Your {what} will appear here.")).weak());
        }
    }
}
