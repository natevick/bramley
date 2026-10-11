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
    let sink = FakeSink::default();
    let s = sink.clone();
    let factory = |_| {
        let cdm = FakeCdm::default();
        cdm.0.borrow_mut().events.push(CdmEvent::Initialized(true));
        Ok(Box::new(cdm) as Box<dyn CdmBackend>)
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

#[test]
fn detach_removes_source_buffers_and_unreachable_objects_release_native_state() {
    let (mut e, sink) = engine();
    let keys = r#"navigator.requestMediaKeySystemAccess("com.widevine.alpha", [{ audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }] }])"#;
    run(
        &mut e,
        &sink,
        &script(&format!(
            r#"
            for (let i = 0; i < 3; i++) {{
                const a = fresh();
                const url = URL.createObjectURL(a.ms);
                audio.src = url;
                URL.revokeObjectURL(url);
                await fill(a.ms, a.opened, bytes);
                await audio.setMediaKeys(await (await {keys}).createMediaKeys());
            }}
            audio.removeAttribute("src");
            audio.load();
            await audio.setMediaKeys(null);
            await later(10);
        "#
        )),
    );
    let left = e.status()["sourceBuffers"].as_object().unwrap().len();
    assert_eq!(left, 0, "detach left SourceBuffers behind");
    e.run_gc();
    e.tick();
    assert_eq!(
        e.status()["live"],
        json!({"medias": 0, "mediaSources": 0, "sourceBuffers": 0, "cdms": 0})
    );
}

#[test]
fn source_buffer_setters_follow_mse_validation_and_the_window_filters_appends() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            const sb = await fill(a.ms, a.opened, bytes);
            const t = (name, f) => { try { f(); L(name + ":ok"); } catch (err) { L(name + ":" + err.name); } };
            t("start-negative", () => { sb.appendWindowStart = -1; });
            t("end-nan", () => { sb.appendWindowEnd = NaN; });
            t("end-3", () => { sb.appendWindowEnd = 3; });
            t("start-at-end", () => { sb.appendWindowStart = 3; });
            t("start-1", () => { sb.appendWindowStart = 1; });
            t("end-below-start", () => { sb.appendWindowEnd = 0.5; });
            a.ms.duration = 10;
            sb.remove(0, Infinity);
            await new Promise(r => sb.addEventListener("updateend", r, { once: true }));
            sb.appendBuffer(bytes);
            t("offset-while-updating", () => { sb.timestampOffset = 5; });
            t("window-while-updating", () => { sb.appendWindowEnd = 9; });
            await new Promise(r => sb.addEventListener("updateend", r, { once: true }));
            const b = sb.buffered;
            L("buffered:" + (b.start(0) >= 1 && b.end(b.length - 1) <= 3));
            sb.abort();
            L("reset:" + sb.appendWindowStart + "," + sb.appendWindowEnd);
            t("offset-idle", () => { sb.timestampOffset = 5; });
            t("offset-nan", () => { sb.timestampOffset = NaN; });
            t("offset-infinite", () => { sb.timestampOffset = Infinity; });
            L("offset:" + sb.timestampOffset);
        "#,
        ),
    );
    assert_eq!(
        log,
        [
            "start-negative:TypeError",
            "end-nan:TypeError",
            "end-3:ok",
            "start-at-end:TypeError",
            "start-1:ok",
            "end-below-start:TypeError",
            "offset-while-updating:InvalidStateError",
            "window-while-updating:InvalidStateError",
            "buffered:true",
            "reset:0,Infinity",
            "offset-idle:ok",
            "offset-nan:TypeError",
            "offset-infinite:TypeError",
            "offset:5"
        ]
    );
}

#[test]
fn media_elements_get_a_native_player_only_once_a_source_attaches() {
    let (mut e, sink) = engine();
    run(
        &mut e,
        &sink,
        &script(
            r#"
            globalThis.pool = [];
            for (let i = 0; i < 50; i++) { const el = document.createElement(i % 2 ? "video" : "audio"); el.load(); el.volume = 0.5; pool.push(el); }
            L("no-native");
        "#,
        ),
    );
    assert_eq!(e.status()["live"]["medias"], 0);
    run(
        &mut e,
        &sink,
        r#"
        const ms = new MediaSource();
        pool[0].src = URL.createObjectURL(ms);
        await new Promise(r => ms.addEventListener("sourceopen", r, { once: true }));
        L("attached");
    "#,
    );
    assert_eq!(e.status()["live"]["medias"], 1);
    assert_eq!(e.status()["opCounts"]["media_set_gain"], 1);
}

#[test]
fn unknown_globals_and_host_properties_are_logged() {
    let (mut e, sink) = engine();
    e.enable_diagnostics().unwrap();
    run(
        &mut e,
        &sink,
        r#"
        L(typeof NoSuchGlobal + typeof window.AlsoMissing + ("inProbe" in window));
        L(String(navigator.noSuchProp) + document.createElement("audio").webkitNoSuch + typeof document.body.appendChild);
    "#,
    );
    let unknown = &e.status()["platform"]["unknown"];
    for key in [
        "window.NoSuchGlobal",
        "window.AlsoMissing",
        "window.inProbe",
        "navigator.noSuchProp",
        "HTMLAudioElement.webkitNoSuch",
    ] {
        assert_eq!(unknown[key], 1, "{key}: {unknown}");
    }
    assert!(
        unknown.get("HTMLBodyElement.appendChild").is_none(),
        "{unknown}"
    );
}

#[test]
fn misses_are_not_logged_unless_diagnostics_are_loaded() {
    let (mut e, sink) = engine();
    run(&mut e, &sink, "L(typeof NoSuchGlobal);");
    assert_eq!(e.status()["platform"]["unknown"], serde_json::Value::Null);
    assert_eq!(
        e.eval_json("Object.getPrototypeOf(globalThis) === Object.prototype")
            .unwrap(),
        true
    );
}

