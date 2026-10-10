use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value as J, json};

use crate::{Engine, EngineConfig, redact};

const PLAYER: &str = include_str!("../js/player.js");
const MUSICKIT_URL: &str = "https://js-cdn.music.apple.com/musickit/v3/musickit.js";
const PLAYER_STACK: usize = 8 * 1024 * 1024;
#[cfg(feature = "diagnostics")]
const ENGINE_STATUS_EVERY: Duration = Duration::from_secs(1);

pub struct Tokens {
    pub developer: String,
    pub user: String,
}

#[derive(Clone, Debug)]
pub struct PlayerConfig {
    pub engine: EngineConfig,
    pub snapshot_interval: Duration,
    #[cfg(feature = "sabotage")]
    pub truncate_musickit: bool,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        PlayerConfig {
            engine: EngineConfig::default(),
            snapshot_interval: Duration::from_millis(250),
            #[cfg(feature = "sabotage")]
            truncate_musickit: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Queue {
    Songs(Vec<String>),
    Album(String),
    Playlist(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    SetQueue { queue: Queue, start: usize },
    Play,
    Pause,
    Seek(f64),
    Next,
    Previous,
    Volume(f32),
    Shutdown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PlayState {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerError {
    SignInNeeded(String),
    Playback(String),
    LoadFailed(String),
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostics {
    pub js: J,
    pub engine: Option<J>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub state: PlayState,
    pub error: Option<PlayerError>,
    #[cfg(feature = "diagnostics")]
    pub diagnostics: Option<Diagnostics>,
}

pub(crate) trait Backend {
    fn tick(&mut self) -> Duration;
    fn eval_script(&self, name: &str, source: &str) -> Result<()>;
    fn eval_json(&self, expression: &str) -> Result<J>;
    fn current_time(&self) -> Option<f64>;
    #[cfg(feature = "diagnostics")]
    fn status(&self) -> J;
}

impl Backend for Engine {
    fn tick(&mut self) -> Duration {
        Engine::tick(self)
    }
    fn eval_script(&self, name: &str, source: &str) -> Result<()> {
        Engine::eval_script(self, name, source)
    }
    fn eval_json(&self, expression: &str) -> Result<J> {
        Engine::eval_json(self, expression)
    }
    fn current_time(&self) -> Option<f64> {
        Engine::current_time(self)
    }
    #[cfg(feature = "diagnostics")]
    fn status(&self) -> J {
        Engine::status(self)
    }
}

pub struct Player;

impl Player {
    pub fn spawn(
        mut cfg: PlayerConfig,
        tokens: Tokens,
        waker: impl Fn() + Send + 'static,
    ) -> (Sender<Command>, Receiver<Snapshot>) {
        cfg.engine
            .redact
            .extend([tokens.developer.clone(), tokens.user.clone()]);
        let secrets = cfg.engine.redact.clone();
        let every = cfg.snapshot_interval;
        let setup = json!({"developerToken": tokens.developer, "userToken": tokens.user, "musickitUrl": MUSICKIT_URL, "build": env!("CARGO_PKG_VERSION")});
        spawn_with(
            move || {
                let engine = Engine::new(cfg.engine)?;
                #[cfg(feature = "diagnostics")]
                engine.enable_diagnostics();
                #[cfg(feature = "sabotage")]
                if cfg.truncate_musickit {
                    engine.sabotage_truncate_scripts();
                }
                Ok(engine)
            },
            format!("globalThis.__bramley_player = {setup};"),
            secrets,
            every,
            waker,
        )
    }
}

pub(crate) fn spawn_with<B: Backend>(
    make: impl FnOnce() -> Result<B> + Send + 'static,
    setup: String,
    secrets: Vec<String>,
    every: Duration,
    waker: impl Fn() + Send + 'static,
) -> (Sender<Command>, Receiver<Snapshot>) {
    let (command_tx, commands) = channel();
    let (snapshots, snapshot_rx) = channel();
    let thread = std::thread::Builder::new()
        .name("bramley-player".into())
        .stack_size(PLAYER_STACK)
        .spawn(move || {
            let started = make().and_then(|b| {
                b.eval_script("config.js", &setup)?;
                b.eval_script("player.js", PLAYER)?;
                Ok(b)
            });
            match started {
                Ok(b) => run(b, &commands, &snapshots, &waker, every, &secrets),
                Err(e) => {
                    let error = PlayerError::LoadFailed(redact(&format!("{e:#}"), &secrets));
                    let _ = snapshots.send(Snapshot {
                        error: Some(error),
                        ..Snapshot::default()
                    });
                    waker();
                }
            }
        });
    if let Err(e) = thread {
        eprintln!("player thread failed to start: {e}");
    }
    (command_tx, snapshot_rx)
}

fn run<B: Backend>(
    mut b: B,
    commands: &Receiver<Command>,
    snapshots: &Sender<Snapshot>,
    waker: &dyn Fn(),
    every: Duration,
    secrets: &[String],
) {
    let mut last: Option<Snapshot> = None;
    let mut due = Instant::now();
    #[cfg(feature = "diagnostics")]
    let mut engine_due = Instant::now();
    loop {
        let wait = b.tick();
        match commands.recv_timeout(wait) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(command) => {
                if let Err(e) = b.eval_script("command", &command_js(&command)) {
                    eprintln!("player command failed: {}", redact(&e.to_string(), secrets));
                }
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() < due {
            continue;
        }
        due = Instant::now() + every;
        #[allow(unused_mut)]
        let mut snap = snapshot(&b, secrets);
        #[cfg(feature = "diagnostics")]
        if let Some(d) = snap.diagnostics.as_mut()
            && Instant::now() >= engine_due
        {
            engine_due = Instant::now() + ENGINE_STATUS_EVERY;
            d.engine = Some(b.status());
        }
        if last.as_ref() != Some(&snap) {
            if snapshots.send(snap.clone()).is_err() {
                return;
            }
            waker();
            last = Some(snap);
        }
    }
}

pub(crate) fn command_js(command: &Command) -> String {
    let (method, arg) = match command {
        Command::SetQueue { queue, start } => {
            let mut options = match queue {
                Queue::Songs(ids) => json!({"songs": ids}),
                Queue::Album(id) => json!({"album": id}),
                Queue::Playlist(id) => json!({"playlist": id}),
            };
            options["startWith"] = json!(start);
            ("setQueue", options)
        }
        Command::Play => ("play", J::Null),
        Command::Pause => ("pause", J::Null),
        Command::Seek(t) => ("seek", json!(t)),
        Command::Next => ("next", J::Null),
        Command::Previous => ("previous", J::Null),
        Command::Volume(v) => ("volume", json!(v)),
        Command::Shutdown => return String::new(),
    };
    format!("__player.{method}({arg});")
}

fn snapshot<B: Backend>(b: &B, secrets: &[String]) -> Snapshot {
    let js = match b.eval_json("__player.state()") {
        Ok(js) => js,
        Err(e) => {
            return Snapshot {
                error: Some(PlayerError::LoadFailed(redact(&e.to_string(), secrets))),
                ..Snapshot::default()
            };
        }
    };
    let text = |v: &J| v.as_str().map(|s| redact(s, secrets));
    let error = if let Some(reason) = text(&js["signInNeeded"]) {
        Some(PlayerError::SignInNeeded(reason))
    } else if let Some(phase @ ("musickit-load-failed" | "configure-failed")) = js["phase"].as_str()
    {
        Some(PlayerError::LoadFailed(phase.into()))
    } else {
        text(&js["playbackError"]).map(PlayerError::Playback)
    };
    Snapshot {
        item_id: text(&js["item"]["id"]),
        title: text(&js["item"]["title"]),
        position: b.current_time(),
        duration: js["item"]["durationInMillis"]
            .as_f64()
            .map(|ms| ms / 1000.0),
        state: match js["playbackState"].as_u64() {
            Some(1 | 6 | 8 | 9) => PlayState::Loading,
            Some(2) => PlayState::Playing,
            Some(3) => PlayState::Paused,
            _ => PlayState::Stopped,
        },
        error,
        #[cfg(feature = "diagnostics")]
        diagnostics: Some(Diagnostics { js, engine: None }),
    }
}
