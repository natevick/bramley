use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bramley_engine::{Command, PlayState, PlayerError, Queue, Snapshot};
use serde_json::json;

use crate::covers::{self, Grid, Source};
use crate::playback::{Action, Banner, HOLD_LIMIT, Link, Playback, SignedOut, Spawn};

type Ends = Rc<RefCell<Vec<(Receiver<Command>, Sender<Snapshot>)>>>;

fn live(ends: &Ends) -> anyhow::Result<Link> {
    let (command_tx, command_rx) = channel();
    let (snapshot_tx, snapshot_rx) = channel();
    ends.borrow_mut().push((command_rx, snapshot_tx));
    Ok((command_tx, snapshot_rx))
}

fn spawning(mut f: impl FnMut() -> anyhow::Result<Link> + 'static) -> Spawn {
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
    let refused = crate::boot(&dir, |t| {
        Err(anyhow!("refused {} for {}", t.user, t.developer))
    });
    assert_eq!(
        format!("{:#}", refused.unwrap_err()),
        "refused <redacted> for <redacted>"
    );
    std::fs::remove_file(dir.join("music-user-token")).unwrap();
    let missing = crate::boot(&dir, |_| unreachable!("started without a user token"));
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
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<Seen>,
    _guard: fastframe_instance::Guard,
}

impl FakeApp {
    fn start(slot: &fastframe_instance::Slot, signed_out: bool) -> Self {
        let (inject, inbox) = channel();
        let waker = std::sync::Arc::new(std::sync::OnceLock::new());
        let handler = crate::cli::handler(inject.clone(), waker.clone());
        let fastframe_instance::Claim::First(_guard) = slot.claim("[]", handler) else {
            panic!("the test slot is already held");
        };
        let stop: std::sync::Arc<std::sync::atomic::AtomicBool> = Default::default();
        let stopped = std::sync::Arc::clone(&stop);
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
            let mut app = crate::app::App::new(&ctx, args, spawn, signed_out, (inbox, true), None);
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
        (
            "search bohemian rhapsody",
            Request::Search("bohemian rhapsody".into()),
        ),
    ];
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
        "search",
        "pause now",
        "stop",
    ] {
        assert!(parse(&words(line)).is_err(), "{line:?} parsed");
    }
}

#[test]
fn cli_exit_codes_without_a_running_app() {
    use crate::cli::{NOT_RUNNING, UNAVAILABLE, USAGE};
    let slot = slot("none");
    let (code, _, err) = run(&slot, "now");
    assert_eq!(code, NOT_RUNNING);
    assert!(err.contains("not running"), "{err}");
    assert_eq!(run(&slot, "volume 500").0, USAGE);
    let (code, _, err) = run(&slot, "search bohemian rhapsody");
    assert_eq!(code, UNAVAILABLE);
    assert!(err.contains("not yet available"), "{err}");
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
    for flag in ["--api-base", "--screenshot", "--after"] {
        let args = [flag, "1"].map(String::from);
        assert!(crate::parse_args(args.into_iter()).is_err(), "{flag}");
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
        seen.push(crate::signed_out(&dir));
    }
    std::fs::remove_dir_all(&dir).unwrap();
    seen.push(crate::signed_out(&dir));
    assert_eq!(seen, [true, false, true]);
}

const FIXTURE_TOKEN: &str = "dev-token-fixture-0123456789";
const FIXTURE_USER: &str = "user-token-fixture-9876543210";

type Requests = Arc<Mutex<Vec<(String, Option<String>, Option<String>)>>>;

#[derive(Clone, Copy)]
struct Serve {
    me: u16,
    chart: u16,
    big: bool,
    redirect: bool,
    bad_albums: bool,
    slow_chart: bool,
}

const SERVE: Serve = Serve {
    me: 200,
    chart: 200,
    big: false,
    redirect: false,
    bad_albums: false,
    slow_chart: false,
};

struct Fixture {
    base: String,
    requests: Requests,
}

impl Fixture {
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
            seen.lock().unwrap().push((path.clone(), auth, user));
            let refusal = json!({"errors": [{"detail": format!("rejected {FIXTURE_TOKEN}")}]});
            let (status, extra, body) = if path == "/v1/me/storefront" {
                if serve.me == 0 {
                    continue;
                }
                (serve.me, String::new(), b"{}".to_vec())
            } else if path.starts_with("/v1/") {
                if serve.slow_chart {
                    std::thread::sleep(Duration::from_millis(300));
                }
                let url = |i: usize| match i {
                    7 if serve.big => format!("{art_base}/art/7big/{{w}}x{{h}}bb.jpg"),
                    3 if serve.bad_albums => format!("{art_base}/art/3/{{w}}x{{h}}{{c}}.jpg"),
                    _ => format!("{art_base}/art/{i}/{{w}}x{{h}}bb.jpg"),
                };
                let albums: Vec<_> = (0..50)
                    .map(|i| match i {
                        4 if serve.bad_albums => json!({"attributes": {}}),
                        _ => json!({"attributes": {"artwork": {"url": url(i)}}}),
                    })
                    .collect();
                let chart = json!({"results": {"albums": [{"data": albums}]}});
                let body = if serve.chart == 200 { chart } else { refusal };
                (serve.chart, String::new(), body.to_string().into_bytes())
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
        config: config.clone(),
    };
    (source, config)
}

fn settle(grid: &mut Grid, ctx: &egui::Context) {
    let until = Instant::now() + Duration::from_secs(20);
    while grid.uploaded() + grid.failures() < 50 && !grid.rejected() && Instant::now() < until {
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
    assert_eq!(
        requests[1],
        (
            "/v1/catalog/us/charts?types=albums&limit=50".into(),
            bearer,
            None
        )
    );
    assert_eq!(requests.len(), 52);
    for (path, auth, user) in &requests[2..] {
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

#[test]
fn covers_need_a_storefront_200_and_a_refusal_reads_as_sign_in() {
    for (me, rejected, error) in [
        (401, true, None),
        (403, true, None),
        (0, false, Some("sign-in check")),
    ] {
        let (grid, fx, _) = load_covers(Serve { me, ..SERVE }, true);
        assert_eq!((grid.rejected(), grid.uploaded()), (rejected, 0), "{me}");
        assert_eq!(
            grid.failed().map(|e| e.split(':').next().unwrap()),
            error,
            "{me}"
        );
        assert_eq!(fx.paths(), ["/v1/me/storefront"], "{me}");
    }
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
    assert_eq!(fx.paths().len(), 2);
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
        false,
        (inbox, true),
        Some(source),
    );
    (app, ends, inbox_tx, ctx, config)
}

#[test]
fn a_sign_in_needed_snapshot_hides_the_covers_in_the_frame_it_arrives() {
    let (mut app, ends, inbox, ctx, config) = app_on(SERVE);
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
fn a_storefront_401_shows_the_sign_in_banner_and_no_covers() {
    let (mut app, _ends, _inbox, ctx, config) = app_on(Serve { me: 401, ..SERVE });
    let until = Instant::now() + Duration::from_secs(10);
    while app.status_json()["banner"].is_null() && Instant::now() < until {
        cover_meshes(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::remove_dir_all(&config).unwrap();
    let status = app.status_json();
    assert_eq!(
        (
            status["banner"].as_str(),
            status["texturesUploaded"].as_u64()
        ),
        (Some("SignInNeeded"), Some(0))
    );
}
