use egui::{RichText, WidgetType};

use super::labeled;
use super::nav::Nav;

pub fn show(ui: &mut egui::Ui, navs: &mut Vec<Nav>) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.heading("Queue");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = labeled(ui.button("🗙"), WidgetType::Button, "Close queue");
            if close.clicked() {
                navs.push(Nav::ToggleQueue);
            }
        });
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Nothing queued.").weak());
}
