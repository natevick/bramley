use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use anyhow::Result;
use bramley_engine::{Command, PlayState, PlayerError, Queue, Snapshot, redact};

pub type Link = (Sender<Command>, Receiver<Snapshot>);
pub type Spawn = Box<dyn FnMut() -> Result<Link>>;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    #[cfg_attr(not(any(feature = "dev", test)), expect(dead_code))]
    PlaySong(String),
    Play,
    Pause,
    Seek(f64),
    Next,
    Previous,
}

#[derive(Default)]
struct Pending {
    seq: u64,
    state: Option<PlayState>,
    position: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Banner {
    SignInNeeded,
    Error(String),
}

pub struct Playback {
    spawn: Spawn,
    link: Option<Link>,
    seq: u64,
    snap: Snapshot,
    pending: Option<Pending>,
    signed_out: bool,
}

impl Playback {
    pub fn new(spawn: Spawn, signed_out: bool) -> Self {
        Playback {
            spawn,
            link: None,
            seq: 0,
            snap: Snapshot::default(),
            pending: None,
            signed_out,
        }
    }

    pub fn started(&self) -> bool {
        self.link.is_some()
    }

    pub fn apply(&mut self, action: Action) {
        let (command, state, position) = match action {
            Action::PlaySong(id) => return self.play_song(id),
            _ if self.link.is_none() => return,
            Action::Play => (Command::Play, Some(PlayState::Playing), None),
            Action::Pause => (Command::Pause, Some(PlayState::Paused), None),
            Action::Seek(t) => (Command::Seek(t), None, Some(t)),
            Action::Next => (Command::Next, None, Some(0.0)),
            Action::Previous => (Command::Previous, None, Some(0.0)),
        };
        self.send(command, state, position);
    }

    fn play_song(&mut self, id: String) {
        if self.signed_out {
            return;
        }
        if self.link.is_none() {
            match (self.spawn)() {
                Ok(link) => (self.link, self.seq, self.snap) = (Some(link), 0, Snapshot::default()),
                Err(e) => {
                    let message = redact(&format!("{e:#}"), &[]);
                    self.snap.error = Some(PlayerError::LoadFailed(message));
                    return;
                }
            }
        }
        let queue = Queue::Songs(vec![id]);
        self.send(Command::SetQueue { queue, start: 0 }, None, Some(0.0));
        self.send(Command::Play, Some(PlayState::Playing), None);
    }

    fn send(&mut self, command: Command, state: Option<PlayState>, position: Option<f64>) {
        let Some((tx, _)) = &self.link else { return };
        if tx.send(command).is_err() {
            self.link = None;
            return;
        }
        self.seq += 1;
        let pending = self.pending.get_or_insert_with(Pending::default);
        pending.seq = self.seq;
        pending.state = state.or(pending.state);
        pending.position = position.or(pending.position);
    }

    pub fn receive(&mut self) -> bool {
        let mut changed = false;
        while let Some((_, rx)) = &self.link {
            match rx.try_recv() {
                Ok(snap) => {
                    if snap.error.is_some()
                        || self.pending.as_ref().is_some_and(|p| snap.applied >= p.seq)
                    {
                        self.pending = None;
                    }
                    self.snap = snap;
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.link = None;
                    self.pending = None;
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn shutdown(&mut self) {
        if let Some((tx, _)) = self.link.take() {
            let _ = tx.send(Command::Shutdown);
        }
    }

    pub fn shown(&self) -> Snapshot {
        let mut shown = self.snap.clone();
        if let Some(p) = &self.pending {
            shown.state = p.state.unwrap_or(shown.state);
            shown.position = p.position.or(shown.position);
        }
        shown
    }

    pub fn banner(&self) -> Option<Banner> {
        if self.signed_out || self.snap.preview_only {
            return Some(Banner::SignInNeeded);
        }
        match &self.snap.error {
            Some(PlayerError::SignInNeeded(_)) => Some(Banner::SignInNeeded),
            Some(PlayerError::Playback(e) | PlayerError::LoadFailed(e)) => {
                Some(Banner::Error(e.clone()))
            }
            None => None,
        }
    }
}
