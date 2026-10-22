use bramley_engine::Snapshot;
use egui::{RichText, WidgetType};

use super::cursor::{self, Region};
use super::labeled;
use super::nav::Nav;
use super::player_bar::clock;
use crate::playback::Action;

pub fn jump(shown: &Snapshot, row: usize) -> Option<Action> {
    let current = row == 0 && shown.queue_position.is_some();
    let item = shown
        .queue
        .get(row)
        .filter(|item| !current && !item.id.is_empty())?;
    let at = shown.queue_position.unwrap_or(0);
    Some(Action::JumpTo(at + row, item.id.clone()))
}

fn row(
    ui: &mut egui::Ui,
    shown: &Snapshot,
    row: usize,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let item = &shown.queue[row];
    let title = item.title.as_deref().unwrap_or("Untitled");
    let length = item.duration.map(clock).unwrap_or_default();
    let current = row == 0 && shown.queue_position.is_some();
    let button = egui::Button::selectable(current, title)
        .right_text(length)
        .truncate();
    let response = ui.add_sized([ui.available_width(), 26.0], button);
    let response = labeled(response, WidgetType::Button, &format!("Play {title}"));
    if response.clicked() {
        actions.extend(jump(shown, row));
    }
    response
}

pub fn show(
    ui: &mut egui::Ui,
    shown: &Snapshot,
    (actions, navs): (&mut Vec<Action>, &mut Vec<Nav>),
) {
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
    if shown.queue.is_empty() {
        ui.label(RichText::new("Nothing queued.").weak());
        return;
    }
    let first = usize::from(shown.queue_position.is_some());
    if first == 1 {
        ui.label(RichText::new("Now playing").weak());
        row(ui, shown, 0, actions);
        ui.add_space(8.0);
    }
    let ahead = (shown.queue_length + first).saturating_sub(shown.queue_position.unwrap_or(0) + 1);
    if shown.queue.len() > first {
        ui.label(RichText::new(format!("Up next · {ahead}")).weak());
    }
    let rows = shown.queue.len() - first;
    let cursor = cursor::drive(
        ui.ctx(),
        egui::Id::new("queue-rows"),
        Region::Queue,
        (rows, 1),
        |_| true,
    );
    let area = egui::ScrollArea::vertical().auto_shrink(false);
    cursor
        .scroll(area, 26.0 + ui.spacing().item_spacing.y)
        .show_rows(ui, 26.0, rows, |ui, rows| {
            cursor.seen(&rows);
            for index in rows {
                let response = row(ui, shown, index + first, actions);
                cursor.mark(ui, index, &response);
            }
        });
}
