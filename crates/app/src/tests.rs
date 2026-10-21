use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bramley_engine::{Command, PlayState, PlayerError, Queue, Repeat, Snapshot};
use serde_json::{Value, json};

use crate::covers::{self, Grid, Source};
use crate::credentials::Credentials;
use crate::devtoken::DeveloperToken;
use crate::playback::{Action, Banner, HOLD_LIMIT, Link, Playback, SignedOut, Spawn};
use crate::ui::nav::{Edge, Nav, Route};

type Ends = Rc<RefCell<Vec<(Receiver<Command>, Sender<Snapshot>)>>>;

fn live(ends: &Ends) -> anyhow::Result<Link> {
    let (command_tx, command_rx) = channel();
    let (snapshot_tx, snapshot_rx) = channel();
    ends.borrow_mut().push((command_rx, snapshot_tx));
    Ok((command_tx, snapshot_rx))
}

pub(crate) fn spawning(mut f: impl FnMut() -> anyhow::Result<Link> + 'static) -> Spawn {
    Box::new(move || {
        let (tx, rx) = channel();
        tx.send(f()).unwrap();
        rx
    })
}

fn fake(signed_out: bool) -> (Playback, Ends) {
    let ends: Ends = Rc::default();
    let spawned = ends.clone();
    (
        Playback::new(spawning(move || live(&spawned)), signed_out),
        ends,
    )
}

fn snap(state: PlayState, position: f64, applied: u64, error: Option<PlayerError>) -> Snapshot {
    let mut s = Snapshot::default();
    (s.state, s.position, s.applied, s.error) = (state, Some(position), applied, error);
    s
}

fn feed(p: &mut Playback, ends: &Ends, state: PlayState, position: f64, applied: u64) {
    ends.borrow()[0]
        .1
        .send(snap(state, position, applied, None))
        .unwrap();
    p.receive();
}

pub(crate) fn mock_user(dir: &std::path::Path) -> Credentials {
    let store = keyring_core::mock::Store::new().unwrap();
    Credentials::new(Some(store), "bramley-test", dir)
}

fn shown(p: &Playback) -> (PlayState, Option<f64>) {
    let s = p.shown();
    (s.state, s.position)
}

#[test]
fn no_player_is_spawned_until_the_first_play_action() {
    let (mut p, ends) = fake(false);
    for action in [
        Action::Play,
        Action::Pause,
        Action::Seek(5.0),
        Action::Next,
        Action::Previous,
    ] {
        p.apply(action);
    }
    assert_eq!((ends.borrow().len(), p.started()), (0, false));
    p.apply(Action::PlaySong("6781024437".into()));
    p.apply(Action::PlaySong("1".into()));
    assert_eq!((ends.borrow().len(), p.started()), (1, true));
}

#[test]
fn signed_out_shows_the_banner_and_never_spawns() {
    let (mut p, ends) = fake(true);
    p.apply(Action::PlaySong("6781024437".into()));
    assert_eq!(
        (ends.borrow().len(), p.banner()),
        (0, Some(Banner::SignInNeeded))
    );
}

#[test]
fn actions_map_to_commands() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    p.apply(Action::Pause);
    p.receive();
    for action in [
        Action::Play,
        Action::Seek(12.5),
        Action::Next,
        Action::Previous,
    ] {
        p.apply(action);
    }
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().collect();
    let queue = Queue::Songs(vec!["42".into()]);
    let expected = [
        Command::SetQueue { queue, start: 0 },
        Command::Play,
        Command::Pause,
        Command::Play,
        Command::Seek(12.5),
        Command::Next,
        Command::Previous,
    ];
    assert_eq!((sent, p.seq()), (expected.to_vec(), 7));
}

#[test]
fn queue_actions_send_in_order_and_a_top_up_runs_in_the_background() {
    let (mut p, ends) = fake(false);
    assert!(
        !p.apply(Action::PlayNext("x".into())),
        "no queue before a play"
    );
    p.apply(Action::PlaySong("a".into()));
    let generation = p.generation();
    p.receive();
    let songs = |ids: &[&str]| Queue::Songs(ids.iter().map(|id| id.to_string()).collect());
    let seq = p.seq();
    assert!(p.apply(Action::TopUp(vec!["c".into(), "d".into()])));
    assert_eq!(p.seq(), seq, "a top-up is not a pending user command");
    assert!(!p.background_settled());
    for action in [
        Action::QueueAdd("b".into()),
        Action::PlayNext("x".into()),
        Action::JumpTo(2, "b".into()),
    ] {
        assert!(p.apply(action));
    }
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().skip(2).collect();
    let jump = Command::JumpTo {
        index: 2,
        id: "b".into(),
    };
    let expected = [
        Command::TopUp(songs(&["c", "d"])),
        Command::PlayLater(songs(&["b"])),
    ];
    assert_eq!(sent[..2], expected);
    assert_eq!(sent[2..], [Command::PlayNext(songs(&["x"])), jump]);
    let mut done = snap(PlayState::Playing, 1.0, p.seq(), None);
    done.background_done = 1;
    ends.borrow()[0].1.send(done).unwrap();
    p.receive();
    assert!(p.background_settled());
    p.apply(Action::PlaySong("e".into()));
    assert_eq!(
        p.generation(),
        generation + 1,
        "every new queue is a new generation"
    );
    p.apply(Action::Shuffle(Queue::Album("l.1".into())));
    p.apply(Action::PlayIn(Queue::Playlist("p.1".into()), 3));
    assert_eq!(
        p.generation(),
        generation + 3,
        "so are a detail row's PlayIn and Shuffle"
    );
}

#[test]
fn play_in_starts_the_engine_on_its_queue_at_the_start_index_unless_signed_out() {
    let queue = Queue::Playlist("p.LIST".into());
    let (mut out, never) = fake(true);
    assert!(!out.apply(Action::PlayIn(queue.clone(), 3)));
    assert_eq!(never.borrow().len(), 0);
    let (mut p, ends) = fake(false);
    assert!(p.apply(Action::PlayIn(queue.clone(), 3)));
    p.receive();
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().collect();
    assert_eq!(sent, [Command::SetQueue { queue, start: 3 }, Command::Play]);
}

#[test]
fn shuffle_sets_the_container_then_shuffles_and_the_next_play_unshuffles() {
    let (mut p, ends) = fake(false);
    let album = Queue::Album("1".into());
    p.apply(Action::Shuffle(album.clone()));
    p.apply(Action::PlayIn(album.clone(), 2));
    p.apply(Action::PlayIn(album.clone(), 3));
    p.receive();
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().collect();
    let set = |start| Command::SetQueue {
        queue: album.clone(),
        start,
    };
    let expected = [
        set(0),
        Command::Shuffle(true),
        Command::Play,
        Command::Shuffle(false),
        set(2),
        Command::Play,
        set(3),
        Command::Play,
    ];
    assert_eq!(sent, expected);
    let (mut signed_out, ends) = fake(true);
    assert!(!signed_out.apply(Action::Shuffle(album.clone())));
    assert!(!signed_out.apply(Action::PlayIn(album, 0)));
    assert!(ends.borrow().is_empty());
}

#[test]
fn a_chosen_shuffle_and_repeat_reach_the_engine_and_survive_the_next_play() {
    let (mut p, ends) = fake(false);
    let album = Queue::Album("1".into());
    assert!(
        !p.apply(Action::SetShuffle(true)),
        "no engine before a play"
    );
    p.apply(Action::Shuffle(album.clone()));
    p.apply(Action::SetShuffle(true));
    p.apply(Action::SetRepeat(Repeat::One));
    p.apply(Action::PlayIn(album.clone(), 2));
    p.apply(Action::SetShuffle(false));
    p.receive();
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().collect();
    let set = |start| Command::SetQueue {
        queue: album.clone(),
        start,
    };
    let expected = [
        set(0),
        Command::Shuffle(true),
        Command::Play,
        Command::Shuffle(true),
        Command::Repeat(Repeat::One),
        set(2),
        Command::Play,
        Command::Shuffle(false),
    ];
    assert_eq!(sent, expected);
}

#[test]
fn status_reports_shuffle_and_repeat_from_the_engine_and_off_before_it_starts() {
    let (mut app, ends, inbox, ctx, config, _) = app_on(SERVE);
    let modes = |app: &crate::app::App| {
        let status = app.status_json();
        (status["shuffleMode"].clone(), status["repeatMode"].clone())
    };
    assert_eq!(modes(&app), (json!("off"), json!("off")));
    let play = crate::cli::Request::Do(Action::PlaySong("1".into()));
    inbox
        .send((play, Instant::now() + Duration::from_secs(5), channel().0))
        .unwrap();
    cover_meshes(&ctx, &mut app);
    let mut on = snap(PlayState::Playing, 1.0, 1, None);
    (on.shuffle, on.repeat) = (true, Repeat::All);
    ends.borrow()[0].1.send(on).unwrap();
    cover_meshes(&ctx, &mut app);
    let seen = modes(&app);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!(seen, (json!("on"), json!("all")));
}

#[test]
fn shown_state_holds_until_a_snapshot_has_applied_the_command() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    feed(&mut p, &ends, PlayState::Playing, 30.0, 2);
    p.apply(Action::Pause);
    p.apply(Action::Seek(100.0));
    assert_eq!(shown(&p), (PlayState::Paused, Some(100.0)));
    feed(&mut p, &ends, PlayState::Playing, 31.0, 3);
    assert_eq!(
        shown(&p),
        (PlayState::Paused, Some(100.0)),
        "a stale snapshot undid the action"
    );
    feed(&mut p, &ends, PlayState::Paused, 100.0, 4);
    feed(&mut p, &ends, PlayState::Paused, 100.2, 4);
    assert_eq!(shown(&p), (PlayState::Paused, Some(100.2)));
}

#[test]
fn a_hung_last_command_releases_the_hold_after_the_limit_without_progress() {
    let (mut p, ends) = fake(false);
    let sent = Instant::now();
    p.apply(Action::PlaySong("42".into()));
    let advance = sent + Duration::from_secs(4);
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 1, None))
        .unwrap();
    p.receive_at(advance);
    p.receive_at(sent + HOLD_LIMIT + Duration::from_millis(500));
    assert_eq!(
        shown(&p).0,
        PlayState::Playing,
        "progress at 4 s restarts the limit"
    );
    p.receive_at(advance + HOLD_LIMIT + Duration::from_millis(500));
    assert_eq!(shown(&p), (PlayState::Stopped, Some(0.0)));
}

#[test]
fn a_lingering_playback_error_does_not_release_a_newer_hold() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    let mut s = Snapshot::default();
    (s.state, s.position, s.applied) = (PlayState::Playing, Some(30.0), 2);
    s.error = Some(PlayerError::Playback("seek: x".into()));
    ends.borrow()[0].1.send(s.clone()).unwrap();
    p.receive();
    p.apply(Action::Pause);
    ends.borrow()[0].1.send(s).unwrap();
    p.receive();
    assert_eq!(shown(&p).0, PlayState::Paused);
}

#[test]
fn sign_in_needed_releases_the_hold_shows_the_banner_and_shuts_the_engine_down() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    let refused = PlayerError::SignInNeeded("401".into());
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 0, Some(refused)))
        .unwrap();
    p.receive();
    assert_eq!(
        (shown(&p).0, p.banner(), p.started()),
        (PlayState::Stopped, Some(Banner::SignInNeeded), false)
    );
    assert_eq!(
        ends.borrow()[0].0.try_iter().last(),
        Some(Command::Shutdown)
    );
}

#[test]
fn a_load_failure_drops_the_engine_without_signing_out_and_the_next_play_starts_a_fresh_one() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    let why = "the Apple Music storefront check failed (503)";
    let failed = PlayerError::LoadFailed(why.into());
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 0, Some(failed)))
        .unwrap();
    p.receive();
    assert_eq!(
        (p.started(), p.sign_in_reason(), p.banner()),
        (false, None, Some(Banner::Error(why.into())))
    );
    assert_eq!(
        ends.borrow()[0].0.try_iter().last(),
        Some(Command::Shutdown)
    );
    assert!(p.apply(Action::PlaySong("42".into())));
    assert_eq!((ends.borrow().len(), p.started(), p.engine()), (2, true, 2));
}

#[test]
fn a_playback_error_mid_song_keeps_the_engine() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    feed(&mut p, &ends, PlayState::Playing, 30.0, 3);
    let hiccup = PlayerError::Playback("eval failed: out of memory".into());
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 0, Some(hiccup)))
        .unwrap();
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Playing, 30.5, 3, None))
        .unwrap();
    p.receive();
    let shut = ends.borrow()[0]
        .0
        .try_iter()
        .any(|c| c == Command::Shutdown);
    assert_eq!((shut, p.started(), p.banner()), (false, true, None));
}

#[test]
fn a_late_startup_check_answer_neither_signs_a_signed_in_user_out_nor_touches_the_engine() {
    let (mut p, ends) = fake(true);
    p.signed_in();
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    p.relabel_signed_out("boot-no-token");
    let shut = ends.borrow()[0]
        .0
        .try_iter()
        .any(|c| c == Command::Shutdown);
    assert_eq!((p.sign_in_reason(), p.started(), shut), (None, true, false));
    let (mut out, _) = fake(true);
    out.relabel_signed_out("boot-no-token");
    assert_eq!(out.sign_in_reason(), Some("boot-no-token"));
}

#[test]
fn boot_noentry_does_not_replace_an_earlier_reason() {
    let mut p = Playback::new(spawning(|| Err(anyhow!(SignedOut))), false);
    p.apply(Action::PlaySong("42".into()));
    p.mark_signed_out("covers-403");
    p.receive();
    assert_eq!(p.sign_in_reason(), Some("covers-403"));
}

#[test]
fn every_way_of_becoming_signed_out_records_its_reason_and_signing_in_clears_it() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    let refused = PlayerError::SignInNeeded("engine-auth-403".into());
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 0, Some(refused)))
        .unwrap();
    p.receive();
    p.mark_signed_out("covers-403");
    assert_eq!(p.sign_in_reason(), Some("engine-auth-403"));
    p.signed_in();
    assert_eq!(p.sign_in_reason(), None);
    p.mark_signed_out("covers-403");
    assert_eq!(p.sign_in_reason(), Some("covers-403"));
    p.sign_out();
    assert_eq!(p.sign_in_reason(), Some("user-sign-out"));
    assert_eq!(fake(true).0.sign_in_reason(), Some("boot-unchecked"));
    let mut gone = Playback::new(spawning(|| Err(anyhow!(SignedOut))), false);
    gone.apply(Action::PlaySong("42".into()));
    gone.receive();
    assert_eq!(gone.sign_in_reason(), Some("boot-noentry"));
}

#[test]
fn a_click_on_a_dead_player_still_shows_its_last_error() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    let (rx, tx) = ends.borrow_mut().pop().unwrap();
    let panicked = PlayerError::LoadFailed("the player thread panicked".into());
    tx.send(snap(PlayState::Stopped, 0.0, 0, Some(panicked)))
        .unwrap();
    drop((rx, tx));
    p.apply(Action::Pause);
    p.receive();
    let error = Banner::Error("the player thread panicked".into());
    assert_eq!(
        (shown(&p).0, p.banner(), p.started()),
        (PlayState::Stopped, Some(error), false)
    );
}

#[test]
fn a_failed_start_shows_its_error_a_vanished_token_reads_as_sign_in_and_play_retries() {
    let ends: Ends = Rc::default();
    let (spawned, mut attempts) = (ends.clone(), 0);
    let mut p = Playback::new(
        spawning(move || {
            attempts += 1;
            match attempts {
                1 => Err(anyhow!("no CDM")),
                _ => live(&spawned),
            }
        }),
        false,
    );
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    assert_eq!(
        (p.banner(), p.started()),
        (Some(Banner::Error("no CDM".into())), false)
    );
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    assert_eq!(
        (
            p.banner(),
            p.started(),
            ends.borrow()[0].0.try_iter().count()
        ),
        (None, true, 2)
    );
    let mut gone = Playback::new(spawning(|| Err(anyhow!(SignedOut))), false);
    gone.apply(Action::PlaySong("42".into()));
    gone.receive();
    assert_eq!(gone.banner(), Some(Banner::SignInNeeded));
}

#[test]
fn boot_redacts_the_real_tokens_and_a_missing_user_token_is_signed_out() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/tmp/boot-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("developer-token"),
        "dev-token-not-a-jwt-0123456789\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("music-user-token"),
        "user-token-opaque-0123456789\n",
    )
    .unwrap();
    let user = mock_user(&dir);
    let refused = crate::boot(&user, |t| {
        Err(anyhow!("refused {} for {}", t.user, t.developer))
    });
    assert_eq!(
        format!("{:#}", refused.unwrap_err()),
        "refused <redacted> for <redacted>"
    );
    assert!(user.clear().is_ok());
    let missing = crate::boot(&user, |_| unreachable!("started without a user token"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(missing.unwrap_err().is::<SignedOut>());
}

#[test]
fn every_app_feature_carries_a_marker_and_release_unwinds() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let manifest = std::fs::read_to_string(format!("{dir}/Cargo.toml")).unwrap();
    let main = std::fs::read_to_string(format!("{dir}/src/main.rs")).unwrap();
    let section = manifest.split("\n[features]\n").nth(1).unwrap();
    let features: Vec<&str> = section
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(f, _)| f.trim())
        .collect();
    assert!(
        features.contains(&"dev"),
        "no features parsed from {dir}/Cargo.toml"
    );
    for f in features {
        let marker = format!("\"bramley-unshipped-feature:app/{f}\\0\"");
        assert!(
            main.contains(&marker),
            "feature {f} has no {marker} in main.rs"
        );
    }
    let workspace = std::fs::read_to_string(format!("{dir}/../../Cargo.toml")).unwrap();
    let release = workspace
        .split("[profile.release]")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    assert!(
        release.contains("panic = \"unwind\""),
        "a player panic must reach the banner"
    );
}

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

