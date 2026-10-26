mod app;
mod cli;
mod covers;
mod credentials;
mod devtoken;
mod mcp;
mod musickit;
mod playback;
mod signin;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod warm;

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, OnceLock};
#[cfg(feature = "dev")]
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
#[cfg(not(target_os = "macos"))]
use bramley_engine::{Player, PlayerConfig};
#[cfg(any(test, not(target_os = "macos")))]
use bramley_engine::{Tokens, redact};
use fastframe_instance::Claim;

use crate::credentials::Credentials;
use crate::playback::{Link, SignedOut, Spawn};

#[cfg(feature = "dev")]
#[used]
static DEV_MARKER: &str = "bramley-unshipped-feature:app/dev\0";
#[cfg(feature = "trace")]
#[used]
static TRACE_MARKER: &str = "bramley-unshipped-feature:app/trace\0";
#[cfg(feature = "gate-sabotage")]
#[used]
static GATE_SABOTAGE_MARKER: &str = "bramley-unshipped-feature:app/gate-sabotage\0";

#[cfg(feature = "gate-sabotage")]
fn touched(mib: usize) -> Vec<u8> {
    let mut block = vec![0u8; mib << 20];
    for page in block.chunks_mut(4096) {
        page[0] = 1;
    }
    std::hint::black_box(block)
}

#[cfg(feature = "gate-sabotage")]
pub struct Ballast {
    trigger: Option<PathBuf>,
    kept: Vec<u8>,
}

#[cfg(feature = "gate-sabotage")]
impl Ballast {
    pub fn from_env() -> Self {
        let trigger = std::env::var_os("BRAMLEY_GATE_BALLAST_FILE").map(PathBuf::from);
        Ballast {
            trigger,
            kept: Vec::new(),
        }
    }

    pub fn follow(&mut self) {
        let asked = self.trigger.as_ref().and_then(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            text.trim().parse::<usize>().ok()
        });
        let Some(mib) = asked else { return };
        self.trigger = None;
        self.kept = touched(mib);
        eprintln!("bramley: gate-sabotage ballast {mib} MiB touched");
    }

    pub fn mib(&self) -> usize {
        self.kept.len() >> 20
    }
}

#[cfg(feature = "gate-sabotage")]
pub struct Leak {
    mib: usize,
    item: Option<String>,
    kept: Vec<Vec<u8>>,
}

#[cfg(feature = "gate-sabotage")]
impl Leak {
    pub fn from_env() -> Self {
        let mib = std::env::var("BRAMLEY_GATE_LEAK_MIB").map_or(0, |v| {
            v.parse()
                .expect("BRAMLEY_GATE_LEAK_MIB is not a whole number")
        });
        Leak {
            mib,
            item: None,
            kept: Vec::new(),
        }
    }

    pub fn follow(&mut self, item: Option<&str>) {
        if self.mib == 0 || item.is_none() || item == self.item.as_deref() {
            return;
        }
        self.item = item.map(str::to_owned);
        self.kept.push(touched(self.mib));
        eprintln!(
            "bramley: gate-sabotage leaked {} MiB",
            self.kept.len() * self.mib
        );
    }
}

