use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::json;

use crate::Args;
use crate::playback::{Action, Playback, Spawn};
use crate::ui;

const REPAINT_EVERY: Duration = Duration::from_secs(1);

struct Shot {
    path: PathBuf,
    after: Duration,
    asked: bool,
}

pub struct App {
    playback: Playback,
    actions: Vec<Action>,
    status: Option<PathBuf>,
    shot: Option<Shot>,
    frames: u64,
    renderer: Option<String>,
    born: Instant,
    status_at: Option<Instant>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        args: Args,
        spawn: Spawn,
        signed_out: bool,
    ) -> Self {
        let rendering = fastframe_text::detect();
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        rendering.apply_to(&mut fonts);
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx
            .all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
        #[cfg(feature = "dev")]
        let actions = args.play.into_iter().map(Action::PlaySong).collect();
        #[cfg(not(feature = "dev"))]
        let actions = Vec::new();
        App {
            playback: Playback::new(spawn, signed_out),
            actions,
            status: args.status,
            shot: args.screenshot.map(|path| Shot {
                path,
                after: args.after,
                asked: false,
            }),
            frames: 0,
            renderer: None,
            born: Instant::now(),
            status_at: None,
        }
    }

    fn write_status(&mut self) {
        let Some(path) = &self.status else { return };
        let shown = self.playback.shown();
        let status = json!({
            "pid": std::process::id(),
            "engineStarted": self.playback.started(),
            "state": format!("{:?}", shown.state),
            "itemId": shown.item_id,
            "title": shown.title,
            "artist": shown.artist,
            "position": shown.position,
            "duration": shown.duration,
            "applied": shown.applied,
            "seq": self.playback.seq(),
            "banner": self.playback.banner().map(|b| format!("{b:?}")),
            "framesPainted": self.frames,
            "glRenderer": self.renderer,
        });
        let partial = path.with_extension("partial");
        let written = std::fs::write(&partial, status.to_string());
        if let Err(e) = written.and_then(|()| std::fs::rename(&partial, path)) {
            eprintln!("bramley: status not written to {}: {e}", path.display());
        }
        self.status_at = Some(Instant::now());
    }

    fn drive_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else {
            return;
        };
        ctx.request_repaint();
        if !shot.asked && self.frames > 0 && self.born.elapsed() >= shot.after {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.asked = true;
        }
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        if let Err(e) = write_png(&shot.path, &image) {
            eprintln!("bramley: screenshot not written: {e:#}");
        }
        self.shot = None;
        self.write_status();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.renderer.is_none() {
            use eframe::glow::HasContext;
            self.renderer = frame
                .gl()
                .map(|gl| unsafe { gl.get_parameter_string(eframe::glow::RENDERER) });
        }
        let changed = self.playback.receive();
        if changed
            || self
                .status_at
                .is_none_or(|at| at.elapsed() >= REPAINT_EVERY)
        {
            self.write_status();
        }
        self.drive_shot(ctx);
        ctx.request_repaint_after(REPAINT_EVERY);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frames += 1;
        let shown = self.playback.shown();
        egui::Panel::bottom("now-playing").show(ui, |ui| {
            ui.add_space(8.0);
            ui::now_playing(ui, &shown, self.playback.started(), &mut self.actions);
            ui.add_space(8.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(banner) = self.playback.banner() {
                ui::banner(ui, &banner);
            }
            ui.heading("Bramley");
        });
        for action in self.actions.drain(..) {
            self.playback.apply(action);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.playback.shutdown();
    }
}

fn write_png(path: &Path, image: &egui::ColorImage) -> Result<()> {
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let [w, h] = image.size.map(|n| n as u32);
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let pixels: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
