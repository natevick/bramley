mod app;
mod cli;
mod playback;
#[cfg(test)]
mod tests;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use bramley_engine::{Player, PlayerConfig, Tokens, redact};
use fastframe_instance::Claim;

use crate::playback::{Link, SignedOut, Spawn};

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

fn config_dir() -> Result<PathBuf> {
    Ok(match std::env::var_os("BRAMLEY_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
            .join(".config/bramley"),
    })
}

fn read_token(dir: &Path, name: &str) -> Result<String> {
    let path = dir.join(name);
    let token =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    match token.trim() {
        "" => bail!("{} is empty", path.display()),
        token => Ok(token.to_owned()),
    }
}

fn boot(dir: &Path, start: impl FnOnce(Tokens) -> Result<Link>) -> Result<Link> {
    let user = read_token(dir, "music-user-token").map_err(|_| anyhow!(SignedOut))?;
    let developer = read_token(dir, "developer-token")?;
    let secrets = [developer.clone(), user.clone()];
    start(Tokens { developer, user }).map_err(|e| anyhow!(redact(&format!("{e:#}"), &secrets)))
}

fn spawner(ctx: egui::Context, output_device: Option<String>) -> Spawn {
    Box::new(move || {
        let (tx, rx) = channel();
        let (ctx, output_device) = (ctx.clone(), output_device.clone());
        std::thread::spawn(move || {
            let started = config_dir().and_then(|dir| {
                boot(&dir, |tokens| {
                    let mut cfg = PlayerConfig::default();
                    cfg.engine.audio_sink_name = "bramley".into();
                    cfg.engine.output_device = output_device;
                    let waker = ctx.clone();
                    Player::spawn(cfg, tokens, move || waker.request_repaint())
                })
            });
            let _ = tx.send(started);
            ctx.request_repaint();
        });
        rx
    })
}

#[cfg(unix)]
const CHANNEL_FILE: &str = "instance.sock";
#[cfg(not(unix))]
const CHANNEL_FILE: &str = "instance.key";

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let slot = cli::slot();
    if cli::is_command(&argv) {
        let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
        std::process::exit(match slot {
            Ok(slot) => cli::client(&slot, &argv, &mut out, &mut err),
            Err(e) => {
                eprintln!("bramley: {e}");
                1
            }
        });
    }
    let slot = slot?;
    let args = parse_args(argv.into_iter())?;
    let _ = fastframe_log::Logging::new("bramley", env!("CARGO_PKG_VERSION")).init();
    let (inbox_tx, inbox) = channel();
    let waker = Arc::new(OnceLock::new());
    let _guard = match slot.claim(&cli::wire(&[]), cli::handler(inbox_tx, waker.clone())) {
        Claim::First(guard) => guard,
        Claim::Running(reply) => {
            if args.status.is_some() || args.screenshot.is_some() {
                eprintln!("bramley: --status and --screenshot ignored: Bramley is already running");
            }
            if let Err((code, why)) = cli::outcome(&reply) {
                eprintln!("bramley: {why}");
                std::process::exit(code);
            }
            return Ok(());
        }
        Claim::Declined | Claim::Unanswered => {
            bail!("another Bramley is running but did not answer")
        }
    };
    let guarded = slot.dir().join(CHANNEL_FILE).exists();
    if !guarded {
        eprintln!(
            "bramley: WARNING: no instance channel at {}; CLI commands cannot reach this copy and another launch will start a second one",
            slot.dir().display()
        );
    }
    let output_device = std::env::var("PULSE_SINK").ok().filter(|s| !s.is_empty());
    #[cfg(feature = "dev")]
    let output_device = args.output_device.clone().or(output_device);
    let user_token = config_dir()?.join("music-user-token");
    let signed_out = !std::fs::metadata(user_token).is_ok_and(|m| m.len() > 0);
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
            let _ = waker.set(cc.egui_ctx.clone());
            let spawn = spawner(cc.egui_ctx.clone(), output_device);
            let channel = (inbox, guarded);
            let app = app::App::new(&cc.egui_ctx, args, spawn, signed_out, channel);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
