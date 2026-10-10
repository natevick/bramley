use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bramley_engine::{Command, PlayState, PlayerError, Queue, Snapshot};

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
