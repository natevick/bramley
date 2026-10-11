use bramley_engine::Snapshot;
use egui::{RichText, WidgetType};

use super::labeled;
use super::nav::Nav;
use super::player_bar::clock;
use crate::playback::Action;

pub fn jump(shown: &Snapshot, row: usize) -> Option<Action> {
    let item = shown.queue.get(row).filter(|_| row > 0)?;
    Some(Action::JumpTo(shown.queue_position? + row, item.id.clone()))
}

fn row(ui: &mut egui::Ui, shown: &Snapshot, row: usize, actions: &mut Vec<Action>) {
    let item = &shown.queue[row];
    let title = item.title.as_deref().unwrap_or("Untitled");
    let length = item.duration.map(clock).unwrap_or_default();
    let text = if row == 0 {
        RichText::new(title).strong()
    } else {
        RichText::new(title)
    };
    let button = egui::Button::selectable(row == 0, text)
        .right_text(length)
        .truncate();
    let response = ui.add_sized([ui.available_width(), 26.0], button);
    if labeled(response, WidgetType::Button, &format!("Play {title}")).clicked() {
        actions.extend(jump(shown, row));
    }
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
    ui.label(RichText::new("Now playing").weak());
    row(ui, shown, 0, actions);
    if shown.queue.len() > 1 {
        ui.add_space(8.0);
        let ahead = shown
            .queue_length
            .saturating_sub(shown.queue_position.unwrap_or(0) + 1);
        ui.label(RichText::new(format!("Up next · {ahead}")).weak());
    }
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        26.0,
        shown.queue.len() - 1,
        |ui, rows| {
            for index in rows {
                row(ui, shown, index + 1, actions);
            }
        },
    );
}
