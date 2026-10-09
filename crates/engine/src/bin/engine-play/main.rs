use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use engine::{Engine, EngineConfig};
use serde_json::{Value as J, json};

const APP: &str = include_str!("app.js");

struct Args {
    config: EngineConfig,
    manifest: String,
    license: String,
    hls_url: String,
    status: PathBuf,
    max_seconds: f64,
    looping: bool,
    hls_debug: bool,
    audio_master_codecs: String,
    back_buffer: Option<f64>,
    max_buffer: Option<f64>,
    scenario: Option<String>,
    skip_pause: bool,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        config: EngineConfig::default(),
        manifest: "https://storage.googleapis.com/shaka-demo-assets/angel-one-widevine-hls/playlist_a-eng-0128k-aac-2c.mp4.m3u8".into(),
        license: "https://cwip-shaka-proxy.appspot.com/no_auth".into(),
        hls_url: "https://cdn.jsdelivr.net/npm/hls.js@1.7.3/dist/hls.min.js".into(),
        status: "status.json".into(),
        max_seconds: 1e9,
        looping: true,
        hls_debug: false,
        audio_master_codecs: "mp4a.40.2".into(),
        back_buffer: None,
        max_buffer: None,
        scenario: None,
        skip_pause: false,
    };
    a.config.audio_sink_name = "engine-play".into();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--manifest" => a.manifest = value()?,
            "--lic" => a.license = value()?,
            "--hls" => a.hls_url = value()?,
            "--cdm" => a.config.cdm_path = value()?.into(),
            "--output-device" => a.config.output_device = Some(value()?),
            "--sink-name" => a.config.audio_sink_name = value()?,
            "--status" => a.status = value()?.into(),
            "--max-seconds" => a.max_seconds = value()?.parse()?,
            "--audio-master-codecs" => a.audio_master_codecs = value()?,
            "--back-buffer" => a.back_buffer = Some(value()?.parse()?),
            "--max-buffer" => a.max_buffer = Some(value()?.parse()?),
            "--scenario" => a.scenario = Some(value()?),
            "--scenario-skip-pause" => a.skip_pause = true,
            "--no-loop" => a.looping = false,
            "--hls-debug" => a.hls_debug = true,
            other => bail!("unknown arg {other}"),
        }
    }
    Ok(a)
}

#[derive(PartialEq)]
enum Stage {
    WaitForPausePoint,
    Paused,
    Seeking,
    Resumed,
    WaitForEnd,
    Done,
}

struct Scenario {
    skip_pause: bool,
    stage: Stage,
    since: Instant,
    steps: Vec<J>,
}

impl Scenario {
    fn snap(&mut self, engine: &Engine, start: Instant, name: &str, js: &J) {
        let native = engine.status()["medias"]
            .as_object()
            .and_then(|m| m.values().next().cloned())
            .unwrap_or(J::Null);
        let pick = |k: &str| native.get(k).cloned().unwrap_or(J::Null);
        self.steps.push(json!({
            "step": name, "wallSec": start.elapsed().as_secs_f64(), "js": js,
            "native": {"currentTime": pick("currentTime"), "outputPosition": pick("outputPosition"), "decodedDecryptedSeconds": pick("decodedDecryptedSeconds"), "decodedClearSeconds": pick("decodedClearSeconds"), "paused": pick("paused")},
        }));
        self.since = Instant::now();
    }

    fn advance(&mut self, engine: &Engine, start: Instant) -> Result<()> {
        let js = engine.eval_json("({currentTime: __app.audio.currentTime, paused: __app.audio.paused, seeking: __app.audio.seeking, ended: __app.audio.ended})")?;
        let t = js["currentTime"].as_f64().unwrap_or(0.0);
        let held = self.since.elapsed();
        match self.stage {
            Stage::WaitForPausePoint if t >= 15.0 => {
                self.snap(engine, start, "pause-at-15", &js);
                if !self.skip_pause {
                    engine.eval_script("scenario", "__app.audio.pause()")?;
                }
                self.stage = Stage::Paused;
            }
            Stage::Paused if held >= Duration::from_secs(20) => {
                self.snap(engine, start, "after-20s-pause", &js);
                engine.eval_script("scenario", "__app.audio.currentTime = 40")?;
                self.stage = Stage::Seeking;
            }
            Stage::Seeking if js["seeking"] == J::Bool(false) => {
                self.snap(engine, start, "seeked-to-40", &js);
                engine.eval_script("scenario", "__app.audio.play()")?;
                self.stage = Stage::Resumed;
            }
            Stage::Resumed if held >= Duration::from_secs(5) => {
                self.snap(engine, start, "5s-after-resume", &js);
                self.stage = Stage::WaitForEnd;
            }
            Stage::WaitForEnd if js["ended"] == J::Bool(true) => {
                self.snap(engine, start, "ended", &js);
                self.stage = Stage::Done;
            }
            _ => {}
        }
        Ok(())
    }
}

fn write_status(engine: &Engine, args: &Args, scenario: Option<&Scenario>) {
    let mut v = engine.status();
    v["app"] = engine
        .eval_json("JSON.parse(__app.statusJson())")
        .unwrap_or(J::Null);
    v["phase"] = v["app"]["phase"].clone();
    v["scenario"] = scenario.map(|s| json!({"name": args.scenario, "skipPause": s.skip_pause, "steps": s.steps, "done": s.stage == Stage::Done})).unwrap_or(J::Null);
    let tmp = args.status.with_extension("tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, &args.status);
    }
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let mut scenario = match args.scenario.as_deref() {
        None => None,
        Some("pause-seek") => Some(Scenario {
            skip_pause: args.skip_pause,
            stage: Stage::WaitForPausePoint,
            since: Instant::now(),
            steps: Vec::new(),
        }),
        Some(other) => bail!("unknown scenario {other}"),
    };
    println!("PID {}", std::process::id());
    let config = json!({
        "manifest": args.manifest, "licenseUrl": args.license, "hlsUrl": args.hls_url, "loop": args.looping && scenario.is_none(), "hlsDebug": args.hls_debug,
        "backBufferLength": args.back_buffer, "maxBufferLength": args.max_buffer, "audioMasterCodecs": Some(&args.audio_master_codecs).filter(|c| !c.is_empty()),
    });
    let mut engine = Engine::new(args.config.clone())?;
    engine.eval_script(
        "config.js",
        &format!("globalThis.__play_config = {config};"),
    )?;
    engine.eval_script("app.js", APP)?;
    let start = Instant::now();
    let mut last_status = start - Duration::from_secs(5);
    while start.elapsed().as_secs_f64() <= args.max_seconds {
        let wait = engine.tick();
        if let Some(s) = scenario.as_mut() {
            s.advance(&engine, start)?;
        }
        if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            write_status(&engine, &args, scenario.as_ref());
        }
        std::thread::sleep(wait);
    }
    write_status(&engine, &args, scenario.as_ref());
    Ok(())
}
