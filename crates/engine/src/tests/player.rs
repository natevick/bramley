use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde_json::{Value as J, json};

use super::element::engine;
use crate::player::{
    Backend, Command, PlayState, PlayerError, Queue, Snapshot, command_js, spawn_with,
};

const SECRET: &str = "sekrit-token-value-123456";
const SNAPSHOT_EVERY: Duration = Duration::from_millis(5);

#[derive(Default)]
struct Fake {
    scripts: Vec<String>,
    unticked_command: bool,
    snapshots_before_tick: usize,
    ticks: usize,
    panic_at_tick: Option<usize>,
    state: J,
    time: Option<f64>,
}

#[derive(Clone, Default)]
struct FakeEngine(Arc<Mutex<Fake>>);

impl FakeEngine {
    fn set(&self, state: J, time: Option<f64>) {
        let mut f = self.0.lock().unwrap();
        f.state = state;
        f.time = time;
    }
}

impl Backend for FakeEngine {
    fn tick(&mut self) -> Duration {
        let panic = {
            let mut f = self.0.lock().unwrap();
            f.unticked_command = false;
            f.ticks += 1;
            f.panic_at_tick == Some(f.ticks)
        };
        if panic {
            panic!("fake engine panic");
        }
        Duration::from_millis(1)
    }
    fn eval_script(&self, _name: &str, source: &str) -> Result<()> {
        let mut f = self.0.lock().unwrap();
        f.unticked_command = source.starts_with("__player.");
        f.scripts.push(source.into());
        Ok(())
    }
    fn eval_json(&self, expression: &str) -> Result<J> {
        assert_eq!(expression, "__player.state()");
        let mut f = self.0.lock().unwrap();
        f.snapshots_before_tick += usize::from(f.unticked_command);
        Ok(f.state.clone())
    }
    fn current_time(&self) -> Option<f64> {
        self.0.lock().unwrap().time
    }
    #[cfg(feature = "diagnostics")]
    fn status(&self) -> J {
        J::Null
    }
}

fn spawn_fake(
    make: impl FnOnce() -> Result<FakeEngine> + Send + 'static,
    every: Duration,
) -> (Sender<Command>, Receiver<Snapshot>, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let (tx, rx) = spawn_with(
        make,
        "setup".into(),
        vec![SECRET.into()],
        every,
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    (tx, rx, wakes)
}

fn wait_for(rx: &Receiver<Snapshot>, pred: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let snap = rx.recv_timeout(left).expect("no matching snapshot");
        if pred(&snap) {
            return snap;
        }
    }
}

fn drain_until_stopped(rx: &Receiver<Snapshot>) -> usize {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received = 0;
    while rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_ok()
    {
        received += 1;
    }
    assert!(Instant::now() < deadline, "the player thread did not stop");
    received
}

#[test]
fn ids_are_passed_as_json_so_they_cannot_run_script() {
    let (e, _sink) = engine();
    e.eval_script("stub", "globalThis.calls = []; globalThis.__player = new Proxy({}, { get: (_, k) => (...a) => calls.push([k, a]) });").unwrap();
    let evil = r#"x");globalThis.pwned=1;(""#;
    for queue in [
        Queue::Songs(vec![evil.into()]),
        Queue::Album(evil.into()),
        Queue::Playlist(evil.into()),
    ] {
        e.eval_script(
            "command",
            &command_js(&Command::SetQueue { queue, start: 2 }),
        )
        .unwrap();
    }
    e.eval_script("command", &command_js(&Command::Seek(30.5)))
        .unwrap();
    assert_eq!(e.eval_json("typeof globalThis.pwned").unwrap(), "undefined");
    assert_eq!(
        e.eval_json("calls").unwrap(),
        json!([
            ["setQueue", [{"songs": [evil], "startWith": 2}]],
            ["setQueue", [{"album": evil, "startWith": 2}]],
            ["setQueue", [{"playlist": evil, "startWith": 2}]],
            ["seek", [30.5]],
        ])
    );
}

#[test]
fn commands_run_in_order_after_setup_and_are_ticked_before_a_snapshot() {
    let fake = FakeEngine::default();
    let f = fake.clone();
    let (tx, rx, _) = spawn_fake(move || Ok(f), Duration::ZERO);
    let commands = [
        Command::SetQueue {
            queue: Queue::Songs(vec!["1".into(), "2".into()]),
            start: 1,
        },
        Command::Play,
        Command::Volume(0.5),
        Command::Next,
        Command::Previous,
        Command::Pause,
        Command::Shutdown,
    ];
    for c in commands {
        tx.send(c).unwrap();
        std::thread::sleep(Duration::from_millis(8));
    }
    drain_until_stopped(&rx);
    assert_eq!(fake.0.lock().unwrap().snapshots_before_tick, 0);
    let scripts = fake.0.lock().unwrap().scripts.clone();
    assert_eq!(scripts[0], "setup");
    assert!(scripts[1].contains("seedUserTokenIntoMusicKitStorage"));
    assert_eq!(
        scripts[2..],
        [
            r#"__player.setQueue({"songs":["1","2"],"startWith":1});"#,
            "__player.play(null);",
            "__player.volume(0.5);",
            "__player.next(null);",
            "__player.previous(null);",
            "__player.pause(null);",
        ]
    );
}

