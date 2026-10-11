use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use bramley_engine::{COMMAND_WAIT, Command, PlayState, PlayerError, Queue, Snapshot};

pub type Link = (Sender<Command>, Receiver<Snapshot>);
pub type Spawn = Box<dyn FnMut() -> Receiver<Result<Link>>>;

pub const HOLD_LIMIT: Duration = COMMAND_WAIT.saturating_add(Duration::from_secs(1));

#[derive(Debug)]
pub struct SignedOut;

impl std::fmt::Display for SignedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sign in needed")
    }
}

impl std::error::Error for SignedOut {}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    PlaySong(String),
    PlayIn(Queue, usize),
    Shuffle(Queue),
    Play,
    Pause,
    Seek(f64),
    Next,
    Previous,
    Volume(f32),
    QueueAdd(String),
    PlayNext(String),
    JumpTo(usize, String),
    TopUp(Vec<String>),
}

pub fn play_or_pause(state: PlayState) -> Action {
    match state {
        PlayState::Playing | PlayState::Loading => Action::Pause,
        PlayState::Stopped | PlayState::Paused => Action::Play,
    }
}

struct Pending {
    seq: u64,
    state: Option<PlayState>,
    position: Option<f64>,
    since: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Banner {
    SignInNeeded,
    SigningIn(String),
    Error(String),
}

pub struct Playback {
    spawn: Spawn,
    starting: Option<Receiver<Result<Link>>>,
    link: Option<Link>,
    queued: Vec<Command>,
    seq: u64,
    engine: u64,
    snap: Snapshot,
    pending: Option<Pending>,
    signed_out: bool,
    shuffled: bool,
}

impl Playback {
    pub fn new(spawn: Spawn, signed_out: bool) -> Self {
        Playback {
            spawn,
            starting: None,
            link: None,
            queued: Vec::new(),
            seq: 0,
            engine: 0,
            snap: Snapshot::default(),
            pending: None,
            signed_out,
            shuffled: false,
        }
    }

    #[cfg(feature = "dev")]
    pub fn fixture(&mut self, snap: Snapshot) {
        self.snap = snap;
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn engine(&self) -> u64 {
        self.engine
    }

    pub fn started(&self) -> bool {
        self.link.is_some() || self.starting.is_some()
    }

    pub fn apply(&mut self, action: Action) -> bool {
        let (command, state, position) = match action {
            Action::PlaySong(id) => return self.play(Queue::Songs(vec![id]), 0, false),
            Action::PlayIn(queue, start) => return self.play(queue, start, false),
            Action::Shuffle(queue) => return self.play(queue, 0, true),
            _ if !self.started() => return false,
            Action::Play => (Command::Play, Some(PlayState::Playing), None),
            Action::Pause => (Command::Pause, Some(PlayState::Paused), None),
            Action::Seek(t) => (Command::Seek(t), None, Some(t)),
            Action::Next => (Command::Next, None, Some(0.0)),
            Action::Previous => (Command::Previous, None, Some(0.0)),
            Action::Volume(v) => (Command::Volume(v), None, None),
            Action::QueueAdd(id) => (Command::PlayLater(Queue::Songs(vec![id])), None, None),
            Action::PlayNext(id) => (Command::PlayNext(Queue::Songs(vec![id])), None, None),
            Action::JumpTo(index, id) => (Command::JumpTo { index, id }, None, Some(0.0)),
            Action::TopUp(ids) => (Command::PlayLater(Queue::Songs(ids)), None, None),
        };
        self.send(command, state, position);
        true
    }

    fn play(&mut self, queue: Queue, start: usize, shuffle: bool) -> bool {
        if self.signed_out {
            return false;
        }
        if !self.started() {
            self.starting = Some((self.spawn)());
            (self.seq, self.snap, self.shuffled) = (0, Snapshot::default(), false);
            self.engine += 1;
        }
        if self.shuffled && !shuffle {
            self.shuffled = false;
            self.send(Command::Shuffle(false), None, None);
        }
        self.send(Command::SetQueue { queue, start }, None, Some(0.0));
        if shuffle && !self.shuffled {
            self.shuffled = true;
            self.send(Command::Shuffle(true), None, None);
        }
        self.send(Command::Play, Some(PlayState::Playing), None);
        true
    }

    fn send(&mut self, command: Command, state: Option<PlayState>, position: Option<f64>) {
        self.queued.push(command);
        self.seq += 1;
        let pending = self.pending.get_or_insert(Pending {
            seq: 0,
            state: None,
            position: None,
            since: Instant::now(),
        });
        pending.seq = self.seq;
        pending.since = Instant::now();
        pending.state = state.or(pending.state);
        pending.position = position.or(pending.position);
        self.flush();
    }

    fn flush(&mut self) {
        let Some((tx, _)) = &self.link else { return };
        for command in self.queued.drain(..) {
            if tx.send(command).is_err() {
                self.pending = None;
                break;
            }
        }
        self.queued.clear();
    }

    pub fn receive(&mut self) -> bool {
        self.receive_at(Instant::now())
    }

    pub fn receive_at(&mut self, now: Instant) -> bool {
        let mut changed = self.finish_start();
        while let Some((_, rx)) = &self.link {
            let snap = match rx.try_recv() {
                Ok(snap) => snap,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    (self.link, self.pending, changed) = (None, None, true);
                    break;
                }
            };
            let fatal = matches!(
                snap.error,
                Some(PlayerError::SignInNeeded(_) | PlayerError::LoadFailed(_))
            );
            if let Some(p) = self.pending.as_mut()
                && snap.applied > self.snap.applied
            {
                p.since = now;
            }
            if fatal || self.pending.as_ref().is_some_and(|p| snap.applied >= p.seq) {
                self.pending = None;
            }
            let signed_out = matches!(snap.error, Some(PlayerError::SignInNeeded(_)));
            (self.snap, changed) = (snap, true);
            if signed_out {
                self.signed_out = true;
                self.shutdown();
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| now.saturating_duration_since(p.since) > HOLD_LIMIT)
        {
            (self.pending, changed) = (None, true);
        }
        changed
    }

    fn finish_start(&mut self) -> bool {
        let result = match self.starting.as_ref().map(Receiver::try_recv) {
            None | Some(Err(TryRecvError::Empty)) => return false,
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => Err(anyhow!("the player did not start")),
        };
        self.starting = None;
        match result {
            Ok(link) => {
                self.link = Some(link);
                self.flush();
            }
            Err(e) => {
                (self.pending, self.queued) = (None, Vec::new());
                if e.is::<SignedOut>() {
                    self.signed_out = true;
                } else {
                    self.snap.error = Some(PlayerError::LoadFailed(format!("{e:#}")));
                }
            }
        }
        true
    }

    fn restart(&mut self, signed_out: bool) {
        self.shutdown();
        self.starting = None;
        (self.signed_out, self.snap) = (signed_out, Snapshot::default());
        (self.pending, self.queued) = (None, Vec::new());
    }

    pub fn signed_in(&mut self) {
        self.restart(false);
    }

    pub fn sign_out(&mut self) {
        self.restart(true);
    }

    pub fn mark_signed_out(&mut self) {
        self.signed_out = true;
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
