use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde_json::{Value as J, json};

use super::element::engine;
use crate::player::{
    Backend, Command, PlayState, PlayerError, QUEUE_SHOWN, Queue, QueueItem, Snapshot, command_js,
    spawn_with,
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
    output_failed: bool,
    read_fails: bool,
    output: crate::runtime::OutputCounts,
    wait: Option<Duration>,
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
        let (panic, wait) = {
            let mut f = self.0.lock().unwrap();
            f.unticked_command = false;
            f.ticks += 1;
            (f.panic_at_tick == Some(f.ticks), f.wait)
        };
        if panic {
            panic!("fake engine panic");
        }
        wait.unwrap_or(Duration::from_millis(1))
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
        if f.read_fails {
            return Err(anyhow!("eval failed: {SECRET}"));
        }
        f.snapshots_before_tick += usize::from(f.unticked_command);
        Ok(f.state.clone())
    }
    fn applied(&self) -> u64 {
        self.0.lock().unwrap().state["applied"]
            .as_u64()
            .unwrap_or(0)
    }
    fn current_time(&self) -> Option<f64> {
        self.0.lock().unwrap().time
    }
    fn output_failed(&self) -> bool {
        self.0.lock().unwrap().output_failed
    }
    fn output_counts(&self) -> crate::runtime::OutputCounts {
        self.0.lock().unwrap().output
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
    spawn_fake_stalling_after(make, every, Duration::from_secs(60))
}

