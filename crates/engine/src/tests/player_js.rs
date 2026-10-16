use std::time::Duration;

use serde_json::{Value as J, json};

use super::FakeSink;
use super::element::engine;
use crate::Engine;
use crate::player::{Command, PLAYER, Queue, Repeat, command_js};

const FAKE_MK: &str = r#"
globalThis.mklog = [];
globalThis.storefrontCalls = 0;
globalThis.storefrontReply = async () => ({});
globalThis.mkError = (code, status) => Object.assign(new Error(code), { name: code, reason: code, errorCode: code, data: { status } });
const later = (ms, f) => new Promise(r => setTimeout(() => r(f && f()), ms));
globalThis.fakeM = { isAuthorized: true, previewOnly: false, api: { music: () => { storefrontCalls++; return storefrontReply(); } }, bitrate: 0, q: null, handlers: {},
  addEventListener(ev, f) { (this.handlers[ev] = this.handlers[ev] || []).push(f); },
  fire(ev) { (this.handlers[ev] || []).forEach(f => f({})); },
  setQueue(o) {
    const id = o.songs[0];
    mklog.push("setQueue-start " + id);
    if (id === "HUNG") return new Promise(() => {});
    if (id === "BAD") return Promise.reject(new Error("NOT_FOUND"));
    return later(id === "A" ? 80 : 10, () => { fakeM.q = id; mklog.push("setQueue-done " + id); });
  },
  play() { mklog.push("play q=" + fakeM.q); return Promise.resolve(); },
  pause() { mklog.push("pause"); return Promise.resolve(); },
  playLater(o) { mklog.push("playLater " + o.songs.join(",") + " q=" + fakeM.q); return Promise.resolve(); },
  stop() { mklog.push("stop"); return Promise.resolve(); },
  skipToNextItem() { mklog.push("next-start"); return later(50, () => mklog.push("next-done")); },
  seekToTime() { return Promise.reject(new Error("seek while loading")); },
};
globalThis.MusicKit = { configure: async () => {}, getInstance: () => fakeM, PlaybackBitrate: { HIGH: 256 } };
"#;

fn player(wait_ms: u32) -> (Engine, FakeSink) {
    player_with_deadline(wait_ms, 10_000)
}

fn player_with_deadline(wait_ms: u32, storefront_deadline_ms: u32) -> (Engine, FakeSink) {
    let (e, sink) = engine();
    e.eval_script("fake.js", FAKE_MK).unwrap();
    let config = format!(
        "globalThis.__bramley_player = {{ developerToken: 'x.' + btoa('{{\"iss\":\"T\"}}') + '.y', userToken: 'u', musickitUrl: 'http://127.0.0.1:9/none.js', build: '0', commandWaitMs: {wait_ms}, storefrontDeadlineMs: {storefront_deadline_ms}, queueShown: 4 }};"
    );
    e.eval_script("config.js", &config).unwrap();
    e.eval_script("player.js", PLAYER).unwrap();
    (e, sink)
}

fn ready(e: &mut Engine) {
    e.eval_script(
        "loaded",
        "document.dispatchEvent(new Event('musickitloaded'));",
    )
    .unwrap();
    e.run_for(Duration::from_millis(30));
}

fn boot_for(e: &mut Engine, reply: &str, ms: u64) {
    e.eval_script("reply", &format!("storefrontReply = {reply};"))
        .unwrap();
    e.eval_script(
        "loaded",
        "document.dispatchEvent(new Event('musickitloaded'));",
    )
    .unwrap();
    e.run_for(Duration::from_millis(ms));
}

fn field(e: &Engine, name: &str) -> J {
    e.eval_json(&format!("__player.state().{name}")).unwrap()
}

fn send(e: &mut Engine, commands: &[Command], settle_ms: u64) {
    for c in commands {
        e.eval_script("command", &command_js(c)).unwrap();
    }
    e.run_for(Duration::from_millis(settle_ms));
}

fn songs(id: &str) -> Command {
    Command::SetQueue {
        queue: Queue::Songs(vec![id.into()]),
        start: 0,
    }
}

fn log(e: &Engine) -> J {
    e.eval_json("mklog.splice(0)").unwrap()
}

#[test]
fn commands_wait_for_sign_in_then_run_in_arrival_order() {
    let (mut e, _sink) = player(1000);
    send(&mut e, &[Command::Pause], 50);
    assert_eq!(log(&e), json!([]));
    ready(&mut e);
    assert_eq!(log(&e), json!(["pause"]));
    send(&mut e, &[Command::Next, Command::Pause], 300);
    assert_eq!(log(&e), json!(["next-start", "next-done", "pause"]));
    assert_eq!(e.eval_json("__player.state().applied").unwrap(), 3);
}

