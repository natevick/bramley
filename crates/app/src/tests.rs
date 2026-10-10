use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};

use bramley_engine::{Command, PlayState, PlayerError, Queue, Snapshot};

use crate::playback::{Action, Banner, Playback};

type Ends = Rc<RefCell<Vec<(Receiver<Command>, Sender<Snapshot>)>>>;

fn fake(signed_out: bool) -> (Playback, Ends) {
    let ends: Ends = Rc::default();
    let spawned = ends.clone();
    let spawn = Box::new(move || {
        let (command_tx, command_rx) = channel();
        let (snapshot_tx, snapshot_rx) = channel();
        spawned.borrow_mut().push((command_rx, snapshot_tx));
        Ok((command_tx, snapshot_rx))
    });
    (Playback::new(spawn, signed_out), ends)
}

fn feed(p: &mut Playback, ends: &Ends, state: PlayState, position: f64, applied: u64) {
    let mut s = Snapshot::default();
    (s.state, s.position, s.applied) = (state, Some(position), applied);
    ends.borrow()[0].1.send(s).unwrap();
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
    for action in [
        Action::Pause,
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
    assert_eq!(sent, expected);
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
fn a_sign_in_error_releases_the_hold_and_shows_the_banner() {
    let (mut p, ends) = fake(false);
    p.apply(Action::PlaySong("42".into()));
    let mut s = Snapshot::default();
    s.error = Some(PlayerError::SignInNeeded("401".into()));
    ends.borrow()[0].1.send(s).unwrap();
    p.receive();
    assert_eq!(
        (shown(&p), p.banner()),
        ((PlayState::Stopped, None), Some(Banner::SignInNeeded))
    );
}

#[test]
fn a_failed_spawn_is_a_redacted_error_and_the_next_play_retries() {
    let mut attempts = 0;
    let mut p = Playback::new(
        Box::new(move || {
            attempts += 1;
            anyhow::ensure!(
                attempts > 1,
                "token eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJ4In0.c2ln"
            );
            Ok((channel().0, channel().1))
        }),
        false,
    );
    p.apply(Action::PlaySong("42".into()));
    assert_eq!(p.banner(), Some(Banner::Error("token <jwt>".into())));
    p.apply(Action::PlaySong("42".into()));
    assert_eq!(p.banner(), None);
}