fn spawn_fake_stalling_after(
    make: impl FnOnce() -> Result<FakeEngine> + Send + 'static,
    every: Duration,
    stall_after: Duration,
) -> (Sender<Command>, Receiver<Snapshot>, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let (tx, rx) = spawn_with(
        make,
        "setup".into(),
        vec![SECRET.into()],
        every,
        stall_after,
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
    for command in [
        Command::PlayLater(Queue::Songs(vec![evil.into()])),
        Command::Shuffle(true),
        Command::TopUp(Queue::Songs(vec![evil.into()])),
        Command::PlayNext(Queue::Songs(vec![evil.into()])),
        Command::JumpTo {
            index: 4,
            id: evil.into(),
        },
    ] {
        e.eval_script("command", &command_js(&command)).unwrap();
    }
    assert_eq!(e.eval_json("typeof globalThis.pwned").unwrap(), "undefined");
    assert_eq!(
        e.eval_json("calls").unwrap(),
        json!([
            ["setQueue", [{"songs": [evil], "startWith": 2}]],
            ["setQueue", [{"album": evil, "startWith": 2}]],
            ["setQueue", [{"playlist": evil, "startWith": 2}]],
            ["seek", [30.5]],
            ["playLater", [{"songs": [evil]}]],
            ["shuffle", [true]],
            ["topUp", [{"songs": [evil]}]],
            ["playNext", [{"songs": [evil]}]],
            ["jumpTo", [[4, evil]]],
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
        json!({"playbackState": 3, "currentPlaybackTime": 9, "applied": 3, "previewOnly": true, "item": {"id": "1", "title": "T", "artistName": format!("A {SECRET}"), "durationInMillis": 354000}}),
        Some(9.876),
    );
    let s = wait_for(&rx, |s| s.item_id.is_some());
    assert_eq!(
        (s.title.as_deref(), s.position, s.duration, s.state, s.error),
        (Some("T"), Some(9.876), Some(354.0), PlayState::Paused, None)
    );
    assert_eq!(s.artist.as_deref(), Some("A <redacted>"));
    assert_eq!((s.applied, s.preview_only), (3, true));
    fake.0.lock().unwrap().output = [4, 1, 9, 2, 7, 3, 5, 6];
    let s = wait_for(&rx, |s| s.drains_acked > 0);
    let counts = (s.drains_acked, s.drain_timeouts, s.sent_us, s.skipped_us);
    assert_eq!(counts, (4, 1, 9, 2));
    let rest = (s.decrypted_us, s.readiness_dips, s.decrypt_fails);
    assert_eq!((rest, s.output_underflows), ((7, 3, 5), 6));
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
        (
            json!({"loadFailed": "storefront check failed (503)"}),
            PlayerError::LoadFailed("storefront check failed (503)".into()),
        ),
    ];
    for (state, error) in cases {
        fake.set(state, None);
        let s = wait_for(&rx, |s| s.error.as_ref() == Some(&error));
        assert_eq!(s.applied, 3, "applied went backwards");
        #[cfg(feature = "diagnostics")]
        assert!(!format!("{:?}", s.diagnostics).contains(SECRET));
    }
    fake.0.lock().unwrap().output_failed = true;
    fake.set(json!({"playbackState": 2}), Some(1.0));
    let output = PlayerError::Playback(crate::player::OUTPUT_FAILED.into());
    wait_for(&rx, |s| s.error.as_ref() == Some(&output));
    tx.send(Command::Shutdown).unwrap();
}

#[test]
fn a_state_read_error_mid_song_is_a_playback_error_that_clears_on_the_next_good_read() {
    let fake = FakeEngine::default();
    let f = fake.clone();
    let (tx, rx, _) = spawn_fake(move || Ok(f), SNAPSHOT_EVERY);
    fake.set(json!({"playbackState": 2, "item": {"id": "1"}}), Some(30.0));
    wait_for(&rx, |s| s.state == PlayState::Playing);
    fake.0.lock().unwrap().read_fails = true;
    let s = wait_for(&rx, |s| s.error.is_some());
    let hiccup = PlayerError::Playback("eval failed: <redacted>".into());
    assert_eq!(s.error, Some(hiccup));
    fake.0.lock().unwrap().read_fails = false;
    let s = wait_for(&rx, |s| s.state == PlayState::Playing);
    assert_eq!(s.error, None);
    tx.send(Command::Shutdown).unwrap();
}

#[test]
fn the_snapshot_carries_the_queue_window_from_the_current_item() {
    let fake = FakeEngine::default();
    let f = fake.clone();
    let (tx, rx, _) = spawn_fake(move || Ok(f), SNAPSHOT_EVERY);
    let mut items: Vec<J> = (0..QUEUE_SHOWN + 5)
        .map(|i| json!({"id": format!("i.{i}"), "title": format!("T {SECRET}"), "durationInMillis": 1500}))
        .collect();
    items[1] = json!({"id": null});
    fake.set(
        json!({"queue": {"position": 7, "length": 300, "items": items}}),
        None,
    );
    let s = wait_for(&rx, |s| s.queue_length > 0);
    assert_eq!((s.queue_position, s.queue_length), (Some(7), 300));
    assert_eq!(s.queue.len(), QUEUE_SHOWN);
    let first = QueueItem {
        id: "i.0".into(),
        title: Some("T <redacted>".into()),
        duration: Some(1.5),
    };
    assert_eq!(s.queue[0], first);
    assert_eq!(
        (s.queue[1].id.as_str(), s.queue[2].id.as_str()),
        ("", "i.2")
    );
    fake.set(
        json!({"queue": {"position": -1, "length": 0, "items": []}}),
        None,
    );
    let s = wait_for(&rx, |s| s.queue_length == 0);
    assert_eq!((s.queue_position, s.queue.len()), (None, 0));
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
fn a_rise_in_applied_is_sent_at_once_not_at_the_next_interval() {
    let fake = FakeEngine::default();
    fake.set(json!({"playbackState": 3, "applied": 0}), Some(1.0));
    let f = fake.clone();
    let (_tx, rx, _) = spawn_fake(move || Ok(f), Duration::from_secs(30));
    wait_for(&rx, |_| true);
    let raised = Instant::now();
    fake.set(json!({"playbackState": 3, "applied": 1}), Some(1.0));
    wait_for(&rx, |s| s.applied == 1);
    assert!(
        raised.elapsed() < Duration::from_millis(500),
        "{:?}",
        raised.elapsed()
    );
}

#[test]
fn a_long_engine_wait_does_not_hold_back_the_next_snapshot() {
    let fake = FakeEngine::default();
    fake.0.lock().unwrap().wait = Some(Duration::from_secs(60));
    fake.set(json!({"playbackState": 3}), Some(1.0));
    let f = fake.clone();
    let (_tx, rx, _) = spawn_fake(move || Ok(f), SNAPSHOT_EVERY);
    wait_for(&rx, |_| true);
    let changed = Instant::now();
    fake.set(json!({"playbackState": 3}), Some(2.0));
    wait_for(&rx, |s| s.position == Some(2.0));
    assert!(
        changed.elapsed() < Duration::from_millis(500),
        "{:?}",
        changed.elapsed()
    );
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

const STALL: Duration = Duration::from_millis(300);

fn stalled(s: &Snapshot) -> bool {
    s.error == Some(PlayerError::Playback(crate::player::START_STALLED.into()))
}

fn snapshots_for(rx: &Receiver<Snapshot>, d: Duration) -> Vec<Snapshot> {
    let end = Instant::now() + d;
    std::iter::from_fn(|| {
        rx.recv_timeout(end.saturating_duration_since(Instant::now()))
            .ok()
    })
    .collect()
}

fn playing(fake: &FakeEngine, state: u64, time: f64) {
    fake.set(json!({"playbackState": state, "applied": 1}), Some(time));
}

#[test]
fn a_start_whose_position_never_moves_raises_an_error_within_the_bound_and_clears_when_it_moves() {
    let fake = FakeEngine::default();
    playing(&fake, 2, 0.0);
    let f = fake.clone();
    let (tx, rx, _) = spawn_fake_stalling_after(move || Ok(f), SNAPSHOT_EVERY, STALL);
    let sent = Instant::now();
    tx.send(Command::Play).unwrap();
    wait_for(&rx, stalled);
    let took = sent.elapsed();
    assert!(
        took >= STALL && took < STALL + Duration::from_secs(1),
        "{took:?}"
    );
    for t in [0.1, 0.2, 0.3] {
        playing(&fake, 2, t);
        std::thread::sleep(Duration::from_millis(30));
    }
    let after = wait_for(&rx, |s| s.position == Some(0.3));
    assert_eq!(after.error, None);
}

#[test]
fn advancing_playback_a_pause_and_a_seek_after_playback_started_never_raise_it() {
    let fake = FakeEngine::default();
    playing(&fake, 2, 0.0);
    let f = fake.clone();
    let (tx, rx, _) = spawn_fake_stalling_after(move || Ok(f), SNAPSHOT_EVERY, STALL);
    tx.send(Command::Play).unwrap();
    for i in 1..20 {
        playing(&fake, 2, i as f64 * 0.05);
        std::thread::sleep(Duration::from_millis(30));
    }
    tx.send(Command::Seek(120.0)).unwrap();
    playing(&fake, 1, 120.0);
    assert!(
        !snapshots_for(&rx, STALL * 3).iter().any(stalled),
        "advancing play or a long seek raised it"
    );
    tx.send(Command::Play).unwrap();
    tx.send(Command::Pause).unwrap();
    playing(&fake, 3, 120.0);
    assert!(
        !snapshots_for(&rx, STALL * 3).iter().any(stalled),
        "a pause raised it"
    );
}
