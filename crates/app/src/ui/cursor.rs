use std::collections::HashMap;
use std::ops::Range;

use egui::{Context, Id, Key, Response, Stroke, StrokeKind, Ui};

use crate::theme::Palette;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Up,
    Down,
    Left,
    Right,
    First,
    Last,
    PageUp,
    PageDown,
}

impl Step {
    pub fn of(key: Key) -> Option<Self> {
        Some(match key {
            Key::ArrowUp => Self::Up,
            Key::ArrowDown => Self::Down,
            Key::ArrowLeft => Self::Left,
            Key::ArrowRight => Self::Right,
            Key::Home => Self::First,
            Key::End => Self::Last,
            Key::PageUp => Self::PageUp,
            Key::PageDown => Self::PageDown,
            _ => return None,
        })
    }

    fn from(self, at: Option<usize>, page: usize, (last, columns): (usize, usize)) -> usize {
        let now = at.unwrap_or(0);
        match self {
            Self::Up if now >= columns => now - columns,
            Self::Down if at.is_none() => 0,
            Self::Down if now / columns < last / columns => (now + columns).min(last),
            Self::Up | Self::Down => now,
            Self::Left => now.saturating_sub(1),
            Self::Right => (now + 1).min(last),
            Self::First => 0,
            Self::Last => last,
            Self::PageUp => now.saturating_sub(page),
            Self::PageDown => (now + page).min(last),
        }
    }

    fn onward(self) -> Self {
        match self {
            Self::First | Self::PageDown => Self::Down,
            Self::Last | Self::PageUp => Self::Up,
            step => step,
        }
    }

    fn land(
        self,
        at: Option<usize>,
        page: usize,
        shape: (usize, usize),
        enabled: &impl Fn(usize) -> bool,
    ) -> Option<usize> {
        let mut to = self.from(at, page, shape);
        while !enabled(to) {
            let next = self.onward().from(Some(to), page, shape);
            if next == to {
                return at;
            }
            to = next;
        }
        Some(to)
    }
}

#[derive(Clone, Default)]
struct Slot {
    at: Option<usize>,
    seen: Range<usize>,
}

#[derive(Clone, Default)]
struct Focus {
    steps: Vec<Step>,
    slots: HashMap<Id, Slot>,
}

fn with<R>(ctx: &Context, f: impl FnOnce(&mut Focus) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<Focus>(Id::new("cursor-focus"))))
}

pub fn push(ctx: &Context, step: Step) {
    with(ctx, |focus| focus.steps.push(step));
}

pub fn settle(ctx: &Context) {
    with(ctx, |focus| focus.steps.clear());
}

pub struct Cursor {
    ctx: Context,
    id: Id,
    at: Option<usize>,
    seen: Range<usize>,
    columns: usize,
    moved: bool,
}

pub fn drive(
    ctx: &Context,
    id: Id,
    (len, columns): (usize, usize),
    enabled: impl Fn(usize) -> bool,
) -> Cursor {
    let (at, seen, moved) = with(ctx, |focus| {
        let slot = focus.slots.entry(id).or_default();
        let steps = if len == 0 {
            vec![]
        } else {
            std::mem::take(&mut focus.steps)
        };
        let page = slot.seen.len().max(1) * columns;
        for step in &steps {
            if columns > 1 || !matches!(step, Step::Left | Step::Right) {
                slot.at = step.land(slot.at, page, (len - 1, columns), &enabled);
            }
        }
        slot.at = slot
            .at
            .map(|at| at.min(len.saturating_sub(1)))
            .filter(|_| len > 0);
        (slot.at, slot.seen.clone(), !steps.is_empty())
    });
    Cursor {
        ctx: ctx.clone(),
        id,
        at,
        seen,
        columns,
        moved,
    }
}

impl Cursor {
    pub fn scroll(&self, area: egui::ScrollArea, row_pt: f32) -> egui::ScrollArea {
        let Some(at) = self.at.filter(|_| self.moved) else {
            return area;
        };
        let (at, whole) = (at / self.columns, self.seen.len().max(2) - 1);
        let last = self.seen.start + whole - 1;
        match (at < self.seen.start, at > last) {
            (true, _) => area.vertical_scroll_offset(at as f32 * row_pt),
            (_, true) => area.vertical_scroll_offset((at + 1 - whole) as f32 * row_pt),
            _ => area,
        }
    }

    pub fn seen(&self, rows: &Range<usize>) {
        with(&self.ctx, |focus| {
            focus.slots.entry(self.id).or_default().seen = rows.clone()
        });
    }

    pub fn mark(&self, ui: &Ui, index: usize, response: &Response) -> bool {
        if self.at != Some(index) {
            return false;
        }
        if self.moved {
            response.request_focus();
            ui.ctx().request_repaint();
        }
        if response.has_focus() {
            let ring = Stroke::new(2.0, Palette::of(ui.ctx()).focus());
            ui.painter()
                .rect_stroke(response.rect, 3.0, ring, StrokeKind::Outside);
        }
        self.moved
    }
}