fn slot(name: &str) -> fastframe_instance::Slot {
    let dir = format!("../../target/c/{name}{}", std::process::id());
    let _ = std::fs::remove_dir_all(&dir);
    fastframe_instance::Slot::at(dir, crate::cli::APP_ID)
}

fn run(slot: &fastframe_instance::Slot, line: &str) -> (i32, serde_json::Value, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = crate::cli::client(slot, &words(line), &mut out, &mut err);
    let reply = serde_json::from_slice(&out).unwrap_or_default();
    (code, reply, String::from_utf8(err).unwrap())
}

type Seen = (usize, Vec<Command>, bool);

struct FakeApp {
    inject: Sender<crate::cli::Envelope>,
    waker: std::sync::Arc<std::sync::OnceLock<egui::Context>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<Seen>,
    _guard: fastframe_instance::Guard,
}

impl FakeApp {
    fn start(slot: &fastframe_instance::Slot, signed_out: bool) -> Self {
        let (inject, inbox) = channel();
        let waker = std::sync::Arc::new(std::sync::OnceLock::new());
        let handler = crate::cli::handler(inject.clone(), waker.clone());
        let woke = waker.clone();
        let fastframe_instance::Claim::First(_guard) = slot.claim("[]", handler) else {
            panic!("the test slot is already held");
        };
        let stop: std::sync::Arc<std::sync::atomic::AtomicBool> = Default::default();
        let stopped = std::sync::Arc::clone(&stop);
        let config = slot.dir().to_owned();
        let thread = std::thread::spawn(move || {
            let ctx = egui::Context::default();
            let (wake, woken) = channel();
            ctx.set_request_repaint_callback(move |_| {
                let _ = wake.send(());
            });
            let ends: Ends = Rc::default();
            let spawned = ends.clone();
            let spawn = spawning(move || live(&spawned));
            let args = crate::Args::default();
            let user = (mock_user(&config), signed_out);
            let mut app = crate::app::App::new(&ctx, args, spawn, user, (inbox, true), None);
            waker.set(ctx.clone()).unwrap();
            let mut shown = false;
            let mut pass = true;
            while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
                if std::mem::take(&mut pass)
                    || woken.recv_timeout(Duration::from_millis(20)).is_ok()
                {
                    ctx.begin_pass(egui::RawInput::default());
                    app.pump(&ctx);
                    let mut output = ctx.end_pass();
                    output.textures_delta.clear();
                    let commands = output.viewport_output;
                    let focus = egui::ViewportCommand::Focus;
                    shown |= commands.values().any(|v| v.commands.contains(&focus));
                }
            }
            let sent = ends.borrow().first().map(|e| e.0.try_iter().collect());
            (ends.borrow().len(), sent.unwrap_or_default(), shown)
        });
        FakeApp {
            inject,
            waker: woke,
            stop,
            thread,
            _guard,
        }
    }

    fn finish(self) -> Seen {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.join().unwrap()
    }
}

#[test]
fn cli_arguments_parse_into_requests_and_bad_ones_are_usage_errors() {
    use crate::cli::{Request, parse};
    let cases = [
        ("", Request::Show),
        ("now", Request::Now),
        ("play", Request::Do(Action::Play)),
        (
            "play 6781024437",
            Request::Do(Action::PlaySong("6781024437".into())),
        ),
        ("pause", Request::Do(Action::Pause)),
        ("toggle", Request::Toggle),
        ("next", Request::Do(Action::Next)),
        ("prev", Request::Do(Action::Previous)),
        ("seek 120", Request::Do(Action::Seek(120.0))),
        ("volume 40", Request::Do(Action::Volume(0.4))),
        ("queue add 7", Request::Do(Action::QueueAdd("7".into()))),
        ("shuffle on", Request::Do(Action::SetShuffle(true))),
        ("shuffle off", Request::Do(Action::SetShuffle(false))),
        ("repeat off", Request::Do(Action::SetRepeat(Repeat::Off))),
        ("repeat all", Request::Do(Action::SetRepeat(Repeat::All))),
        ("repeat one", Request::Do(Action::SetRepeat(Repeat::One))),
        (
            "search bohemian rhapsody",
            Request::Search("bohemian rhapsody".into()),
        ),
        (
            "library playlists",
            Request::Library(bramley_api::Shelf::Playlists),
        ),
        ("sign-in", Request::SignIn),
        ("sign-out", Request::SignOut),
        ("open home", Request::Nav(Nav::Go(Route::Home))),
        ("open songs", Request::Nav(Nav::Go(Route::Songs))),
        (
            "open playlist:p.1",
            Request::Nav(Nav::Go(Route::Playlist("p.1".into()))),
        ),
        (
            "open album:1440",
            Request::Nav(Nav::Go(Route::Album("1440".into()))),
        ),
        (
            "open artist:r.1",
            Request::Nav(Nav::Go(Route::Artist("r.1".into()))),
        ),
        ("scroll end", Request::Nav(Nav::Scroll(Edge::End))),
        ("scroll top", Request::Nav(Nav::Scroll(Edge::Top))),
    ];
    let longest = format!("open playlist:p.{}", "x".repeat(62));
    assert!(parse(&words(&longest)).is_ok() && parse(&words(&format!("{longest}x"))).is_err());
    for (line, request) in cases {
        assert_eq!(parse(&words(line)).unwrap(), request, "{line:?}");
    }
    for line in [
        "seek -1",
        "seek NaN",
        "seek inf",
        "volume 101",
        "volume -1",
        "play 1;rm",
        "queue add",
        "queue 7",
        "shuffle",
        "shuffle maybe",
        "repeat",
        "repeat twice",
        "search",
        "pause now",
        "sign-in now",
        "library",
        "library albums",
        "stop",
        "open",
        "open queue",
        "open album",
        "open playlist:",
        "open album:1;rm",
        "open artist:..",
        "open home now",
        "scroll",
        "scroll down",
    ] {
        assert!(parse(&words(line)).is_err(), "{line:?} parsed");
    }
}

#[test]
fn cli_exit_codes_without_a_running_app() {
    use crate::cli::{NOT_RUNNING, USAGE};
    let slot = slot("none");
    let (code, _, err) = run(&slot, "now");
    assert_eq!(code, NOT_RUNNING);
    assert!(err.contains("not running"), "{err}");
    assert_eq!(run(&slot, "volume 500").0, USAGE);
    assert_eq!(run(&slot, "search bohemian rhapsody").0, NOT_RUNNING);
    let (code, _, err) = run(&slot, "library playlists");
    assert!(code == USAGE && err.contains("bramley mcp"), "{err}");
}

#[test]
fn cli_round_trip_acks_each_command_with_the_seq_it_was_assigned() {
    let slot = slot("live");
    let app = FakeApp::start(&slot, false);
    let (code, reply, err) = run(&slot, "pause");
    assert_eq!(
        (code, &reply["seq"], &reply["engine"]),
        (crate::cli::REFUSED, &json!(0), &json!(0))
    );
    assert!(err.contains("nothing is playing"), "{err}");
    for (line, seq) in [
        ("play 6781024437", 2),
        ("pause", 3),
        ("toggle", 4),
        ("seek 120", 5),
        ("volume 40", 6),
        ("queue add 7", 7),
    ] {
        let (code, reply, err) = run(&slot, line);
        assert_eq!(
            (code, &reply["seq"], &reply["engine"]),
            (0, &json!(seq), &json!(1)),
            "{line}: {err}"
        );
    }
    assert_eq!(run(&slot, "now").1["state"], "Playing");
    assert_eq!(run(&slot, "").0, 0);
    let (spawned, sent, shown) = app.finish();
    assert!(shown, "a second launch did not show the window");
    let queue = Queue::Songs(vec!["6781024437".into()]);
    let expected = [
        Command::SetQueue { queue, start: 0 },
        Command::Play,
        Command::Pause,
        Command::Play,
        Command::Seek(120.0),
        Command::Volume(0.4),
        Command::PlayLater(Queue::Songs(vec!["7".into()])),
    ];
    assert_eq!((spawned, sent), (1, expected.to_vec()));
}

#[test]
fn a_volume_set_before_the_first_play_is_kept_and_reaches_the_engine_before_the_queue() {
    let (mut p, ends) = fake(false);
    assert!(p.apply(Action::Volume(0.4)));
    assert_eq!((ends.borrow().len(), p.volume(), p.seq()), (0, 0.4, 0));
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    p.apply(Action::Volume(0.7));
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().collect();
    let queue = Queue::Songs(vec!["42".into()]);
    let expected = [
        Command::Volume(0.4),
        Command::SetQueue { queue, start: 0 },
        Command::Play,
        Command::Volume(0.7),
    ];
    assert_eq!((sent, p.seq()), (expected.to_vec(), 4));
}

#[test]
fn the_volume_is_set_from_the_cli_shown_in_status_and_survives_a_relaunch() {
    let slot = slot("volume");
    let app = FakeApp::start(&slot, false);
    let (code, reply, err) = run(&slot, "volume 40");
    assert_eq!((code, &reply["volume"]), (0, &json!(40)), "{err}");
    app.finish();
    let saved = std::fs::read_to_string(slot.dir().join("volume"));
    assert_eq!(saved.unwrap(), "40\n");
    let relaunched = FakeApp::start(&slot, false);
    assert_eq!(run(&slot, "now").1["volume"], 40);
    relaunched.finish();
}

