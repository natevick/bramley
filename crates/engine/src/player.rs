#[cfg(feature = "runtime")]
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;
#[cfg(feature = "runtime")]
use std::time::Instant;

#[cfg(feature = "runtime")]
use anyhow::Result;
use serde_json::Value as J;
#[cfg(feature = "runtime")]
use serde_json::json;

use crate::EngineConfig;
#[cfg(feature = "runtime")]
use crate::{Engine, redact};

#[cfg(feature = "runtime")]
pub(crate) const PLAYER: &str = include_str!("../js/player.js");
#[cfg(feature = "runtime")]
const MUSICKIT_URL: &str = "https://js-cdn.music.apple.com/musickit/v3/musickit.js";
#[cfg(feature = "runtime")]
const PLAYER_STACK: usize = 8 * 1024 * 1024;
const COMMAND_WAIT_MS: u64 = 5000;
const STOREFRONT_DEADLINE_MS: u64 = 10_000;
pub const STOREFRONT_DEADLINE: Duration = Duration::from_millis(STOREFRONT_DEADLINE_MS);
pub const STOREFRONT_BACKOFF: [Duration; 2] =
    [Duration::from_millis(250), Duration::from_millis(750)];
pub const QUEUE_SHOWN: usize = 50;
pub const COMMAND_WAIT: Duration = Duration::from_millis(COMMAND_WAIT_MS);
#[cfg(feature = "runtime")]
const HEARTBEAT: Duration = Duration::from_secs(1);
#[cfg(all(feature = "runtime", feature = "diagnostics"))]
const ENGINE_STATUS_EVERY: Duration = Duration::from_secs(1);

pub struct Tokens {
    pub developer: String,
    pub user: String,
}

#[derive(Clone)]
#[non_exhaustive]
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

