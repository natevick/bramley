use std::io::{ErrorKind, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use fastframe_instance::Slot;
use serde_json::{Value, json};

use crate::playback::{Action, Banner, Playback, play_or_pause};

pub const APP_ID: &str = "bramley";
pub const USAGE: i32 = 2;
pub const NOT_RUNNING: i32 = 3;
pub const UNAVAILABLE: i32 = 4;
pub const REFUSED: i32 = 5;
const APPLY_TIME: Duration = Duration::from_secs(3);
const ANSWER_TIME: Duration = Duration::from_secs(4);
const LATE: &str = "the app did not answer in time; the command was not applied";
const COMMANDS: &str = "usage: bramley [play [id] | pause | toggle | next | prev | seek <seconds> | volume <0-100> | now | queue add <id> | search <term>]";

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Show,
    Now,
    Toggle,
    Search(String),
    Do(Action),
}

pub type Envelope = (Request, Instant, Sender<String>);
pub type Inbox = Receiver<Envelope>;

pub fn is_command(args: &[String]) -> bool {
    args.first().is_some_and(|first| !first.starts_with('-'))
}

pub fn parse(args: &[String]) -> Result<Request> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(match words.as_slice() {
        [] => Request::Show,
        ["now"] => Request::Now,
        ["play"] => Request::Do(Action::Play),
        ["play", id] => Request::Do(Action::PlaySong(catalog_id(id)?)),
        ["pause"] => Request::Do(Action::Pause),
        ["toggle"] => Request::Toggle,
        ["next"] => Request::Do(Action::Next),
        ["prev"] => Request::Do(Action::Previous),
        ["seek", s] => match s.parse::<f64>() {
            Ok(s) if s.is_finite() && s >= 0.0 => Request::Do(Action::Seek(s)),
            _ => bail!("seek needs a number of seconds, not {s:?}"),
        },
        ["volume", v] => match v.parse::<u8>() {
            Ok(v) if v <= 100 => Request::Do(Action::Volume(f32::from(v) / 100.0)),
            _ => bail!("volume needs a whole number from 0 to 100, not {v:?}"),
        },
        ["queue", "add", id] => Request::Do(Action::QueueAdd(catalog_id(id)?)),
        ["search", term @ ..] if !term.is_empty() => Request::Search(term.join(" ")),
        _ => bail!("{COMMANDS}"),
    })
}

fn catalog_id(id: &str) -> Result<String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        bail!("{id:?} is not a catalog id");
    }
    Ok(id.to_owned())
}

pub fn wire(args: &[String]) -> String {
    json!(args).to_string()
}

pub fn client(slot: &Slot, args: &[String], out: &mut impl Write, err: &mut impl Write) -> i32 {
    let mut fail = |code, message: &str| {
        let _ = writeln!(err, "bramley: {message}");
        code
    };
    match parse(args) {
        Err(e) => return fail(USAGE, &format!("{e}")),
        Ok(Request::Search(_)) => {
            return fail(UNAVAILABLE, "search is not yet available in this build");
        }
        Ok(_) => {}
    }
    match slot.send(&wire(args)) {
        Ok(reply) => {
            let _ = writeln!(out, "{reply}");
            let parsed = serde_json::from_str::<Value>(&reply).unwrap_or_default();
            match (parsed["refused"].as_str(), parsed["error"].as_str()) {
                (Some(why), _) => fail(REFUSED, why),
                (None, Some(why)) => fail(1, why),
                (None, None) => 0,
            }
        }
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
            let dir = slot.dir().display();
            fail(
                NOT_RUNNING,
                &format!("Bramley is not running (nothing at {dir}); start it first"),
            )
        }
        Err(e) => fail(
            1,
            &format!("the running Bramley did not take the command: {e}"),
        ),
    }
}

pub fn handler(
    inbox: Sender<Envelope>,
    waker: Arc<OnceLock<egui::Context>>,
) -> impl FnMut(&str) -> Option<String> + Send + 'static {
    move |line| {
        let args: Vec<String> = serde_json::from_str(line).ok()?;
        let request = parse(&args).ok()?;
        let (reply, answer) = channel();
        inbox
            .send((request, Instant::now() + APPLY_TIME, reply))
            .ok()?;
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }
        Some(
            answer
                .recv_timeout(ANSWER_TIME)
                .unwrap_or_else(|_| json!({ "error": LATE }).to_string()),
        )
    }
}

pub fn command(
    playback: &mut Playback,
    request: Request,
    deadline: Instant,
) -> Option<(&'static str, &'static str)> {
    let refused = |why| Some(("refused", why));
    let action = match request {
        _ if Instant::now() > deadline => return Some(("error", LATE)),
        Request::Show | Request::Now => return None,
        Request::Search(_) => return refused("search is not yet available in this build"),
        Request::Toggle => play_or_pause(playback.shown().state),
        Request::Do(action) => action,
    };
    if playback.apply(action) {
        None
    } else if playback.banner() == Some(Banner::SignInNeeded) {
        refused("sign in needed: nothing was played")
    } else {
        refused("nothing is playing; start a song with bramley play <id>")
    }
}
