use std::rc::Rc;

use serde_json::json;

use super::{FIXTURE, FakeCdm, FakeSink, fixture_segments};
use crate::Engine;
use crate::cdm::{CdmBackend, CdmEvent};

const PRELUDE: &str = r#"
globalThis.log = []; globalThis.done = false;
globalThis.L = x => log.push(x);
globalThis.outcome = (name, p) => p.then(() => L(name + ":ok"), e => L(name + ":" + e.name));
globalThis.later = ms => new Promise(r => setTimeout(r, ms));
"#;

pub fn engine() -> (Engine, FakeSink) {
    let cdm = FakeCdm::default();
    let sink = FakeSink::default();
    let s = sink.clone();
    let factory = move || {
        cdm.0.borrow_mut().events.push(CdmEvent::Initialized(true));
        Ok(Box::new(cdm.clone()) as Box<dyn CdmBackend>)
    };
    let e = Engine::with_backends(Box::new(factory), Rc::new(move || Box::new(s.clone()))).unwrap();
    e.eval_script("prelude.js", PRELUDE).unwrap();
    (e, sink)
}

pub fn run(e: &mut Engine, sink: &FakeSink, script: &str) -> Vec<String> {
    e.eval_script(
        "test.js",
        &format!("(async () => {{ {script} }})().catch(e => L('threw:' + e)).finally(() => {{ done = true; }});"),
    )
    .unwrap();
    for _ in 0..8000 {
        e.tick();
        sink.advance(480);
        if e.eval_json("done").unwrap() == json!(true) {
            return serde_json::from_value(e.eval_json("log").unwrap()).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!(
        "script never finished; log = {}",
        e.eval_json("log").unwrap()
    );
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn script(body: &str) -> String {
    let (_, media) = fixture_segments();
    format!(
        r#"
        const H = h => new Uint8Array(h.match(/../g).map(x => parseInt(x, 16)));
        const bytes = H("{all}");
        const mediaOnly = H("{seg}");
        const audio = document.createElement("audio");
        for (const t of ["emptied", "play", "playing", "pause", "waiting", "ended", "error"]) audio.addEventListener(t, () => L("ev:" + t));
        const fresh = () => {{
            const ms = new MediaSource();
            const opened = new Promise(r => ms.addEventListener("sourceopen", r, {{ once: true }}));
            return {{ ms, opened }};
        }};
        const fill = async (ms, opened, data) => {{
            await opened;
            const sb = ms.addSourceBuffer('audio/mp4; codecs="mp4a.40.2"');
            sb.appendBuffer(data);
            await new Promise(r => sb.addEventListener("updateend", r, {{ once: true }}));
            return sb;
        }};
        {body}
    "#,
        all = hex(FIXTURE),
        seg = hex(&media[0]),
    )
}

fn has(log: &[String], entry: &str) -> bool {
    log.iter().any(|l| l == entry)
}

const PLAY_THEN_LOAD: &str = r#"
    const a = fresh();
    audio.src = URL.createObjectURL(a.ms);
    await fill(a.ms, a.opened, bytes);
    a.ms.endOfStream();
    await audio.play();
    await later(30);
    PAUSE_FIRST
    const b = fresh();
    audio.src = URL.createObjectURL(b.ms);
    L("paused-after-load:" + audio.paused);
    const r = outcome("second-play", audio.play());
    await fill(b.ms, b.opened, bytes);
    b.ms.endOfStream();
    await r;
"#;

#[test]
fn loading_a_new_src_while_playing_does_not_reject_the_next_play() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(&PLAY_THEN_LOAD.replace("PAUSE_FIRST", "")),
    );
    assert!(has(&log, "second-play:ok"), "{log:?}");
    assert!(has(&log, "paused-after-load:true"), "{log:?}");
    assert_eq!(
        log.iter().filter(|l| *l == "ev:pause").count(),
        0,
        "{log:?}"
    );
}

#[test]
fn guard_pausing_before_a_new_src_lets_the_next_play_resolve() {
    let (mut e, sink) = engine();
    let pause_first = r#"audio.pause(); await new Promise(r => audio.addEventListener("pause", r, { once: true }));"#;
    let log = run(
        &mut e,
        &sink,
        &script(&PLAY_THEN_LOAD.replace("PAUSE_FIRST", pause_first)),
    );
    assert!(has(&log, "second-play:ok"), "{log:?}");
}

#[test]
fn a_playing_play_queued_before_a_load_does_not_resolve_the_new_play() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            await fill(a.ms, a.opened, bytes);
            await audio.play();
            outcome("replayed", audio.play());
            const b = fresh();
            audio.src = URL.createObjectURL(b.ms);
            let settled = false;
            audio.play().then(() => { settled = true; }, () => { settled = true; });
            await later(30);
            L("new-play-settled-without-data:" + settled);
        "#,
        ),
    );
    assert!(has(&log, "replayed:AbortError"), "{log:?}");
    assert!(has(&log, "new-play-settled-without-data:false"), "{log:?}");
}

