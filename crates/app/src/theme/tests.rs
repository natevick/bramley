use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bramley_engine::{QueueItem, Snapshot};
use egui::{Color32, Shape, Theme as Mode, Visuals};

use super::{Choice, Look, Palette, Theme, colors_file, contrast, install};
use crate::ui::queue_panel;

const TOKYO: &str = include_str!("fixtures/tokyo-night.toml");
const LATTE: &str = include_str!("fixtures/catppuccin-latte.toml");

fn fixtures() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/theme/fixtures");
    let mut all: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(p).unwrap())
        })
        .collect();
    all.sort();
    all
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/tmp/theme-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("omarchy/current")).unwrap();
        Scratch(root)
    }

    fn file(&self) -> PathBuf {
        self.0.join("omarchy/current/theme/colors.toml")
    }

    fn swap(&self, text: &str) {
        let staging = self.0.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("colors.toml"), text).unwrap();
        let _ = std::fs::remove_dir_all(self.file().parent().unwrap());
        std::fs::rename(staging, self.file().parent().unwrap()).unwrap();
    }

    fn remove(&self) {
        std::fs::remove_dir_all(self.file().parent().unwrap()).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn shown(ctx: &egui::Context) -> (Color32, Mode) {
    (Palette::of(ctx).panel, ctx.theme())
}

fn applied(palette: &Palette) -> Visuals {
    let mut visuals = if palette.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    palette.apply(&mut visuals);
    visuals
}

fn checks(p: &Palette) -> Vec<(&'static str, Color32, Color32, f32)> {
    let v = applied(p);
    let w = &v.widgets;
    let mut all = vec![
        (
            "inactive",
            w.inactive.fg_stroke.color,
            w.inactive.bg_fill,
            4.5,
        ),
        ("hovered", w.hovered.fg_stroke.color, w.hovered.bg_fill, 4.5),
        ("active", w.active.fg_stroke.color, w.active.bg_fill, 4.5),
        ("open", w.open.fg_stroke.color, w.open.bg_fill, 4.5),
        (
            "selected",
            v.selection.stroke.color,
            v.selection.bg_fill,
            4.5,
        ),
        ("banner", Palette::ON_BANNER, p.banner(), 4.5),
        ("notice", p.notice(), p.panel, 4.5),
        ("placeholder", p.placeholder(), p.panel, 1.2),
    ];
    for surface in p.surfaces() {
        all.push(("text", v.text_color(), surface, 4.5));
        all.push(("strong", v.strong_text_color(), surface, 4.5));
        all.push(("weak", v.weak_text_color(), surface, 3.0));
    }
    all
}

fn palettes() -> Vec<(String, Palette)> {
    let mut all = vec![
        ("plain dark".to_owned(), Palette::plain(Mode::Dark)),
        ("plain light".to_owned(), Palette::plain(Mode::Light)),
    ];
    for (name, text) in fixtures() {
        all.push((name, Palette::omarchy(&text).expect("a stock theme parses")));
    }
    all
}

#[test]
fn a_dark_theme_maps_each_colors_toml_role_to_its_surface() {
    let p = Palette::omarchy(TOKYO).unwrap();
    assert!(p.dark);
    assert_eq!(p.panel, rgb(0x1a1b26));
    assert_eq!(p.sidebar, rgb(0x13141c));
    assert_eq!(p.extreme, rgb(0x0e0e14));
    assert_eq!(p.inactive, rgb(0x24283b));
    assert_eq!(p.hover, rgb(0x292e42));
    assert_eq!(p.accent, rgb(0x7aa2f7));
    assert_eq!(p.border, rgb(0x414868));
    assert_eq!(p.text, rgb(0xa9b1d6));
    assert_eq!(p.strong, rgb(0xc0caf5));
    let v = applied(&p);
    assert_eq!(v.panel_fill, p.panel);
    assert_eq!(v.extreme_bg_color, p.extreme);
    assert_eq!(v.selection.bg_fill, p.accent);
    assert_eq!(v.widgets.noninteractive.bg_stroke.color, p.border);
    assert_eq!(v.widgets.hovered.bg_fill, p.hover);
    assert_eq!(v.widgets.inactive.bg_fill, p.inactive);
    assert!(v.slider_trailing_fill);
}

#[test]
fn a_light_theme_reads_as_light_and_keeps_its_own_surfaces() {
    let p = Palette::omarchy(LATTE).unwrap();
    assert!(!p.dark);
    assert_eq!(p.panel, rgb(0xeff1f5));
    assert_eq!(p.accent, rgb(0x1e66f5));
    assert!(!applied(&p).dark_mode);
}

#[test]
fn colors_toml_that_is_not_strictly_key_equals_quoted_value_is_refused() {
    let good = TOKYO;
    let without = |key: &str| {
        good.lines()
            .filter(|l| !l.starts_with(key))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(Palette::omarchy(good).is_some());
    let cases = [
        String::new(),
        without("mode"),
        without("accent"),
        without("selection"),
        good.replace("mode = \"dark\"", "mode = \"dusk\""),
        good.replace("#7aa2f7", "#7aa2f"),
        good.replace("#7aa2f7", "#7aa2f7ff"),
        good.replace("#7aa2f7", "#+7aa2f7"),
        good.replace("#7aa2f7", "7aa2f7"),
        good.replace("#7aa2f7", "#gggggg"),
        good.replace("\"#1a1b26\"", "#1a1b26"),
        format!("[colors]\n{good}"),
        format!("{good}\nnot a pair"),
        "\u{0}\u{1}binary".to_owned(),
    ];
    for (n, case) in cases.iter().enumerate() {
        assert!(Palette::omarchy(case).is_none(), "case {n}");
    }
}

#[test]
fn colors_toml_with_comments_blank_lines_crlf_and_other_keys_still_parses() {
    let noisy = format!(
        "# a comment\r\n\r\n{}\r\nhyprland_active_border = \"rgba(26a269ee) rgba(2ec27eee) 45deg\"\r\n",
        TOKYO.replace("#7aa2f7", "#7AA2F7")
    );
    let p = Palette::omarchy(&noisy).expect("noise is tolerated");
    assert_eq!((p.panel, p.accent), (rgb(0x1a1b26), rgb(0x7aa2f7)));
}

#[test]
fn every_stock_omarchy_theme_meets_the_contrast_floors() {
    let all = palettes();
    assert!(all.len() >= 24, "{} palettes ran", all.len());
    let mut failures = Vec::new();
    for (name, p) in &all {
        for (what, fg, bg, min) in checks(p) {
            assert_eq!(fg.a(), 255, "{name} {what} is opaque");
            let ratio = contrast(fg, bg);
            if ratio < min {
                failures.push(format!("{name} {what}: {ratio:.2} < {min}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn stock_egui_weak_text_alone_misses_three_to_one_which_is_why_it_is_set_explicitly() {
    for mode in [Mode::Dark, Mode::Light] {
        let v = if mode == Mode::Dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        let blended = v
            .text_color()
            .lerp_to_gamma(v.panel_fill, 1.0 - v.weak_text_alpha);
        assert!(contrast(blended, v.panel_fill) < 3.0, "{mode:?}");
        assert!(
            contrast(Palette::plain(mode).weak, v.panel_fill) >= 3.0,
            "{mode:?}"
        );
    }
}

#[test]
fn the_state_directory_comes_from_xdg_state_home_else_home() {
    let file = |state: Option<&str>, home: Option<&str>| {
        colors_file(state.map(Into::into), home.map(Into::into))
    };
    let tail = "omarchy/current/theme/colors.toml";
    assert_eq!(
        file(Some("/s"), Some("/h")),
        Some(PathBuf::from("/s").join(tail))
    );
    for unusable in [None, Some(""), Some("relative/state")] {
        let want = Some(PathBuf::from("/h/.local/state").join(tail));
        assert_eq!(file(unusable, Some("/h")), want);
    }
    assert_eq!(file(None, None), None);
}

fn ticking(choice: Choice, scratch: &Scratch) -> (Theme, egui::Context, Instant) {
    let t0 = Instant::now();
    let theme = Theme::new(choice, Some(scratch.file()), t0);
    (theme, egui::Context::default(), t0)
}

#[test]
fn the_default_theme_follows_omarchy_and_a_missing_or_broken_file_means_plain() {
    let scratch = Scratch::new("startup");
    let (mut theme, ctx, t0) = ticking(Choice::default(), &scratch);
    theme.tick(&ctx, t0);
    assert_eq!(Palette::of(&ctx), Palette::plain(ctx.theme()));
    scratch.swap("mode = \"light\"\nnot toml at all");
    theme.tick(&ctx, t0 + Duration::from_secs(1));
    assert_eq!(Palette::of(&ctx), Palette::plain(ctx.theme()));
    scratch.swap(TOKYO);
    theme.tick(&ctx, t0 + Duration::from_secs(2));
    assert_eq!(shown(&ctx), (rgb(0x1a1b26), Mode::Dark));
}

#[test]
fn a_changed_colors_toml_is_picked_up_on_the_next_poll_and_not_before() {
    let scratch = Scratch::new("live");
    scratch.swap(TOKYO);
    let (mut theme, ctx, t0) = ticking(Choice::Omarchy, &scratch);
    theme.tick(&ctx, t0);
    assert_eq!(shown(&ctx), (rgb(0x1a1b26), Mode::Dark));
    scratch.swap(LATTE);
    theme.tick(&ctx, t0 + Duration::from_millis(400));
    assert_eq!(shown(&ctx), (rgb(0x1a1b26), Mode::Dark));
    theme.tick(&ctx, t0 + Duration::from_millis(1100));
    assert_eq!(shown(&ctx), (rgb(0xeff1f5), Mode::Light));
    let light = ctx.style_of(Mode::Light).visuals.clone();
    assert_eq!(light.selection.bg_fill, rgb(0x1e66f5));
    assert!(!light.dark_mode);
}

#[test]
fn the_directory_missing_during_the_swap_keeps_the_theme_and_only_a_long_absence_drops_it() {
    let scratch = Scratch::new("swap");
    scratch.swap(LATTE);
    let (mut theme, ctx, t0) = ticking(Choice::Omarchy, &scratch);
    theme.tick(&ctx, t0);
    scratch.remove();
    for secs in [1, 2, 3] {
        theme.tick(&ctx, t0 + Duration::from_secs(secs));
        assert_eq!(shown(&ctx), (rgb(0xeff1f5), Mode::Light), "{secs}s");
    }
    scratch.swap(TOKYO);
    theme.tick(&ctx, t0 + Duration::from_secs(4));
    assert_eq!(shown(&ctx), (rgb(0x1a1b26), Mode::Dark));
    scratch.remove();
    theme.tick(&ctx, t0 + Duration::from_secs(5));
    assert_eq!(shown(&ctx).0, rgb(0x1a1b26));
    theme.tick(&ctx, t0 + Duration::from_secs(9));
    assert_eq!(Palette::of(&ctx), Palette::plain(ctx.theme()));
}

#[test]
fn a_file_that_turns_malformed_falls_back_and_comes_back_when_fixed() {
    let scratch = Scratch::new("bad");
    scratch.swap(TOKYO);
    let (mut theme, ctx, t0) = ticking(Choice::Omarchy, &scratch);
    theme.tick(&ctx, t0);
    scratch.swap("mode = \"dark\"");
    theme.tick(&ctx, t0 + Duration::from_secs(1));
    assert_eq!(Palette::of(&ctx), Palette::plain(ctx.theme()));
    scratch.swap(LATTE);
    theme.tick(&ctx, t0 + Duration::from_secs(2));
    assert_eq!(shown(&ctx), (rgb(0xeff1f5), Mode::Light));
}

#[test]
fn an_explicit_choice_beats_the_omarchy_file() {
    let scratch = Scratch::new("choice");
    scratch.swap(LATTE);
    for (choice, mode) in [
        (Choice::Fixed(Mode::Dark), Mode::Dark),
        (Choice::Fixed(Mode::Light), Mode::Light),
    ] {
        let (mut theme, ctx, t0) = ticking(choice, &scratch);
        theme.tick(&ctx, t0);
        assert_eq!(ctx.theme(), mode);
        assert_eq!(Palette::of(&ctx), Palette::plain(mode));
    }
    let (mut theme, ctx, t0) = ticking(Choice::System, &scratch);
    theme.tick(&ctx, t0);
    assert_eq!(Palette::of(&ctx), Palette::plain(ctx.theme()));
    let (mut theme, ctx, t0) = ticking(Choice::Omarchy, &scratch);
    theme.tick(&ctx, t0);
    assert_eq!(shown(&ctx), (rgb(0xeff1f5), Mode::Light));
}

#[cfg(feature = "dev")]
#[test]
fn the_dev_theme_flag_takes_omarchy_system_dark_and_light_only() {
    let parse = |name: &str| {
        crate::parse_args(["--theme", name].map(String::from).into_iter()).map(|a| a.theme)
    };
    assert_eq!(parse("omarchy").unwrap(), Choice::Omarchy);
    assert_eq!(parse("system").unwrap(), Choice::System);
    assert_eq!(parse("dark").unwrap(), Choice::Fixed(Mode::Dark));
    assert_eq!(parse("light").unwrap(), Choice::Fixed(Mode::Light));
    assert!(parse("solarized").is_err());
    assert_eq!(
        crate::parse_args(std::iter::empty()).unwrap().theme,
        Choice::Omarchy
    );
}

#[test]
fn ui_code_takes_colours_from_the_palette_and_never_names_one() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src.join("ui"), src.join("ui.rs")];
    let mut seen = 0;
    while let Some(path) = stack.pop() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("tests") {
            continue;
        }
        if path.is_dir() {
            stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        } else if name.ends_with(".rs") {
            seen += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            let named = |l: &&str| {
                let unnamed = ["TRANSPARENT", "PLACEHOLDER"].iter().any(|c| l.contains(c));
                let literal = l.contains("Color32::") && !unnamed;
                literal || l.contains("visuals().dark_mode") || l.contains("faint_bg_color")
            };
            let hits: Vec<_> = text.lines().filter(named).collect();
            assert!(hits.is_empty(), "{name}: {hits:?}");
        }
    }
    assert!(seen >= 8, "{seen} files scanned");
}

fn text_colour(shape: &Shape, needle: &str) -> Option<Color32> {
    match shape {
        Shape::Vec(shapes) => shapes.iter().find_map(|s| text_colour(s, needle)),
        Shape::Text(t) if t.galley.text() == needle => {
            let section = t.galley.job.sections[0].format.color;
            Some(if section == Color32::PLACEHOLDER {
                t.override_text_color.unwrap_or(t.fallback_color)
            } else {
                section
            })
        }
        _ => None,
    }
}

#[test]
fn the_playing_queue_row_is_drawn_in_the_selection_text_colour_on_every_theme() {
    for (name, palette) in palettes() {
        let ctx = egui::Context::default();
        install(&ctx, Look::Omarchy(palette));
        let mut shown = Snapshot::default();
        shown.queue = vec![QueueItem {
            id: "i.1".into(),
            title: Some("Row One".into()),
            duration: Some(60.0),
        }];
        (shown.queue_position, shown.queue_length) = (Some(0), 1);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            queue_panel::show(ui, &shown, (&mut Vec::new(), &mut Vec::new()));
        });
        out.textures_delta.clear();
        let colour = out
            .shapes
            .iter()
            .find_map(|s| text_colour(&s.shape, "Row One"));
        let selected = ctx.style_of(ctx.theme()).visuals.selection;
        assert_eq!(colour, Some(selected.stroke.color), "{name}");
        assert!(
            contrast(selected.stroke.color, selected.bg_fill) >= 4.5,
            "{name}"
        );
    }
}