#[test]
fn quitting_right_after_a_volume_change_still_writes_it() {
    let dir = std::path::PathBuf::from(format!("../../target/c/quit{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ctx = egui::Context::default();
    let spawn = spawning(|| unreachable!("no player may start"));
    let user = (mock_user(&dir), true);
    let args = crate::Args::default();
    let mut app = crate::app::App::new(&ctx, args, spawn, user, (channel().1, true), None);
    app.apply(Action::Volume(0.25));
    eframe::App::on_exit(&mut app, None);
    let saved = std::fs::read_to_string(dir.join("volume"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(saved.unwrap(), "25\n");
}

#[test]
fn mcp_requests_reach_the_app_marked_as_an_agent() {
    let slot = slot("agent");
    let app = FakeApp::start(&slot, false);
    let channel = crate::mcp::agent_channel(slot);
    let reply = channel(&words("now")).unwrap();
    app.finish();
    let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["agentNotice"], true, "{reply}");
}

#[test]
fn cli_play_while_signed_out_is_refused_and_spawns_nothing() {
    let slot = slot("signed-out");
    let app = FakeApp::start(&slot, true);
    let (code, reply, err) = run(&slot, "play 6781024437");
    assert_eq!(
        (code, reply["seq"].as_u64()),
        (crate::cli::REFUSED, Some(0))
    );
    assert!(err.contains("sign in needed"), "{err}");
    let (reply, answer) = channel();
    let late = Instant::now() - Duration::from_millis(1);
    app.inject
        .send((crate::cli::Request::Show, late, reply))
        .unwrap();
    app.waker.get().unwrap().request_repaint();
    let answer = answer.recv_timeout(Duration::from_secs(1));
    assert!(answer.unwrap().contains("\"error\""));
    assert_eq!(app.finish(), (0, Vec::new(), false));
}

#[test]
fn a_command_past_its_deadline_is_never_applied_and_reports_an_error() {
    use crate::cli::{Request, command};
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    let late = Instant::now() - Duration::from_millis(1);
    let outcome = command(&mut p, Request::Do(Action::Pause), late);
    assert_eq!(outcome.map(|(key, _)| key), Some("error"));
    let sent: Vec<Command> = ends.borrow()[0].0.try_iter().collect();
    assert_eq!((p.seq(), sent.len()), (2, 2));
}

#[test]
fn the_server_deadline_is_the_earlier_of_its_own_bound_and_the_clients() {
    use crate::cli::deadline;
    let now = Instant::now();
    assert_eq!(deadline(u64::MAX, 0, now), now + Duration::from_secs(3));
    assert_eq!(
        deadline(2_500, 2_000, now),
        now + Duration::from_millis(500)
    );
    assert_eq!(deadline(1_000, 2_000, now), now);
}

#[test]
fn a_request_whose_client_deadline_passed_is_answered_late_and_never_queued() {
    let (tx, inbox) = channel();
    let mut handle = crate::cli::handler(tx, Default::default());
    let reply = handle(&json!({ "args": ["next"], "deadline_ms": 1 }).to_string());
    assert_eq!(crate::cli::outcome(&reply.unwrap()).unwrap_err().0, 1);
    assert!(inbox.try_recv().is_err());
}

#[test]
fn replies_without_a_seq_are_failures() {
    use crate::cli::{REFUSED, outcome};
    for bad in [
        "not json at all",
        "{}",
        r#"{"seq":"2"}"#,
        r#"{"seq":2,"error":"x"}"#,
    ] {
        assert_eq!(outcome(bad).unwrap_err().0, 1, "{bad}");
    }
    assert_eq!(
        outcome(r#"{"seq":2,"refused":"x"}"#).unwrap_err().0,
        REFUSED
    );
    assert_eq!(outcome(r#"{"seq":2}"#), Ok(()));
    let slot = slot("g");
    let fastframe_instance::Claim::First(_guard) =
        slot.claim("[]", |_| Some("not json at all".to_owned()))
    else {
        panic!("the test slot is already held");
    };
    assert_eq!(run(&slot, "pause").0, 1);
}

fn stalled(stall: Duration) -> (i32, i32, Vec<Option<&'static str>>, Vec<Command>) {
    let slot = slot(&format!("q{}", stall.as_millis()));
    let (tx, inbox) = channel();
    let handler = crate::cli::handler(tx, Default::default());
    let fastframe_instance::Claim::First(_guard) = slot.claim("[]", handler) else {
        panic!("the test slot is already held");
    };
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    p.receive();
    ends.borrow()[0].0.try_iter().for_each(drop);
    let t0 = Instant::now();
    let (first, second) = (slot.clone(), slot.clone());
    let a = std::thread::spawn(move || run(&first, "pause").0);
    std::thread::sleep(Duration::from_millis(300));
    let b = std::thread::spawn(move || run(&second, "next").0);
    std::thread::sleep(stall.saturating_sub(t0.elapsed()));
    let mut outcomes = Vec::new();
    while t0.elapsed() < stall + Duration::from_millis(500) {
        for (request, deadline, reply) in inbox.try_iter() {
            let outcome = crate::cli::command(&mut p, request, deadline);
            outcomes.push(outcome.map(|(key, _)| key));
            let _ = reply.send(json!({ "seq": p.seq() }).to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let codes = (a.join().unwrap(), b.join().unwrap());
    let sent = ends.borrow()[0].0.try_iter().collect();
    (codes.0, codes.1, outcomes, sent)
}

#[test]
fn a_request_queued_behind_a_stalled_one_is_never_applied_after_its_client_gave_up() {
    let (code_a, code_b, outcomes, sent) = stalled(Duration::from_secs(6));
    assert_eq!((code_a, code_b), (1, 1));
    assert_eq!(
        outcomes,
        vec![Some("error")],
        "only the stalled request queued"
    );
    assert_eq!(sent, Vec::<Command>::new());
}

#[test]
fn requests_drained_in_time_are_both_applied() {
    let (code_a, code_b, outcomes, sent) = stalled(Duration::from_secs(1));
    assert_eq!((code_a, code_b), (0, 0));
    assert_eq!(outcomes, vec![None, None]);
    assert_eq!(sent, vec![Command::Pause, Command::Next]);
}

#[test]
fn only_the_shared_temp_fallback_slot_is_refused() {
    use crate::cli::shared_fallback;
    use std::path::Path;
    let runtime = Path::new("/run/user/1000");
    assert!(!shared_fallback(&runtime.join("bramley"), runtime));
    assert!(!shared_fallback(
        Path::new("/home/u/.local/share/bramley/instance"),
        Path::new("/home/u")
    ));
    assert!(shared_fallback(
        Path::new("/tmp/bramley-instance"),
        Path::new("/tmp")
    ));
}

#[test]
fn artwork_fills_the_template_at_the_cell_pixel_size() {
    let template = "https://is1-ssl.mzstatic.com/image/thumb/a/b.jpg/{w}x{h}bb.jpg";
    let url = covers::art_url(template, covers::px(160.0, 2.0)).unwrap();
    assert!(url.ends_with("/b.jpg/320x320bb.jpg"), "{url}");
    assert_eq!(covers::px(covers::COVER_PT, 1.25), 120);
    assert_eq!(
        covers::art_url("https://x.mzstatic.com/{w}x{h}{c}.jpg", 96),
        None
    );
}

#[test]
fn only_https_mzstatic_artwork_is_fetched_unless_dev_names_the_exact_origin() {
    let ok = "https://is1-ssl.mzstatic.com/image/thumb/x/96x96bb.jpg";
    assert!(covers::allowed_art(ok, None));
    for bad in [
        "http://is1-ssl.mzstatic.com/x.jpg",
        "https://mzstatic.com.evil.example/x.jpg",
        "https://evil.example/x.jpg#.mzstatic.com",
        "https://a.mzstatic.com@evil.example/x.jpg",
        "https://a.mzstatic.com:8443/x.jpg",
        "https://evil.example/?h=.mzstatic.com",
        "not a url",
    ] {
        assert!(!covers::allowed_art(bad, None), "{bad}");
    }
    let base = Some("http://127.0.0.1:9");
    assert!(covers::allowed_art("http://127.0.0.1:9/art/1.jpg", base));
    assert!(!covers::allowed_art("http://127.0.0.1:9/art/1.jpg", None));
    for bad in [
        "http://127.0.0.1:9000/a.jpg",
        "http://127.0.0.1:9@evil.example/a.jpg",
        "https://127.0.0.1:9/a.jpg",
    ] {
        assert!(!covers::allowed_art(bad, base), "{bad}");
    }
}

#[cfg(not(feature = "dev"))]
#[test]
fn the_api_base_override_and_screenshots_are_refused_outside_dev() {
    for flag in [
        "--api-base",
        "--screenshot",
        "--after",
        "--developer-token-url",
        "--route",
        "--queue-open",
        "--theme",
        "--size",
        "--search",
        "--scroll-end",
        "--click-row",
    ] {
        let args = [flag, "1"].map(String::from);
        assert!(crate::parse_args(args.into_iter()).is_err(), "{flag}");
    }
    for flag in ["--queue-open", "--click-first-song", "--click-last"] {
        assert!(
            crate::parse_args([flag.to_owned()].into_iter()).is_err(),
            "{flag}"
        );
    }
}

#[test]
fn a_blank_user_token_is_signed_out_like_boot_sees_it() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/tmp/signed-out-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let token = dir.join("music-user-token");
    let mut seen = Vec::new();
    for body in [" \n", "user-token-opaque"] {
        std::fs::write(&token, body).unwrap();
        seen.push(mock_user(&dir).load().unwrap().is_none());
    }
    std::fs::remove_dir_all(&dir).unwrap();
    seen.push(mock_user(&dir).load().unwrap().is_none());
    assert_eq!(seen, [true, false, true]);
}

const FIXTURE_TOKEN: &str = "dev-token-fixture-0123456789";
const FIXTURE_USER: &str = "user-token-fixture-9876543210";

type Requests = Arc<Mutex<Vec<(String, Option<String>, Option<String>)>>>;

#[derive(Clone, Copy)]
struct Serve {
    me: u16,
    blips: usize,
    renewed: bool,
    chart: u16,
    big: bool,
    redirect: bool,
    bad_albums: bool,
    slow_chart: bool,
    chart_fails: usize,
    albums: usize,
    listening: bool,
}

const SERVE: Serve = Serve {
    me: 200,
    blips: 0,
    renewed: false,
    chart: 200,
    big: false,
    redirect: false,
    bad_albums: false,
    slow_chart: false,
    chart_fails: usize::MAX,
    albums: 50,
    listening: false,
};

struct Fixture {
    base: String,
    requests: Requests,
}

impl Fixture {
    fn storefront_requests(&self) -> usize {
        self.paths()
            .iter()
            .filter(|path| *path == "/v1/me/storefront")
            .count()
    }

    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.0.clone())
            .collect()
    }
}

fn header(line: &str, name: &str) -> Option<String> {
    let (key, value) = line.split_once(':')?;
    key.eq_ignore_ascii_case(name)
        .then(|| value.trim().to_owned())
}

fn fixture(serve: Serve) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (seen, art_base) = (requests.clone(), base.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            let (mut auth, mut user) = (None, None);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                auth = auth.or(header(&line, "authorization"));
                user = user.or(header(&line, "music-user-token"));
            }
            seen.lock()
                .unwrap()
                .push((path.clone(), auth.clone(), user));
            let refusal = json!({"errors": [{"detail": format!("rejected {FIXTURE_TOKEN}")}]});
            let (status, extra, body) = if path == "/v1/me/storefront" {
                if serve.me == 0 {
                    continue;
                }
                let renewed = serve.renewed && auth.as_deref() == Some("Bearer renewed-token");
                let asked = seen.lock().unwrap().iter().filter(|r| r.0 == path).count();
                let status = match renewed {
                    true => 200,
                    false if asked <= serve.blips => 403,
                    false => serve.me,
                };
                let found = json!({"data": [{"id": "zz"}]});
                (status, String::new(), found.to_string().into_bytes())
            } else if serve.listening
                && path.starts_with("/v1/")
                && !path.starts_with("/v1/me/library/albums")
            {
                let owned = |id: &str| {
                    let art = format!("{art_base}/art/{}/{{w}}x{{h}}bb.jpg", &id[2..]);
                    json!({"id": id, "type": "library-albums", "attributes": {"artwork": {"url": art}}})
                };
                let item = |id: &str, kind: &str| match kind {
                    "library-albums" => owned(id),
                    _ => json!({"id": id, "type": kind}),
                };
                let related =
                    |name: &str, found: Value| json!({"relationships": {name: {"data": [found]}}});
                let many = |name: &str, kind: &str, ids: &[&str]| -> Vec<Value> {
                    ids.iter().map(|id| related(name, item(id, kind))).collect()
                };
                let body = match path.split(['?', '/']).nth(3) {
                    Some("music-summaries") => json!({"data": [{"views": {
                        "top-albums": {"data": [related("album", item("c1005", "albums"))]},
                        "top-songs": {"data": many("song", "songs", &["7", "8"])},
                    }}]}),
                    Some("zz") if path.contains("/songs?") => {
                        json!({"data": many("albums", "albums", &["c1003", "c1005"])})
                    }
                    Some("zz") => {
                        let ids = path.split("ids=").nth(1).unwrap().split(',');
                        let shelved = |id: &str| match id {
                            "c1009" => {
                                json!({"id": id, "relationships": {"library": {"data": []}}})
                            }
                            _ => {
                                json!({"id": id, "relationships": {"library": {"data": [owned(&format!("l.{}", &id[1..]))]}}})
                            }
                        };
                        json!({"data": ids.map(shelved).collect::<Vec<_>>()})
                    }
                    Some("history") => {
                        json!({"data": [item("l.1001", "library-albums"), item("c1009", "albums")]})
                    }
                    Some("recent") => {
                        json!({"data": [item("c1002", "albums"), item("p.1", "library-playlists")]})
                    }
                    _ => json!({"data": [item("l.1049", "library-albums")]}),
                };
                (200, String::new(), body.to_string().into_bytes())
            } else if path.starts_with("/v1/me/library/albums?") {
                if serve.slow_chart {
                    std::thread::sleep(Duration::from_millis(300));
                }
                let url = |i: usize| match i {
                    7 if serve.big => format!("{art_base}/art/7big/{{w}}x{{h}}bb.jpg"),
                    3 if serve.bad_albums => format!("{art_base}/art/3/{{w}}x{{h}}{{c}}.jpg"),
                    _ => format!("{art_base}/art/{i}/{{w}}x{{h}}bb.jpg"),
                };
                let albums: Vec<_> = (0..serve.albums)
                    .map(|i| match i {
                        4 if serve.bad_albums => json!({"id": "l.1004", "attributes": {}}),
                        _ => json!({"id": format!("l.{}", 1000 + i), "attributes": {"artwork": {"url": url(i)}},
                            "relationships": {"catalog": {"data": [{"id": format!("c{}", 1000 + i)}]}}}),
                    })
                    .collect();
                let chart = json!({"data": albums});
                let charts = seen.lock().unwrap().iter().filter(|r| r.0 == path).count();
                let status = if charts > serve.chart_fails {
                    200
                } else {
                    serve.chart
                };
                let body = if status == 200 { chart } else { refusal };
                let wait = if status == 429 {
                    "Retry-After: 45\r\n"
                } else {
                    ""
                };
                (status, wait.into(), body.to_string().into_bytes())
            } else if serve.redirect && path.starts_with("/art/9/") {
                (
                    302,
                    format!("Location: {art_base}/elsewhere/9.jpg\r\n"),
                    Vec::new(),
                )
            } else if path.starts_with("/art/") {
                let size: u32 = path
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .split('x')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                let size = if path.contains("big/") {
                    size * 3
                } else {
                    size
                };
                let image =
                    image::RgbImage::from_fn(size, size, |x, y| image::Rgb([x as u8, y as u8, 90]));
                let mut jpeg = Vec::new();
                image
                    .write_to(
                        &mut std::io::Cursor::new(&mut jpeg),
                        image::ImageFormat::Jpeg,
                    )
                    .unwrap();
                (200, String::new(), jpeg)
            } else {
                (404, String::new(), Vec::new())
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(&body));
        }
    });
    Fixture { base, requests }
}

fn fixture_source(fx: &Fixture, art_prefix: bool) -> (Source, std::path::PathBuf) {
    let config = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/tmp/covers-test-{}-{}",
        std::process::id(),
        fx.base.rsplit(':').next().unwrap()
    ));
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("developer-token"), FIXTURE_TOKEN).unwrap();
    std::fs::write(config.join("music-user-token"), FIXTURE_USER).unwrap();
    let api = fx.base.clone();
    let source = Source {
        api,
        art_prefix: art_prefix.then(|| fx.base.clone()),
        user: mock_user(&config),
        art_cache: None,
        api_cache: None,
    };
    (source, config)
}

