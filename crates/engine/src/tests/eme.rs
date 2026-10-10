use std::rc::Rc;

use serde_json::{Value, json};

use super::{FIXTURE, FakeCdm, FakeSink};
use crate::Engine;
use crate::cdm::{CdmBackend, CdmEvent};
use crate::eme::eme_check;

const PRELUDE: &str = r#"
globalThis.log = []; globalThis.done = false;
globalThis.L = x => log.push(x);
globalThis.outcome = (name, p) => p.then(() => L(name + ":ok"), e => L(name + ":" + e.name));
globalThis.keys = async () => (await navigator.requestMediaKeySystemAccess("com.widevine.alpha", [{ initDataTypes: ["cenc"], audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }] }])).createMediaKeys();
globalThis.later = ms => new Promise(r => setTimeout(r, ms));
"#;

fn engine(message_first: bool) -> (Engine, FakeSink) {
    let (e, sink, _) = engine_with_cdm(message_first);
    (e, sink)
}

fn engine_with_cdm(message_first: bool) -> (Engine, FakeSink, FakeCdm) {
    let cdm = FakeCdm::default();
    cdm.0.borrow_mut().message_first = message_first;
    let sink = FakeSink::default();
    let s = sink.clone();
    let c = cdm.clone();
    let factory = move |persistent| {
        let mut st = c.0.borrow_mut();
        st.allow_persistent_state = Some(persistent);
        st.events.push(CdmEvent::Initialized(true));
        Ok(Box::new(c.clone()) as Box<dyn CdmBackend>)
    };
    let e = Engine::with_backends(Box::new(factory), Rc::new(move || Box::new(s.clone()))).unwrap();
    e.eval_script("prelude.js", PRELUDE).unwrap();
    (e, sink, cdm)
}

