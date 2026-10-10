mod app;
mod playback;
#[cfg(test)]
mod tests;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bramley_engine::{Player, PlayerConfig, Tokens};

use crate::playback::Spawn;

#[cfg(feature = "dev")]
#[used]
static DEV_MARKER: &str = "bramley-unshipped-feature:app/dev\0";

#[derive(Default)]
pub struct Args {
    screenshot: Option<PathBuf>,
    after: Duration,
    status: Option<PathBuf>,
    #[cfg(feature = "dev")]
    play: Option<String>,
    #[cfg(feature = "dev")]
    output_device: Option<String>,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args> {
    let mut a = Args::default();
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--screenshot" => a.screenshot = Some(value()?.into()),
            "--after" => a.after = Duration::try_from_secs_f64(value()?.parse()?)?,
            "--status" => a.status = Some(value()?.into()),
            #[cfg(feature = "dev")]
            "--play" => a.play = Some(value()?),
            #[cfg(feature = "dev")]
            "--output-device" => a.output_device = Some(value()?),
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

fn read_token(name: &str) -> Result<String> {
    let dir = match std::env::var_os("BRAMLEY_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
            .join(".config/bramley"),
    };
    let path = dir.join(name);
    let token =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    match token.trim() {
        "" => bail!("{} is empty", path.display()),
        token => Ok(token.to_owned()),
    }
}

fn spawner(ctx: egui::Context, output_device: Option<String>) -> Spawn {
    Box::new(move || {
        let tokens = Tokens {
            developer: read_token("developer-token")?,
            user: read_token("music-user-token")?,
        };
        let mut cfg = PlayerConfig::default();
        cfg.engine.audio_sink_name = "bramley".into();
        cfg.engine.output_device = output_device.clone();
        let ctx = ctx.clone();
        Player::spawn(cfg, tokens, move || ctx.request_repaint())
    })
}

fn main() -> Result<()> {
    let args = parse_args(std::env::args().skip(1))?;
    #[cfg(feature = "dev")]
    let output_device = args.output_device.clone();
    #[cfg(not(feature = "dev"))]
    let output_device = None;
    let signed_out = read_token("music-user-token").is_err();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("Bramley")
            .with_app_id("bramley"),
        ..Default::default()
    };
    eframe::run_native(
        "bramley",
        options,
        Box::new(move |cc| {
            let spawn = spawner(cc.egui_ctx.clone(), output_device);
            Ok(Box::new(app::App::new(cc, args, spawn, signed_out)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