fn settle(grid: &mut Grid, ctx: &egui::Context) {
    let until = Instant::now() + Duration::from_secs(20);
    while grid.uploaded() + grid.failures() < 50
        && !grid.rejected()
        && !grid.empty()
        && !grid.rejected_developer_token().is_some()
        && Instant::now() < until
    {
        grid.receive(ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn load_covers(serve: Serve, art_prefix: bool) -> (Grid, Fixture, egui::Context) {
    let fx = fixture(serve);
    let (source, config) = fixture_source(&fx, art_prefix);
    let ctx = egui::Context::default();
    let mut grid = Grid::new(Some(source));
    grid.follow(&ctx, true);
    settle(&mut grid, &ctx);
    std::fs::remove_dir_all(&config).unwrap();
    (grid, fx, ctx)
}

#[test]
fn fifty_covers_become_textures_and_nothing_decoded_stays_once_a_frame_ends() {
    let (mut grid, fx, ctx) = load_covers(SERVE, true);
    assert_eq!(
        (grid.failed(), grid.uploaded(), grid.in_flight()),
        (None, 50, 0)
    );
    let kept = if cfg!(feature = "gate-sabotage") {
        50 * 96 * 96 * 4
    } else {
        0
    };
    assert_eq!(grid.kept(), kept);
    let requests = fx.requests.lock().unwrap().clone();
    let (bearer, user) = (
        Some(format!("Bearer {FIXTURE_TOKEN}")),
        Some(FIXTURE_USER.to_owned()),
    );
    assert_eq!(
        requests[0],
        ("/v1/me/storefront".into(), bearer.clone(), user)
    );
    let library = (
        "/v1/me/library/albums?limit=100&offset=0".into(),
        bearer,
        Some(FIXTURE_USER.to_owned()),
    );
    assert!(requests[1..6].contains(&library));
    assert_eq!(requests.len(), 56);
    for (path, auth, user) in &requests[6..] {
        assert!(path.ends_with("/96x96bb.jpg"), "{path}");
        assert_eq!((auth, user), (&None, &None), "a token went to the art host");
    }
    let mut frame = ctx.run_ui(Default::default(), |_| {});
    let uploads = frame.textures_delta.set.len();
    frame.textures_delta.clear();
    assert!(uploads >= 50, "{uploads}");
    let mut pending = ctx.tex_manager().write().take_delta();
    let left = pending.set.len();
    pending.clear();
    assert_eq!(left, 0, "decoded pixels still pending after the frame");
    grid.follow(&ctx, false);
    assert_eq!(
        (grid.uploaded(), grid.in_flight(), grid.kept()),
        (0, 0, 0),
        "sign-in lost kept covers"
    );
}

fn album(id: u32) -> covers::Album {
    covers::Album {
        id: format!("l.{id}"),
        art: Some(format!("art-{id}")),
        edition: format!("edition-{id}"),
    }
}

#[test]
fn editions_of_one_title_share_a_key_and_other_brackets_stay_part_of_the_title() {
    let key = covers::edition;
    assert_eq!(
        key("A  B", "Title (Super Deluxe Edition) [2019 Remaster]"),
        key("a b", "title")
    );
    assert_eq!(key("A", "Title (2019 Mix)"), key("A", "Title"));
    assert_ne!(key("A", "Live (Vol. 1)"), key("A", "Live (Vol. 2)"));
    assert_ne!(key("A", "Title"), key("A", "Title (Live)"));
    assert_ne!(key("A", "Title"), key("B", "Title"));
    assert_eq!(key("A", "(Deluxe Edition)"), None);
}

#[test]
fn home_ranks_library_albums_by_listening_then_fills_with_the_rest_without_repeats() {
    let same_art = covers::Album {
        id: "l.10".into(),
        edition: "other".into(),
        ..album(6)
    };
    let same_edition = covers::Album {
        id: "l.11".into(),
        art: Some("other".into()),
        ..album(9)
    };
    let library = [1, 2, 3, 4, 5, 6, 7, 8, 9]
        .map(album)
        .into_iter()
        .chain([same_art, same_edition]);
    let found: HashMap<_, _> = [1, 3, 5, 7].map(|n| (format!("c{n}"), album(n))).into();
    let (library_pick, catalog_pick) = (
        |n: u32| covers::Pick::Library(album(n)),
        |n: u32| covers::Pick::Catalog(format!("c{n}")),
    );
    let sources = [
        vec![catalog_pick(5), catalog_pick(3), catalog_pick(404)],
        vec![catalog_pick(3), catalog_pick(7)],
        vec![
            library_pick(2),
            catalog_pick(5),
            catalog_pick(4),
            library_pick(404),
        ],
        vec![catalog_pick(1)],
        vec![library_pick(8)],
    ];
    let ranked = covers::rank(&sources, &found, library.collect());
    let ranked: Vec<_> = ranked.into_iter().map(|a| a.id).collect();
    let expected = [5, 3, 7, 2, 404, 1, 8, 4, 6, 9].map(|n| format!("l.{n}"));
    assert_eq!(ranked, expected);
}

#[test]
fn home_asks_replay_heavy_rotation_recent_and_added_with_the_users_storefront() {
    let (grid, fx, _) = load_covers(
        Serve {
            listening: true,
            ..SERVE
        },
        true,
    );
    let order: Vec<_> = (0..8).filter_map(|n| grid.album(n)).collect();
    let expected = [
        "l.1005", "l.1003", "l.1001", "l.1002", "l.1049", "l.1000", "l.1004", "l.1006",
    ];
    assert_eq!(order, expected);
    assert_eq!(grid.uploaded(), 50);
    let paths = fx.paths();
    let ranking = paths.iter().filter(|p| !p.starts_with("/art/")).count();
    assert_eq!(ranking, 9, "{paths:?}");
    let replay = "/v1/me/music-summaries?filter%5Byear%5D=latest&views=top-albums,top-songs";
    let songs = "/v1/catalog/zz/songs?include=albums&ids=7,8";
    let shelves = "/v1/catalog/zz/albums?include=library&ids=c1005,c1003,c1009,c1002";
    let replay_first = "/v1/catalog/zz/albums?include=library&ids=c1005";
    assert!(
        [replay, songs, shelves, replay_first]
            .iter()
            .all(|p| paths.contains(&p.to_string()))
    );
    assert!(paths.iter().all(|p| !p.contains("/us/")), "{paths:?}");
}

#[test]
fn an_empty_library_says_so_and_neither_retries_nor_shows_the_chart() {
    let (mut grid, fx, ctx) = load_covers(Serve { albums: 0, ..SERVE }, true);
    assert!(grid.empty());
    assert!(!charts_after(&fx, &mut grid, &ctx, (3600, true)).0);
    assert_eq!(
        (grid.uploaded(), grid.failed(), grid.failures()),
        (0, None, 0)
    );
    assert!(
        !fx.paths()
            .iter()
            .any(|p| p.contains("charts") || p.starts_with("/art/"))
    );
}

#[test]
fn clicking_a_home_cover_opens_that_albums_detail() {
    let (grid, _fx, ctx) = load_covers(SERVE, true);
    ctx.enable_accesskit();
    let mut navs = Vec::new();
    let mut frame = |events| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| grid.show(ui, &mut navs));
        out.textures_delta.clear();
        out.platform_output.accesskit_update
    };
    let nodes = frame(Vec::new()).expect("an accesskit tree").nodes;
    let third = nodes
        .iter()
        .find(|(_, n)| n.label() == Some("Open album 3"));
    let at = third
        .and_then(|(_, n)| n.bounds())
        .expect("a labelled cover");
    let at = egui::pos2(
        ((at.x0 + at.x1) / 2.0) as f32,
        ((at.y0 + at.y1) / 2.0) as f32,
    );
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(vec![egui::Event::PointerMoved(at), button(true)]);
    frame(vec![button(false)]);
    let opened = crate::ui::nav::Nav::Go(crate::ui::nav::Route::Album("l.1002".into()));
    assert_eq!((navs, grid.album(2)), (vec![opened], Some("l.1002")));
}

#[test]
fn covers_need_a_storefront_200_and_a_refusal_reads_as_sign_in() {
    for (me, rejected, developer, error) in [
        (401, false, true, None),
        (403, true, false, None),
        (0, false, false, Some("sign-in check")),
    ] {
        let (grid, fx, _) = load_covers(Serve { me, ..SERVE }, true);
        let seen = (
            grid.rejected(),
            grid.rejected_developer_token().is_some(),
            grid.uploaded(),
        );
        assert_eq!(seen, (rejected, developer, 0), "{me}");
        assert_eq!(
            grid.failed().map(|e| e.split(':').next().unwrap()),
            error,
            "{me}"
        );
        let tries = match me {
            0 => 2,
            403 => 3,
            _ => 1,
        };
        assert_eq!(fx.paths(), vec!["/v1/me/storefront"; tries], "{me}");
    }
}

#[test]
fn a_storefront_403_that_clears_on_retry_is_not_a_sign_out() {
    let (grid, fx, _) = load_covers(Serve { blips: 1, ..SERVE }, true);
    assert_eq!((grid.rejected(), grid.uploaded()), (false, 50));
    assert_eq!(fx.storefront_requests(), 2);
}

#[test]
fn a_storefront_403_that_persists_signs_out_within_the_deadline() {
    let started = Instant::now();
    let (grid, _fx, _) = load_covers(Serve { me: 403, ..SERVE }, true);
    assert!(grid.rejected());
    assert!(started.elapsed() < bramley_engine::STOREFRONT_DEADLINE);
}

#[test]
fn a_storefront_403_is_asked_at_most_three_times() {
    let (_, fx, _) = load_covers(Serve { me: 403, ..SERVE }, true);
    assert_eq!(fx.storefront_requests(), 3);
}

#[test]
fn a_refused_chart_shows_apples_reason_without_the_token() {
    let (grid, _fx, _) = load_covers(
        Serve {
            chart: 500,
            ..SERVE
        },
        true,
    );
    let why = grid.failed().unwrap();
    assert!(why.contains("HTTP 500: rejected"), "{why}");
    assert!(!why.contains(FIXTURE_TOKEN), "{why}");
    assert_eq!((grid.uploaded(), grid.failures()), (0, 50));
}

#[test]
fn bad_or_off_host_albums_are_skipped_without_their_urls() {
    let (grid, _fx, _) = load_covers(
        Serve {
            bad_albums: true,
            ..SERVE
        },
        true,
    );
    assert_eq!((grid.uploaded(), grid.failures()), (48, 2));
    let (off_host, fx, _) = load_covers(SERVE, false);
    assert_eq!((off_host.uploaded(), off_host.failures()), (0, 50));
    assert_eq!(fx.paths().len(), 6);
    let (mut refilled, _, ctx) = load_covers(
        Serve {
            bad_albums: true,
            albums: 52,
            ..SERVE
        },
        true,
    );
    let until = Instant::now() + Duration::from_secs(20);
    while refilled.uploaded() < 50 && Instant::now() < until {
        refilled.receive(&ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!((refilled.uploaded(), refilled.failures()), (50, 2));
    for why in [grid.failed().unwrap(), off_host.failed().unwrap()] {
        assert!(!why.contains("http") && !why.contains('{'), "{why}");
    }
}

#[test]
fn an_oversized_or_redirected_cover_fails_alone() {
    let (big, _fx, _) = load_covers(Serve { big: true, ..SERVE }, true);
    assert_eq!((big.uploaded(), big.failures()), (49, 1));
    assert!(big.failed().unwrap().starts_with("cover 7:"));
    let (moved, fx, _) = load_covers(
        Serve {
            redirect: true,
            ..SERVE
        },
        true,
    );
    assert_eq!((moved.uploaded(), moved.failures()), (49, 1));
    let why = moved.failed().unwrap();
    assert!(
        why.starts_with("cover 9: redirected (HTTP 302)") && !why.contains("http:"),
        "{why}"
    );
    assert!(
        !fx.paths().iter().any(|p| p.starts_with("/elsewhere")),
        "the redirect was followed"
    );
}

#[test]
fn losing_sign_in_mid_load_cancels_every_later_request() {
    let fx = fixture(Serve {
        slow_chart: true,
        ..SERVE
    });
    let (source, config) = fixture_source(&fx, true);
    let ctx = egui::Context::default();
    let mut grid = Grid::new(Some(source));
    grid.follow(&ctx, true);
    std::thread::sleep(Duration::from_millis(100));
    grid.follow(&ctx, false);
    std::thread::sleep(Duration::from_millis(800));
    std::fs::remove_dir_all(&config).unwrap();
    assert!(
        !fx.paths().iter().any(|p| p.starts_with("/art/")),
        "{:?}",
        fx.paths()
    );
}

fn charts(fx: &Fixture) -> usize {
    fx.paths()
        .iter()
        .filter(|p| p.starts_with("/v1/me/library/albums"))
        .count()
}

fn charts_after(
    fx: &Fixture,
    grid: &mut Grid,
    ctx: &egui::Context,
    (secs, visible): (u64, bool),
) -> (bool, usize) {
    let before = charts(fx);
    let started = grid.retry(ctx, Instant::now() + Duration::from_secs(secs), visible);
    let until = Instant::now() + Duration::from_secs(5);
    while started && charts(fx) == before && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    (started, charts(fx))
}

#[test]
fn a_failed_chart_is_retried_after_5_then_30_then_60_s_only_while_home_shows() {
    let fx = fixture(Serve {
        chart: 503,
        chart_fails: 4,
        ..SERVE
    });
    let (source, config) = fixture_source(&fx, true);
    let ctx = egui::Context::default();
    let mut grid = Grid::new(Some(source));
    grid.follow(&ctx, true);
    for (tries, wait) in [(1, 5), (2, 30), (3, 60), (4, 60)] {
        settle(&mut grid, &ctx);
        assert_eq!((grid.uploaded(), charts(&fx)), (0, tries));
        let early = charts_after(&fx, &mut grid, &ctx, (wait - 1, true));
        let hidden = charts_after(&fx, &mut grid, &ctx, (wait, false));
        let due = charts_after(&fx, &mut grid, &ctx, (wait, true));
        assert_eq!(
            [early, hidden, due],
            [(false, tries), (false, tries), (true, tries + 1)]
        );
    }
    settle(&mut grid, &ctx);
    assert_eq!((grid.uploaded(), grid.failed()), (50, None));
    assert_eq!(charts_after(&fx, &mut grid, &ctx, (3600, true)), (false, 5));
    std::fs::remove_dir_all(&config).unwrap();
}

#[test]
fn a_chart_retry_waits_out_retry_after_and_stops_on_a_403_or_sign_out() {
    let serve = |me, chart| Serve { me, chart, ..SERVE };
    let (mut limited, fx, ctx) = load_covers(serve(200, 429), true);
    let config = fixture_source(&fx, true).1;
    assert_eq!(
        charts_after(&fx, &mut limited, &ctx, (44, true)),
        (false, 1)
    );
    assert_eq!(charts_after(&fx, &mut limited, &ctx, (45, true)), (true, 2));
    std::fs::remove_dir_all(&config).unwrap();
    let mut asked = Vec::new();
    for (me, chart, sign_out) in [
        (200, 403, false),
        (403, 200, false),
        (200, 503, true),
        (200, 401, false),
    ] {
        let (mut grid, fx, ctx) = load_covers(serve(me, chart), true);
        let config = fixture_source(&fx, true).1;
        if sign_out {
            grid.follow(&ctx, false);
        }
        let (started, _) = charts_after(&fx, &mut grid, &ctx, (3600, true));
        std::fs::remove_dir_all(&config).unwrap();
        asked.push((started, fx.paths().len()));
    }
    assert_eq!(asked, [(false, 6), (false, 3), (false, 6), (false, 6)]);
}

#[test]
fn a_zoom_change_refetches_covers_at_the_new_pixel_size() {
    let (mut grid, fx, ctx) = load_covers(SERVE, true);
    let config = fixture_source(&fx, true).1;
    ctx.set_zoom_factor(2.0);
    ctx.run_ui(Default::default(), |_| {})
        .textures_delta
        .clear();
    grid.follow(&ctx, true);
    settle(&mut grid, &ctx);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!(grid.uploaded(), 50);
    assert_eq!(
        fx.paths()
            .iter()
            .filter(|p| p.ends_with("/192x192bb.jpg"))
            .count(),
        50
    );
}

fn cover_meshes(ctx: &egui::Context, app: &mut crate::app::App) -> usize {
    let mut out = ctx.run_ui(Default::default(), |ui| {
        app.tick(ui.ctx());
        app.draw(ui);
    });
    out.textures_delta.clear();
    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
    let textured = |p: &egui::ClippedPrimitive| matches!(&p.primitive, egui::epaint::Primitive::Mesh(m) if m.texture_id != egui::TextureId::default());
    primitives.iter().filter(|p| textured(p)).count()
}

fn app_on(
    serve: Serve,
) -> (
    crate::app::App,
    Ends,
    Sender<crate::cli::Envelope>,
    egui::Context,
    std::path::PathBuf,
    Fixture,
) {
    let fx = fixture(serve);
    let (source, config) = fixture_source(&fx, true);
    let ends: Ends = Rc::default();
    let spawned = ends.clone();
    let (inbox_tx, inbox) = channel();
    let ctx = egui::Context::default();
    let spawn = spawning(move || live(&spawned));
    let app = crate::app::App::new(
        &ctx,
        crate::Args::default(),
        spawn,
        (source.user.clone(), false),
        (inbox, true),
        Some(source),
    );
    (app, ends, inbox_tx, ctx, config, fx)
}

#[test]
fn a_sign_in_needed_snapshot_hides_the_covers_in_the_frame_it_arrives() {
    let (mut app, ends, inbox, ctx, config, _) = app_on(SERVE);
    let until = Instant::now() + Duration::from_secs(20);
    while app.status_json()["texturesUploaded"] != 50 && Instant::now() < until {
        cover_meshes(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::remove_dir_all(&config).unwrap();
    assert!(cover_meshes(&ctx, &mut app) >= 50);
    let (reply, _answer) = channel();
    let play = crate::cli::Request::Do(Action::PlaySong("1".into()));
    inbox
        .send((play, Instant::now() + Duration::from_secs(5), reply))
        .unwrap();
    cover_meshes(&ctx, &mut app);
    let refused = Some(PlayerError::SignInNeeded("expired".into()));
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 2, refused))
        .unwrap();
    assert_eq!(cover_meshes(&ctx, &mut app), 0);
    let status = app.status_json();
    assert_eq!(
        (
            status["banner"].as_str(),
            status["texturesUploaded"].as_u64()
        ),
        (Some("SignInNeeded"), Some(0))
    );
}

#[test]
fn a_storefront_403_asks_for_sign_in_and_a_401_blames_the_developer_token() {
    let developer = format!("Error({:?})", covers::DEVELOPER_REJECTED);
    let cases = [
        (403, "SignInNeeded".to_owned(), Some("covers-403")),
        (401, developer, None),
    ];
    for (me, banner, reason) in cases {
        let (mut app, _ends, _inbox, ctx, config, _) = app_on(Serve { me, ..SERVE });
        let until = Instant::now() + Duration::from_secs(10);
        while app.status_json()["banner"].is_null() && Instant::now() < until {
            cover_meshes(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = app.status_json();
        std::fs::remove_dir_all(&config).unwrap();
        let seen = (
            status["banner"].as_str(),
            status["texturesUploaded"].as_u64(),
        );
        assert_eq!(seen, (Some(banner.as_str()), Some(0)), "{me}");
        assert_eq!(status["signInReason"].as_str(), reason, "{me}");
        assert_eq!(cover_meshes(&ctx, &mut app), 0, "{me}");
    }
}

#[test]
fn a_storefront_403_that_clears_on_retry_leaves_the_user_signed_in() {
    let (mut app, _ends, _inbox, ctx, config, _) = app_on(Serve { blips: 1, ..SERVE });
    let until = Instant::now() + Duration::from_secs(20);
    while app.status_json()["texturesUploaded"] != 50 && Instant::now() < until {
        cover_meshes(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::remove_dir_all(&config).unwrap();
    let status = app.status_json();
    assert_eq!(status["texturesUploaded"], 50);
    assert!(status["signInReason"].is_null(), "{status}");
    assert!(status["banner"].is_null(), "{status}");
}

fn rejected_then(error: PlayerError) -> (crate::app::App, egui::Context, std::path::PathBuf) {
    let (mut app, ends, inbox, ctx, config, _) = app_on(Serve { me: 401, ..SERVE });
    let until = Instant::now() + Duration::from_secs(10);
    while app.status_json()["banner"].is_null() && Instant::now() < until {
        cover_meshes(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    let play = crate::cli::Request::Do(Action::PlaySong("1".into()));
    let deadline = Instant::now() + Duration::from_secs(5);
    inbox.send((play, deadline, channel().0)).unwrap();
    cover_meshes(&ctx, &mut app);
    ends.borrow()[0]
        .1
        .send(snap(PlayState::Stopped, 0.0, 2, Some(error)))
        .unwrap();
    cover_meshes(&ctx, &mut app);
    assert_eq!(ends.borrow().len(), 1);
    (app, ctx, config)
}

fn offers_sign_in(ctx: &egui::Context, app: &mut crate::app::App) -> bool {
    ctx.enable_accesskit();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.draw(ui));
    out.textures_delta.clear();
    let tree = out.platform_output.accesskit_update.unwrap();
    tree.nodes.iter().any(|(_, n)| n.label() == Some("Sign in"))
}

#[test]
fn status_names_why_the_engine_asked_for_sign_in_or_failed_to_load() {
    let why = "the Apple Music storefront check failed (503)";
    let cases = [
        (
            PlayerError::SignInNeeded("engine-auth-403".into()),
            json!(["engine-auth-403", null]),
        ),
        (PlayerError::LoadFailed(why.into()), json!([null, why])),
    ];
    for (error, expected) in cases {
        let (app, _ctx, config) = rejected_then(error);
        let status = app.status_json();
        std::fs::remove_dir_all(&config).unwrap();
        assert_eq!(
            json!([status["signInReason"], status["loadFailReason"]]),
            expected
        );
    }
}

#[test]
fn a_rejected_developer_token_outranks_sign_in_needed_and_still_offers_sign_in() {
    let refused = PlayerError::SignInNeeded("configure".into());
    let (mut app, ctx, config) = rejected_then(refused);
    let developer = format!("Error({:?})", covers::DEVELOPER_REJECTED);
    let shown = app.status_json()["banner"].clone();
    let offered = offers_sign_in(&ctx, &mut app);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!((shown.as_str(), offered), (Some(developer.as_str()), true));
}

#[test]
fn a_renewed_developer_token_clears_the_rejected_banner_on_either_renewal_path() {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 86_400 * 90;
    let renewals: [(&str, String); 2] = [
        ("developer-token", "renewed-token\n".into()),
        (
            "developer-token.json",
            json!({"token": "renewed-token", "exp": exp}).to_string(),
        ),
    ];
    for (file, text) in renewals {
        let (mut app, ctx, config) = rejected_then(PlayerError::SignInNeeded("401".into()));
        std::fs::write(config.join(file), text).unwrap();
        cover_meshes(&ctx, &mut app);
        let shown = app.status_json()["banner"].clone();
        let offered = offers_sign_in(&ctx, &mut app);
        std::fs::remove_dir_all(&config).unwrap();
        assert_eq!(
            (shown.as_str(), offered),
            (Some("SignInNeeded"), true),
            "{file}"
        );
    }
}

#[test]
fn a_playback_error_other_than_sign_in_outranks_the_developer_banner() {
    let output = PlayerError::Playback("the audio output could not be opened".into());
    let (app, _ctx, config) = rejected_then(output);
    let shown = app.status_json()["banner"].clone();
    std::fs::remove_dir_all(&config).unwrap();
    let want = "Error(\"the audio output could not be opened\")";
    assert_eq!(shown.as_str(), Some(want));
}

#[test]
fn ureq_wire_dumps_never_reach_the_log_at_any_level() {
    let line = |level, target| {
        let record = log::Record::builder()
            .level(level)
            .target(target)
            .args(format_args!("2042 6561 7265 7220 6579 4a68  .Bearer.eyJh"))
            .build();
        crate::withhold_wire_dumps(&record, "2042 6561  .Bearer.eyJh").is_some()
    };
    let query = log::Record::builder()
        .level(log::Level::Debug)
        .target("ureq::run")
        .args(format_args!(
            "GET https://api.music.apple.com/v1/catalog/us/search?term=queen"
        ))
        .build();
    let shown = crate::withhold_wire_dumps(&query, "GET …/search?term=queen");
    assert!(shown.is_some_and(|s| !s.contains("queen")));
    assert!(line(log::Level::Trace, "ureq_proto::util"));
    assert!(line(log::Level::Trace, "ureq_proto::client"));
    assert!(line(log::Level::Debug, "ureq_proto::client"));
    assert!(line(log::Level::Debug, "ureq::run"));
    assert!(!line(log::Level::Warn, "ureq::run"));
    assert!(!line(log::Level::Info, "ureq_proto::client"));
    assert!(!line(log::Level::Error, "ureq"));
    assert!(line(log::Level::Trace, "ureq::run"));
    assert!(line(log::Level::Trace, "ureq"));
    assert!(!line(log::Level::Trace, "ureqx"));
    assert!(!line(log::Level::Trace, "zbus::connection"));
}

#[test]
fn main_installs_a_logger_that_withholds_ureq_wire_dumps() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/tmp/log-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("app.log");
    crate::logging()
        .file(&log)
        .filter("ureq=trace,ureq_proto=trace")
        .init()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = String::new();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        while reader.read_line(&mut head).unwrap() > 2 {}
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
    let secret = "user-token-for-the-log-test-0123456789";
    ureq::get(&format!("http://{addr}/"))
        .header("Music-User-Token", secret)
        .call()
        .unwrap();
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let hex: String = text
        .lines()
        .filter_map(|l| l.split("ureq_proto::util] ").nth(1))
        .flat_map(|l| l.split("  ").next().unwrap_or("").split(' '))
        .collect();
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
        .collect();
    assert!(text.contains("<HTTP bytes withheld"), "{text}");
    assert!(!text.contains(secret) && !String::from_utf8_lossy(&bytes).contains(secret));
}

const MARKED: &str = "X-Bramley-Sign-In: 1\r\n";

fn raw(addr: &str, request: &str) -> (u16, String) {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut stream, &mut reply).unwrap();
    let status = reply.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, reply)
}

fn split(url: &str) -> (String, String) {
    let (addr, home) = url.trim_start_matches("http://").split_once('/').unwrap();
    (addr.to_owned(), format!("/{home}"))
}

fn ask(
    url: &str,
    method: &str,
    path: &str,
    host: Option<&str>,
    headers: &str,
    body: &str,
) -> (u16, String) {
    let (addr, home) = split(url);
    let host = host.map_or(String::new(), |h| format!("Host: {h}\r\n"));
    let path = path.replace("{home}", &home);
    let length = body.len();
    raw(
        &addr,
        &format!(
            "{method} {path} HTTP/1.1\r\n{host}{headers}Content-Length: {length}\r\n\r\n{body}"
        ),
    )
}

fn kept_into(kept: &Arc<Mutex<Vec<String>>>) -> crate::signin::Keep {
    let seen = kept.clone();
    Box::new(move |token| {
        seen.lock().unwrap().push(token.to_owned());
        Ok(())
    })
}

fn header_value(reply: &str, name: &str) -> Option<String> {
    reply
        .split("\r\n\r\n")
        .next()?
        .lines()
        .find_map(|line| header(line, name))
}

#[test]
fn the_sign_in_page_needs_the_exact_host_and_the_nonce_and_carries_a_csp() {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let server =
        crate::signin::start("dev</script>\"x", crate::signin::LIFETIME, kept_into(&kept)).unwrap();
    let (host, home) = split(&server.url);
    assert_eq!(home.len(), 34);
    let (status, reply) = ask(&server.url, "GET", "{home}", Some(&host), "", "");
    assert_eq!(status, 200);
    let page = reply.split_once("\r\n\r\n").unwrap().1;
    assert!(page.contains(r#"window.BRAMLEY_DEV_TOKEN = "dev<\/script>\"x";"#));
    assert!(page.contains(r#"src="https://js-cdn.music.apple.com/musickit/v3/musickit.js""#));
    assert!(page.contains("if (r.ok) { localStorage.clear(); sessionStorage.clear();"));
    let csp = header_value(&reply, "content-security-policy").unwrap();
    let script = csp
        .split("'nonce-")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap();
    assert!(
        csp.starts_with("script-src 'self' 'nonce-")
            && csp.contains("https://js-cdn.music.apple.com; frame-ancestors 'none'"),
        "{csp}"
    );
    assert_eq!(
        page.matches(&format!("<script nonce=\"{script}\">"))
            .count(),
        2
    );
    assert!(!home.contains(script));
    assert_eq!(
        header_value(&reply, "x-content-type-options").as_deref(),
        Some("nosniff")
    );
    assert_eq!(
        header_value(&reply, "referrer-policy").as_deref(),
        Some("strict-origin-when-cross-origin")
    );
    let port = host.rsplit(':').next().unwrap();
    for bad in [
        Some(format!("localhost:{port}")),
        Some("127.0.0.1:1".into()),
        Some("127.0.0.1".into()),
        None,
    ] {
        assert_eq!(
            ask(&server.url, "GET", "{home}", bad.as_deref(), "", "").0,
            403,
            "{bad:?}"
        );
    }
    let wrong = format!("/{}/", "0".repeat(32));
    let bare = home.trim_end_matches('/');
    for path in [wrong.as_str(), "/", "/token", bare, &format!("{home}x")] {
        assert_eq!(
            ask(&server.url, "GET", path, Some(&host), "", "").0,
            404,
            "{path}"
        );
    }
    let twice = format!("Host: {host}\r\n");
    assert_eq!(
        ask(&server.url, "GET", "{home}", Some(&host), &twice, "").0,
        400
    );
}

#[test]
fn the_token_is_taken_once_with_a_storage_wipe_and_a_refused_post_does_not_use_it_up() {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let (seen, mut fail) = (kept.clone(), true);
    let keep: crate::signin::Keep = Box::new(move |token| {
        seen.lock().unwrap().push(token.to_owned());
        if std::mem::take(&mut fail) {
            Err("the keyring said no")
        } else {
            Ok(())
        }
    });
    let server = crate::signin::start("dev", crate::signin::LIFETIME, keep).unwrap();
    let host = split(&server.url).0;
    let post = |body: &str| {
        ask(
            &server.url,
            "POST",
            "{home}token",
            Some(&host),
            MARKED,
            body,
        )
    };
    assert_eq!(
        ask(
            &server.url,
            "POST",
            "{home}token",
            Some("evil.test"),
            MARKED,
            "t0k"
        )
        .0,
        403
    );
    assert_eq!([post("").0, post("two words").0], [400, 400]);
    let (status, refused) = post("first");
    assert_eq!(
        (status, refused.ends_with("the keyring said no")),
        (500, true)
    );
    assert_eq!(header_value(&refused, "clear-site-data"), None);
    assert!(server.done.try_recv().is_err());
    let (status, ok) = post(" mut-token \n");
    assert_eq!(
        (status, header_value(&ok, "clear-site-data").as_deref()),
        (200, Some("\"storage\""))
    );
    assert_eq!(
        [
            post("again").0,
            ask(&server.url, "GET", "{home}", Some(&host), "", "").0
        ],
        [410, 410]
    );
    assert_eq!(*kept.lock().unwrap(), ["first", "mut-token"]);
    assert!(server.done.recv_timeout(Duration::from_secs(1)).is_ok());
}

#[test]
fn a_cross_origin_text_plain_post_is_refused_and_preflights_get_405() {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let server = crate::signin::start("dev", crate::signin::LIFETIME, kept_into(&kept)).unwrap();
    let host = split(&server.url).0;
    let evil = "Origin: https://evil.example\r\nSec-Fetch-Site: cross-site\r\nContent-Type: text/plain;charset=UTF-8\r\n";
    let preflight = "Origin: https://evil.example\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: x-bramley-sign-in\r\n";
    let (status, reply) = ask(
        &server.url,
        "OPTIONS",
        "{home}token",
        Some(&host),
        preflight,
        "",
    );
    assert_eq!(
        (
            status,
            reply.to_ascii_lowercase().contains("access-control-allow")
        ),
        (405, false)
    );
    assert_eq!(
        ask(
            &server.url,
            "POST",
            "{home}token",
            Some(&host),
            evil,
            "attacker-token"
        )
        .0,
        403
    );
    assert!(kept.lock().unwrap().is_empty());
    assert_eq!(
        ask(
            &server.url,
            "POST",
            "{home}token",
            Some(&host),
            MARKED,
            "user-token"
        )
        .0,
        200
    );
    assert_eq!(*kept.lock().unwrap(), ["user-token"]);
}

#[test]
fn the_sign_in_server_stops_at_its_lifetime_even_for_an_open_connection_or_when_dropped() {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let short = crate::signin::start("dev", Duration::from_millis(300), kept_into(&kept)).unwrap();
    let (addr, home) = split(&short.url);
    let mut early = std::net::TcpStream::connect(&addr).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    write!(
        early,
        "POST {home}token HTTP/1.1\r\nHost: {addr}\r\n{MARKED}Content-Length: 4\r\n\r\nlate"
    )
    .unwrap();
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut early, &mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 410"), "{reply}");
    assert!(kept.lock().unwrap().is_empty());
    let gone = short.done.recv_timeout(Duration::from_secs(2));
    assert_eq!(gone, Err(std::sync::mpsc::RecvTimeoutError::Disconnected));
    assert!(std::net::TcpStream::connect(&addr).is_err());
    let dropped = crate::signin::start("dev", crate::signin::LIFETIME, kept_into(&kept)).unwrap();
    let host = split(&dropped.url).0;
    drop(dropped);
    std::thread::sleep(Duration::from_millis(300));
    assert!(std::net::TcpStream::connect(&host).is_err());
}

#[test]
fn a_dripping_client_cannot_hold_the_sign_in_server_past_its_budget() {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let server = crate::signin::start("dev", crate::signin::LIFETIME, kept_into(&kept)).unwrap();
    let (addr, home) = split(&server.url);
    let mut drip = std::net::TcpStream::connect(&addr).unwrap();
    let dripper = std::thread::spawn(move || {
        for byte in b"GET /xxxxxxxxxx" {
            if drip.write_all(&[*byte]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    });
    std::thread::sleep(Duration::from_millis(200));
    let start = Instant::now();
    let status = raw(
        &addr,
        &format!("GET {home} HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
    )
    .0;
    let waited = start.elapsed();
    dripper.join().unwrap();
    assert_eq!(status, 200);
    assert!(waited < Duration::from_millis(6500), "{waited:?}");
}

#[test]
fn the_browser_opens_an_owner_only_redirect_file_so_argv_never_holds_the_nonce() {
    let dir = token_dir("redirect-test");
    let url = "http://127.0.0.1:1/0123456789abcdef0123456789abcdef/";
    let page = crate::signin::redirect_file(&dir, url).unwrap();
    let body = std::fs::read_to_string(&page).unwrap();
    #[cfg(unix)]
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&page).unwrap().permissions())
            & 0o777;
    #[cfg(not(unix))]
    let mode = 0o600;
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!page.to_string_lossy().contains("0123456789abcdef"));
    assert_eq!(
        (body.contains(&format!("content=\"0;url={url}\"")), mode),
        (true, 0o600)
    );
}

fn token_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/tmp/{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn keyring_entry(store: &Arc<keyring_core::mock::Store>) -> keyring_core::Entry {
    keyring_core::api::CredentialStoreApi::build(&**store, "bramley-test", "music-user-token", None)
        .unwrap()
}

fn fail_next(store: &Arc<keyring_core::mock::Store>) {
    let entry = keyring_entry(store);
    let cred: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
    cred.set_error(keyring_core::Error::NoStorageAccess("locked".into()));
}

#[test]
fn the_keyring_is_the_one_copy_and_an_import_deletes_the_file_only_once_read_back() {
    let dir = token_dir("keyring-test");
    let store = keyring_core::mock::Store::new().unwrap();
    let user = Credentials::new(Some(store.clone()), "bramley-test", &dir);
    let file = dir.join("music-user-token");
    std::fs::write(&file, "old-file-token\n").unwrap();
    fail_next(&store);
    assert!(user.load().is_err());
    assert!(file.exists());
    assert_eq!(user.load(), Ok(Some("old-file-token".into())));
    assert_eq!(
        (keyring_entry(&store).get_password().unwrap(), file.exists()),
        ("old-file-token".into(), false)
    );
    std::fs::write(&file, "stale-file-token\n").unwrap();
    assert_eq!(user.save("fresh-token", || true), Ok(()));
    let restarted = Credentials::new(Some(store.clone()), "bramley-test", &dir).load();
    let left = file.exists();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        (left, restarted, user.storage()),
        (false, Ok(Some("fresh-token".into())), Some("keyring"))
    );
}

#[test]
fn keyring_errors_are_errors_and_never_fall_back_to_a_file() {
    let dir = token_dir("keyring-error-test");
    let store = keyring_core::mock::Store::new().unwrap();
    let user = Credentials::new(Some(store.clone()), "bramley-test", &dir);
    assert_eq!(user.save("old-token", || true), Ok(()));
    fail_next(&store);
    assert!(user.save("new-token", || true).is_err());
    let file = dir.join("music-user-token").exists();
    fail_next(&store);
    let locked = user.load();
    let later = user.load();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        (file, locked.is_err(), later),
        (false, true, Ok(Some("old-token".into())))
    );
}

#[test]
fn without_a_secret_service_the_token_goes_to_an_owner_only_file_written_atomically() {
    let dir = token_dir("no-keyring-test");
    let file = dir.join("music-user-token");
    let stale = dir.join("music-user-token.partial");
    std::fs::write(&stale, "stale\n").unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&stale, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    let user = Credentials::new(None, "bramley-test", &dir);
    assert_eq!(user.save("file-token", || true), Ok(()));
    let saved = std::fs::read_to_string(&file).unwrap();
    #[cfg(unix)]
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&file).unwrap().permissions())
            & 0o777;
    #[cfg(not(unix))]
    let mode = 0o600;
    let leftovers = std::fs::read_dir(&dir).unwrap().count();
    let loaded = user.load();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        (saved.as_str(), mode, leftovers),
        ("file-token\n", 0o600, 1)
    );
    assert_eq!(
        (loaded, user.storage()),
        (Ok(Some("file-token".into())), Some("file"))
    );
}

#[test]
fn sign_out_clears_the_keyring_and_a_late_save_is_discarded() {
    let dir = token_dir("sign-out-test");
    let store = keyring_core::mock::Store::new().unwrap();
    let user = Credentials::new(Some(store.clone()), "bramley-test", &dir);
    std::fs::write(dir.join("music-user-token"), "old-file-token\n").unwrap();
    assert_eq!(user.save("keyring-token", || true), Ok(()));
    assert_eq!(user.clear(), Ok(()));
    assert!(user.save("late-token", || false).is_err());
    let restarted = Credentials::new(Some(store), "bramley-test", &dir).load();
    let file = dir.join("music-user-token").exists();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!((restarted, file), (Ok(None), false));
}

fn ask_app(
    inbox: &Sender<crate::cli::Envelope>,
    ctx: &egui::Context,
    app: &mut crate::app::App,
    request: crate::cli::Request,
) -> serde_json::Value {
    let (reply, answer) = channel();
    inbox
        .send((request, Instant::now() + Duration::from_secs(5), reply))
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        cover_meshes(ctx, app);
        if let Ok(text) = answer.try_recv() {
            return serde_json::from_str(&text).unwrap();
        }
        assert!(Instant::now() < until, "no reply");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn post_token(url: &str, token: &str) -> u16 {
    let host = split(url).0;
    ask(url, "POST", "{home}token", Some(&host), MARKED, token).0
}

fn wait_for(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    done: &dyn Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let until = Instant::now() + Duration::from_secs(20);
    while !done(&app.status_json()) && Instant::now() < until {
        cover_meshes(ctx, app);
        std::thread::sleep(Duration::from_millis(10));
    }
    app.status_json()
}

fn frames(ctx: &egui::Context, app: &mut crate::app::App, n: usize) -> serde_json::Value {
    for _ in 0..n {
        cover_meshes(ctx, app);
        std::thread::sleep(Duration::from_millis(10));
    }
    app.status_json()
}

#[test]
fn sign_in_revalidates_with_the_new_token_and_sign_out_shuts_down_and_forgets() {
    let (mut app, ends, inbox, ctx, config, fx) = app_on(SERVE);
    wait_for(&ctx, &mut app, &|s| s["texturesUploaded"] == 50);
    let out = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignIn);
    let url = out["signInUrl"].as_str().unwrap().to_owned();
    assert_eq!(
        out["banner"].as_str(),
        Some(format!("SigningIn({url:?})").as_str())
    );
    let again = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignIn);
    assert_eq!(again["signInUrl"].as_str(), Some(url.as_str()));
    assert_eq!(post_token(&url, "new-token"), 200);
    let revalidated = || {
        let seen = fx.requests.lock().unwrap();
        seen.iter()
            .any(|r| r.0 == "/v1/me/storefront" && r.2.as_deref() == Some("new-token"))
    };
    let status = wait_for(&ctx, &mut app, &|s| {
        revalidated() && s["texturesUploaded"] == 50
    });
    assert_eq!(
        (
            status["banner"].as_str(),
            status["signInUrl"].as_str(),
            revalidated()
        ),
        (None, None, true)
    );
    ask_app(
        &inbox,
        &ctx,
        &mut app,
        crate::cli::Request::Do(Action::PlaySong("1".into())),
    );
    app.api = Some(channel().0);
    let out = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignOut);
    assert!(
        app.api.is_none(),
        "sign-out kept the API worker and its storefront cache"
    );
    let shut = ends.borrow()[0]
        .0
        .try_iter()
        .any(|c| c == Command::Shutdown);
    let seen = (
        out["banner"].as_str(),
        out["texturesUploaded"].as_u64(),
        out["error"].as_str(),
        shut,
    );
    assert_eq!(seen, (Some("SignInNeeded"), Some(0), None, true));
    let url = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignIn)["signInUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(post_token(&url, "back-token"), 200);
    let status = wait_for(&ctx, &mut app, &|s| s["texturesUploaded"] == 50);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!(
        (status["banner"].as_str(), status["tokenStorage"].as_str()),
        (None, Some("keyring"))
    );
    let play = ask_app(
        &inbox,
        &ctx,
        &mut app,
        crate::cli::Request::Do(Action::PlaySong("2".into())),
    );
    assert_eq!((play["refused"].as_str(), ends.borrow().len()), (None, 2));
}

#[test]
fn play_then_sign_out_in_one_frame_never_reaches_the_engine() {
    let (mut app, ends, inbox, ctx, config, _fx) = app_on(SERVE);
    wait_for(&ctx, &mut app, &|s| s["texturesUploaded"] == 50);
    let at = Instant::now() + Duration::from_secs(5);
    let ((played, _), (signed_out, answer)) = (channel(), channel());
    inbox
        .send((
            crate::cli::Request::Do(Action::PlaySong("1".into())),
            at,
            played,
        ))
        .unwrap();
    inbox
        .send((crate::cli::Request::SignOut, at, signed_out))
        .unwrap();
    let after = frames(&ctx, &mut app, 30);
    let out: serde_json::Value = serde_json::from_str(&answer.recv().unwrap()).unwrap();
    std::fs::remove_dir_all(&config).unwrap();
    let sent: Vec<Command> = ends
        .borrow()
        .iter()
        .flat_map(|e| e.0.try_iter().collect::<Vec<_>>())
        .collect();
    assert_eq!(
        (out["banner"].as_str(), after["engineStarted"].as_bool()),
        (Some("SignInNeeded"), Some(false))
    );
    assert!(
        !sent
            .iter()
            .any(|c| matches!(c, Command::Play | Command::SetQueue { .. })),
        "{sent:?}"
    );
}

#[test]
fn a_token_post_in_flight_across_sign_out_is_refused_and_never_stored() {
    let (mut app, _ends, inbox, ctx, config, _fx) = app_on(SERVE);
    wait_for(&ctx, &mut app, &|s| s["texturesUploaded"] == 50);
    let url = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignIn)["signInUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let (addr, home) = split(&url);
    let mut stream = std::net::TcpStream::connect(&addr).unwrap();
    write!(
        stream,
        "POST {home}token HTTP/1.1\r\nHost: {addr}\r\n{MARKED}Content-Length: 10\r\n\r\n"
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let out = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignOut);
    stream.write_all(b"late-token").unwrap();
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut stream, &mut reply).unwrap();
    let status = frames(&ctx, &mut app, 30);
    let stored = crate::boot(&mock_user(&config), |_| {
        unreachable!("a late token was stored")
    });
    std::fs::remove_dir_all(&config).unwrap();
    assert!(
        reply.starts_with("HTTP/1.1 500") && reply.contains("signed out while"),
        "{reply}"
    );
    assert_eq!(
        (out["banner"].as_str(), status["banner"].as_str()),
        (Some("SignInNeeded"), Some("SignInNeeded"))
    );
    assert!(stored.unwrap_err().is::<SignedOut>());
}

#[test]
fn until_the_startup_keyring_check_answers_play_is_refused_and_no_banner_flashes() {
    let (mut app, ends, inbox, ctx, config, _fx) = app_on(SERVE);
    app.check_sign_in();
    let play = ask_app(
        &inbox,
        &ctx,
        &mut app,
        crate::cli::Request::Do(Action::PlaySong("1".into())),
    );
    assert_eq!(
        (
            play["refused"].is_string(),
            play["banner"].as_str(),
            ends.borrow().len()
        ),
        (true, None, 0)
    );
    let status = wait_for(&ctx, &mut app, &|s| s["texturesUploaded"] == 50);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!(
        (status["banner"].as_str(), status["tokenStorage"].as_str()),
        (None, Some("keyring"))
    );
}

#[test]
fn the_startup_keyring_check_records_why_it_left_the_user_signed_out() {
    for (fail, reason) in [(false, "boot-no-token"), (true, "keyring-unavailable")] {
        let dir = token_dir(&format!("startup-check-{reason}"));
        let store = keyring_core::mock::Store::new().unwrap();
        let user = Credentials::new(Some(store.clone()), "bramley-test", &dir);
        if fail {
            fail_next(&store);
        }
        let ctx = egui::Context::default();
        let spawn = spawning(|| Err(anyhow!("no player in this test")));
        let args = crate::Args::default();
        let mut app =
            crate::app::App::new(&ctx, args, spawn, (user, true), (channel().1, true), None);
        app.check_sign_in();
        let until = Instant::now() + Duration::from_secs(5);
        while app.status_json()["signInReason"] == "boot-unchecked" && Instant::now() < until {
            app.tick(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
        let seen = app.status_json()["signInReason"].clone();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(seen, reason);
    }
}

#[test]
fn the_status_file_is_owner_only() {
    let dir = token_dir("status-test");
    let path = dir.join("status.json");
    let ctx = egui::Context::default();
    let flags = ["--status".to_owned(), path.display().to_string()];
    let args = crate::parse_args(flags.into_iter()).unwrap();
    let user = (mock_user(&dir), true);
    let mut app = crate::app::App::new(
        &ctx,
        args,
        spawning(|| unreachable!()),
        user,
        (channel().1, true),
        None,
    );
    cover_meshes(&ctx, &mut app);
    drop(app);
    #[cfg(unix)]
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions())
            & 0o777;
    #[cfg(not(unix))]
    let mode = 0o600;
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(mode, 0o600);
}

fn signed_in_app(
    dir: &std::path::Path,
    ends: &Ends,
) -> (
    crate::app::App,
    Sender<crate::cli::Envelope>,
    Receiver<bramley_api::Job>,
) {
    let (inbox_tx, inbox) = channel();
    let flags = [
        "--status".to_owned(),
        dir.join("status.json").display().to_string(),
    ];
    let args = crate::parse_args(flags.into_iter()).unwrap();
    let spawned = ends.clone();
    let spawn = spawning(move || live(&spawned));
    let mut app = crate::app::App::new(
        &egui::Context::default(),
        args,
        spawn,
        (mock_user(dir), false),
        (inbox, true),
        None,
    );
    let (jobs, queued) = channel();
    app.api = Some(jobs);
    (app, inbox_tx, queued)
}

#[test]
fn the_status_file_never_holds_the_title_or_artist_but_now_still_answers_with_them() {
    let dir = token_dir("status-names");
    let (ctx, ends) = (egui::Context::default(), Ends::default());
    let (mut app, inbox, _jobs) = signed_in_app(&dir, &ends);
    let deadline = Instant::now() + Duration::from_secs(5);
    let play = crate::cli::Request::Do(Action::PlaySong("1".into()));
    inbox.send((play, deadline, channel().0)).unwrap();
    cover_meshes(&ctx, &mut app);
    cover_meshes(&ctx, &mut app);
    let mut playing = snap(PlayState::Playing, 3.0, 2, None);
    (playing.title, playing.artist) = (Some("Secret Title".into()), Some("Secret Artist".into()));
    playing.queue = vec![bramley_engine::QueueItem {
        id: "i.7".into(),
        title: Some("Secret Queued".into()),
        duration: Some(200.0),
    }];
    (playing.queue_position, playing.queue_length) = (Some(0), 1);
    ends.borrow()[0].1.send(playing).unwrap();
    let (reply, answered) = channel();
    inbox
        .send((crate::cli::Request::Now, deadline, reply))
        .unwrap();
    cover_meshes(&ctx, &mut app);
    let now: serde_json::Value = serde_json::from_str(&answered.try_recv().unwrap()).unwrap();
    drop(app);
    let written = std::fs::read_to_string(dir.join("status.json")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        (now["title"].as_str(), now["artist"].as_str()),
        (Some("Secret Title"), Some("Secret Artist"))
    );
    assert!(
        written.contains("\"Playing\"") && !written.contains("Secret"),
        "{written}"
    );
    assert!(
        !written.contains("i.7")
            && !written.contains("\"queue\"")
            && !written.contains("queueLength"),
        "{written}"
    );
    assert_eq!(now["queue"], json!([{"id": "i.7", "duration": 200.0}]));
    assert_eq!(
        (now["queuePosition"].clone(), now["queueLength"].clone()),
        (json!(0), json!(1))
    );
}

#[test]
fn a_library_403_shows_sign_in_needed_like_the_covers_do() {
    let dir = token_dir("library-403");
    let (ctx, ends) = (egui::Context::default(), Ends::default());
    let (mut app, _inbox, jobs) = signed_in_app(&dir, &ends);
    cover_meshes(&ctx, &mut app);
    cover_meshes(&ctx, &mut app);
    let job = jobs.try_recv().expect("the sidebar asks for playlists");
    assert_eq!(
        job.call,
        bramley_api::Call::Library(bramley_api::Shelf::Playlists)
    );
    (job.done)(Err(bramley_api::Error::SignInNeeded));
    for _ in 0..2 {
        cover_meshes(&ctx, &mut app);
        std::thread::sleep(Duration::from_secs(1));
        cover_meshes(&ctx, &mut app);
        let job = jobs.try_recv().expect("a first 403 is asked again");
        (job.done)(Err(bramley_api::Error::SignInNeeded));
    }
    cover_meshes(&ctx, &mut app);
    cover_meshes(&ctx, &mut app);
    std::fs::remove_dir_all(&dir).unwrap();
    let status = app.status_json();
    assert_eq!(status["banner"], "SignInNeeded");
    assert_eq!(status["signInReason"], "library-denied");
    assert!(jobs.try_recv().is_err(), "a 403 is not retried");
}

fn searched(answer: bramley_api::Result<bramley_api::Reply>) -> String {
    let (inbox_tx, inbox) = channel();
    let ctx = egui::Context::default();
    let spawn = spawning(|| Err(anyhow!("no player in this test")));
    let args = crate::Args::default();
    let user = (
        mock_user(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/no-config"),
        ),
        false,
    );
    let mut app = crate::app::App::new(&ctx, args, spawn, user, (inbox, true), None);
    let (jobs, queued) = channel();
    app.api = Some(jobs);
    let (reply, answered) = channel();
    let search = crate::cli::Request::Search("bohemian rhapsody".into());
    let deadline = Instant::now() + Duration::from_secs(5);
    inbox_tx.send((search, deadline, reply)).unwrap();
    app.pump(&ctx);
    let job: bramley_api::Job = queued.try_recv().unwrap();
    let asked = bramley_api::Call::Search("bohemian rhapsody".into());
    assert_eq!((job.call, job.deadline), (asked, deadline));
    assert!(
        answered.try_recv().is_err(),
        "the UI answered before the worker"
    );
    (job.done)(answer);
    answered.recv_timeout(Duration::from_secs(1)).unwrap()
}

#[test]
fn cli_search_is_handed_to_the_api_worker_and_its_results_relayed() {
    let songs = json!({"songs": {"data": [{"id": "6781024437", "type": "songs"}]}});
    let results = serde_json::from_value(songs).unwrap();
    let reply = searched(Ok(bramley_api::Reply::Search(results)));
    let parsed: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(parsed["results"]["songs"]["items"][0]["id"], "6781024437");
    assert_eq!(crate::cli::outcome(&reply), Ok(()));
}

#[test]
fn cli_open_moves_the_window_like_a_click_and_scroll_is_refused_outside_a_list() {
    use crate::cli::Request;
    let (inbox_tx, inbox) = channel();
    let ctx = egui::Context::default();
    let spawn = spawning(|| Err(anyhow!("no player in this test")));
    let config =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/no-config");
    let user = (mock_user(&config), false);
    let mut app = crate::app::App::new(
        &ctx,
        crate::Args::default(),
        spawn,
        user,
        (inbox, true),
        None,
    );
    let mut ask = |request, late: bool| {
        let (reply, answered) = channel();
        let deadline = match late {
            true => Instant::now() - Duration::from_millis(1),
            false => Instant::now() + Duration::from_secs(5),
        };
        inbox_tx.send((request, deadline, reply)).unwrap();
        app.pump(&ctx);
        let reply: serde_json::Value = serde_json::from_str(&answered.recv().unwrap()).unwrap();
        let error = reply["error"]
            .as_str()
            .map(|e| e.split(' ').next().unwrap().to_owned());
        (reply["route"].clone(), error)
    };
    let refused = ask(Request::Nav(Nav::Scroll(Edge::End)), false);
    let late = ask(Request::Nav(Nav::Go(Route::Songs)), true);
    let opened = ask(Request::Nav(Nav::Go(Route::Playlist("p.1".into()))), false);
    let scrolled = ask(Request::Nav(Nav::Scroll(Edge::End)), false);
    let words = |w: &str| Some(w.to_owned());
    assert_eq!(
        (refused, late),
        (
            (json!("Home"), words("scroll")),
            (json!("Home"), words("the"))
        )
    );
    assert_eq!(
        (opened, scrolled),
        ((json!("Playlist"), None), (json!("Playlist"), None))
    );
    let pinned = app.scroll();
    app.check_sign_in();
    app.tick(&ctx);
    assert_eq!(
        (pinned, app.scroll()),
        (Some(Edge::End), None),
        "sign-out drops the pin"
    );
}

#[test]
fn the_app_retries_a_failed_home_chart_only_while_home_is_the_route() {
    let fx = fixture(Serve {
        chart: 503,
        ..SERVE
    });
    let (source, config) = fixture_source(&fx, true);
    let ctx = egui::Context::default();
    let spawn = spawning(|| Err(anyhow!("no player in this test")));
    let user = (mock_user(&config), false);
    let inbox = channel().1;
    let args = crate::Args::default();
    let mut app = crate::app::App::new(&ctx, args, spawn, user, (inbox, true), Some(source));
    app.go(Route::Search);
    let ticked = |app: &mut crate::app::App, for_ms: u64| {
        let until = Instant::now() + Duration::from_millis(for_ms);
        while Instant::now() < until {
            app.tick(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        charts(&fx)
    };
    let on_search = ticked(&mut app, 6500);
    app.go(Route::Home);
    let until = Instant::now() + Duration::from_millis(5000);
    while Instant::now() < until && charts(&fx) < 2 {
        app.tick(&ctx);
        std::thread::sleep(Duration::from_millis(20));
    }
    let on_home = ticked(&mut app, 500);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!((on_search, on_home), (1, 2));
}

#[test]
fn cli_library_playlists_runs_on_the_api_worker_and_relays_the_page() {
    let request = crate::cli::Request::Library(bramley_api::Shelf::Playlists);
    let call = bramley_api::Call::Library(bramley_api::Shelf::Playlists);
    assert_eq!(crate::cli::api_call(&request), Some(call));
    let page = json!({"data": [{"id": "p.1", "type": "library-playlists"}], "meta": {"total": 1}});
    let page = bramley_api::Reply::Page(serde_json::from_value(page).unwrap());
    let reply = crate::cli::api_reply(Ok(page));
    let parsed: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(
        (
            parsed["results"]["total"].as_u64(),
            parsed["results"]["items"][0]["id"].as_str()
        ),
        (Some(1), Some("p.1"))
    );
    assert_eq!(crate::cli::outcome(&reply), Ok(()));
}

fn click(ctx: &egui::Context, app: &mut crate::app::App, label: &str) {
    ctx.enable_accesskit();
    let mut frame = |events| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.draw(ui));
        output.textures_delta.clear();
        output.platform_output.accesskit_update
    };
    let tree = frame(Vec::new()).unwrap();
    let button = tree.nodes.iter().find(|(_, n)| n.label() == Some(label));
    let bounds = button
        .and_then(|(_, n)| n.bounds())
        .expect("no such button");
    let at = egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(vec![egui::Event::PointerMoved(at), press(true)]);
    frame(vec![press(false)]);
}

#[test]
fn a_search_past_its_deadline_is_not_applied_and_leaves_the_next_one_alone() {
    use crate::cli::Request;
    let ctx = egui::Context::default();
    let spawn = spawning(|| Err(anyhow!("no player in this test")));
    let (inbox_tx, inbox) = channel();
    let config =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/no-config");
    let user = (mock_user(&config), false);
    let args = crate::Args::default();
    let mut app = crate::app::App::new(&ctx, args, spawn, user, (inbox, true), None);
    let mut search = |agent: bool, late: bool| {
        let deadline = match late {
            true => Instant::now() - Duration::from_millis(1),
            false => Instant::now() + Duration::from_secs(5),
        };
        if agent {
            inbox_tx
                .send((Request::Agent, deadline, channel().0))
                .unwrap();
        }
        let (reply, answered) = channel();
        let request = Request::Search("queen".into());
        inbox_tx.send((request, deadline, reply)).unwrap();
        app.pump(&ctx);
        let answer = answered.recv_timeout(Duration::from_secs(5));
        let refused = answer.map(|r| r.contains(crate::cli::LATE));
        (refused.ok(), app.status_json()["route"].clone())
    };
    let by_cli = search(false, true);
    let by_agent = search(true, true);
    let applied = search(false, false);
    assert_eq!(
        (by_cli, by_agent, applied),
        (
            (Some(true), json!("Home")),
            (Some(true), json!("Home")),
            (Some(false), json!("Search"))
        )
    );
}

#[test]
fn an_agent_request_raises_the_app_notice_and_only_dismissal_records_it() {
    use crate::cli::Request;
    let line = crate::cli::agent_wire(&words("now"));
    let (inbox_tx, inbox) = channel();
    let mut handle = crate::cli::handler(inbox_tx, Arc::new(std::sync::OnceLock::new()));
    let asked = std::thread::spawn(move || handle(&line));
    let (first, _, _) = inbox.recv_timeout(Duration::from_secs(2)).unwrap();
    let (second, _, reply) = inbox.recv_timeout(Duration::from_secs(2)).unwrap();
    reply.send("{}".into()).unwrap();
    asked.join().unwrap();
    assert_eq!((first, second), (Request::Agent, Request::Now));
    let config = token_dir("agent-notice");
    let ctx = egui::Context::default();
    let spawn = spawning(|| Err(anyhow!("no player in this test")));
    let (inbox_tx, inbox) = channel();
    let user = (mock_user(&config), false);
    let args = crate::Args::default();
    let mut app = crate::app::App::new(&ctx, args, spawn, user, (inbox, true), None);
    let shown = |app: &crate::app::App| app.status_json()["agentNotice"] == true;
    let deadline = Instant::now() + Duration::from_secs(5);
    assert!(!shown(&app));
    inbox_tx
        .send((Request::Agent, deadline, channel().0))
        .unwrap();
    app.pump(&ctx);
    let marker = config.join(crate::mcp::DISCLOSED);
    let (raised, unrecorded) = (shown(&app), !marker.exists());
    click(&ctx, &mut app, "Got it");
    let (cleared, recorded) = (!shown(&app), marker.exists());
    std::fs::remove_file(&marker).unwrap();
    let library = |reply| {
        (
            Request::Library(bramley_api::Shelf::Playlists),
            deadline,
            reply,
        )
    };
    let (answered, settled) = channel();
    inbox_tx.send(library(answered)).unwrap();
    app.pump(&ctx);
    let by_library = shown(&app);
    settled.recv_timeout(Duration::from_secs(5)).unwrap();
    click(&ctx, &mut app, "Got it");
    inbox_tx
        .send((Request::Agent, deadline, channel().0))
        .unwrap();
    inbox_tx.send(library(channel().0)).unwrap();
    app.pump(&ctx);
    let again = shown(&app);
    std::fs::remove_dir_all(&config).unwrap();
    assert!(raised && unrecorded && cleared && recorded && by_library && !again);
}

#[test]
fn a_rejected_developer_token_fails_cli_search() {
    let reply = searched(Err(bramley_api::Error::DeveloperToken));
    let why = covers::DEVELOPER_REJECTED.to_owned();
    assert_eq!(crate::cli::outcome(&reply), Err((1, why)));
}

#[test]
fn cli_search_takes_the_user_token_from_the_keyring_and_refuses_without_one() {
    let dir = token_dir("api-tokens-test");
    std::fs::write(dir.join("developer-token"), "dev-token-for-search\n").unwrap();
    let user = mock_user(&dir);
    assert_eq!(user.save("keyring-only-token", || true), Ok(()));
    let tokens = crate::api_tokens(&user).unwrap();
    let file = dir.join("music-user-token").exists();
    assert_eq!(
        (tokens.developer.as_str(), tokens.user.as_deref(), file),
        ("dev-token-for-search", Some("keyring-only-token"), false)
    );
    assert_eq!(user.clear(), Ok(()));
    let signed_out = user.clone();
    let client = bramley_api::Client::new(move || crate::api_tokens(&signed_out));
    let reply = crate::cli::api_reply(client.run(bramley_api::Call::Search("queen".into())));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        crate::cli::outcome(&reply),
        Err((crate::cli::REFUSED, "sign in needed".into()))
    );
}

const DAY: i64 = 86_400;

fn published(days: i64, token: &str, pad: usize) -> Vec<u8> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let now = now.unwrap().as_secs() as i64;
    let pad = "p".repeat(pad);
    let body = json!({"token": token, "iat": now, "exp": now + days * DAY, "pad": pad});
    body.to_string().into_bytes()
}

fn token_server_one(status: u16, extra: String, body: Vec<u8>) -> (String, Arc<Mutex<usize>>) {
    token_server(vec![(status, extra, body)])
}

fn token_server(mut replies: Vec<(u16, String, Vec<u8>)>) -> (String, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/bramley/developer-token.json",
        listener.local_addr().unwrap()
    );
    let hits = Arc::new(Mutex::new(0));
    let counted = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            *counted.lock().unwrap() += 1;
            let (status, extra, body) = match replies.len() {
                1 => replies[0].clone(),
                _ => replies.remove(0),
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\n{extra}Content-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(&body));
        }
    });
    (url, hits)
}

