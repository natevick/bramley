use egui::{RichText, WidgetType};

use super::detail::{self, Detail};
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
    (shell, search, library, detail): (&mut Shell, &mut Search, &mut Library, &mut Detail),
    covers: &Grid,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
    compact: bool,
) {
    let title = ui.horizontal(|ui| {
        if compact {
            let menu = ui.button(RichText::new("☰").size(16.0));
            if labeled(menu, WidgetType::Button, "Menu").clicked() {
                navs.push(Nav::ToggleDrawer);
            }
        }
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
        Route::Home => covers.show(ui, navs),
        Route::Search => search::show(ui, (shell, search), title.inner, (actions, navs)),
        route if route.lists() => {
            library::show(ui, library, (route, &mut shell.scroll), (actions, navs))
        }
        route if detail::container(route).is_some() => detail::show(ui, detail, actions),
        route => {
            let what = route.title().to_lowercase();
            ui.label(RichText::new(format!("Your {what} will appear here.")).weak());
        }
    }
}
