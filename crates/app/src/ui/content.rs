use egui::{RichText, WidgetType};

use super::labeled;
use super::nav::{Nav, Route, Shell};
use crate::covers::Grid;

pub fn search_id() -> egui::Id {
    egui::Id::new("bramley-search")
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell, covers: &Grid, navs: &mut Vec<Nav>) {
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
    match shell.route {
        Route::Home => covers.show(ui),
        Route::Search => search(ui, shell, title.inner),
        Route::Playlists | Route::Albums | Route::Songs => {
            let what = shell.route.title().to_lowercase();
            ui.label(RichText::new(format!("Your {what} will appear here.")).weak());
        }
    }
}

fn search(ui: &mut egui::Ui, shell: &mut Shell, title: egui::Id) {
    let field = egui::TextEdit::singleline(&mut shell.query)
        .id(search_id())
        .hint_text("Search Apple Music")
        .desired_width(f32::INFINITY);
    let response = ui.add(field).labelled_by(title);
    if std::mem::take(&mut shell.focus_search) {
        response.request_focus();
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Results will appear here.").weak());
}