fn dev_token(
    url: &str,
    cached: Option<Vec<u8>>,
    config: bool,
) -> (DeveloperToken, std::path::PathBuf) {
    let dir = token_dir(&format!(
        "devtoken-{}",
        url.rsplit(':').next().unwrap().replace('/', "-")
    ));
    if let Some(body) = cached {
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        std::fs::write(dir.join("cache/developer-token.json"), body).unwrap();
    }
    if config {
        std::fs::write(dir.join("developer-token"), "config-dir-token\n").unwrap();
    }
    (
        DeveloperToken::new(Some(url.into()), &dir.join("cache"), &dir),
        dir,
    )
}

fn cache_of(dir: &std::path::Path) -> Vec<u8> {
    std::fs::read(dir.join("cache/developer-token.json")).unwrap_or_default()
}

#[test]
fn a_cached_token_with_30_or_more_days_left_is_used_without_a_request() {
    let (url, hits) = token_server_one(200, String::new(), published(180, "fetched", 0));
    let (token, dir) = dev_token(&url, Some(published(100, "cached", 0)), false);
    let got = (token.get(), token.get());
    let status = token.status();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(got, (Ok("cached".into()), Ok("cached".into())));
    assert_eq!(
        (*hits.lock().unwrap(), &status["devTokenSource"]),
        (0, &json!("cache"))
    );
}