#[test]
fn appending_a_document_fragment_moves_its_children() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        r#"
        const f = document.createDocumentFragment();
        const d = document.createElement("div");
        d.id = "mk-dialog";
        f.appendChild(d);
        f.appendChild(document.createElement("div"));
        document.body.appendChild(f);
        L(f.childNodes.length + ":" + document.body.childNodes.length + ":" + (document.querySelector('#mk-dialog') === d) + ":" + f.nodeType);
        const g = document.createDocumentFragment();
        const a = document.createElement("a"), b = document.createElement("b");
        g.appendChild(a); g.appendChild(b);
        document.body.insertBefore(g, d);
        L(document.body.childNodes.map(n => n.localName).join(","));
    "#,
    );
    assert_eq!(log, ["0:2:true:11", "a,b,div,div"]);
}

#[test]
fn video_answers_maybe_on_both_elements_but_video_source_buffers_fail_loudly() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        r#"
        const avc = 'video/mp4;codecs="avc1.42E01E"';
        for (const tag of ["video", "audio"]) L(tag + ":" + document.createElement(tag).canPlayType(avc) + ":" + (document.createElement(tag).canPlayType('video/mp4;codecs="hev1.1.6.L93.B0"') || "no"));
        L("mse:" + MediaSource.isTypeSupported(avc));
        const ms = new MediaSource();
        document.createElement("audio").src = URL.createObjectURL(ms);
        await new Promise(r => ms.addEventListener("sourceopen", r, { once: true }));
        for (const t of [avc, "video/mp4", 'video/webm;codecs="vp9"']) { try { ms.addSourceBuffer(t); L("added"); } catch (err) { L(err.name); } }
    "#,
    );
    assert_eq!(
        log,
        [
            "video:maybe:no",
            "audio:maybe:no",
            "mse:false",
            "NotSupportedError",
            "NotSupportedError",
            "NotSupportedError"
        ]
    );
}

#[test]
fn the_first_init_segment_sets_a_nan_duration_so_remove_works() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.addEventListener("durationchange", () => L("durationchange"));
            audio.src = URL.createObjectURL(a.ms);
            const sb = await fill(a.ms, a.opened, bytes);
            await later(5);
            L("duration-set:" + !isNaN(a.ms.duration));
            sb.remove(0, 1);
            await new Promise(r => sb.addEventListener("updateend", r, { once: true }));
            L("removed:" + sb.buffered.start(0).toFixed(1));
        "#,
        ),
    );
    assert_eq!(log, ["durationchange", "duration-set:true", "removed:1.0"]);
}

#[test]
fn host_objects_are_ordinary_objects_by_default() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        r#"L(String([window, navigator, document.createElement("audio"), new MediaSource()].every(o => o instanceof Object && Object.prototype.isPrototypeOf(o))));"#,
    );
    assert_eq!(log, ["true"]);
}

#[test]
fn appended_media_past_the_mvhd_duration_extends_it() {
    let mut bytes = FIXTURE.to_vec();
    let mvhd = bytes.windows(4).position(|w| w == b"mvhd").unwrap() + 4;
    let timescale = u32::from_be_bytes(bytes[mvhd + 12..mvhd + 16].try_into().unwrap());
    bytes[mvhd + 16..mvhd + 20].copy_from_slice(&timescale.to_be_bytes());
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &format!(
            r#"
            const ms = new MediaSource();
            const audio = document.createElement("audio");
            audio.src = URL.createObjectURL(ms);
            await new Promise(r => ms.addEventListener("sourceopen", r, {{ once: true }}));
            const sb = ms.addSourceBuffer('audio/mp4; codecs="mp4a.40.2"');
            sb.appendBuffer(new Uint8Array("{}".match(/../g).map(h => parseInt(h, 16))));
            await new Promise(r => sb.addEventListener("updateend", r, {{ once: true }}));
            const end = sb.buffered.end(0);
            L("duration-is-end:" + (ms.duration === end && end > 4));
            sb.remove(1.5, 2);
            await new Promise(r => sb.addEventListener("updateend", r, {{ once: true }}));
            L("removed");
        "#,
            hex(&bytes)
        ),
    );
    assert_eq!(log, ["duration-is-end:true", "removed"]);
}

#[test]
fn endpoint_logging_records_only_the_scheme_of_data_urls() {
    let (mut e, sink) = engine();
    run(
        &mut e,
        &sink,
        r#"await fetch("data:text/plain;base64,c2VjcmV0LXBheWxvYWQ="); L("fetched");"#,
    );
    let endpoints = e.status()["platform"]["xhr"]["endpoints"].clone();
    let keys: Vec<&String> = endpoints.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["GET data:"], "{endpoints}");
}

#[test]
fn drain_acks_count_across_tracks_and_outlive_the_element_that_played_them() {
    let (mut e, sink) = engine();
    let log = run(
        &mut e,
        &sink,
        &script(
            r#"
            const a = fresh();
            audio.src = URL.createObjectURL(a.ms);
            await fill(a.ms, a.opened, bytes);
            a.ms.endOfStream();
            const ended = new Promise(r => audio.addEventListener("ended", r, { once: true }));
            await audio.play();
            await ended;
            audio.removeAttribute("src");
            audio.load();
            await later(10);
        "#,
        ),
    );
    assert!(has(&log, "ev:ended"), "{log:?}");
    assert_eq!(e.output_counts()[..2], [1, 0]);
    e.run_gc();
    e.tick();
    assert_eq!(e.status()["live"]["medias"], 0);
    assert_eq!(e.output_counts()[..2], [1, 0]);
}