impl std::fmt::Debug for PlayerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerConfig")
            .field("engine", &self.engine)
            .field("snapshot_interval", &self.snapshot_interval)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Queue {
    Songs(Vec<String>),
    Album(String),
    Playlist(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn as_str(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }

    pub fn parse(word: &str) -> Option<Repeat> {
        [Repeat::Off, Repeat::All, Repeat::One]
            .into_iter()
            .find(|r| r.as_str() == word)
    }

    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    #[cfg(feature = "runtime")]
    fn musickit(self) -> &'static str {
        match self {
            Repeat::Off => "none",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Command {
    SetQueue { queue: Queue, start: usize },
    Play,
    Pause,
    Seek(f64),
    Next,
    Previous,
    Volume(f32),
    PlayLater(Queue),
    Shuffle(bool),
    Repeat(Repeat),
    TopUp(Queue),
    PlayNext(Queue),
    JumpTo { index: usize, id: String },
    Shutdown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum PlayState {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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
pub struct QueueItem {
    pub id: String,
    pub title: Option<String>,
    pub duration: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Snapshot {
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub state: PlayState,
    pub error: Option<PlayerError>,
    pub preview_only: bool,
    pub applied: u64,
    pub drains_acked: u64,
    pub drain_timeouts: u64,
    pub sent_us: u64,
    pub skipped_us: u64,
    pub decrypted_us: u64,
    pub readiness_dips: u64,
    pub decrypt_fails: u64,
    pub output_underflows: u64,
    pub queue: Vec<QueueItem>,
    pub queue_position: Option<usize>,
    pub queue_length: usize,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub background_done: u64,
    #[cfg(feature = "diagnostics")]
    pub diagnostics: Option<Diagnostics>,
}

impl Snapshot {
    pub fn http_statuses(&self, path: &str) -> J {
        #[cfg(feature = "diagnostics")]
        if let Some(J::Object(seen)) = self
            .diagnostics
            .as_ref()
            .and_then(|d| d.engine.as_ref())
            .map(|e| &e["platform"]["xhr"]["endpoints"])
        {
            let ours = seen.iter().filter(|(key, _)| key.contains(path));
            return ours
                .map(|(key, v)| (key.clone(), v["statuses"].clone()))
                .collect();
        }
        let _ = path;
        J::Null
    }
}

#[cfg(feature = "runtime")]
pub(crate) trait Backend {
    fn tick(&mut self) -> Duration;
    fn eval_script(&self, name: &str, source: &str) -> Result<()>;
    fn eval_json(&self, expression: &str) -> Result<J>;
    fn applied(&self) -> u64;
    fn item_time(&self) -> Option<f64>;
    fn output_failed(&self) -> bool;
    fn output_counts(&self) -> crate::runtime::OutputCounts;
    #[cfg(feature = "diagnostics")]
    fn status(&self) -> J;
}

#[cfg(feature = "runtime")]
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
    fn applied(&self) -> u64 {
        Engine::global_number(self, "__player", "applied").map_or(0, |n| n as u64)
    }
    fn item_time(&self) -> Option<f64> {
        Engine::item_time(self)
    }
    fn output_failed(&self) -> bool {
        Engine::output_failed(self)
    }
    fn output_counts(&self) -> crate::runtime::OutputCounts {
        Engine::output_counts(self)
    }
    #[cfg(feature = "diagnostics")]
    fn status(&self) -> J {
        Engine::status(self)
    }
}

#[cfg(feature = "runtime")]
pub struct Player;

#[cfg(feature = "runtime")]
impl Player {
    pub fn spawn(
        mut cfg: PlayerConfig,
        tokens: Tokens,
        waker: impl Fn() + Send + 'static,
    ) -> Result<(Sender<Command>, Receiver<Snapshot>)> {
        cfg.engine
            .redact
            .extend([tokens.developer.clone(), tokens.user.clone()]);
        let secrets = cfg.engine.redact.clone();
        let every = cfg.snapshot_interval;
        let setup = json!({"developerToken": tokens.developer, "userToken": tokens.user, "musickitUrl": MUSICKIT_URL, "build": env!("CARGO_PKG_VERSION"), "commandWaitMs": COMMAND_WAIT_MS, "storefrontDeadlineMs": STOREFRONT_DEADLINE_MS, "storefrontBackoffMs": STOREFRONT_BACKOFF.map(|wait| wait.as_millis() as u64), "queueShown": QUEUE_SHOWN});
        spawn_with(
            move || {
                let engine = Engine::new(cfg.engine)?;
                #[cfg(feature = "diagnostics")]
                engine.enable_diagnostics()?;
                #[cfg(feature = "sabotage")]
                if cfg.truncate_musickit {
                    engine.sabotage_truncate_scripts();
                }
                Ok(engine)
            },
            format!("globalThis.__bramley_player = {setup};"),
            secrets,
            every,
            START_STALL,
            waker,
        )
    }
}

#[cfg(feature = "runtime")]
struct Exit<W: Fn()> {
    snapshots: Sender<Snapshot>,
    waker: W,
    applied: u64,
}

#[cfg(feature = "runtime")]
impl<W: Fn()> Exit<W> {
    fn send(&self, snap: Snapshot) -> bool {
        let sent = self.snapshots.send(snap).is_ok();
        (self.waker)();
        sent
    }
}

#[cfg(feature = "runtime")]
impl<W: Fn()> Drop for Exit<W> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.send(Snapshot {
                error: Some(PlayerError::LoadFailed("the player thread panicked".into())),
                applied: self.applied,
                ..Snapshot::default()
            });
        }
    }
}

#[cfg(feature = "runtime")]
pub(crate) fn spawn_with<B: Backend>(
    make: impl FnOnce() -> Result<B> + Send + 'static,
    setup: String,
    secrets: Vec<String>,
    every: Duration,
    stall_after: Duration,
    waker: impl Fn() + Send + 'static,
) -> Result<(Sender<Command>, Receiver<Snapshot>)> {
    let (command_tx, commands) = channel();
    let (snapshots, snapshot_rx) = channel();
    std::thread::Builder::new()
        .name("bramley-player".into())
        .stack_size(PLAYER_STACK)
        .spawn(move || {
            let mut exit = Exit {
                snapshots,
                waker,
                applied: 0,
            };
            let started = make().and_then(|b| {
                b.eval_script("config.js", &setup)?;
                b.eval_script("player.js", PLAYER)?;
                Ok(b)
            });
            match started {
                Ok(b) => run(b, &commands, &mut exit, every, stall_after, &secrets),
                Err(e) => {
                    exit.send(Snapshot {
                        error: Some(PlayerError::LoadFailed(redact(&format!("{e:#}"), &secrets))),
                        ..Snapshot::default()
                    });
                }
            }
        })?;
    Ok((command_tx, snapshot_rx))
}

#[cfg(feature = "runtime")]
fn run<B: Backend, W: Fn()>(
    mut b: B,
    commands: &Receiver<Command>,
    exit: &mut Exit<W>,
    every: Duration,
    stall_after: Duration,
    secrets: &[String],
) {
    let mut last: Option<Snapshot> = None;
    let mut watch = StartWatch::default();
    let mut due = Instant::now();
    let mut sent_at = Instant::now();
    #[cfg(feature = "diagnostics")]
    let mut engine_due = Instant::now();
    loop {
        let wait = b.tick().min(due.saturating_duration_since(Instant::now()));
        match commands.recv_timeout(wait) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(command) => {
                watch.command(&command);
                if let Err(e) = b.eval_script("command", &command_js(&command)) {
                    eprintln!("player command failed: {}", redact(&e.to_string(), secrets));
                }
                due = Instant::now();
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if b.applied() > exit.applied {
            due = Instant::now();
        }
        if Instant::now() < due {
            continue;
        }
        due = Instant::now() + every;
        let mut snap = snapshot(&b, secrets);
        snap.applied = snap.applied.max(exit.applied);
        exit.applied = snap.applied;
        watch.observe(&mut snap, stall_after);
        #[cfg(feature = "diagnostics")]
        if let Some(d) = snap.diagnostics.as_mut()
            && Instant::now() >= engine_due
        {
            engine_due = Instant::now() + ENGINE_STATUS_EVERY;
            d.engine = Some(b.status());
        }
        if last.as_ref() != Some(&snap) || sent_at.elapsed() >= HEARTBEAT {
            if !exit.send(snap.clone()) {
                return;
            }
            sent_at = Instant::now();
            last = Some(snap);
        }
    }
}

#[cfg(feature = "runtime")]
pub const OUTPUT_FAILED: &str = "the audio output could not be opened";
#[cfg(feature = "runtime")]
pub const START_STALL: Duration = Duration::from_secs(10);
#[cfg(feature = "runtime")]
pub const START_STALLED: &str =
    "playback did not start within 10 s; the license or the network may have failed";

#[cfg(feature = "runtime")]
#[derive(Default)]
struct StartWatch {
    armed: Option<Instant>,
    last: Option<f64>,
}

#[cfg(feature = "runtime")]
impl StartWatch {
    fn command(&mut self, command: &Command) {
        match command {
            Command::SetQueue { .. }
            | Command::Play
            | Command::Next
            | Command::Previous
            | Command::JumpTo { .. } => self.arm(),
            Command::Seek(_) if self.armed.is_some() => self.arm(),
            Command::Pause => self.armed = None,
            _ => {}
        }
    }

    fn arm(&mut self) {
        self.armed = Some(Instant::now());
        self.last = None;
    }

    fn observe(&mut self, snap: &mut Snapshot, limit: Duration) {
        let Some(armed) = self.armed else { return };
        if snap.state == PlayState::Playing {
            if let (Some(now), Some(before)) = (snap.position, self.last)
                && now > before + 0.01
            {
                self.armed = None;
                return;
            }
            self.last = snap.position.or(self.last);
        }
        let waiting = matches!(snap.state, PlayState::Playing | PlayState::Loading);
        if waiting && snap.error.is_none() && armed.elapsed() >= limit {
            snap.error = Some(PlayerError::Playback(START_STALLED.into()));
        }
    }
}

#[cfg(feature = "runtime")]
pub(crate) fn command_js(command: &Command) -> String {
    let (method, arg) = match command {
        Command::SetQueue { queue, start } => {
            let mut options = queue_options(queue);
            options["startWith"] = json!(start);
            ("setQueue", options)
        }
        Command::Play => ("play", J::Null),
        Command::Pause => ("pause", J::Null),
        Command::Seek(t) => ("seek", json!(t)),
        Command::Next => ("next", J::Null),
        Command::Previous => ("previous", J::Null),
        Command::Volume(v) => ("volume", json!(v)),
        Command::PlayLater(queue) => ("playLater", queue_options(queue)),
        Command::Shuffle(on) => ("shuffle", json!(on)),
        Command::Repeat(mode) => ("repeat", json!(mode.musickit())),
        Command::TopUp(queue) => ("topUp", queue_options(queue)),
        Command::PlayNext(queue) => ("playNext", queue_options(queue)),
        Command::JumpTo { index, id } => ("jumpTo", json!([index, id])),
        Command::Shutdown => return String::new(),
    };
    format!("__player.{method}({arg});")
}

#[cfg(feature = "runtime")]
fn queue_options(queue: &Queue) -> J {
    match queue {
        Queue::Songs(ids) => json!({"songs": ids}),
        Queue::Album(id) => json!({"album": id}),
        Queue::Playlist(id) => json!({"playlist": id}),
    }
}

#[cfg(feature = "runtime")]
fn snapshot<B: Backend>(b: &B, secrets: &[String]) -> Snapshot {
    let js = match b.eval_json("__player.state()") {
        Ok(js) => serde_json::from_str(&redact(&js.to_string(), secrets)).unwrap_or(J::Null),
        Err(e) => {
            return Snapshot {
                error: Some(PlayerError::Playback(redact(&e.to_string(), secrets))),
                ..Snapshot::default()
            };
        }
    };
    let text = |v: &J| v.as_str().map(str::to_owned);
    let seconds = |v: &J| v.as_f64().map(|ms| ms / 1000.0);
    let queue = &js["queue"];
    let shown = queue["items"].as_array().map_or(&[][..], Vec::as_slice);
    let shown = shown.iter().take(QUEUE_SHOWN).map(|item| QueueItem {
        id: text(&item["id"]).unwrap_or_default(),
        title: text(&item["title"]),
        duration: seconds(&item["durationInMillis"]),
    });
    let error = if let Some(reason) = text(&js["signInNeeded"]) {
        Some(PlayerError::SignInNeeded(reason))
    } else if let Some(phase @ ("musickit-load-failed" | "configure-failed")) = js["phase"].as_str()
    {
        Some(PlayerError::LoadFailed(phase.into()))
    } else if let Some(reason) = text(&js["loadFailed"]) {
        Some(PlayerError::LoadFailed(reason))
    } else if b.output_failed() {
        Some(PlayerError::Playback(OUTPUT_FAILED.into()))
    } else {
        text(&js["playbackError"]).map(PlayerError::Playback)
    };
    let [
        drains_acked,
        drain_timeouts,
        sent_us,
        skipped_us,
        decrypted_us,
        readiness_dips,
        decrypt_fails,
        output_underflows,
    ] = b.output_counts();
    Snapshot {
        item_id: text(&js["item"]["id"]),
        title: text(&js["item"]["title"]),
        artist: text(&js["item"]["artistName"]),
        position: b.item_time(),
        duration: seconds(&js["item"]["durationInMillis"]),
        state: match js["playbackState"].as_u64() {
            Some(1 | 6 | 8 | 9) => PlayState::Loading,
            Some(2) => PlayState::Playing,
            Some(3) => PlayState::Paused,
            _ => PlayState::Stopped,
        },
        error,
        preview_only: js["previewOnly"] == true,
        applied: js["applied"].as_u64().unwrap_or(0),
        drains_acked,
        drain_timeouts,
        sent_us,
        skipped_us,
        decrypted_us,
        readiness_dips,
        decrypt_fails,
        output_underflows,
        queue: shown.collect(),
        queue_position: queue["position"].as_u64().map(|at| at as usize),
        queue_length: queue["length"].as_u64().unwrap_or(0) as usize,
        shuffle: js["shuffleMode"] == 1,
        repeat: match js["repeatMode"].as_u64() {
            Some(1) => Repeat::One,
            Some(2) => Repeat::All,
            _ => Repeat::Off,
        },
        background_done: js["background"].as_u64().unwrap_or(0),
        #[cfg(feature = "diagnostics")]
        diagnostics: Some(Diagnostics {
            js: serde_json::from_str(&without_queue(&js)).unwrap_or(J::Null),
            engine: None,
        }),
    }
}

#[cfg(feature = "runtime")]
#[cfg(feature = "diagnostics")]
fn without_queue(js: &J) -> String {
    let mut js = js.clone();
    if let Some(state) = js.as_object_mut() {
        state.remove("queue");
    }
    js.to_string()
}
