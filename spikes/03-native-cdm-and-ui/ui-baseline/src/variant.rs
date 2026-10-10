use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, FontData, FontDefinitions, FontId};

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

#[derive(Clone, Copy, Default)]
pub struct Parts {
    pub list: bool,
    pub fonts: bool,
    pub fallbacks: bool,
    pub theme_text: bool,
    pub icons: bool,
    pub emoji: bool,
    pub mmap_cjk: bool,
    pub ballast_mib: usize,
}

pub const BARELIST: Parts = Parts { list: true, fonts: false, fallbacks: false, theme_text: false, icons: false, emoji: false, mmap_cjk: false, ballast_mib: 0 };
pub const FULL: Parts = Parts { fonts: true, fallbacks: true, theme_text: true, icons: true, ..BARELIST };

const CJK_FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";

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
    parts: Parts,
    rows: Vec<Row>,
    ballast: Vec<Vec<u8>>,
}

fn log_fallbacks() -> usize {
    let mut total = 0;
    for face in fastframe_fonts::system::fallbacks() {
        eprintln!("SPIKE fallback name={} index={} bytes={}", face.name, face.index, face.bytes.len());
        total += face.bytes.len();
    }
    eprintln!("SPIKE fallback_faces={} fallback_bytes={total}", fastframe_fonts::system::fallbacks().len());
    total
}

fn push_mapped_cjk(fonts: &mut FontDefinitions) {
    let path = std::env::var("S0_CJK_FONT").unwrap_or_else(|_| CJK_FONT.to_owned());
    let index = std::env::var("S0_CJK_INDEX").ok().and_then(|i| i.parse().ok()).unwrap_or(2);
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("open {path}: {e}"));
    let map: &'static memmap2::Mmap = Box::leak(Box::new(unsafe { memmap2::Mmap::map(&file) }.expect("mmap")));
    let mut data = FontData::from_static(&map[..]);
    data.index = index;
    fonts.font_data.insert("mapped-cjk".to_owned(), Arc::new(data));
    for family in fonts.families.values_mut() {
        family.push("mapped-cjk".to_owned());
    }
    eprintln!("SPIKE mapped_cjk path={path} index={index} bytes={}", map.len());
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, started: Instant, parts: Parts) -> Self {
        let ctx = &cc.egui_ctx;
        let rendering = parts.theme_text.then(fastframe_text::detect);
        if parts.fonts || parts.mmap_cjk || rendering.is_some() {
            let mut fonts = if parts.fonts { fastframe_fonts::FontSetup::default().system_fallbacks(parts.fallbacks).definitions() } else { FontDefinitions::default() };
            if let Some(rendering) = &rendering {
                rendering.apply_to(&mut fonts);
            }
            if parts.mmap_cjk {
                push_mapped_cjk(&mut fonts);
            }
            ctx.set_fonts(fonts);
        }
        let fallback_bytes = if parts.fonts && parts.fallbacks { log_fallbacks() } else { 0 };
        if parts.icons {
            fastframe_icons::install::<Icon>(ctx);
        }
        if parts.emoji {
            ctx.add_plugin(fastframe_emoji::EmojiPlugin::default());
        }
        if let Some(rendering) = &rendering {
            let tokyo = fastframe_theme::presets::FILES.iter().find(|(name, _)| name.starts_with("Tokyo")).map(|(_, json)| *json).unwrap_or("{}");
            let palette: Colors = fastframe_theme::parse_palette(tokyo).unwrap_or_else(|_| <Colors as fastframe_theme::Palette>::base(fastframe_theme::Base::Dark));
            let mut visuals = if palette.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
            visuals.window_fill = palette.window;
            visuals.panel_fill = palette.panel;
            visuals.override_text_color = Some(palette.text);
            visuals.selection.bg_fill = palette.accent;
            ctx.set_visuals(visuals);
            ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
        }
        let rows = if parts.list { (0..1000).map(|i| row(i, parts.mmap_cjk && std::env::var_os("S0_NO_CJK_ROWS").is_none())).collect() } else { Vec::new() };
        let ballast: Vec<Vec<u8>> = (0..parts.ballast_mib).map(|_| vec![0xA5u8; 1 << 20]).collect();
        eprintln!(
            "SPIKE components=list:{} fonts:{} fallbacks:{} theme_text:{} icons:{} emoji:{} mmap_cjk:{} ballast_mib:{} fallback_bytes={fallback_bytes}",
            parts.list, parts.fonts, parts.fallbacks, parts.theme_text, parts.icons, parts.emoji, parts.mmap_cjk, ballast.len()
        );
        App { started, logged: false, parts, rows, ballast }
    }
}

fn row(i: usize, cjk: bool) -> Row {
    let title = if cjk && i % 10 == 0 { format!("第{i}曲 東京の夜に聴く静かな歌 한국어 노래") } else { format!("Track number {i} with a reasonably long title") };
    Row { title, artist: format!("Artist {}", i % 37), album: format!("Album {}", i % 91), duration: format!("{}:{:02}", 2 + i % 4, (i * 7) % 60) }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::report_frame(self.started, &mut self.logged, frame);
        black_box(&self.ballast);
        let parts = self.parts;
        let font = |weight: fastframe_fonts::Weight, size: f32| if parts.fonts { weight.font_id(size) } else { FontId::proportional(size) };
        ui.horizontal(|ui| {
            if parts.icons {
                ui.add(Icon::Left.image(Color32::WHITE, 18.0));
                ui.add(Icon::Right.image(Color32::WHITE, 18.0));
            }
            ui.label(egui::RichText::new("Library").font(font(fastframe_fonts::Weight::SemiBold, 20.0)));
            if parts.emoji {
                ui.label("\u{1F3B5} \u{1F600} \u{1F1EF}\u{1F1F5} \u{2764}\u{FE0F}");
            }
        });
        ui.separator();
        if !parts.list {
            ui.label("idle");
            return;
        }
        let row_height = 28.0;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_height, self.rows.len(), |ui, range| {
            for i in range {
                let row = &self.rows[i];
                ui.horizontal(|ui| {
                    ui.set_height(row_height);
                    if parts.icons {
                        ui.add(Icon::Clock.image(Color32::GRAY, 14.0));
                    }
                    ui.label(egui::RichText::new(&row.title).font(font(fastframe_fonts::Weight::Medium, 14.0)));
                    ui.label(&row.artist);
                    ui.label(&row.album);
                    ui.label(&row.duration);
                    if parts.icons {
                        ui.add(Icon::More.image(Color32::GRAY, 14.0));
                    }
                });
            }
        });
    }
}

pub fn run(name: &str, parts: Parts) -> eframe::Result {
    let started = Instant::now();
    crate::install_logger();
    if parts.emoji {
        fastframe_emoji::EmojiSetup::default().install();
        std::thread::spawn(fastframe_emoji::warm_up);
    }
    eframe::run_native(&format!("s0-{name}"), crate::options(), Box::new(move |cc| Ok(Box::new(App::new(cc, started, parts)))))
}