#[test]
fn under_30_days_one_fetch_replaces_the_cache_owner_only() {
    let (url, hits) = token_server_one(200, String::new(), published(180, "fetched", 0));
    let (token, dir) = dev_token(&url, Some(published(20, "cached", 0)), false);
    let got = (token.get(), token.get());
    let cache = String::from_utf8(cache_of(&dir)).unwrap();
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(dir.join("cache/developer-token.json"))
            .unwrap()
            .permissions(),
    ) & 0o777;
    #[cfg(not(unix))]
    let mode = 0o600;
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(got, (Ok("fetched".into()), Ok("fetched".into())));
    assert_eq!(
        (*hits.lock().unwrap(), cache.contains("\"fetched\""), mode),
        (1, true, 0o600)
    );
    assert_eq!(token.warning(), None);
}

#[test]
fn a_failed_renewal_uses_the_cache_and_warns_with_the_days_left() {
    for (days, status, served) in [(20, 500, 180), (10, 500, 180), (100, 200, 10)] {
        let (url, _) = token_server_one(status, String::new(), published(served, "fetched", 0));
        let cached = (days < 30).then(|| published(days, "cached", 0));
        let (token, dir) = dev_token(&url, cached, false);
        let got = token.get().unwrap();
        let left = token.status()["devTokenDaysLeft"].as_i64().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let expect = if status == 200 { served } else { days };
        assert!((expect - 1..=expect).contains(&left), "{days} {left}");
        assert_eq!(
            got,
            if status == 200 { "fetched" } else { "cached" },
            "{days}"
        );
        assert!(token.warning().is_some(), "{days}");
    }
}

