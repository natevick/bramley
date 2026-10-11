use std::io::{ErrorKind, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use fastframe_instance::Slot;
use serde_json::{Value, json};

use crate::playback::{Action, Banner, Playback, play_or_pause};

pub const APP_ID: &str = "bramley";
pub const USAGE: i32 = 2;
pub const NOT_RUNNING: i32 = 3;
pub const REFUSED: i32 = 5;
const APPLY_TIME: Duration = Duration::from_secs(3);
const REPLY_MARGIN: Duration = Duration::from_secs(1);
const LIBRARY_VIA_MCP: &str =
    "your library is shared only with an agent you connect through `bramley mcp`";
const AGENT: &str = "mcp";
const LATE: &str = "the app did not answer in time; the command was not applied";
const COMMANDS: &str = "usage: bramley [play [id] | pause | toggle | next | prev | seek <seconds> | volume <0-100> | now | queue add <id> | search <term> | sign-in | sign-out | mcp]";

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Show,
    Now,
    Toggle,
    Search(String),
    Library(bramley_api::Shelf),
    Agent,
    SignIn,
    SignOut,
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
        ["library", "playlists"] => Request::Library(bramley_api::Shelf::Playlists),
        ["sign-in"] => Request::SignIn,
        ["sign-out"] => Request::SignOut,
        _ => bail!("{COMMANDS}"),
    })
}

pub(crate) fn catalog_id(id: &str) -> Result<String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        bail!("{id:?} is not a catalog id");
    }
    Ok(id.to_owned())
}

pub fn shared_fallback(dir: &std::path::Path, tmp: &std::path::Path) -> bool {
    dir == tmp.join(format!("{APP_ID}-instance"))
}

pub fn slot() -> Result<Slot> {
    let slot = Slot::new(APP_ID);
    if shared_fallback(slot.dir(), &std::env::temp_dir()) {
        bail!("no home or runtime directory for a private instance slot");
    }
    Ok(slot)
}

fn unix_ms() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |d| d.as_millis() as u64)
}

pub fn wire(args: &[String]) -> String {
    let deadline_ms = unix_ms() + APPLY_TIME.as_millis() as u64;
    json!({ "args": args, "deadline_ms": deadline_ms }).to_string()
}

pub fn agent_wire(args: &[String]) -> String {
    let deadline_ms = unix_ms() + APPLY_TIME.as_millis() as u64;
    json!({ "args": args, "deadline_ms": deadline_ms, "via": AGENT }).to_string()
}

pub fn deadline(client_ms: u64, now_ms: u64, now: Instant) -> Instant {
    now + APPLY_TIME.min(Duration::from_millis(client_ms.saturating_sub(now_ms)))
}

pub fn outcome(reply: &str) -> Result<(), (i32, String)> {
    let reply: Value = serde_json::from_str(reply).unwrap_or_default();
    let text = |key: &str| reply[key].as_str().map(str::to_owned);
    match (text("error"), text("refused"), reply["seq"].as_u64()) {
        (Some(why), _, _) => Err((1, why)),
        (None, Some(why), _) => Err((REFUSED, why)),
        (None, None, _) if reply["results"].is_object() => Ok(()),
        (None, None, None) => Err((1, "the running Bramley sent an unreadable reply".into())),
        (None, None, Some(_)) => Ok(()),
    }
}

pub fn send(slot: &Slot, line: &str) -> Result<String, (i32, String)> {
    slot.send(line).map_err(|e| match e.kind() {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => {
            let dir = slot.dir().display();
            let why = format!("Bramley is not running (nothing at {dir}); start it first");
            (NOT_RUNNING, why)
        }
        _ => (
            1,
            format!("the running Bramley did not take the command: {e}"),
        ),
    })
}

pub fn client(slot: &Slot, args: &[String], out: &mut impl Write, err: &mut impl Write) -> i32 {
    let mut fail = |code, message: &str| {
        let _ = writeln!(err, "bramley: {message}");
        code
    };
    let request = match parse(args) {
        Err(e) => return fail(USAGE, &format!("{e}")),
        Ok(Request::Library(_)) => return fail(USAGE, LIBRARY_VIA_MCP),
        Ok(request) => request,
    };
    match send(slot, &wire(args)) {
        Ok(reply) => {
            let _ = writeln!(out, "{reply}");
            let url = serde_json::from_str::<Value>(&reply).unwrap_or_default()["signInUrl"].take();
            if let (Request::SignIn, Some(url)) = (request, url.as_str()) {
                let _ = writeln!(out, "{url}");
            }
            match outcome(&reply) {
                Ok(()) => 0,
                Err((code, why)) => fail(code, &why),
            }
        }
        Err((code, why)) => fail(code, &why),
    }
}

pub fn api_call(request: &Request) -> Option<bramley_api::Call> {
    match request {
        Request::Search(term) => Some(bramley_api::Call::Search(term.clone())),
        Request::Library(shelf) => Some(bramley_api::Call::Library(*shelf)),
        _ => None,
    }
}

pub fn api_reply(found: bramley_api::Result<bramley_api::Reply>) -> String {
    match found {
        Ok(bramley_api::Reply::Search(results)) => json!({ "results": results }),
        Ok(bramley_api::Reply::Page(page)) => json!({ "results": page }),
        Ok(_) => json!({ "error": "the API worker answered a different call" }),
        Err(bramley_api::Error::SignInNeeded) => json!({ "refused": "sign in needed" }),
        Err(e) => json!({ "error": e.to_string() }),
    }
    .to_string()
}

pub fn handler(
    inbox: Sender<Envelope>,
    waker: Arc<OnceLock<egui::Context>>,
) -> impl FnMut(&str) -> Option<String> + Send + 'static {
    move |line| {
        let line: Value = serde_json::from_str(line).ok()?;
        let args: Vec<String> = serde_json::from_value(line["args"].clone()).ok()?;
        let request = parse(&args).ok()?;
        let late = json!({ "error": LATE }).to_string();
        let now = Instant::now();
        let deadline = deadline(line["deadline_ms"].as_u64()?, unix_ms(), now);
        if deadline <= now {
            return Some(late);
        }
        if line["via"] == AGENT {
            inbox.send((Request::Agent, deadline, channel().0)).ok()?;
        }
        let (reply, answer) = channel();
        inbox.send((request, deadline, reply)).ok()?;
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }
        let wait = deadline.saturating_duration_since(Instant::now()) + REPLY_MARGIN;
        Some(answer.recv_timeout(wait).unwrap_or(late))
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
        Request::Show | Request::Now | Request::SignIn | Request::SignOut | Request::Agent => {
            return None;
        }
        Request::Search(_) | Request::Library(_) => {
            return Some(("error", "API requests run on the API worker"));
        }
        Request::Toggle => play_or_pause(playback.shown().state),
        Request::Do(action) => action,
    };
    if playback.apply(action) {
        None
    } else if playback.banner() == Some(Banner::SignInNeeded) {
        refused("sign in needed: nothing was played")
    } else {
        refused("nothing is playing yet; play a song first")
    }
}