#[test]
fn waker_fires_once_per_snapshot_and_unchanged_state_sends_none() {
    let fake = FakeEngine::default();
    fake.set(json!({"playbackState": 2, "item": {"id": "1"}}), Some(1.0));
    let f = fake.clone();
    let (tx, rx, wakes) = spawn_fake(move || Ok(f), SNAPSHOT_EVERY);
    std::thread::sleep(Duration::from_millis(100));
    fake.set(json!({"playbackState": 2, "item": {"id": "1"}}), Some(2.0));
    std::thread::sleep(Duration::from_millis(100));
    tx.send(Command::Shutdown).unwrap();
    let received = drain_until_stopped(&rx);
    #[cfg(not(feature = "diagnostics"))]
    assert_eq!(received, 2, "an unchanged state was resent");
    assert_eq!(wakes.load(Ordering::SeqCst), received);
}

#[test]
fn snapshots_use_native_time_and_map_errors_redacted() {
    let fake = FakeEngine::default();
    let f = fake.clone();
    let (tx, rx, _) = spawn_fake(move || Ok(f), SNAPSHOT_EVERY);
    fake.set(
        json!({"playbackState": 3, "currentPlaybackTime": 9, "applied": 3, "previewOnly": true, "item": {"id": "1", "title": "T", "durationInMillis": 354000}}),
        Some(9.876),
    );
    let s = wait_for(&rx, |s| s.item_id.is_some());
    assert_eq!(
        (s.title.as_deref(), s.position, s.duration, s.state, s.error),
        (Some("T"), Some(9.876), Some(354.0), PlayState::Paused, None)
    );
    assert_eq!((s.applied, s.preview_only), (3, true));
    let cases = [
        (
            json!({"signInNeeded": format!("401 for {SECRET}")}),
            PlayerError::SignInNeeded("401 for <redacted>".into()),
        ),
        (
            json!({"playbackError": "NOT_FOUND"}),
            PlayerError::Playback("NOT_FOUND".into()),
        ),
        (
            json!({"phase": "musickit-load-failed"}),
            PlayerError::LoadFailed("musickit-load-failed".into()),
        ),
    ];
    for (state, error) in cases {
        fake.set(state, None);
        let s = wait_for(&rx, |s| s.error.as_ref() == Some(&error));
        assert_eq!(s.applied, 3, "applied went backwards");
        #[cfg(feature = "diagnostics")]
        assert!(!format!("{:?}", s.diagnostics).contains(SECRET));
    }
    tx.send(Command::Shutdown).unwrap();
}

#[test]
fn an_engine_that_cannot_start_reports_load_failed_once() {
    let (_tx, rx, wakes) = spawn_fake(|| Err(anyhow!("no CDM for {SECRET}")), SNAPSHOT_EVERY);
    let s = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        s.error,
        Some(PlayerError::LoadFailed("no CDM for <redacted>".into()))
    );
    assert_eq!(drain_until_stopped(&rx), 0);
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
}

#[test]
fn dropping_the_command_sender_stops_the_player() {
    let (tx, rx, _) = spawn_fake(|| Ok(FakeEngine::default()), SNAPSHOT_EVERY);
    drop(tx);
    drain_until_stopped(&rx);
}

#[test]
fn dropping_the_snapshot_receiver_stops_an_idle_player() {
    let (tx, rx, _) = spawn_fake(|| Ok(FakeEngine::default()), SNAPSHOT_EVERY);
    wait_for(&rx, |_| true);
    drop(rx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while tx.send(Command::Pause).is_ok() {
        assert!(
            Instant::now() < deadline,
            "the player outlived its receiver"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_panicking_engine_sends_a_last_error_and_wakes() {
    let fake = FakeEngine::default();
    fake.0.lock().unwrap().panic_at_tick = Some(20);
    let f = fake.clone();
    let (_tx, rx, wakes) = spawn_fake(move || Ok(f), SNAPSHOT_EVERY);
    let snaps: Vec<Snapshot> =
        std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(5)).ok()).collect();
    let panicked = PlayerError::LoadFailed("the player thread panicked".into());
    assert_eq!(snaps.last().and_then(|s| s.error.clone()), Some(panicked));
    assert_eq!(wakes.load(Ordering::SeqCst), snaps.len());
}

#[test]
fn config_debug_never_prints_tokens_or_the_redact_list() {
    let mut cfg = crate::PlayerConfig::default();
    cfg.engine.redact = vec![SECRET.into()];
    let text = format!("{cfg:?}");
    assert!(text.contains("snapshot_interval"), "{text}");
    assert!(!text.contains(SECRET) && !text.contains("redact"), "{text}");
}