#[test]
fn with_no_cache_a_failed_fetch_falls_back_to_the_config_file_and_then_errors_loudly() {
    let (url, hits) = token_server_one(500, String::new(), Vec::new());
    let (token, dir) = dev_token(&url, None, true);
    let fallback = (token.get(), token.status()["devTokenSource"].clone());
    assert_eq!((token.get().is_ok(), *hits.lock().unwrap()), (true, 1));
    std::fs::remove_file(dir.join("developer-token")).unwrap();
    let (token, _) = dev_token(&url, None, false);
    let missing = token.get().unwrap_err();
    let wrote = dir.join("cache").exists();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(fallback, (Ok("config-dir-token".into()), json!("config")));
    assert!(
        missing.contains("developer token is unavailable: couldn't reach 127.0.0.1: HTTP 500"),
        "{missing}"
    );
    assert_eq!((token.missing(), wrote), (Some(missing), false));
}

#[test]
fn malformed_expired_oversize_or_redirected_tokens_leave_the_cache_untouched() {
    let (elsewhere, followed) =
        token_server_one(200, String::new(), published(180, "elsewhere", 0));
    let oversize = published(180, "big", 0).len();
    let refused = [
        (200, String::new(), b"{not json".to_vec()),
        (200, String::new(), published(-1, "expired", 0)),
        (200, String::new(), published(180, "", 0)),
        (
            200,
            String::new(),
            published(180, "big", (64 << 10) + 1 - oversize),
        ),
        (
            302,
            format!("Location: {elsewhere}\r\n"),
            published(180, "moved", 0),
        ),
    ];
    for (status, extra, body) in refused {
        let (url, _) = token_server_one(status, extra, body);
        let old = published(20, "cached", 0);
        let (token, dir) = dev_token(&url, Some(old.clone()), false);
        let got = token.get();
        let cache = cache_of(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((got, cache == old), (Ok("cached".into()), true), "{url}");
    }
    assert_eq!(*followed.lock().unwrap(), 0);
}

#[test]
fn a_developer_token_401_on_covers_refetches_once_and_only_a_new_token_is_retried() {
    for (served, loads) in [("renewed-token", true), (FIXTURE_TOKEN, false)] {
        let fx = fixture(Serve {
            me: 401,
            renewed: true,
            ..SERVE
        });
        let (mut source, config) = fixture_source(&fx, true);
        let (url, hits) = token_server_one(200, String::new(), published(180, served, 0));
        let cache = source.user.dir().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(
            cache.join("developer-token.json"),
            published(100, FIXTURE_TOKEN, 0),
        )
        .unwrap();
        let developer = DeveloperToken::new(Some(url), &cache, &config);
        source.user = source.user.with_developer(developer);
        let ctx = egui::Context::default();
        let mut grid = Grid::new(Some(source));
        grid.follow(&ctx, true);
        settle(&mut grid, &ctx);
        std::fs::remove_dir_all(&config).unwrap();
        let seen = (
            grid.uploaded() == 50,
            grid.rejected_developer_token().is_some(),
            grid.rejected(),
        );
        assert_eq!(seen, (loads, !loads, false), "{served}");
        assert_eq!(*hits.lock().unwrap(), 1, "{served}");
        let asked = fx
            .paths()
            .iter()
            .filter(|p| *p == "/v1/me/storefront")
            .count();
        assert_eq!(asked, if loads { 2 } else { 1 }, "{served}");
    }
}

#[test]
fn no_developer_token_anywhere_is_a_banner_in_the_app() {
    let dir = token_dir("no-developer-token");
    let (_inbox, inbox) = channel();
    let spawn = spawning(|| Err(anyhow!("no player in this test")));
    let user = mock_user(&dir);
    let ctx = egui::Context::default();
    let app = crate::app::App::new(
        &ctx,
        crate::Args::default(),
        spawn,
        (user.clone(), false),
        (inbox, true),
        None,
    );
    let missing = user.developer().get().unwrap_err();
    let banner = app.status_json()["banner"].clone();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(banner, json!(format!("{:?}", Banner::Error(missing))));
}

#[test]
fn a_failed_fetch_retries_after_its_backoff_and_keeps_the_real_cause_until_then() {
    let fetched = published(180, "fetched", 0);
    let (url, hits) = token_server(vec![
        (500, String::new(), Vec::new()),
        (200, String::new(), fetched),
    ]);
    let (token, dir) = dev_token(&url, None, false);
    let first = token.get().unwrap_err();
    let again = token.get().unwrap_err();
    let held = (*hits.lock().unwrap(), token.missing());
    let started = Instant::now();
    let got = token.wait(|| started.elapsed() > Duration::from_secs(15));
    let waited = started.elapsed();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        first.ends_with("couldn't reach 127.0.0.1: HTTP 500"),
        "{first}"
    );
    assert_eq!((again, held), (first.clone(), (1, Some(first))));
    assert_eq!(got, Ok("fetched".into()));
    assert!(waited >= Duration::from_secs(3), "{waited:?}");
    assert_eq!((*hits.lock().unwrap(), token.missing()), (2, None));
}

#[test]
fn a_401_refetch_happens_at_most_once_a_minute() {
    let (url, hits) = token_server_one(200, String::new(), published(180, "revoked", 0));
    let (token, dir) = dev_token(&url, Some(published(100, "revoked", 0)), false);
    let renewed = [token.refetch("revoked"), token.refetch("revoked")];
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!((renewed, *hits.lock().unwrap()), ([false, false], 1));
}

#[test]
fn covers_wait_for_a_developer_token_that_arrives_after_a_failed_fetch() {
    let fx = fixture(SERVE);
    let (mut source, config) = fixture_source(&fx, true);
    std::fs::remove_file(config.join("developer-token")).unwrap();
    let served = published(180, FIXTURE_TOKEN, 0);
    let (url, hits) = token_server(vec![
        (500, String::new(), Vec::new()),
        (200, String::new(), served),
    ]);
    let developer = DeveloperToken::new(Some(url), &config.join("cache"), &config);
    source.user = source.user.with_developer(developer);
    let ctx = egui::Context::default();
    let mut grid = Grid::new(Some(source));
    grid.follow(&ctx, true);
    settle(&mut grid, &ctx);
    std::fs::remove_dir_all(&config).unwrap();
    assert_eq!(
        (grid.uploaded(), grid.failed(), *hits.lock().unwrap()),
        (50, None, 2)
    );
}

