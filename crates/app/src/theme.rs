use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Color32, Id, Theme as Mode, ThemePreference, Visuals};

#[cfg(test)]
mod tests;

const POLL: Duration = Duration::from_secs(1);
const GRACE: Duration = Duration::from_secs(3);
const TEXT_MIN: f32 = 4.5;
const WEAK_MIN: f32 = 3.0;
const FOCUS_MIN: f32 = 3.0;

#[cfg_attr(not(feature = "dev"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Choice {
    #[default]
    Omarchy,
    System,
    Fixed(Mode),
}

impl Choice {
    #[cfg(feature = "dev")]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "omarchy" => Some(Self::Omarchy),
            "system" => Some(Self::System),
            "dark" => Some(Self::Fixed(Mode::Dark)),
            "light" => Some(Self::Fixed(Mode::Light)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub panel: Color32,
    pub sidebar: Color32,
    pub extreme: Color32,
    pub inactive: Color32,
    pub hover: Color32,
    pub accent: Color32,
    pub border: Color32,
    pub text: Color32,
    pub strong: Color32,
    pub weak: Color32,
    pub alert: Color32,
}

pub fn luminance(c: Color32) -> f32 {
    let linear = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn fit(color: Color32, against: &[Color32], min: f32) -> Color32 {
    let mean = against.iter().map(|c| luminance(*c)).sum::<f32>() / against.len() as f32;
    let pole = if mean < 0.179 {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    (0..20)
        .map(|step| color.lerp_to_gamma(pole, step as f32 / 20.0))
        .find(|c| against.iter().all(|bg| contrast(*c, *bg) >= min))
        .unwrap_or(pole)
}

fn on(fill: Color32) -> Color32 {
    if contrast(Color32::WHITE, fill) >= contrast(Color32::BLACK, fill) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

impl Palette {
    pub fn plain(mode: Mode) -> Self {
        let v = match mode {
            Mode::Dark => Visuals::dark(),
            Mode::Light => Visuals::light(),
        };
        let w = &v.widgets;
        Self {
            dark: v.dark_mode,
            panel: v.panel_fill,
            sidebar: v.panel_fill,
            extreme: v.extreme_bg_color,
            inactive: w.inactive.bg_fill,
            hover: w.hovered.bg_fill,
            accent: v.selection.bg_fill,
            border: w.noninteractive.bg_stroke.color,
            text: w.noninteractive.fg_stroke.color,
            strong: w.active.fg_stroke.color,
            weak: w
                .noninteractive
                .fg_stroke
                .color
                .lerp_to_gamma(v.panel_fill, 0.4),
            alert: Color32::from_rgb(250, 45, 72),
        }
        .fitted()
    }

    pub fn omarchy(text: &str) -> Option<Self> {
        let mut entries = HashMap::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
            entries.insert(key.trim(), value);
        }
        let color = |key: &str| {
            let hex = entries.get(key)?.strip_prefix('#')?;
            let valid = hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit());
            let rgb = u32::from_str_radix(hex, 16).ok().filter(|_| valid)?;
            Some(Color32::from_rgb(
                (rgb >> 16) as u8,
                (rgb >> 8) as u8,
                rgb as u8,
            ))
        };
        let dark = match *entries.get("mode")? {
            "dark" => true,
            "light" => false,
            _ => return None,
        };
        Some(
            Self {
                dark,
                panel: color("background")?,
                sidebar: color("dark_background")?,
                extreme: color("darker_background")?,
                inactive: color("lighter_background")?,
                hover: color("selection")?,
                accent: color("accent")?,
                border: color("muted")?,
                text: color("foreground")?,
                strong: color("bright_foreground")?,
                weak: color("dark_foreground")?,
                alert: color("red")?,
            }
            .fitted(),
        )
    }

    fn surfaces(&self) -> [Color32; 3] {
        [self.panel, self.sidebar, self.extreme]
    }

    fn fitted(self) -> Self {
        let surfaces = self.surfaces();
        Self {
            text: fit(self.text, &surfaces, TEXT_MIN),
            strong: fit(self.strong, &surfaces, TEXT_MIN),
            weak: fit(self.weak, &surfaces, WEAK_MIN),
            ..self
        }
    }

    pub const ON_BANNER: Color32 = Color32::WHITE;

    pub fn banner(&self) -> Color32 {
        fit(self.alert, &[Self::ON_BANNER], TEXT_MIN)
    }

    pub fn notice(&self) -> Color32 {
        fit(self.alert, &[self.panel], TEXT_MIN)
    }

    pub fn focus(&self) -> Color32 {
        fit(self.accent, &self.surfaces(), FOCUS_MIN)
    }

    pub fn placeholder(&self) -> Color32 {
        self.panel.lerp_to_gamma(self.text, 0.15)
    }

    pub fn of(ctx: &egui::Context) -> Self {
        let mode = ctx.theme();
        ctx.data(|d| d.get_temp(Id::new(("palette", mode))))
            .unwrap_or_else(|| Self::plain(mode))
    }

    fn apply(&self, v: &mut Visuals) {
        let active = fit(self.accent, &[self.strong], TEXT_MIN);
        let hovered = fit(self.strong, &[self.hover], TEXT_MIN);
        let inactive = fit(self.text, &[self.inactive], TEXT_MIN);
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.window_stroke.color = self.border;
        v.extreme_bg_color = self.extreme;
        v.weak_text_color = Some(self.weak);
        v.error_fg_color = self.alert;
        v.slider_trailing_fill = true;
        v.selection.bg_fill = self.accent;
        v.selection.stroke.color = on(self.accent);
        let w = &mut v.widgets;
        w.noninteractive.bg_stroke.color = self.border;
        w.noninteractive.fg_stroke.color = self.text;
        w.hovered.bg_stroke.color = self.accent;
        w.active.bg_stroke.color = self.strong;
        for (state, fill, text) in [
            (&mut w.noninteractive, self.panel, self.text),
            (&mut w.inactive, self.inactive, inactive),
            (&mut w.hovered, self.hover, hovered),
            (&mut w.active, active, self.strong),
            (&mut w.open, self.hover, hovered),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.fg_stroke.color = text;
        }
    }
}

enum Read {
    Found(Palette),
    Missing,
    Malformed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    Plain(ThemePreference),
    Omarchy(Palette),
}

pub struct Theme {
    choice: Choice,
    file: Option<PathBuf>,
    shown: Option<Look>,
    due: Instant,
    gone: Option<Instant>,
    warned: bool,
}

pub fn colors_file(state_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = state_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(PathBuf::from(home?).join(".local/state")))?;
    Some(base.join("omarchy/current/theme/colors.toml"))
}

impl Theme {
    pub fn new(choice: Choice, file: Option<PathBuf>, now: Instant) -> Self {
        Self {
            choice,
            file: file.filter(|_| choice == Choice::Omarchy),
            shown: None,
            due: now,
            gone: None,
            warned: false,
        }
    }