#[test]
fn a_valid_src_clears_the_error_of_an_unsupported_one() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            audio.src = "https://example.invalid/preview.m4a";
            await later(10);
            L("error-after-bad-src:" + (audio.error && audio.error.code));
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            L("error-after-good-src:" + (audio.error && audio.error.code));
            const r = outcome("play-after-good-src", audio.play());
            await fill(a.ms, a.opened, bytes);
            a.ms.endOfStream();
            await r;
        "#,
        ),
    );
    assert!(has(&log, "error-after-bad-src:4"), "{log:?}");
    assert!(has(&log, "error-after-good-src:null"), "{log:?}");
    assert!(has(&log, "play-after-good-src:ok"), "{log:?}");
}

#[test]
fn an_append_error_before_metadata_is_a_src_not_supported_error() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            await a.opened;
            const r = outcome("play", audio.play());
            const sb = a.ms.addSourceBuffer('audio/mp4; codecs="mp4a.40.2"');
            sb.appendBuffer(mediaOnly);
            await new Promise(r => sb.addEventListener("updateend", r, { once: true }));
            await r;
            await later(20);
            L("ms:" + a.ms.readyState + " error:" + (audio.error && audio.error.code));
        "#,
        ),
    );
    assert!(has(&log, "ev:error"), "{log:?}");
    assert!(has(&log, "play:NotSupportedError"), "{log:?}");
    assert!(has(&log, "ms:ended error:4"), "{log:?}");
}

#[test]
fn a_corrupt_segment_mid_playback_is_a_decode_error_not_the_end() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            const sb = await fill(a.ms, a.opened, bytes);
            await audio.play();
            const bx = (t, ...parts) => { const body = parts.flatMap(x => Array.from(x)); const n = body.length + 8; return [n >>> 24, (n >> 16) & 255, (n >> 8) & 255, n & 255, ...Array.from(t, c => c.charCodeAt(0)), ...body]; };
            const tfhd = bx("tfhd", [0,0,0,0, 0,0,0,1]);
            const trun = bx("trun", [0,0,2,1, 0,0,0,1, 0x7f,0xff,0xff,0x00, 0,0,0,10]);
            sb.appendBuffer(new Uint8Array([...bx("moof", bx("traf", tfhd, trun)), ...bx("mdat", new Array(10).fill(0))]));
            await new Promise(r => audio.addEventListener("error", r, { once: true }));
            await later(200);
            L("error:" + audio.error.code + " ended:" + audio.ended);
        "#,
        ),
    );
    assert!(has(&log, "error:3 ended:false"), "{log:?}");
    assert!(!has(&log, "ev:ended"), "{log:?}");
}

#[test]
fn abort_cancels_a_queued_append() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            await a.opened;
            const sb = a.ms.addSourceBuffer('audio/mp4; codecs="mp4a.40.2"');
            const seen = [];
            for (const t of ["updatestart", "update", "abort", "updateend"]) sb.addEventListener(t, () => seen.push(t));
            sb.appendBuffer(bytes);
            sb.abort();
            L("updating-after-abort:" + sb.updating);
            await later(30);
            L(seen.join(",") + " buffered:" + sb.buffered.length);
        "#,
        ),
    );
    assert!(has(&log, "updating-after-abort:false"), "{log:?}");
    assert!(
        has(&log, "updatestart,abort,updateend buffered:0"),
        "{log:?}"
    );
}

#[test]
fn he_aac_is_not_advertised() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        r#"
        for (const c of ["mp4a.40.2", "mp4a.40.5", "mp4a.40.29"]) {
            L(c + ":" + MediaSource.isTypeSupported('audio/mp4; codecs="' + c + '"') + ":" + (document.createElement("audio").canPlayType('audio/mp4; codecs="' + c + '"') || "no"));
        }
    "#,
    );
    assert_eq!(
        log,
        [
            "mp4a.40.2:true:probably",
            "mp4a.40.5:false:no",
            "mp4a.40.29:false:no"
        ]
    );
}