#[test]
fn a_cli_search_fills_the_search_view_but_an_agent_search_leaves_the_window_alone() {
    let dir = std::path::PathBuf::from(format!("../../target/c/show-search{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shown = |agent: bool| {
        let (inbox_tx, inbox) = channel();
        let ctx = egui::Context::default();
        let spawn = spawning(|| Err(anyhow!("no engine in this test")));
        let user = (mock_user(&dir), false);
        let mut app = crate::app::App::new(
            &ctx,
            crate::Args::default(),
            spawn,
            user,
            (inbox, true),
            None,
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        if agent {
            inbox_tx
                .send((crate::cli::Request::Agent, deadline, channel().0))
                .unwrap();
        }
        let search = crate::cli::Request::Search("bohemian rhapsody".into());
        inbox_tx.send((search, deadline, channel().0)).unwrap();
        let s = frames(&ctx, &mut app, 3);
        (
            s["route"].clone(),
            s["search"]["state"].clone(),
            s["search"]["seq"].clone(),
        )
    };
    let (cli, agent) = (shown(false), shown(true));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(cli.0, "Search");
    assert_ne!(cli.1, "idle", "the view never ran the CLI's search");
    assert!(
        cli.2.as_u64() > Some(0),
        "the view's search sequence did not move"
    );
    assert_eq!(agent, (json!("Home"), json!("idle"), json!(0)));
}

#[cfg(feature = "gate-sabotage")]
#[test]
fn the_leak_sabotage_keeps_its_size_once_per_new_track_and_only_when_asked() {
    let mut leak = crate::Leak {
        mib: 1,
        item: None,
        kept: Vec::new(),
    };
    for item in [None, Some("a"), Some("a"), Some("b"), None, Some("a")] {
        leak.follow(item);
    }
    assert_eq!(
        leak.kept.iter().map(Vec::len).collect::<Vec<_>>(),
        [1 << 20; 3]
    );
    let mut off = crate::Leak {
        mib: 0,
        item: None,
        kept: Vec::new(),
    };
    off.follow(Some("a"));
    assert!(off.kept.is_empty());
}

#[cfg(feature = "gate-sabotage")]
#[test]
fn the_ballast_sabotage_touches_its_size_once_when_its_trigger_appears() {
    let dir = std::path::PathBuf::from(format!("../../target/c/ballast{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let trigger = dir.join("mib");
    let mut ballast = crate::Ballast {
        trigger: Some(trigger.clone()),
        kept: Vec::new(),
    };
    ballast.follow();
    let before = ballast.mib();
    std::fs::write(&trigger, "two").unwrap();
    ballast.follow();
    let unreadable = ballast.mib();
    std::fs::write(&trigger, "2\n").unwrap();
    ballast.follow();
    let touched = ballast.mib();
    std::fs::write(&trigger, "5").unwrap();
    ballast.follow();
    std::fs::remove_dir_all(&dir).unwrap();
    let pages = ballast.kept.chunks(4096).filter(|p| p[0] == 1).count();
    assert_eq!((before, unreadable, touched, ballast.mib()), (0, 0, 2, 2));
    assert_eq!(pages, 512);
}

#[test]
fn engine_counts_reach_their_own_status_keys() {
    let dir = std::path::PathBuf::from(format!("../../target/c/counts{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ends: Ends = Rc::default();
    let spawned = ends.clone();
    let (inbox_tx, inbox) = channel();
    let ctx = egui::Context::default();
    let spawn = spawning(move || live(&spawned));
    let user = (mock_user(&dir), false);
    let mut app = crate::app::App::new(
        &ctx,
        crate::Args::default(),
        spawn,
        user,
        (inbox, true),
        None,
    );
    let play = crate::cli::Request::Do(Action::PlaySong("1".into()));
    let deadline = Instant::now() + Duration::from_secs(5);
    inbox_tx.send((play, deadline, channel().0)).unwrap();
    frames(&ctx, &mut app, 3);
    let mut snap = Snapshot::default();
    (snap.drains_acked, snap.drain_timeouts) = (1, 2);
    (snap.sent_us, snap.skipped_us, snap.decrypted_us) = (3_000_000, 4_000_000, 5_000_000);
    (
        snap.readiness_dips,
        snap.decrypt_fails,
        snap.output_underflows,
    ) = (6, 7, 8);
    ends.borrow()[0].1.send(snap).unwrap();
    let s = frames(&ctx, &mut app, 3);
    std::fs::remove_dir_all(&dir).unwrap();
    let keys = [
        "drainsAcked",
        "drainTimeouts",
        "sentMs",
        "skippedMs",
        "decryptedMs",
        "readinessDips",
        "decryptFails",
        "outputUnderflows",
    ];
    let seen: Vec<_> = keys.iter().map(|k| s[*k].as_u64()).collect();
    let want = [1, 2, 3000, 4000, 5000, 6, 7, 8].map(Some);
    assert_eq!(seen, want);
}

#[test]
fn an_art_request_on_a_pooled_socket_that_went_silent_is_retried_fresh_in_about_10_s() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/art.jpg", listener.local_addr().unwrap());
    let mut jpeg = Vec::new();
    image::RgbImage::from_pixel(8, 8, image::Rgb([9, 9, 9]))
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    std::thread::spawn(move || {
        for (conn, stream) in listener.incoming().take(3).enumerate() {
            let (mut stream, jpeg) = (stream.unwrap(), jpeg.clone());
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                for n in 0.. {
                    if reader.read_line(&mut String::new()).unwrap_or(0) == 0 {
                        return;
                    }
                    while reader.read_line(&mut String::new()).unwrap() > 2 {}
                    if conn == 0 && n == 1 {
                        std::thread::sleep(Duration::from_secs(30));
                    }
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", jpeg.len());
                    let _ = stream
                        .write_all(head.as_bytes())
                        .and_then(|()| stream.write_all(&jpeg));
                }
            });
        }
    });
    let (mut agent, tally) = (covers::agent(), covers::Tally::default());
    assert!(covers::cover(&mut agent, &url, 8, &tally).is_ok());
    let at = Instant::now();
    let again = covers::cover(&mut agent, &url, 8, &tally).map(|_| at.elapsed());
    assert!(
        matches!(again, Ok(took) if took < Duration::from_secs(13)),
        "{again:?}"
    );
}

mod disk_cache {
    use bramley_api::store::Store;

    use super::*;

    fn root(name: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        let dir = dir.join(format!("cache-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn arts(fx: &Fixture) -> usize {
        fx.paths().iter().filter(|p| p.starts_with("/art/")).count()
    }

    fn cached(store: &Arc<Store>) -> covers::Tally {
        covers::Tally::caching(Some(store.clone()))
    }

    fn open_home(fx: &Fixture, store: &Arc<Store>) -> Grid {
        let (mut source, config) = fixture_source(fx, true);
        source.art_cache = Some(store.clone());
        let ctx = egui::Context::default();
        let mut grid = Grid::new(Some(source));
        grid.follow(&ctx, true);
        settle(&mut grid, &ctx);
        std::fs::remove_dir_all(&config).unwrap();
        grid
    }

    #[test]
    fn an_art_cache_hit_makes_no_network_request_and_survives_a_restart() {
        let fx = fixture(SERVE);
        let dir = root("art");
        let url = format!("{}/art/3/96x96bb.jpg", fx.base);
        let mut agent = covers::agent();
        let store = Arc::new(Store::open(&dir, "art", 1 << 20).unwrap());
        let fetched = covers::cover(&mut agent, &url, 96, &cached(&store)).unwrap();
        let hit = covers::cover(&mut agent, &url, 96, &cached(&store)).unwrap();
        let restarted = Arc::new(Store::open(&dir, "art", 1 << 20).unwrap());
        let again = covers::cover(&mut agent, &url, 96, &cached(&restarted)).unwrap();
        assert_eq!((arts(&fx), hit.pixels == fetched.pixels), (1, true));
        assert_eq!(again.pixels, fetched.pixels);
    }

    #[test]
    fn a_corrupt_art_entry_is_refetched_not_trusted() {
        let fx = fixture(SERVE);
        let url = format!("{}/art/3/96x96bb.jpg", fx.base);
        let store = Arc::new(Store::open(&root("corrupt"), "art", 1 << 20).unwrap());
        store.put(&["art", &url], b"not a jpeg", store.epoch());
        let image = covers::cover(&mut covers::agent(), &url, 96, &cached(&store));
        assert_eq!((image.is_ok(), arts(&fx)), (true, 1));
    }

    #[test]
    fn the_home_grid_fills_from_the_art_cache_on_a_second_open() {
        let fx = fixture(SERVE);
        let store = Arc::new(Store::open(&root("home"), "art", 1 << 24).unwrap());
        assert_eq!(open_home(&fx, &store).uploaded(), 50);
        assert_eq!(arts(&fx), 50);
        assert_eq!(open_home(&fx, &store).uploaded(), 50);
        assert_eq!(arts(&fx), 50, "the second open fetched no art");
    }

    #[test]
    fn sign_out_wipes_the_api_cache_and_keeps_the_art_cache() {
        let (mut app, _ends, inbox, ctx, config, _fx) = app_on(SERVE);
        let dir = root("sign-out");
        let api = Arc::new(Store::open(&dir, "api", 1 << 20).unwrap());
        let art = Store::open(&dir, "art", 1 << 20).unwrap();
        api.put(&["library"], b"library data", api.epoch());
        art.put(&["cover"], b"art bytes", art.epoch());
        app.api_cache = Some(api.clone());
        let counts = app.status_json()["cache"].clone();
        ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignOut);
        std::fs::remove_dir_all(&config).unwrap();
        assert_eq!(
            api.epoch(),
            2,
            "once on sign-out, once when the keyring clear finishes"
        );
        assert_eq!(api.get(&["library"]), None);
        assert!(art.get(&["cover"]).is_some());
        let mut keys: Vec<_> = counts.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        let want = [
            "apiHits",
            "apiMisses",
            "apiRevalidations",
            "artHits",
            "artMisses",
            "homeHits",
        ];
        assert_eq!(keys, want);
    }

    fn start_home(
        fx: &Fixture,
        art: &Arc<Store>,
        api: &Arc<Store>,
    ) -> (Grid, egui::Context, std::path::PathBuf) {
        let (mut source, config) = fixture_source(fx, true);
        (source.art_cache, source.api_cache) = (Some(art.clone()), Some(api.clone()));
        let ctx = egui::Context::default();
        let mut grid = Grid::new(Some(source));
        grid.follow(&ctx, true);
        (grid, ctx, config)
    }

    fn order(grid: &Grid) -> Vec<String> {
        (0..50)
            .filter_map(|n| grid.album(n).map(str::to_owned))
            .collect()
    }

    fn stores(name: &str) -> (Arc<Store>, Arc<Store>) {
        let dir = root(name);
        let open = |kind| Arc::new(Store::open(&dir, kind, 1 << 24).unwrap());
        (open("art"), open("api"))
    }

    fn ranking_requests(fx: &Fixture) -> usize {
        fx.paths()
            .iter()
            .filter(|p| !p.starts_with("/art/"))
            .count()
    }

    fn watch(
        grid: &mut Grid,
        ctx: &egui::Context,
        fx: &Fixture,
        library_requests: usize,
    ) -> Vec<Vec<String>> {
        let until = Instant::now() + Duration::from_secs(20);
        let mut seen = Vec::new();
        let mut settled = None;
        while Instant::now() < until
            && settled.is_none_or(|at: Instant| at.elapsed() < Duration::from_millis(600))
        {
            grid.receive(ctx);
            if grid.uploaded() == 50 {
                seen.push(order(grid));
            }
            if settled.is_none() && charts(fx) >= library_requests {
                settled = Some(Instant::now());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        seen
    }

    #[test]
    fn a_cached_grid_shows_before_the_ranking_answers_and_one_swap_replaces_it() {
        let fx = fixture(Serve {
            slow_chart: true,
            ..SERVE
        });
        let (art, api) = stores("home-swap");
        let (mut first, ctx, config) = start_home(&fx, &art, &api);
        settle(&mut first, &ctx);
        let mut grid = api.home(FIXTURE_USER).unwrap();
        let live: Vec<_> = grid.iter().map(|(id, _)| id.clone()).collect();
        grid.reverse();
        let stale: Vec<_> = grid.iter().map(|(id, _)| id.clone()).collect();
        api.remember_home(FIXTURE_USER, &grid, api.epoch());
        let before = ranking_requests(&fx);
        let (mut second, ctx, _) = start_home(&fx, &art, &api);
        let seen = watch(&mut second, &ctx, &fx, 2);
        std::fs::remove_dir_all(&config).unwrap();
        assert_eq!(seen.first(), Some(&stale), "the cached grid shows first");
        assert_eq!(seen.last(), Some(&live), "the live grid replaces it");
        assert!(
            seen.iter().all(|o| *o == stale || *o == live),
            "a swap reshuffled tiles one by one"
        );
        assert_eq!(
            ranking_requests(&fx) - before,
            before,
            "the request count grew"
        );
        let rewritten = api.home(FIXTURE_USER).unwrap();
        let ids: Vec<_> = rewritten.into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, live);
    }

    #[test]
    fn an_unchanged_live_grid_swaps_nothing_and_a_cached_grid_makes_no_extra_requests() {
        let fx = fixture(SERVE);
        let (art, api) = stores("home-same");
        let (mut first, ctx, config) = start_home(&fx, &art, &api);
        settle(&mut first, &ctx);
        let uses = ranking_requests(&fx);
        let (mut second, ctx, _) = start_home(&fx, &art, &api);
        let seen = watch(&mut second, &ctx, &fx, 2);
        std::fs::remove_dir_all(&config).unwrap();
        let mut delta = ctx.tex_manager().write().take_delta();
        let uploads = delta.set.len();
        delta.clear();
        assert_eq!(
            (seen.is_empty(), uploads < 100),
            (false, true),
            "{uploads} uploads"
        );
        assert_eq!(ranking_requests(&fx), 2 * uses);
    }

    #[test]
    fn another_accounts_cached_grid_is_never_shown() {
        let fx = fixture(SERVE);
        let (art, api) = stores("home-scope");
        let seed = (
            "l.9".to_owned(),
            format!("{}/art/seed/{{w}}x{{h}}bb.jpg", fx.base),
        );
        api.remember_home("someone else", std::slice::from_ref(&seed), api.epoch());
        let (mut grid, ctx, config) = start_home(&fx, &art, &api);
        settle(&mut grid, &ctx);
        std::fs::remove_dir_all(&config).unwrap();
        assert!(fx.paths().iter().all(|p| !p.contains("/art/seed")));
        assert_eq!(api.home("someone else"), Some(vec![seed]));
        assert_eq!(api.home(FIXTURE_USER).map(|g| g.len()), Some(50));
    }

    #[test]
    fn sign_out_wipes_the_cached_home_grid() {
        let (mut app, _ends, inbox, ctx, config, _fx) = app_on(SERVE);
        let api = Arc::new(Store::open(&root("sign-out-home"), "api", 1 << 20).unwrap());
        app.api_cache = Some(api.clone());
        api.remember_home(FIXTURE_USER, &[("l.1".into(), "a/{w}".into())], api.epoch());
        ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignOut);
        std::fs::remove_dir_all(&config).unwrap();
        assert_eq!(api.home(FIXTURE_USER), None);
    }

    #[test]
    fn a_stale_cover_cannot_land_after_the_fresh_grid_took_over() {
        let ctx = egui::Context::default();
        let (tx, rx) = channel();
        let mut grid = Grid::default();
        grid.listen(rx);
        let image = || egui::ColorImage::filled([2, 2], egui::Color32::WHITE);
        let cover =
            |round, index: usize, id: &str| covers::Loaded::Cover(round, index, id.into(), image());
        tx.send(cover(0, 0, "stale-0")).unwrap();
        grid.receive(&ctx);
        assert_eq!(grid.album(0), Some("stale-0"));
        tx.send(covers::Loaded::Live(2)).unwrap();
        tx.send(cover(1, 0, "fresh-0")).unwrap();
        grid.receive(&ctx);
        assert_eq!(
            grid.album(0),
            Some("stale-0"),
            "the swap waits for the whole grid"
        );
        tx.send(covers::Loaded::Missed(1, "no art".into())).unwrap();
        tx.send(cover(0, 1, "stale-1")).unwrap();
        grid.receive(&ctx);
        assert_eq!((grid.album(0), grid.uploaded()), (Some("fresh-0"), 1));
        tx.send(cover(0, 0, "stale-0")).unwrap();
        grid.receive(&ctx);
        assert_eq!((grid.album(0), grid.uploaded()), (Some("fresh-0"), 1));
    }

    #[test]
    fn home_issues_its_independent_ranking_requests_at_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (open, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (library, overlapped) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let (seen, gauge) = (
            requests.clone(),
            (open, peak.clone(), library, overlapped.clone()),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (seen, (open, peak, library, overlapped)) = (seen.clone(), gauge.clone());
                std::thread::spawn(move || {
                    let mut stream = stream.unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
                    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                        line.clear();
                    }
                    seen.lock().unwrap().push((path.clone(), None, None));
                    peak.fetch_max(open.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                    let history = path.contains("music-summaries")
                        || path.contains("/history/")
                        || path.contains("/recent/")
                        || path.contains("recently-added");
                    if path.starts_with("/v1/me/library/albums?") {
                        library.store(true, Ordering::SeqCst);
                    }
                    let until = Instant::now() + Duration::from_secs(2);
                    while history && !library.load(Ordering::SeqCst) && Instant::now() < until {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    overlapped
                        .fetch_or(history && library.load(Ordering::SeqCst), Ordering::SeqCst);
                    let body = match path.as_str() {
                        "/v1/me/storefront" => r#"{"data":[{"id":"zz"}]}"#,
                        _ => r#"{"data":[]}"#,
                    };
                    let head = format!(
                        "HTTP/1.1 200 X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    open.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        let fx = Fixture { base, requests };
        let (source, config) = fixture_source(&fx, true);
        let ctx = egui::Context::default();
        let mut grid = Grid::new(Some(source));
        grid.follow(&ctx, true);
        settle(&mut grid, &ctx);
        std::fs::remove_dir_all(&config).unwrap();
        assert!(
            overlapped.load(Ordering::SeqCst),
            "the library page waited for the history"
        );
        assert!(peak.load(Ordering::SeqCst) > 1);
    }
}

fn native_user(
    dir: &std::path::Path,
    status: Arc<Mutex<&'static str>>,
    (answer, tokens): (&'static str, bool),
) -> Credentials {
    let asked = status.clone();
    let session = crate::musickit::Session::new(
        move |request| {
            let mut held = asked.lock().unwrap();
            if request {
                *held = answer;
            }
            Ok((*held).into())
        },
        move |_| {
            let issued = bramley_engine::Tokens {
                developer: "dev-native".into(),
                user: "user-native".into(),
            };
            tokens.then_some(issued).ok_or("tokens refused".into())
        },
    );
    let developer = DeveloperToken::new(None, dir, dir).with_musickit(Arc::new(session));
    let store = keyring_core::mock::Store::new().unwrap();
    fail_next(&store);
    Credentials::new(Some(store), "bramley-test", dir).with_developer(developer)
}

#[test]
fn native_credentials_come_from_musickit_and_never_touch_the_keyring_or_the_disk() {
    let dir = token_dir("native-credentials");
    let user = native_user(
        &dir,
        Arc::new(Mutex::new("authorized")),
        ("authorized", true),
    );
    let seen = (
        user.load(),
        user.developer().get(),
        user.developer().refetch("rejected"),
        user.save("token", || true),
        user.clear(),
        user.developer().peek(),
        user.storage(),
        user.developer().status()["devTokenSource"].clone(),
    );
    let left = std::fs::read_dir(&dir).unwrap().count();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        seen,
        (
            Ok(Some("user-native".into())),
            Ok("dev-native".into()),
            true,
            Err("macOS holds the Apple Music sign-in"),
            Ok(()),
            None,
            Some("musickit"),
            json!("musickit")
        )
    );
    assert_eq!(left, 0);
}

#[test]
fn a_signed_out_native_account_loads_no_token_without_prompting() {
    let dir = token_dir("native-signed-out");
    let status = Arc::new(Mutex::new("notDetermined"));
    let user = native_user(&dir, status.clone(), ("authorized", true));
    let seen = (user.signed_in(), user.load(), user.developer().get());
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        seen,
        (
            Ok(false),
            Ok(None),
            Err(crate::musickit::NOT_AUTHORIZED.into())
        )
    );
    assert_eq!(*status.lock().unwrap(), "notDetermined");
}

fn native_app(
    status: &'static str,
    (answer, tokens): (&'static str, bool),
) -> (
    crate::app::App,
    Sender<crate::cli::Envelope>,
    egui::Context,
    std::path::PathBuf,
) {
    let dir = token_dir(&format!("native-app-{status}-{answer}-{tokens}"));
    let user = native_user(&dir, Arc::new(Mutex::new(status)), (answer, tokens));
    let (inbox_tx, inbox) = channel();
    let ctx = egui::Context::default();
    let ends: Ends = Rc::default();
    let spawn = spawning(move || live(&ends));
    let mut app = crate::app::App::new(
        &ctx,
        crate::Args::default(),
        spawn,
        (user, true),
        (inbox, true),
        None,
    );
    app.check_sign_in();
    (app, inbox_tx, ctx, dir)
}

#[test]
fn the_native_sign_in_button_asks_macos_and_signs_the_app_in() {
    let (mut app, inbox, ctx, dir) = native_app("notDetermined", ("authorized", true));
    let signed_out = wait_for(&ctx, &mut app, &|s| s["signInReason"] == "boot-no-token");
    let asked = ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignIn);
    let signed_in = wait_for(&ctx, &mut app, &|s| s["signInReason"].is_null());
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(signed_out["banner"], "SignInNeeded");
    assert_eq!(asked["error"], Value::Null);
    assert_eq!(
        (signed_in["banner"].clone(), signed_in["signInUrl"].clone()),
        (Value::Null, Value::Null)
    );
}

#[test]
fn a_denied_native_sign_in_stays_signed_out_and_says_where_to_allow_it() {
    let (mut app, inbox, ctx, dir) = native_app("notDetermined", ("denied", true));
    wait_for(&ctx, &mut app, &|s| s["signInReason"] == "boot-no-token");
    ask_app(&inbox, &ctx, &mut app, crate::cli::Request::SignIn);
    let status = wait_for(&ctx, &mut app, &|s| s["notice"].is_string());
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        status["notice"]
            .as_str()
            .unwrap()
            .contains("System Settings")
    );
    assert_eq!(status["banner"], "SignInNeeded");
}

#[test]
fn the_startup_check_reads_the_access_status_without_asking_for_tokens() {
    let (mut app, _inbox, ctx, dir) = native_app("authorized", ("authorized", false));
    let status = wait_for(&ctx, &mut app, &|s| s["signInReason"] != "boot-unchecked");
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        (status["signInReason"].clone(), status["notice"].clone()),
        (Value::Null, Value::Null)
    );
}
