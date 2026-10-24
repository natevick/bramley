use std::ffi::{CStr, CString, c_char};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use bramley_engine::{Command, PlayerError, QUEUE_SHOWN, Snapshot, Tokens};
use serde_json::{Value, json};

use crate::wire::{self, Failure, Progress, QueueGate};

unsafe extern "C" {
    fn bramley_mk_free(reply: *mut c_char);
    fn bramley_mk_authorization(request: bool) -> *mut c_char;
    fn bramley_mk_tokens(fresh: bool) -> *mut c_char;
    fn bramley_mk_command(json: *const c_char) -> *mut c_char;
    fn bramley_mk_snapshot(shown: i32) -> *mut c_char;
}

const SNAPSHOT_EVERY: Duration = Duration::from_millis(250);
const HEARTBEAT: Duration = Duration::from_secs(1);

fn take(reply: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(reply) }
        .to_string_lossy()
        .into_owned();
    unsafe { bramley_mk_free(reply) };
    text
}

pub fn authorization(request: bool) -> Result<String, Failure> {
    wire::authorization(&take(unsafe { bramley_mk_authorization(request) }))
}

pub fn tokens(fresh: bool) -> Result<Tokens, Failure> {
    wire::tokens(&take(unsafe { bramley_mk_tokens(fresh) }))
}

fn send(command: &Value) -> Result<(), Failure> {
    let text =
        CString::new(command.to_string()).map_err(|e| Failure::new("Command", &e.to_string()))?;
    wire::decode::<Value>(&take(unsafe { bramley_mk_command(text.as_ptr()) })).map(drop)
}

pub fn probe(request: bool) -> String {
    let summary = |result: Result<Value, Failure>| match result {
        Ok(value) => value,
        Err(e) => json!({"error": e.to_string()}),
    };
    let status = summary(authorization(request).map(Value::from));
    let tokens = summary(
        tokens(false)
            .map(|t| json!({"developerBytes": t.developer.len(), "userBytes": t.user.len()})),
    );
    json!({"authorization": status, "tokens": tokens}).to_string()
}

pub fn spawn(waker: impl Fn() + Send + 'static) -> Result<(Sender<Command>, Receiver<Snapshot>)> {
    let status = authorization(false)?;
    if status != "authorized" {
        return Err(anyhow!("Apple Music access is {status}"));
    }
    let (command_tx, commands) = channel();
    let (snapshots, snapshot_rx) = channel();
    std::thread::Builder::new()
        .name("bramley-musickit".into())
        .spawn(move || run(&commands, &snapshots, waker))?;
    Ok((command_tx, snapshot_rx))
}

fn run(commands: &Receiver<Command>, snapshots: &Sender<Snapshot>, waker: impl Fn()) {
    let (mut progress, mut gate, mut error, mut last) =
        (Progress::default(), QueueGate::default(), None, None);
    let mut sent_at = Instant::now();
    loop {
        match commands.recv_timeout(SNAPSHOT_EVERY) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(command) => {
                progress.record(&command);
                let outcome = wire::command(&command)
                    .filter(|_| gate.permits(&command))
                    .map(|wire| send(&wire));
                if let (Some(outcome), false) = (outcome, matches!(command, Command::TopUp(_))) {
                    gate.settle(&command, outcome.is_err());
                    error = outcome.err().map(|e| {
                        log::warn!("a MusicKit command failed: {e}");
                        PlayerError::Playback(e.to_string())
                    });
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let mut snap =
            match wire::snapshot(&take(unsafe { bramley_mk_snapshot(QUEUE_SHOWN as i32) })) {
                Ok(snap) => snap,
                Err(e) => {
                    let mut failed = Snapshot::default();
                    failed.error = Some(PlayerError::Playback(e.to_string()));
                    failed
                }
            };
        (snap.applied, snap.background_done) = (progress.applied, progress.background_done);
        snap.error = snap.error.or_else(|| error.clone());
        if last.as_ref() != Some(&snap) || sent_at.elapsed() >= HEARTBEAT {
            if snapshots.send(snap.clone()).is_err() {
                return;
            }
            waker();
            sent_at = Instant::now();
            last = Some(snap);
        }
    }
}