#[test]
fn a_newer_set_queue_supersedes_one_still_waiting_and_play_uses_it() {
    let (mut e, _sink) = player(1000);
    ready(&mut e);
    send(
        &mut e,
        &[songs("A"), songs("B"), songs("C"), Command::Play],
        400,
    );
    assert_eq!(
        log(&e),
        json!([
            "setQueue-start A",
            "setQueue-done A",
            "setQueue-start C",
            "setQueue-done C",
            "play q=C"
        ])
    );
    send(&mut e, &[songs("A"), songs("B"), Command::Play], 400);
    assert_eq!(
        log(&e),
        json!([
            "setQueue-start A",
            "setQueue-done A",
            "setQueue-start B",
            "setQueue-done B",
            "play q=B"
        ])
    );
    assert_eq!(e.eval_json("__player.state().applied").unwrap(), 7);
}

#[test]
fn a_hung_last_command_is_never_applied_until_another_command_arrives() {
    let (mut e, _sink) = player(50);
    ready(&mut e);
    send(&mut e, &[songs("HUNG")], 500);
    assert_eq!(e.eval_json("__player.state().applied").unwrap(), 0);
    send(&mut e, &[Command::Pause], 200);
    assert_eq!(e.eval_json("__player.state().applied").unwrap(), 2);
}

#[test]
fn a_hung_set_queue_holds_later_commands_only_until_the_bound() {
    let (mut e, _sink) = player(150);
    ready(&mut e);
    send(&mut e, &[songs("HUNG"), Command::Pause, Command::Play], 40);
    assert_eq!(log(&e), json!(["setQueue-start HUNG"]));
    e.run_for(Duration::from_millis(450));
    assert_eq!(log(&e), json!(["pause", "play q=null"]));
}

#[test]
fn playback_errors_survive_skipped_steps_and_clear_on_the_next_success() {
    let (mut e, _sink) = player(1000);
    ready(&mut e);
    let error = |e: &Engine| e.eval_json("__player.state().playbackError").unwrap();
    send(&mut e, &[songs("BAD"), Command::Play], 100);
    assert_eq!(error(&e), "setQueue: Error NOT_FOUND");
    send(&mut e, &[songs("B"), Command::Seek(10.0)], 100);
    assert_eq!(error(&e), "seek: Error seek while loading");
    send(&mut e, &[Command::Pause], 50);
    assert_eq!(error(&e), J::Null);
    assert_eq!(
        log(&e),
        json!([
            "setQueue-start BAD",
            "setQueue-start B",
            "setQueue-done B",
            "pause"
        ])
    );
}

#[test]
fn losing_sign_in_or_a_preview_only_account_stops_playback() {
    let cases = [
        (
            "fakeM.isAuthorized = false; fakeM.fire('authorizationStatusDidChange');",
            json!(["stop"]),
            "isAuthorized became false",
        ),
        (
            "fakeM.previewOnly = true;",
            json!([]),
            "previewOnly is true",
        ),
    ];
    for (flip, stopped_by_event, reason) in cases {
        let (mut e, _sink) = player(1000);
        ready(&mut e);
        send(&mut e, &[songs("B"), Command::Play], 100);
        log(&e);
        e.eval_script("flip", flip).unwrap();
        e.run_for(Duration::from_millis(50));
        assert_eq!(log(&e), stopped_by_event);
        send(&mut e, &[Command::Play, Command::Pause], 100);
        let state = e.eval_json("__player.state().signInNeeded").unwrap();
        assert_eq!(state, reason);
        let rest = if stopped_by_event == json!([]) {
            json!(["stop", "pause"])
        } else {
            json!(["pause"])
        };
        assert_eq!(log(&e), rest);
    }
}

#[test]
fn play_later_appends_after_the_queue_and_is_skipped_once_the_queue_failed() {
    let (mut e, _sink) = player(1000);
    ready(&mut e);
    let later = Command::PlayLater(Queue::Songs(vec!["L".into()]));
    send(&mut e, &[songs("A"), later.clone()], 300);
    assert_eq!(
        log(&e),
        json!(["setQueue-start A", "setQueue-done A", "playLater L q=A"])
    );
    send(&mut e, &[songs("BAD"), later], 300);
    assert_eq!(log(&e), json!(["setQueue-start BAD"]));
}