fn run(e: &mut Engine, script: &str) -> Value {
    e.eval_script("test.js", &format!("(async () => {{ {script} }})().catch(e => L('threw:' + e)).finally(() => {{ done = true; }});")).unwrap();
    for _ in 0..2000 {
        e.tick();
        if e.eval_json("done").unwrap() == json!(true) {
            return e.eval_json("log").unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!(
        "script never finished; log = {}",
        e.eval_json("log").unwrap()
    );
}

const SESSION_FLOW: &str = r#"
    const s = (await keys()).createSession();
    s.addEventListener("message", e => L("message:" + e.messageType + ":" + e.message.byteLength));
    s.addEventListener("keystatuseschange", () => { const st = []; s.keyStatuses.forEach(v => st.push(v)); L("keys:" + st.join(",")); });
    await s.generateRequest("cenc", new Uint8Array([1, 2, 3]));
    L("generated:" + s.sessionId);
    await later(20);
    await s.update(new Uint8Array([9]));
    L("updated");
    await later(20);
    await s.close();
    await s.closed;
    L("closed:" + s.keyStatuses.size);
"#;

#[test]
fn session_lifecycle_orders_promises_and_events() {
    for message_first in [false, true] {
        let (mut e, _) = engine(message_first);
        assert_eq!(
            run(&mut e, SESSION_FLOW),
            json!([
                "generated:s1",
                "message:license-request:9",
                "updated",
                "keys:usable",
                "closed:0",
                "keys:"
            ]),
            "message_first={message_first}"
        );
    }
}

#[test]
fn session_methods_enforce_the_eme_state_machine() {
    let (mut e, _) = engine(false);
    let log = run(
        &mut e,
        r#"
        const mk = await keys();
        const s = mk.createSession();
        await outcome("update-new", s.update(new Uint8Array([1])));
        await outcome("close-new", s.close());
        await outcome("remove-new", s.remove());
        const first = s.generateRequest("cenc", new Uint8Array([1]));
        await outcome("second-generate", s.generateRequest("cenc", new Uint8Array([1])));
        await first;
        await outcome("empty-update", s.update(new Uint8Array(0)));
        await outcome("rejected-update", s.update(new TextEncoder().encode("bad")));
        await s.close();
        await outcome("second-close", s.close());
        await outcome("update-closed", s.update(new Uint8Array([1])));
        await outcome("bogus-init-type", mk.createSession().generateRequest("bogus", new Uint8Array([1])));
        await outcome("persistent", Promise.resolve().then(() => mk.createSession("persistent-license")));
    "#,
    );
    let expected = [
        "update-new:InvalidStateError",
        "close-new:InvalidStateError",
        "remove-new:InvalidStateError",
        "second-generate:InvalidStateError",
        "empty-update:TypeError",
        "rejected-update:InvalidStateError",
        "second-close:ok",
        "update-closed:InvalidStateError",
        "bogus-init-type:NotSupportedError",
        "persistent:NotSupportedError",
    ];
    assert_eq!(log, json!(expected));
}

#[test]
fn key_system_access_grants_only_audio_widevine_temporary_sessions() {
    let audio = r#"[{"audioCapabilities":[{"contentType":"audio/mp4; codecs=\"mp4a.40.2\""}]}]"#;
    let granted = eme_check("com.widevine.alpha", audio).unwrap();
    assert_eq!(
        (
            granted["persistentState"].as_str(),
            granted["sessionTypes"].clone()
        ),
        (Some("not-allowed"), json!(["temporary"]))
    );
    assert!(eme_check("org.w3.clearkey", audio).is_none());
    assert!(
        eme_check("com.widevine.alpha", r#"[{"videoCapabilities":[{"contentType":"video/mp4; codecs=\"avc1.640028\""}],"audioCapabilities":[{"contentType":"audio/mp4; codecs=\"mp4a.40.2\""}]}]"#)
            .is_none()
    );
    assert!(eme_check("com.widevine.alpha", r#"[{"audioCapabilities":[{"contentType":"audio/mp4; codecs=\"mp4a.40.2\"","robustness":"HW_SECURE_ALL"}]}]"#).is_none());
    for he_aac in ["mp4a.40.5", "mp4a.40.29"] {
        let caps = format!(
            r#"[{{"audioCapabilities":[{{"contentType":"audio/mp4; codecs=\"{he_aac}\""}}]}}]"#
        );
        assert!(eme_check("com.widevine.alpha", &caps).is_none(), "{he_aac}");
    }
}

const AUDIO_PERSISTENT: &str = r#"[{"initDataTypes":["cenc","keyids"],"audioCapabilities":[{"contentType":"audio/mp4;codecs=\"mp4a.40.2\""}],"distinctiveIdentifier":"optional","persistentState":"required"}]"#;

#[test]
fn eme_check_grants_required_persistent_state_and_keyids_but_not_unsupported_video() {
    let granted = eme_check("com.widevine.alpha", AUDIO_PERSISTENT).unwrap();
    assert_eq!(
        (
            &granted["persistentState"],
            &granted["distinctiveIdentifier"],
            &granted["initDataTypes"]
        ),
        (
            &json!("required"),
            &json!("not-allowed"),
            &json!(["cenc", "keyids"])
        )
    );
    let with_video = AUDIO_PERSISTENT.replace(
        r#""audioCapabilities""#,
        r#""videoCapabilities":[{"contentType":"video/mp4;codecs=\"avc1.42E01E\"","robustness":"SW_SECURE_DECODE"}],"audioCapabilities""#,
    );
    assert!(eme_check("com.widevine.alpha", &with_video).is_none());
    let two_configs = format!(
        "[{},{}]",
        &with_video[1..with_video.len() - 1],
        &AUDIO_PERSISTENT[1..AUDIO_PERSISTENT.len() - 1]
    );
    assert_eq!(eme_check("com.widevine.alpha", &two_configs), Some(granted));
    let distinctive = AUDIO_PERSISTENT.replace(r#""optional""#, r#""required""#);
    assert!(eme_check("com.widevine.alpha", &distinctive).is_none());
}

#[test]
fn persistent_state_access_lets_the_cdm_persist_and_temporary_access_does_not() {
    for (config, expected) in [
        (AUDIO_PERSISTENT, true),
        (
            r#"[{"audioCapabilities":[{"contentType":"audio/mp4;codecs=\"mp4a.40.2\""}]}]"#,
            false,
        ),
    ] {
        let (mut e, _, cdm) = engine_with_cdm(false);
        let log = run(
            &mut e,
            &format!(
                r#"const a = await navigator.requestMediaKeySystemAccess("com.widevine.alpha", {config}); await a.createMediaKeys(); L(a.getConfiguration().persistentState);"#
            ),
        );
        let state = if expected { "required" } else { "not-allowed" };
        assert_eq!(log, json!([state]));
        assert_eq!(cdm.0.borrow().allow_persistent_state, Some(expected));
    }
}

#[test]
fn set_server_certificate_reaches_the_cdm_and_rejects_an_empty_one() {
    let (mut e, _, cdm) = engine_with_cdm(false);
    let log = run(
        &mut e,
        r#"
        const mk = await keys();
        L("set:" + await mk.setServerCertificate(new Uint8Array([1, 2, 3])));
        await outcome("empty", mk.setServerCertificate(new Uint8Array(0)));
        await outcome("not-a-buffer", mk.setServerCertificate("nope"));
    "#,
    );
    assert_eq!(
        log,
        json!(["set:true", "empty:TypeError", "not-a-buffer:TypeError"])
    );
    assert_eq!(cdm.0.borrow().certificate, [1, 2, 3]);
}

#[test]
fn eme_check_grants_video_only_for_exactly_avc1_42e01e_at_sw_secure_crypto() {
    let config = |codec: &str, robustness: &str| {
        format!(
            r#"[{{"initDataTypes":["keyids","cenc"],"distinctiveIdentifier":"optional","persistentState":"required","videoCapabilities":[{{"contentType":"video/mp4;codecs=\"{codec}\"","robustness":"{robustness}"}}],"audioCapabilities":[{{"contentType":"audio/mp4;codecs=\"mp4a.40.2\""}}]}}]"#
        )
    };
    let granted = eme_check(
        "com.widevine.alpha",
        &config("avc1.42E01E", "SW_SECURE_CRYPTO"),
    )
    .unwrap();
    assert_eq!(
        granted["videoCapabilities"][0]["contentType"],
        "video/mp4;codecs=\"avc1.42E01E\""
    );
    for (codec, robustness) in [
        ("avc1.42E01E", ""),
        ("mp4a.40.2", "SW_SECURE_CRYPTO"),
        ("avc1.42E01E,mp4a.40.2", "SW_SECURE_CRYPTO"),
        ("avc1.42E01E", "SW_SECURE_DECODE"),
        ("avc1.42E01E", "HW_SECURE_CRYPTO"),
        ("avc1.42E01E", "HW_SECURE_ALL"),
        ("avc1.640028", "SW_SECURE_CRYPTO"),
        ("hev1.1.6.L93.B0", "SW_SECURE_CRYPTO"),
        ("vp09.00.10.08", "SW_SECURE_CRYPTO"),
    ] {
        assert!(
            eme_check("com.widevine.alpha", &config(codec, robustness)).is_none(),
            "{codec} {robustness}"
        );
    }
}

#[test]
fn can_play_type_answers_maybe_only_for_avc1_mp4_video() {
    use crate::eme::can_play_type;
    for (t, answer) in [
        ("audio/mp4; codecs=\"mp4a.40.2\"", "probably"),
        ("video/mp4", ""),
        ("video/mp4;codecs=\"avc1.42E01E\"", "maybe"),
        ("video/mp4; codecs=\"avc1.42E01E, mp4a.40.2\"", ""),
        ("video/mp4;codecs=\"mp4a.40.2\"", ""),
        ("video/mp4;codecs=\"avc1.640028\"", ""),
        ("video/mp4;codecs=\"hev1.1.6.L93.B0\"", ""),
        ("video/webm;codecs=\"vp9\"", ""),
        ("application/vnd.apple.mpegurl", ""),
    ] {
        assert_eq!(can_play_type(t), answer, "{t}");
    }
}

fn media_script(body: &str) -> String {
    let fixture: String = FIXTURE.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        r#"
        const bytes = new Uint8Array("{fixture}".match(/../g).map(h => parseInt(h, 16)));
        const audio = document.createElement("audio");
        for (const t of ["play", "playing", "pause", "waiting", "seeking", "seeked", "volumechange", "canplaythrough"]) audio.addEventListener(t, () => L(t));
        const ms = new MediaSource();
        const opened = new Promise(r => ms.addEventListener("sourceopen", r, {{ once: true }}));
        audio.src = URL.createObjectURL(ms);
        const attach = async () => {{
            await opened;
            const sb = ms.addSourceBuffer('audio/mp4; codecs="mp4a.40.2"');
            sb.appendBuffer(bytes);
            await new Promise(r => sb.addEventListener("updateend", r, {{ once: true }}));
            return sb;
        }};
        {body}
    "#
    )
}

#[test]
fn media_element_play_promise_volume_and_seek_reach_native() {
    let (mut e, sink) = engine(false);
    let log = run(
        &mut e,
        &media_script(
            r#"
            const sb = await attach();
            ms.endOfStream();
            await audio.play();
            L("play-resolved");
            audio.volume = 0.5;
            audio.volume = 0.5;
            await later(30);
            audio.currentTime = 2;
            await new Promise(r => audio.addEventListener("seeked", r, { once: true }));
            L("at:" + audio.currentTime);
        "#,
        ),
    );
    let log: Vec<&str> = log
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let playing = log.iter().position(|x| *x == "playing").unwrap();
    assert!(
        log.iter().position(|x| *x == "play-resolved").unwrap() > playing,
        "{log:?}"
    );
    assert_eq!(log.iter().filter(|x| **x == "volumechange").count(), 1);
    let after_play: Vec<&str> = log[playing..]
        .iter()
        .copied()
        .filter(|x| ["seeking", "waiting", "seeked", "at:2"].contains(x))
        .collect();
    assert_eq!(
        after_play,
        ["seeking", "waiting", "seeked", "at:2"],
        "{log:?}"
    );
    assert_eq!(sink.state().gain, 0.5);
}

#[test]
fn media_element_relays_abort_range_and_index_errors() {
    let (mut e, _) = engine(false);
    let log = run(
        &mut e,
        &media_script(
            r#"
            const p = audio.play();
            audio.pause();
            await outcome("play-then-pause", p);
            try { audio.volume = 1.5; } catch (err) { L("volume:" + err.name); }
            const sb = await attach();
            ms.duration = 4;
            try { sb.remove(3, 1); } catch (err) { L("remove:" + err.name); }
        "#,
        ),
    );
    let log: Vec<&str> = log
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        log[..5],
        [
            "play",
            "waiting",
            "pause",
            "play-then-pause:AbortError",
            "volume:IndexSizeError"
        ],
        "{log:?}"
    );
    assert!(log.contains(&"remove:TypeError"));
}
