use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bramley_engine::{Engine, EngineConfig, redact};
use serde_json::{Value as J, json};

const PLAYER: &str = include_str!("player.js");
const MUSICKIT_URL: &str = "https://js-cdn.music.apple.com/musickit/v3/musickit.js";

struct Args {
    config: EngineConfig,
    status: PathBuf,
    max_seconds: f64,
    songs: Vec<String>,
    controls_at: Option<f64>,
    no_user_token: bool,
    corrupt_developer_token: bool,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        config: EngineConfig::default(),
        status: "musickit-status.json".into(),
        max_seconds: 120.0,
        songs: vec!["6781024437".into(), "6781079518".into()],
        controls_at: None,
        no_user_token: false,
        corrupt_developer_token: false,
    };
    a.config.audio_sink_name = "bramley-musickit".into();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--cdm" => a.config.cdm_path = value()?.into(),
            "--output-device" => a.config.output_device = Some(value()?),
            "--sink-name" => a.config.audio_sink_name = value()?,
            "--status" => a.status = value()?.into(),
            "--max-seconds" => a.max_seconds = value()?.parse()?,
            "--songs" => a.songs = value()?.split(',').map(String::from).collect(),
            "--controls-at" => a.controls_at = Some(value()?.parse()?),
            "--no-user-token" => a.no_user_token = true,
            "--corrupt-developer-token" => a.corrupt_developer_token = true,
            "--truncate-musickit" => a.config.truncate_scripts = true,
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
    fn advance(&mut self, engine: &Engine, js: &J, start: Instant) -> Result<()> {
        let held = self.since.map_or(Duration::ZERO, |s| s.elapsed());
        let cpt = js["currentPlaybackTime"].as_f64().unwrap_or(-1.0);
        let item = js["item"]["id"].clone();
        let step = |name: &str, call: &str, me: &mut Controls| -> Result<()> {
            me.steps
                .push(json!({"step": name, "wallSec": start.elapsed().as_secs_f64(), "js": js}));
            if !call.is_empty() {
                engine.eval_script("controls", call)?;
            }
            me.since = Some(Instant::now());
            me.stage += 1;
            Ok(())
        };
        if self.stage > 0 {
            self.trace.push(json!([
                start.elapsed().as_secs_f64(),
                cpt,
                js["playbackState"],
                item
            ]));
        }
        match self.stage {
            0 if cpt > 35.0 => {
                self.before = engine.status();
                step("pause", "__player.pause()", self)?
            }
            1 if held >= Duration::from_secs(4) => step("paused-4s", "__player.seek(120)", self)?,
            2 if held >= Duration::from_millis(2500) => {
                step("seeked-120", "__player.play()", self)?
            }
            3 if held >= Duration::from_secs(5) => step("resumed-5s", "__player.next()", self)?,
            4 if held >= Duration::from_secs(8) => step("next-8s", "", self)?,
            5 if held >= Duration::from_secs(4) => step("next-12s", "", self)?,
            _ => {}
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let mut args = parse_args()?;
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
    args.config.redact = vec![developer, presented.clone(), user.clone()];
    let secrets = args.config.redact.clone();
    println!("PID {}", std::process::id());
    let mut engine = Engine::new(args.config.clone())?;
    let config = json!({"developerToken": presented, "userToken": user, "songs": args.songs, "musickitUrl": MUSICKIT_URL, "build": env!("CARGO_PKG_VERSION")});
    engine
        .eval_script(
            "config.js",
            &format!("globalThis.__bramley_musickit = {config};"),
        )
        .map_err(|e| anyhow::anyhow!(redact(&e.to_string(), &secrets)))?;
    engine.eval_script("player.js", PLAYER)?;
    let start = Instant::now();
    let mut controls = Controls::default();
    let mut samples: Vec<J> = Vec::new();
    let mut last_status = start - Duration::from_secs(5);
    loop {
        let wait = engine.tick();
        let elapsed = start.elapsed().as_secs_f64();
        let poll = controls.stage > 0 || last_status.elapsed() >= Duration::from_secs(1);
        if poll {
            let js = engine.eval_json("__player.state()")?;
            if args.controls_at.is_some_and(|at| elapsed >= at) {
                controls.advance(&engine, &js, start)?;
            }
            if last_status.elapsed() >= Duration::from_secs(1) {
                last_status = Instant::now();
                samples.push(json!([
                    elapsed,
                    js["currentPlaybackTime"],
                    js["playbackState"],
                    js["item"]["id"]
                ]));
                write_status(&engine, &args, &js, &controls, &samples, &secrets);
            }
            if controls.stage == 6 || elapsed > args.max_seconds {
                write_status(&engine, &args, &js, &controls, &samples, &secrets);
                break;
            }
        }
        std::thread::sleep(wait);
    }
    Ok(())
}

fn write_status(
    engine: &Engine,
    args: &Args,
    js: &J,
    controls: &Controls,
    samples: &[J],
    secrets: &[String],
) {
    let mut v = engine.status();
    v["player"] = js.clone();
    v["samples"] = json!(samples);
    v["controls"] = json!({"steps": controls.steps, "trace": controls.trace, "beforeControls": controls.before, "done": controls.stage == 6});
    let text = redact(&serde_json::to_string_pretty(&v).unwrap(), secrets);
    let tmp = args.status.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &args.status);
    }
}