const FAKE_QUEUE: &str = r#"
const item = id => ({ id, title: "T" + id, attributes: { durationInMillis: 1000 } });
fakeM.queue = { position: 0, list: [item("A")], get length() { return this.list.length; }, get items() { return this.list.slice(); } };
fakeM.playLater = async o => { fakeM.queue.list.push(...o.songs.map(item)); };
fakeM.queue.list.push({ id: 7 });
fakeM.playNext = async o => { fakeM.queue.list.splice(fakeM.queue.position + 1, 0, ...o.songs.map(item)); };
fakeM.changeToMediaAtIndex = async i => { mklog.push("change " + i); fakeM.queue.position = i; };
"#;

#[test]
fn the_queue_window_reads_the_public_queue_and_jumps_only_to_the_item_shown() {
    let (mut e, _sink) = player(1000);
    ready(&mut e);
    e.eval_script("queue.js", FAKE_QUEUE).unwrap();
    let one = |id: &str| Queue::Songs(vec![id.into()]);
    let (later, next) = (Command::PlayLater, Command::PlayNext);
    send(
        &mut e,
        &[
            later(one("B")),
            later(one("C")),
            next(one("X")),
            later(one("D")),
        ],
        300,
    );
    let queue = |e: &Engine| e.eval_json("__player.state().queue").unwrap();
    let ids = |e: &Engine| {
        queue(e)["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&e),
        [json!("A"), json!("X"), J::Null, json!("B")],
        "a bad entry keeps its slot"
    );
    assert_eq!(
        (queue(&e)["position"].clone(), queue(&e)["length"].clone()),
        (json!(0), json!(6))
    );
    assert_eq!(
        queue(&e)["items"][1],
        json!({"id": "X", "title": "TX", "durationInMillis": 1000})
    );
    let jump = |index, id: &str| Command::JumpTo {
        index,
        id: id.into(),
    };
    send(&mut e, &[jump(4, "B"), jump(4, "C")], 300);
    assert_eq!(log(&e), json!(["change 4"]));
    assert_eq!(ids(&e), ["C", "D"]);
}

const FAKE_MODES: &str = r#"
MusicKit.PlayerRepeatMode = { none: 0, one: 1, all: 2 };
let shuffleMode = 0;
fakeM.repeatMode = 0;
Object.defineProperty(fakeM, "shuffleMode", {
  get: () => shuffleMode,
  set: v => { shuffleMode = v; const [now, ...rest] = fakeM.queue.list; fakeM.queue.list = [now, ...rest.reverse()]; },
});
"#;

#[test]
fn shuffle_and_repeat_use_musickits_public_modes_and_the_queue_window_follows_the_shuffled_order() {
    let (mut e, _sink) = player(1000);
    ready(&mut e);
    e.eval_script("queue.js", FAKE_QUEUE).unwrap();
    e.eval_script("modes.js", FAKE_MODES).unwrap();
    let one = |id: &str| Queue::Songs(vec![id.into()]);
    send(
        &mut e,
        &[
            Command::PlayLater(one("B")),
            Command::PlayLater(one("C")),
            Command::PlayLater(one("D")),
        ],
        300,
    );
    let modes = |e: &Engine| (field(e, "shuffleMode"), field(e, "repeatMode"));
    let ids = |e: &Engine| {
        let items = field(e, "queue")["items"].clone();
        items
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(modes(&e), (json!(0), json!(0)));
    let unshuffled = [json!("A"), J::Null, json!("B"), json!("C")];
    assert_eq!(ids(&e), unshuffled);
    send(&mut e, &[Command::Shuffle(true)], 100);
    assert_eq!(
        (modes(&e).0, ids(&e)),
        (
            json!(1),
            vec![json!("A"), json!("D"), json!("C"), json!("B")]
        )
    );
    for (mode, want) in [(Repeat::One, 1), (Repeat::All, 2), (Repeat::Off, 0)] {
        send(&mut e, &[Command::Repeat(mode)], 100);
        assert_eq!(modes(&e).1, json!(want), "{mode:?}");
    }
    send(&mut e, &[Command::Shuffle(false)], 100);
    assert_eq!((modes(&e).0, ids(&e)), (json!(0), unshuffled.to_vec()));
}

const SLOW_ADD: &str = r#"
fakeM.q = "OLD";
fakeM.playLater = o => later(300, () => mklog.push("added " + o.songs + " to " + fakeM.q));
"#;

#[test]
fn a_new_queue_waits_for_a_top_up_in_flight_but_a_pause_does_not() {
    let (mut e, _sink) = player(100);
    ready(&mut e);
    e.eval_script("slow.js", SLOW_ADD).unwrap();
    let top_up = Command::TopUp(Queue::Songs(vec!["T".into()]));
    send(&mut e, &[top_up.clone(), Command::Pause], 100);
    assert_eq!(
        log(&e),
        json!(["pause"]),
        "the pause did not queue behind the top-up"
    );
    e.run_for(Duration::from_millis(400));
    assert_eq!(log(&e), json!(["added T to OLD"]));
    assert_eq!(e.eval_json("__player.state().background").unwrap(), 1);
    send(&mut e, &[top_up, songs("NEW")], 600);
    let order = json!(["added T to OLD", "setQueue-start NEW", "setQueue-done NEW"]);
    assert_eq!(
        log(&e),
        order,
        "the top-up landed before the queue it would have joined"
    );
}

#[test]
fn a_transient_storefront_failure_is_retried_twice_then_reports_a_load_failure_not_sign_in() {
    let cases = [
        (
            "async () => { throw mkError('SERVICE_UNAVAILABLE', 503); }",
            "503",
        ),
        (
            "async () => { throw new TypeError('fetch failed'); }",
            "network error",
        ),
    ];
    for (reply, seen) in cases {
        let (mut e, _sink) = player(1000);
        boot_for(&mut e, reply, 1600);
        assert_eq!(e.eval_json("storefrontCalls").unwrap(), 3, "{seen}");
        assert_eq!(field(&e, "signInNeeded"), J::Null, "{seen}");
        let why = format!("the Apple Music storefront check failed ({seen})");
        assert_eq!(field(&e, "loadFailed"), why.as_str());
    }
}

#[test]
fn a_storefront_failure_that_clears_on_retry_still_signs_in_and_plays() {
    let (mut e, _sink) = player(1000);
    let reply =
        "async () => { if (storefrontCalls < 2) throw mkError('SERVER_ERROR', 500); return {}; }";
    boot_for(&mut e, reply, 600);
    assert_eq!(e.eval_json("storefrontCalls").unwrap(), 2);
    send(&mut e, &[songs("A"), Command::Play], 300);
    assert_eq!(
        log(&e),
        json!(["setQueue-start A", "setQueue-done A", "play q=A"])
    );
}

#[test]
fn a_storefront_auth_rejection_asks_for_sign_in_without_retrying() {
    let cases = [
        ("UNAUTHORIZED_ERROR", 401, "engine-auth-401"),
        ("ACCESS_DENIED", 403, "engine-auth-403"),
    ];
    for (code, status, reason) in cases {
        let (mut e, _sink) = player(1000);
        let reply = format!("async () => {{ throw mkError('{code}', {status}); }}");
        boot_for(&mut e, &reply, 1600);
        assert_eq!(e.eval_json("storefrontCalls").unwrap(), 1, "{code}");
        assert_eq!(field(&e, "signInNeeded"), reason);
        assert_eq!(field(&e, "loadFailed"), J::Null, "{code}");
    }
}

#[test]
fn an_unauthorized_user_asks_for_sign_in_without_calling_the_storefront() {
    let (mut e, _sink) = player(1000);
    e.eval_script("off", "fakeM.isAuthorized = false;").unwrap();
    boot_for(&mut e, "async () => ({})", 100);
    assert_eq!(e.eval_json("storefrontCalls").unwrap(), 0);
    assert_eq!(field(&e, "signInNeeded"), "engine-unauthorized");
}

#[test]
fn the_storefront_check_gives_up_at_one_overall_deadline_whether_it_hangs_or_keeps_failing() {
    let cases = [
        ("() => new Promise(() => {})", 1, "timed out"),
        (
            "async () => { throw mkError('SERVICE_UNAVAILABLE', 503); }",
            2,
            "503",
        ),
    ];
    for (reply, calls, seen) in cases {
        let (mut e, _sink) = player_with_deadline(1000, 600);
        boot_for(&mut e, reply, 1000);
        assert_eq!(e.eval_json("storefrontCalls").unwrap(), calls, "{seen}");
        assert_eq!(field(&e, "signInNeeded"), J::Null, "{seen}");
        let why = format!("the Apple Music storefront check failed ({seen})");
        assert_eq!(field(&e, "loadFailed"), why.as_str());
    }
}