    fn read(&self) -> Read {
        let Some(file) = &self.file else {
            return Read::Missing;
        };
        match std::fs::read_to_string(file) {
            Ok(text) => Palette::omarchy(&text).map_or(Read::Malformed, Read::Found),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Read::Missing,
            Err(_) => Read::Malformed,
        }
    }

    pub fn tick(&mut self, ctx: &egui::Context, now: Instant) {
        if now < self.due {
            return;
        }
        self.due = now + POLL;
        let plain = Look::Plain(match self.choice {
            Choice::Fixed(mode) => mode.into(),
            _ => ThemePreference::System,
        });
        let look = match self.read() {
            Read::Found(palette) => {
                (self.gone, self.warned) = (None, false);
                Look::Omarchy(palette)
            }
            Read::Missing => {
                let since = *self.gone.get_or_insert(now);
                match self.shown {
                    Some(shown) if now.duration_since(since) < GRACE => shown,
                    _ => plain,
                }
            }
            Read::Malformed => {
                if !std::mem::replace(&mut self.warned, true) {
                    log::warn!("omarchy colors.toml is unreadable; using the normal theme");
                }
                plain
            }
        };
        if self.shown != Some(look) {
            self.shown = Some(look);
            install(ctx, look);
            ctx.request_repaint();
        }
    }
}

fn install(ctx: &egui::Context, look: Look) {
    let (preference, palette) = match look {
        Look::Plain(preference) => (preference, None),
        Look::Omarchy(p) => (
            if p.dark { Mode::Dark } else { Mode::Light }.into(),
            Some(p),
        ),
    };
    for mode in [Mode::Dark, Mode::Light] {
        let palette = palette
            .filter(|p| p.dark == (mode == Mode::Dark))
            .unwrap_or_else(|| Palette::plain(mode));
        ctx.style_mut_of(mode, |style| palette.apply(&mut style.visuals));
        ctx.data_mut(|d| d.insert_temp(Id::new(("palette", mode)), palette));
    }
    ctx.set_theme(preference);
}
