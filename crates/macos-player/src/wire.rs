use std::fmt;

use bramley_engine::{Command, PlayState, Queue, QueueItem, Repeat, Snapshot, Tokens, redact};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Debug, PartialEq, Deserialize)]
pub struct Failure {
    r#type: String,
    message: String,
}

impl Failure {
    pub fn new(kind: &str, message: &str) -> Self {
        Failure {
            r#type: kind.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&redact(&format!("{}: {}", self.r#type, self.message), &[]))
    }
}

impl std::error::Error for Failure {}

#[derive(Deserialize)]
struct Row {
    id: String,
    title: Option<String>,
    duration: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire {
    state: String,
    position: Option<f64>,
    duration: Option<f64>,
    item_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    #[serde(default)]
    queue: Vec<Row>,
    queue_position: Option<usize>,
    #[serde(default)]
    queue_length: usize,
    #[serde(default)]
    shuffle: bool,
    #[serde(default)]
    repeat: String,
}

#[derive(Deserialize)]
struct Pair {
    developer: String,
    user: String,
}

#[derive(Deserialize)]
struct Auth {
    status: String,
}

pub fn decode<T: DeserializeOwned>(json: &str) -> Result<T, Failure> {
    let malformed = |e: serde_json::Error| Failure::new("MalformedReply", &e.to_string());
    let mut value: Value = serde_json::from_str(json).map_err(malformed)?;
    if let Some(error) = value.get_mut("error") {
        return Err(serde_json::from_value(error.take()).map_err(malformed)?);
    }
    serde_json::from_value(value).map_err(malformed)
}

pub fn snapshot(json: &str) -> Result<Snapshot, Failure> {
    let wire: Wire = decode(json)?;
    let state = match wire.state.as_str() {
        "playing" => PlayState::Playing,
        "paused" => PlayState::Paused,
        "loading" => PlayState::Loading,
        _ => PlayState::Stopped,
    };
    let mut snap = Snapshot::default();
    snap.item_id = wire.item_id;
    snap.title = wire.title;
    snap.artist = wire.artist;
    snap.position = wire.position;
    snap.duration = wire.duration;
    snap.state = state;
    snap.queue = wire
        .queue
        .into_iter()
        .map(|row| QueueItem {
            id: row.id,
            title: row.title,
            duration: row.duration,
        })
        .collect();
    snap.queue_position = wire.queue_position;
    snap.queue_length = wire.queue_length;
    snap.shuffle = wire.shuffle;
    snap.repeat = Repeat::parse(&wire.repeat).unwrap_or_default();
    Ok(snap)
}

pub fn tokens(json: &str) -> Result<Tokens, Failure> {
    let pair: Pair = decode(json)?;
    Ok(Tokens {
        developer: pair.developer,
        user: pair.user,
    })
}

pub fn authorization(json: &str) -> Result<String, Failure> {
    Ok(decode::<Auth>(json)?.status)
}

const BATCH: usize = 100;

pub fn is_library(id: &str) -> bool {
    ["i.", "l.", "p."]
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

pub fn batches(ids: &[String]) -> Vec<Value> {
    let (library, catalog): (Vec<&String>, Vec<&String>) =
        ids.iter().partition(|id| is_library(id));
    [(true, library), (false, catalog)]
        .into_iter()
        .flat_map(|(library, ids)| {
            ids.chunks(BATCH)
                .map(|chunk| json!({"library": library, "ids": chunk}))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn with_items(name: &str, queue: &Queue) -> Option<Value> {
    Some(match queue {
        Queue::Songs(ids) => {
            json!({"cmd": name, "kind": "songs", "ids": ids, "batches": batches(ids)})
        }
        Queue::Album(id) => {
            json!({"cmd": name, "kind": "album", "id": id, "library": is_library(id)})
        }
        Queue::Playlist(id) => {
            json!({"cmd": name, "kind": "playlist", "id": id, "library": is_library(id)})
        }
        _ => return None,
    })
}

pub fn command(command: &Command) -> Option<Value> {
    Some(match command {
        Command::SetQueue { queue, start } => {
            let mut set = with_items("setQueue", queue)?;
            set["start"] = json!(start);
            set
        }
        Command::Play => json!({"cmd": "play"}),
        Command::Pause => json!({"cmd": "pause"}),
        Command::Seek(seconds) => json!({"cmd": "seek", "seconds": seconds}),
        Command::Next => json!({"cmd": "next"}),
        Command::Previous => json!({"cmd": "previous"}),
        Command::Shuffle(on) => json!({"cmd": "shuffle", "on": on}),
        Command::Repeat(mode) => json!({"cmd": "repeat", "mode": mode.as_str()}),
        Command::PlayLater(queue) | Command::TopUp(queue) => with_items("append", queue)?,
        Command::PlayNext(queue) => with_items("playNext", queue)?,
        Command::JumpTo { index, id } => json!({"cmd": "jump", "index": index, "id": id}),
        _ => return None,
    })
}

#[derive(Default)]
pub struct Progress {
    pub applied: u64,
    pub background_done: u64,
}

impl Progress {
    pub fn record(&mut self, command: &Command) {
        match command {
            Command::TopUp(_) => self.background_done += 1,
            _ => self.applied += 1,
        }
    }
}
