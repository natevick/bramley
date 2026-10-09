use std::time::Instant;

use egui::Color32;

fastframe_icons::icons! {
    pub enum Icon {
        prefix: "spike3a-icon-",
        directory: "../",
        Pause => lucide "pause",
        Check => lucide "check",
        Clock => lucide "clock",
        More => lucide "ellipsis",
        Left => lucide "chevron-left",
        Right => lucide "chevron-right",
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Colors {
    dark: bool,
    window: Color32,
    panel: Color32,
    text: Color32,
    accent: Color32,
}

impl fastframe_theme::Palette for Colors {
    fn base(base: fastframe_theme::Base) -> Self {
        match base {
            fastframe_theme::Base::Dark => Colors { dark: true, window: Color32::from_gray(20), panel: Color32::from_gray(28), text: Color32::from_gray(230), accent: Color32::from_rgb(250, 45, 72) },
            fastframe_theme::Base::Light => Colors { dark: false, window: Color32::WHITE, panel: Color32::from_gray(245), text: Color32::from_gray(20), accent: Color32::from_rgb(250, 45, 72) },
        }
    }
    fn set(&mut self, name: &str, color: Color32) -> bool {
        match name {
            "window" => self.window = color,
            "panel" => self.panel = color,
            "text" => self.text = color,
            "accent" => self.accent = color,
            _ => return false,
        }
        true
    }
}

struct Row {
    title: String,
    artist: String,
    album: String,
    duration: String,
}

struct App {
    started: Instant,
    logged: bool,
    rows: Vec<Row>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, started: Instant) -> Self {
        let ctx = &cc.egui_ctx;
        let rendering = fastframe_text::detect();
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        rendering.apply_to(&mut fonts);
        ctx.set_fonts(fonts);
        fastframe_icons::install::<Icon>(ctx);
        let tokyo = fastframe_theme::presets::FILES.iter().find(|(name, _)| name.starts_with("Tokyo")).map(|(_, json)| *json).unwrap_or("{}");
        let palette: Colors = fastframe_theme::parse_palette(tokyo).unwrap_or_else(|_| <Colors as fastframe_theme::Palette>::base(fastframe_theme::Base::Dark));
        let mut visuals = if palette.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        visuals.window_fill = palette.window;
        visuals.panel_fill = palette.panel;
        visuals.override_text_color = Some(palette.text);
        visuals.selection.bg_fill = palette.accent;
        ctx.set_visuals(visuals);
        ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
        let rows = (0..1000)
            .map(|i| Row {
                title: format!("Track number {i} with a reasonably long title"),
                artist: format!("Artist {}", i % 37),
                album: format!("Album {}", i % 91),
                duration: format!("{}:{:02}", 2 + i % 4, (i * 7) % 60),
            })
            .collect();
        App { started, logged: false, rows }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui_baseline::report_frame(self.started, &mut self.logged, frame);
        ui.horizontal(|ui| {
            ui.add(Icon::Left.image(Color32::WHITE, 18.0));
            ui.add(Icon::Right.image(Color32::WHITE, 18.0));
            ui.label(egui::RichText::new("Library").font(fastframe_fonts::Weight::SemiBold.font_id(20.0)));
        });
        ui.separator();
        let row_height = 28.0;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_height, self.rows.len(), |ui, range| {
            for i in range {
                let row = &self.rows[i];
                ui.horizontal(|ui| {
                    ui.set_height(row_height);
                    ui.add(Icon::Clock.image(Color32::GRAY, 14.0));
                    ui.label(egui::RichText::new(&row.title).font(fastframe_fonts::Weight::Medium.font_id(14.0)));
                    ui.label(&row.artist);
                    ui.label(&row.album);
                    ui.label(&row.duration);
                    ui.add(Icon::More.image(Color32::GRAY, 14.0));
                });
            }
        });
    }
}

fn main() -> eframe::Result {
    let started = Instant::now();
    eframe::run_native("spike3a-fastframe", ui_baseline::options(), Box::new(move |cc| Ok(Box::new(App::new(cc, started)))))
}