#[derive(Default)]
pub struct Args {
    #[cfg(feature = "dev")]
    screenshot: Option<PathBuf>,
    #[cfg(feature = "dev")]
    after: Duration,
    status: Option<PathBuf>,
    #[cfg(feature = "dev")]
    play: Option<String>,
    #[cfg(feature = "dev")]
    output_device: Option<String>,
    #[cfg(feature = "dev")]
    api_base: Option<String>,
    #[cfg(feature = "dev")]
    developer_token_url: Option<String>,
    #[cfg(feature = "dev")]
    route: Option<ui::nav::Route>,
    #[cfg(feature = "dev")]
    queue_open: bool,
    #[cfg(feature = "dev")]
    drawer_open: bool,
    #[cfg(feature = "dev")]
    scroll_end: Option<Duration>,
    #[cfg(feature = "dev")]
    click_last: bool,
    #[cfg(feature = "dev")]
    theme: theme::Choice,
    #[cfg(feature = "dev")]
    size: Option<[f32; 2]>,
    #[cfg(feature = "dev")]
    search: Option<String>,
    #[cfg(feature = "dev")]
    click_first_song: bool,
    #[cfg(feature = "dev")]
    click_first_playlist: bool,
    #[cfg(feature = "dev")]
    click_cover: Option<usize>,
    #[cfg(feature = "dev")]
    click_track: Vec<usize>,
    #[cfg(feature = "dev")]
    click_row: Option<usize>,
    #[cfg(feature = "dev")]
    jump_to: Option<usize>,
    #[cfg(feature = "dev")]
    jump_letter: Option<String>,
    #[cfg(feature = "dev")]
    queue_fixture: bool,
    #[cfg(feature = "dev")]
    playlists_collapsed: bool,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args> {
    let mut a = Args::default();
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            #[cfg(feature = "dev")]
            "--screenshot" => a.screenshot = Some(value()?.into()),
            #[cfg(feature = "dev")]
            "--after" => a.after = Duration::try_from_secs_f64(value()?.parse()?)?,
            "--status" => a.status = Some(value()?.into()),
            #[cfg(feature = "dev")]
            "--play" => a.play = Some(value()?),
            #[cfg(feature = "dev")]
            "--output-device" => a.output_device = Some(value()?),
            #[cfg(feature = "dev")]
            "--api-base" => a.api_base = Some(value()?),
            #[cfg(feature = "dev")]
            "--developer-token-url" => a.developer_token_url = Some(value()?),
            #[cfg(feature = "dev")]
            "--route" => a.route = Some(ui::nav::Route::parse(&value()?).context("unknown route")?),
            #[cfg(feature = "dev")]
            "--queue-open" => a.queue_open = true,
            #[cfg(feature = "dev")]
            "--drawer-open" => a.drawer_open = true,
            #[cfg(feature = "dev")]
            "--scroll-end" => a.scroll_end = Some(Duration::try_from_secs_f64(value()?.parse()?)?),
            #[cfg(feature = "dev")]
            "--click-last" => a.click_last = true,
            #[cfg(feature = "dev")]
            "--theme" => {
                a.theme = theme::Choice::parse(&value()?)
                    .context("--theme takes omarchy, system, dark or light")?
            }
            #[cfg(feature = "dev")]
            "--size" => a.size = Some(size(&value()?).context("--size takes WxH")?),
            #[cfg(feature = "dev")]
            "--search" => a.search = Some(value()?),
            #[cfg(feature = "dev")]
            "--click-first-song" => a.click_first_song = true,
            #[cfg(feature = "dev")]
            "--click-first-playlist" => a.click_first_playlist = true,
            #[cfg(feature = "dev")]
            "--click-cover" => a.click_cover = Some(value()?.parse()?),
            #[cfg(feature = "dev")]
            "--click-track" => {
                let clicks: Result<Vec<usize>, _> = value()?.split(',').map(str::parse).collect();
                a.click_track = clicks.context("--click-track takes numbers")?;
            }
            #[cfg(feature = "dev")]
            "--click-row" => a.click_row = Some(value()?.parse()?),
            #[cfg(feature = "dev")]
            "--jump-to" => a.jump_to = Some(value()?.parse()?),
            #[cfg(feature = "dev")]
            "--jump-letter" => a.jump_letter = Some(value()?.to_lowercase()),
            #[cfg(feature = "dev")]
            "--queue-fixture" => a.queue_fixture = true,
            #[cfg(feature = "dev")]
            "--playlists-collapsed" => a.playlists_collapsed = true,
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

#[cfg(feature = "dev")]
fn size(text: &str) -> Option<[f32; 2]> {
    let (w, h) = text.split_once('x')?;
    Some([w.parse().ok()?, h.parse().ok()?])
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

fn api_tokens(user: &Credentials) -> Result<bramley_api::Tokens, String> {
    let developer = user.developer().get()?;
    let user = user.load().map_err(str::to_owned)?;
    Ok(bramley_api::Tokens { developer, user })
}

fn cover_source(args: &Args, user: &Credentials) -> Option<covers::Source> {
    #[cfg(feature = "dev")]
    let art_prefix = args.api_base.clone();
    #[cfg(not(feature = "dev"))]
    let (art_prefix, _) = (None, args);
    let (art_cache, api_cache) = covers::open_caches();
    Some(covers::Source {
        api: art_prefix
            .clone()
            .unwrap_or_else(|| covers::API_BASE.into()),
        art_prefix,
        user: user.clone(),
        art_cache,
        api_cache,
    })
}

#[cfg(any(test, not(target_os = "macos")))]
fn boot(user: &Credentials, start: impl FnOnce(Tokens) -> Result<Link>) -> Result<Link> {
    let user_token = user.load().map_err(|why| anyhow!(why))?;
    let user_token = user_token.ok_or_else(|| anyhow!(SignedOut))?;
    let developer = user.developer().get().map_err(|why| anyhow!(why))?;
    let user = user_token;
    let secrets = [developer.clone(), user.clone()];
    start(Tokens { developer, user }).map_err(|e| anyhow!(redact(&format!("{e:#}"), &secrets)))
}

#[cfg(not(target_os = "macos"))]
fn launch(
    user: &Credentials,
    output_device: Option<String>,
    waker: impl Fn() + Send + 'static,
) -> Result<Link> {
    boot(user, |tokens| {
        let mut cfg = PlayerConfig::default();
        cfg.engine.audio_sink_name = "bramley".into();
        cfg.engine.output_device = output_device;
        Player::spawn(cfg, tokens, waker)
    })
}

#[cfg(target_os = "macos")]
fn launch(
    user: &Credentials,
    _output_device: Option<String>,
    waker: impl Fn() + Send + 'static,
) -> Result<Link> {
    if user.signed_in() != Ok(true) {
        bail!(SignedOut);
    }
    bramley_macos_player::spawn(waker)
}

fn spawner(ctx: egui::Context, output_device: Option<String>, user: Credentials) -> Spawn {
    Box::new(move || {
        let (tx, rx) = channel();
        let (ctx, output_device, user) = (ctx.clone(), output_device.clone(), user.clone());
        std::thread::spawn(move || {
            let waker = ctx.clone();
            let started = launch(&user, output_device, move || waker.request_repaint());
            let _ = tx.send(started);
            ctx.request_repaint();
        });
        rx
    })
}

fn logging() -> fastframe_log::Logging {
    fastframe_log::Logging::new("bramley", env!("CARGO_PKG_VERSION")).redact(withhold_wire_dumps)
}

fn withhold_wire_dumps(
    record: &log::Record<'_>,
    _message: &str,
) -> Option<std::borrow::Cow<'static, str>> {
    let carries_payloads = matches!(
        record.target().split("::").next(),
        Some("ureq" | "ureq_proto" | "rmcp")
    );
    let wire = record.level() >= log::Level::Debug && carries_payloads;
    wire.then_some("<HTTP bytes withheld: they carry tokens>".into())
}

#[cfg(unix)]
const CHANNEL_FILE: &str = "instance.sock";
#[cfg(not(unix))]
const CHANNEL_FILE: &str = "instance.key";

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if matches!(argv.as_slice(), [flag] if flag == "--version" || flag == "-V") {
        println!("bramley {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    if let [word, flags @ ..] = argv.as_slice()
        && word == "musickit-probe"
    {
        println!("{}", bramley_macos_player::probe(flags == ["--request"]));
        return Ok(());
    }
    let slot = cli::slot();
    if argv.first().is_some_and(|first| first == "warm") {
        std::process::exit(warm::command(&argv, slot));
    }
    if argv.first().is_some_and(|first| first == "mcp") {
        let served = match (slot, argv.len()) {
            (Ok(slot), 1) => config_dir().and_then(|config| mcp::run(slot, &config)),
            (Ok(_), _) => Err(anyhow!("usage: bramley mcp")),
            (Err(e), _) => Err(e),
        };
        if let Err(e) = served {
            eprintln!("bramley mcp: {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
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
    let _ = logging().init();
    let (inbox_tx, inbox) = channel();
    let waker = Arc::new(OnceLock::new());
    let _guard = match slot.claim(&cli::wire(&[]), cli::handler(inbox_tx, waker.clone())) {
        Claim::First(guard) => guard,
        Claim::Running(reply) => {
            if args.status.is_some() {
                eprintln!("bramley: --status ignored: Bramley is already running");
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
    #[cfg(feature = "dev")]
    let token_url = (args.developer_token_url.clone()).unwrap_or_else(|| devtoken::URL.into());
    #[cfg(not(feature = "dev"))]
    let token_url = devtoken::URL.to_owned();
    let config = config_dir()?;
    let developer = devtoken::DeveloperToken::system(token_url, &config);
    let user = Credentials::system(&config, developer);
    #[cfg(not(target_os = "macos"))]
    {
        let (prefetch, woken) = (user.developer().clone(), waker.clone());
        std::thread::spawn(move || {
            let wake = || woken.get().iter().for_each(|ctx| ctx.request_repaint());
            let last = std::cell::RefCell::new(None);
            let _ = prefetch.wait(|| {
                let now = prefetch.missing();
                if last.replace(now.clone()) != now {
                    wake();
                }
                false
            });
            wake();
        });
    }
    let source = cover_source(&args, &user);
    #[cfg(feature = "dev")]
    let size = args.size.unwrap_or([1100.0, 720.0]);
    #[cfg(not(feature = "dev"))]
    let size = [1100.0, 720.0];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size(ui::MIN_WINDOW)
            .with_title("Bramley")
            .with_app_id("bramley")
            .with_icon(egui::IconData::default()),
        ..renderer_options()
    };
    eframe::run_native(
        "bramley",
        options,
        Box::new(move |cc| {
            let _ = waker.set(cc.egui_ctx.clone());
            let spawn = spawner(cc.egui_ctx.clone(), output_device, user.clone());
            let channel = (inbox, guarded);
            let mut app = app::App::new(&cc.egui_ctx, args, spawn, (user, true), channel, source);
            app.check_sign_in();
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn renderer_options() -> eframe::NativeOptions {
    eframe::NativeOptions::default()
}

#[cfg(target_os = "macos")]
fn renderer_options() -> eframe::NativeOptions {
    use eframe::egui_wgpu::{SurfaceConfig, WgpuConfiguration, wgpu::PresentMode};
    eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: WgpuConfiguration {
            surface: SurfaceConfig {
                present_mode: PresentMode::Fifo,
                desired_maximum_frame_latency: Some(1),
            },
            ..Default::default()
        },
        ..Default::default()
    }
}
