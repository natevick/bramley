use std::time::Duration;

use egui::{Context, Id, Rect};

pub(crate) const WAKE_EVERY: Duration = Duration::from_secs(30);
const SLIVER_PT: f32 = 8.0;

pub(crate) fn is_parked(window: Option<Rect>, displays: &[Rect]) -> bool {
    let Some(window) = window else { return false };
    !displays.is_empty()
        && displays.iter().all(|display| {
            let seen = window.intersect(*display);
            seen.width().min(seen.height()) <= SLIVER_PT
        })
}

pub(crate) fn window(ctx: &Context) -> Option<Rect> {
    let outer = ctx.input(|input| input.viewport().outer_rect)?;
    Some(outer * ctx.zoom_factor())
}

fn flag() -> Id {
    Id::new("bramley-parked")
}

pub(crate) fn publish(ctx: &Context, parked: bool) {
    ctx.data_mut(|data| data.insert_temp(flag(), parked));
}

pub(crate) fn wake(ctx: &Context) {
    if !ctx
        .data(|data| data.get_temp::<bool>(flag()))
        .unwrap_or(false)
    {
        ctx.request_repaint();
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn displays() -> Vec<Rect> {
    Vec::new()
}

#[cfg(target_os = "macos")]
pub(crate) fn displays() -> Vec<Rect> {
    #[repr(C)]
    struct CgRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetActiveDisplayList(max: u32, ids: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayBounds(id: u32) -> CgRect;
    }

    let (mut ids, mut count) = ([0_u32; 16], 0_u32);
    let listed = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
    if listed != 0 {
        return Vec::new();
    }
    ids.iter()
        .take(count as usize)
        .map(|id| {
            let bounds = unsafe { CGDisplayBounds(*id) };
            let origin = egui::pos2(bounds.x as f32, bounds.y as f32);
            Rect::from_min_size(
                origin,
                egui::vec2(bounds.width as f32, bounds.height as f32),
            )
        })
        .collect()
}
