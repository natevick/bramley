use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bramley_engine::{
    Command, Diagnostics, Player, PlayerConfig, PlayerError, Queue, Snapshot, Tokens, redact,
};
use serde_json::{Value as J, json};

struct Args {
    config: PlayerConfig,
    status: PathBuf,
    max_seconds: f64,
    songs: Vec<String>,
    controls_at: Option<f64>,
    no_user_token: bool,
    corrupt_developer_token: bool,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        config: PlayerConfig {
            snapshot_interval: Duration::from_millis(50),
            ..PlayerConfig::default()
        },
        status: "musickit-status.json".into(),
        max_seconds: 120.0,
        songs: vec!["6781024437".into(), "6781079518".into()],
        controls_at: None,
        no_user_token: false,
        corrupt_developer_token: false,
    };
    a.config.engine.audio_sink_name = "bramley-musickit".into();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--cdm" => a.config.engine.cdm_path = value()?.into(),
            "--output-device" => a.config.engine.output_device = Some(value()?),
            "--sink-name" => a.config.engine.audio_sink_name = value()?,
            "--status" => a.status = value()?.into(),
            "--max-seconds" => a.max_seconds = value()?.parse()?,
            "--songs" => a.songs = value()?.split(',').map(String::from).collect(),
            "--controls-at" => a.controls_at = Some(value()?.parse()?),
            "--no-user-token" => a.no_user_token = true,
            "--corrupt-developer-token" => a.corrupt_developer_token = true,
            "--truncate-musickit" => a.config.truncate_musickit = true,
            other => bail!("unknown arg {other}"),
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
    Ok(std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?
        .trim()
        .to_string())
}

fn corrupt(token: &str) -> String {
    let mut parts: Vec<String> = token.split('.').map(String::from).collect();
    if let Some(sig) = parts.get_mut(2) {
        *sig = sig
            .char_indices()
            .map(|(i, c)| match (i, c) {
                (10, 'A') => 'B',
                (10, _) => 'A',
                (20, 'Q') => 'R',
                (20, _) => 'Q',
                _ => c,
            })
            .collect();
    }
    parts.join(".")
}

#[derive(Default)]
struct Controls {
    stage: usize,
    since: Option<Instant>,
    steps: Vec<J>,
    trace: Vec<J>,
    before: J,
}

impl Controls {
    fn advance(
        &mut self,
        snap: &Snapshot,
        d: &Diagnostics,
        start: Instant,
        player: &Sender<Command>,
    ) -> Result<()> {
        let js = &d.js;
        let native = json!(snap.position);
        let held = self.since.map_or(Duration::ZERO, |s| s.elapsed());
        let cpt = js["currentPlaybackTime"].as_f64().unwrap_or(-1.0);
        let step = |name: &str, command: Option<Command>, me: &mut Controls| -> Result<()> {
            me.steps.push(json!({"step": name, "wallSec": start.elapsed().as_secs_f64(), "js": js, "nativeTime": native}));
            if let Some(command) = command {
                player.send(command).context("player stopped")?;
            }
            me.since = Some(Instant::now());
            me.stage += 1;
            Ok(())
        };
        let traced = self.trace.last().and_then(|t| t[0].as_f64()).unwrap_or(0.0);
        if self.stage > 0 && start.elapsed().as_secs_f64() - traced >= 0.05 {
            self.trace.push(json!([
                start.elapsed().as_secs_f64(),
                cpt,
                js["playbackState"],
                js["item"]["id"],
                native
            ]));
        }
        match self.stage {
            0 if cpt > 35.0 && d.engine.is_some() => {
                self.before = d.engine.clone().unwrap_or_default();
                step("pause", Some(Command::Pause), self)?
            }
            1 if held >= Duration::from_secs(4) => {
                step("paused-4s", Some(Command::Seek(120.0)), self)?
            }
            2 if held >= Duration::from_millis(2500) => {
                step("seeked-120", Some(Command::Play), self)?
            }
            3 if held >= Duration::from_secs(5) => step("resumed-5s", Some(Command::Next), self)?,
            4 if held >= Duration::from_secs(8) => step("next-8s", None, self)?,
            5 if held >= Duration::from_secs(4) => step("next-12s", None, self)?,
            _ => {}
        }
        Ok(())
    }
}

struct Report {
    engine: J,
    js: J,
    samples: Vec<J>,
    controls: Controls,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let developer = read_token("developer-token")?;
    let user = if args.no_user_token {
        String::new()
    } else {
        read_token("music-user-token")?
    };
    let presented = if args.corrupt_developer_token {
        corrupt(&developer)
    } else {
        developer.clone()
    };
    let secrets = vec![developer.clone(), presented.clone(), user.clone()];
    let mut config = args.config.clone();
    config.engine.redact = vec![developer];
    println!("PID {}", std::process::id());
    let (player, snapshots) = Player::spawn(
        config,
        Tokens {
            developer: presented,
            user,
        },
        || {},
    );
    player.send(Command::SetQueue {
        queue: Queue::Songs(args.songs.clone()),
        start: 0,
    })?;
    player.send(Command::Play)?;
    let start = Instant::now();
    let mut latest = Snapshot::default();
    let mut r = Report {
        engine: json!({}),
        js: J::Null,
        samples: Vec::new(),
        controls: Controls::default(),
    };
    loop {
        let fresh = match snapshots.recv_timeout(Duration::from_millis(50)) {
            Ok(snap) => {
                latest = snap;
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => bail!("player stopped"),
        };
        let elapsed = start.elapsed().as_secs_f64();
        let Some(d) = latest.diagnostics.clone() else {
            continue;
        };
        r.js = d.js.clone();
        let d = if fresh {
            d
        } else {
            Diagnostics { engine: None, ..d }
        };
        if args.controls_at.is_some_and(|at| elapsed >= at) {
            r.controls.advance(&latest, &d, start, &player)?;
        }
        if let Some(engine) = d.engine {
            r.engine = engine;
            r.samples.push(json!([
                elapsed,
                d.js["currentPlaybackTime"],
                d.js["playbackState"],
                d.js["item"]["id"]
            ]));
            write_status(&args, &r, &secrets);
            if let Some(PlayerError::SignInNeeded(reason)) = &latest.error {
                bail!("sign-in needed: {}", redact(reason, &secrets));
            }
        }
        if r.controls.stage == 6 || elapsed > args.max_seconds {
            write_status(&args, &r, &secrets);
            break;
        }
    }
    let _ = player.send(Command::Shutdown);
    while snapshots.recv().is_ok() {}
    Ok(())
}

fn write_status(args: &Args, r: &Report, secrets: &[String]) {
    let mut v = r.engine.clone();
    v["player"] = r.js.clone();
    v["samples"] = json!(r.samples);
    v["controls"] = json!({"steps": r.controls.steps, "trace": r.controls.trace, "beforeControls": r.controls.before, "done": r.controls.stage == 6});
    let text = redact(&serde_json::to_string_pretty(&v).unwrap(), secrets);
    let tmp = args.status.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &args.status);
    }
}
