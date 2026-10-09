mod audio;
mod cdm;
mod media;
mod mp4;
mod net;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::Result;
use rquickjs::{ArrayBuffer, CatchResultExt, Context, Ctx, Function, Object, Runtime, Value, context::EvalOptions, function::IntoArgs};
use serde_json::{Value as J, json};

use cdm::{Cdm, CdmEvent};
use media::{MediaEngine, MediaSourceNative, SourceBufferNative};
use net::{Completion, HttpRequest};

const PLATFORM: &str = include_str!("../js/platform.js");
const APP: &str = include_str!("../js/app.js");

struct Native {
    start: Instant,
    next_id: u32,
    mses: HashMap<u32, MediaSourceNative>,
    sbs: HashMap<u32, SourceBufferNative>,
    medias: HashMap<u32, MediaEngine>,
    cdms: HashMap<u32, Cdm>,
    cdm_info: J,
    net_tx: Sender<Completion>,
    aborted: HashSet<u32>,
    op_counts: BTreeMap<&'static str, u64>,
    sink: String,
    eme_requests: Vec<J>,
    script_log: Vec<J>,
    http_count: u64,
    http_bytes: u64,
}

impl Native {
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
    fn count(&mut self, op: &'static str) {
        *self.op_counts.entry(op).or_default() += 1;
    }
}

type N = Rc<RefCell<Native>>;

struct Args {
    manifest: String,
    license: String,
    hls_url: String,
    sink: String,
    status: String,
    max_seconds: f64,
    looping: bool,
    hls_debug: bool,
    audio_master_codecs: String,
}

fn parse_args() -> Args {
    let mut a = Args {
        manifest: "https://storage.googleapis.com/shaka-demo-assets/angel-one-widevine-hls/playlist_a-eng-0128k-aac-2c.mp4.m3u8".into(),
        license: "https://cwip-shaka-proxy.appspot.com/no_auth".into(),
        hls_url: "https://cdn.jsdelivr.net/npm/hls.js@1.7.3/dist/hls.min.js".into(),
        sink: "spike5-null".into(),
        status: "status.json".into(),
        max_seconds: 1e9,
        looping: true,
        hls_debug: false,
        audio_master_codecs: "mp4a.40.2".into(),
    };
    let v: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < v.len() {
        let next = v.get(i + 1).cloned().unwrap_or_default();
        match v[i].as_str() {
            "--manifest" => a.manifest = next,
            "--lic" => a.license = next,
            "--hls" => a.hls_url = next,
            "--sink" => a.sink = next,
            "--status" => a.status = next,
            "--max-seconds" => a.max_seconds = next.parse().unwrap_or(1e9),
            "--audio-master-codecs" => a.audio_master_codecs = next,
            "--no-loop" => {
                a.looping = false;
                i += 1;
                continue;
            }
            "--hls-debug" => {
                a.hls_debug = true;
                i += 1;
                continue;
            }
            other => panic!("unknown arg {other}"),
        }
        i += 2;
    }
    a
}

fn opts(name: &str) -> EvalOptions {
    let mut o = EvalOptions::default();
    o.strict = false;
    o.global = true;
    o.filename = Some(name.into());
    o
}

fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    let ab = ArrayBuffer::from_value(v.clone())?;
    unsafe { ab.as_bytes() }.map(|b| b.to_vec())
}

fn eme_check(key_system: &str, configs: &str) -> Option<J> {
    if key_system != "com.widevine.alpha" {
        return None;
    }
    let configs: Vec<J> = serde_json::from_str(configs).ok()?;
    for c in configs {
        let init_types: Vec<String> = c.get("initDataTypes").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
        let supported_init: Vec<String> = if init_types.is_empty() { Vec::new() } else { init_types.iter().filter(|t| *t == "cenc" || *t == "webm").cloned().collect() };
        if !init_types.is_empty() && supported_init.is_empty() {
            continue;
        }
        if c.get("distinctiveIdentifier").and_then(|x| x.as_str()) == Some("required") || c.get("persistentState").and_then(|x| x.as_str()) == Some("required") {
            continue;
        }
        let session_types: Vec<String> = c.get("sessionTypes").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_else(|| vec!["temporary".into()]);
        if session_types.iter().any(|t| t != "temporary") {
            continue;
        }
        let audio_in = c.get("audioCapabilities").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let video_in = c.get("videoCapabilities").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let audio: Vec<J> = audio_in
            .iter()
            .filter(|cap| {
                let ct = cap.get("contentType").and_then(|x| x.as_str()).unwrap_or("");
                let rob = cap.get("robustness").and_then(|x| x.as_str()).unwrap_or("");
                let scheme = cap.get("encryptionScheme").and_then(|x| x.as_str());
                audio_type_supported(ct) && (rob.is_empty() || rob == "SW_SECURE_CRYPTO") && matches!(scheme, None | Some("cenc") | Some("cbcs"))
            })
            .cloned()
            .collect();
        if !audio_in.is_empty() && audio.is_empty() {
            continue;
        }
        if !video_in.is_empty() {
            continue;
        }
        if audio.is_empty() {
            continue;
        }
        return Some(json!({
            "label": c.get("label").cloned().unwrap_or(json!("")),
            "initDataTypes": supported_init,
            "audioCapabilities": audio,
            "videoCapabilities": [],
            "distinctiveIdentifier": "not-allowed",
            "persistentState": "not-allowed",
            "sessionTypes": ["temporary"],
        }));
    }
    None
}

fn audio_type_supported(ct: &str) -> bool {
    let lower = ct.to_ascii_lowercase();
    let mut parts = lower.split(';');
    let mime = parts.next().unwrap_or("").trim();
    if mime != "audio/mp4" {
        return false;
    }
    let mut codecs: Vec<String> = Vec::new();
    for p in parts {
        let p = p.trim();
        if let Some(v) = p.strip_prefix("codecs=") {
            codecs = v.trim_matches('"').split(',').map(|s| s.trim().to_string()).collect();
        }
    }
    codecs.iter().all(|c| c == "mp4a.40.2" || c == "mp4a.40.5" || c == "mp4a.40.29" || c == "mp4a.40")
}

fn install_ops<'js>(ctx: &Ctx<'js>, n: &N) -> rquickjs::Result<()> {
    let g = ctx.globals();
    macro_rules! op {
        ($name:literal, $f:expr) => {
            g.set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }
    let s = n.clone();
    op!("__op_now", move || -> f64 { s.borrow().start.elapsed().as_secs_f64() * 1000.0 });
    op!("__op_print", move |level: String, msg: String| {
        eprintln!("JS[{level}] {msg}");
    });
    op!("__op_random", move |ctx: Ctx<'js>, len: u32| -> rquickjs::Result<ArrayBuffer<'js>> {
        let mut buf = vec![0u8; len as usize];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(&mut buf);
        }
        ArrayBuffer::new(ctx, buf)
    });
    let s = n.clone();
    op!("__op_http", move |id: u32, method: String, url: String, headers: String, body: Value<'js>, timeout: f64| {
        let mut st = s.borrow_mut();
        st.count("http");
        st.http_count += 1;
        let headers: Vec<(String, String)> = serde_json::from_str::<Vec<(String, String)>>(&headers).unwrap_or_default();
        let req = HttpRequest { id, method, url, headers, body: bytes_of(&body), timeout_ms: if timeout > 0.0 { timeout as u64 } else { 60_000 } };
        net::spawn(req, st.net_tx.clone());
    });
    let s = n.clone();
    op!("__op_http_abort", move |id: u32| {
        let mut st = s.borrow_mut();
        st.count("http_abort");
        st.aborted.insert(id);
    });
    let s = n.clone();
    op!("__op_load_script", move |id: u32, url: String| {
        let mut st = s.borrow_mut();
        st.count("load_script");
        st.script_log.push(json!({"id": id, "url": url, "phase": "fetching"}));
        net::spawn_script(id, url, st.net_tx.clone());
    });
    let s = n.clone();
    op!("__op_ms_create", move || -> u32 {
        let mut st = s.borrow_mut();
        st.count("ms_create");
        let id = st.id();
        st.mses.insert(id, MediaSourceNative { duration: f64::NAN, ended: false, sbs: Vec::new() });
        id
    });
    let s = n.clone();
    op!("__op_sb_add", move |ms_id: u32, mime: String| -> i32 {
        let mut st = s.borrow_mut();
        st.count("sb_add");
        if !audio_type_supported(&mime) {
            return -1;
        }
        let id = st.id();
        st.sbs.insert(id, SourceBufferNative::new(mime));
        if let Some(ms) = st.mses.get_mut(&ms_id) {
            ms.sbs.push(id);
        }
        id as i32
    });
    let s = n.clone();
    op!("__op_sb_remove_buffer", move |ms_id: u32, sb_id: u32| {
        let mut st = s.borrow_mut();
        st.count("sb_remove_buffer");
        st.sbs.remove(&sb_id);
        if let Some(ms) = st.mses.get_mut(&ms_id) {
            ms.sbs.retain(|x| *x != sb_id);
        }
    });
    let s = n.clone();
    op!("__op_sb_append", move |sb_id: u32, data: Value<'js>, ts_offset: f64| -> String {
        let mut st = s.borrow_mut();
        st.count("sb_append");
        let Some(bytes) = bytes_of(&data) else { return json!({"error": "not an ArrayBuffer"}).to_string() };
        let Some(sb) = st.sbs.get_mut(&sb_id) else { return json!({"error": "no such SourceBuffer"}).to_string() };
        match sb.append(&bytes, if ts_offset.is_finite() { ts_offset } else { 0.0 }) {
            Ok(r) => json!({"init": r.init_segment, "frames": r.media_frames, "hasInitData": r.init_segment && sb.init_data.is_some()}).to_string(),
            Err(e) => json!({"error": format!("{e:#}")}).to_string(),
        }
    });
    let s = n.clone();
    op!("__op_sb_init_data", move |ctx: Ctx<'js>, sb_id: u32| -> rquickjs::Result<Value<'js>> {
        let st = s.borrow();
        match st.sbs.get(&sb_id).and_then(|sb| sb.init_data.clone()) {
            Some(b) => Ok(ArrayBuffer::new(ctx, b)?.into_value()),
            None => Ok(Value::new_null(ctx)),
        }
    });
    let s = n.clone();
    op!("__op_sb_remove", move |sb_id: u32, start: f64, end: f64| {
        let mut st = s.borrow_mut();
        st.count("sb_remove");
        if let Some(sb) = st.sbs.get_mut(&sb_id) {
            sb.remove(start, end);
        }
    });
    let s = n.clone();
    op!("__op_sb_abort", move |sb_id: u32| {
        let mut st = s.borrow_mut();
        st.count("sb_abort");
        if let Some(sb) = st.sbs.get_mut(&sb_id) {
            sb.abort();
        }
    });
    let s = n.clone();
    op!("__op_sb_change_type", move |sb_id: u32, mime: String| -> bool {
        let mut st = s.borrow_mut();
        st.count("sb_change_type");
        if !audio_type_supported(&mime) {
            return false;
        }
        if let Some(sb) = st.sbs.get_mut(&sb_id) {
            sb.mime = mime;
        }
        true
    });
    let s = n.clone();
    op!("__op_sb_buffered", move |sb_id: u32| -> String {
        let st = s.borrow();
        let r: Vec<[f64; 2]> = st.sbs.get(&sb_id).map(|sb| sb.buffered().into_iter().map(|(a, b)| [a, b]).collect()).unwrap_or_default();
        serde_json::to_string(&r).unwrap()
    });
    let s = n.clone();
    op!("__op_ms_set_duration", move |ms_id: u32, d: f64| {
        let mut st = s.borrow_mut();
        st.count("ms_set_duration");
        if let Some(ms) = st.mses.get_mut(&ms_id) {
            ms.duration = d;
        }
    });
    let s = n.clone();
    op!("__op_ms_end", move |ms_id: u32, ended: bool| {
        let mut st = s.borrow_mut();
        st.count("ms_end");
        if let Some(ms) = st.mses.get_mut(&ms_id) {
            ms.ended = ended;
        }
    });
    let s = n.clone();
    op!("__op_media_create", move || -> u32 {
        let mut st = s.borrow_mut();
        st.count("media_create");
        let id = st.id();
        let sink = st.sink.clone();
        st.medias.insert(id, MediaEngine::new(sink));
        id
    });
    let s = n.clone();
    op!("__op_media_attach", move |id: u32, ms_id: u32| {
        let mut st = s.borrow_mut();
        st.count("media_attach");
        if let Some(m) = st.medias.get_mut(&id) {
            m.attach(ms_id);
        }
    });
    let s = n.clone();
    op!("__op_media_detach", move |id: u32| {
        let mut st = s.borrow_mut();
        st.count("media_detach");
        if let Some(m) = st.medias.get_mut(&id) {
            m.detach();
        }
    });
    let s = n.clone();
    op!("__op_media_play", move |id: u32| {
        let mut st = s.borrow_mut();
        st.count("media_play");
        if let Some(m) = st.medias.get_mut(&id) {
            m.play();
        }
    });
    let s = n.clone();
    op!("__op_media_pause", move |id: u32| {
        let mut st = s.borrow_mut();
        st.count("media_pause");
        if let Some(m) = st.medias.get_mut(&id) {
            m.pause();
        }
    });
    let s = n.clone();
    op!("__op_media_seek", move |id: u32, t: f64| {
        let mut st = s.borrow_mut();
        st.count("media_seek");
        if let Some(m) = st.medias.get_mut(&id) {
            m.seek_to(t);
        }
    });
    let s = n.clone();
    op!("__op_media_set_loop", move |id: u32, looping: bool| {
        let mut st = s.borrow_mut();
        if let Some(m) = st.medias.get_mut(&id) {
            m.looping = looping;
        }
    });
    let s = n.clone();
    op!("__op_media_set_keys", move |id: u32, keys_id: u32| {
        let mut st = s.borrow_mut();
        st.count("media_set_keys");
        if let Some(m) = st.medias.get_mut(&id) {
            m.keys_id = if keys_id == 0 { None } else { Some(keys_id) };
            m.waiting_for_key = false;
        }
    });
    let s = n.clone();
    op!("__op_media_state", move |id: u32| -> String {
        let st = s.borrow();
        st.medias.get(&id).map(media_state).unwrap_or_else(|| "{}".into())
    });
    let s = n.clone();
    op!("__op_eme_check", move |key_system: String, configs: String| -> String {
        let mut st = s.borrow_mut();
        st.count("eme_check");
        let r = eme_check(&key_system, &configs);
        st.eme_requests.push(json!({"keySystem": key_system, "requested": serde_json::from_str::<J>(&configs).unwrap_or(J::Null), "granted": r.clone()}));
        r.map(|x| x.to_string()).unwrap_or_default()
    });
    let s = n.clone();
    op!("__op_cdm_create", move || -> i32 {
        let mut st = s.borrow_mut();
        st.count("cdm_create");
        match Cdm::create() {
            Ok(c) => {
                st.cdm_info = json!({"version": c.version, "hostVersionRequested": c.host_version, "interface": 10});
                let id = st.id();
                st.cdms.insert(id, c);
                id as i32
            }
            Err(e) => {
                eprintln!("ENGINE cdm create failed: {e:#}");
                st.cdm_info = json!({"error": format!("{e:#}")});
                -1
            }
        }
    });
    let s = n.clone();
    op!("__op_cdm_set_cert", move |keys_id: u32, promise: u32, data: Value<'js>| {
        let mut st = s.borrow_mut();
        st.count("cdm_set_cert");
        let b = bytes_of(&data).unwrap_or_default();
        if let Some(c) = st.cdms.get_mut(&keys_id) {
            c.set_server_certificate(promise, &b);
        }
    });
    let s = n.clone();
    op!("__op_cdm_generate", move |keys_id: u32, promise: u32, init_type: u32, data: Value<'js>| {
        let mut st = s.borrow_mut();
        st.count("cdm_generate");
        let b = bytes_of(&data).unwrap_or_default();
        if let Some(c) = st.cdms.get_mut(&keys_id) {
            c.create_session(promise, init_type, &b);
        }
    });
    let s = n.clone();
    op!("__op_cdm_update", move |keys_id: u32, promise: u32, sid: String, data: Value<'js>| {
        let mut st = s.borrow_mut();
        st.count("cdm_update");
        let b = bytes_of(&data).unwrap_or_default();
        if let Some(c) = st.cdms.get_mut(&keys_id) {
            c.update_session(promise, &sid, &b);
        }
    });
    let s = n.clone();
    op!("__op_cdm_close", move |keys_id: u32, promise: u32, sid: String| {
        let mut st = s.borrow_mut();
        st.count("cdm_close");
        if let Some(c) = st.cdms.get_mut(&keys_id) {
            c.close_session(promise, &sid);
        }
    });
    let s = n.clone();
    op!("__op_cdm_remove", move |keys_id: u32, promise: u32, sid: String| {
        let mut st = s.borrow_mut();
        st.count("cdm_remove");
        if let Some(c) = st.cdms.get_mut(&keys_id) {
            c.remove_session(promise, &sid);
        }
    });
    Ok(())
}

fn media_state(m: &MediaEngine) -> String {
    json!({
        "currentTime": m.current_time(),
        "readyState": m.ready_state,
        "paused": m.paused,
        "seeking": m.seeking,
        "ended": m.ended,
    })
    .to_string()
}

fn call<'js, A: IntoArgs<'js>>(ctx: &Ctx<'js>, obj: &str, name: &str, args: A) -> Option<Value<'js>> {
    let r = (|| -> rquickjs::Result<Value<'js>> {
        let p: Object = ctx.globals().get(obj)?;
        let f: Function = p.get(name)?;
        f.call::<_, Value>(args)
    })();
    match r.catch(ctx) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("ENGINE call {obj}.{name} failed: {e}");
            None
        }
    }
}

fn drain(rt: &Runtime) {
    for _ in 0..100_000 {
        match rt.execute_pending_job() {
            Ok(true) => continue,
            Ok(false) => break,
            Err(e) => eprintln!("ENGINE job error: {e:?}"),
        }
    }
}

fn handle_completion(ctx: &Ctx, n: &N, c: Completion) {
    match c {
        Completion::Http(r) => {
            {
                let mut st = n.borrow_mut();
                if st.aborted.remove(&r.id) {
                    return;
                }
                st.http_bytes += r.body.len() as u64;
            }
            let meta = json!({"status": r.status, "statusText": r.status_text, "url": r.url, "headers": r.headers, "error": r.error}).to_string();
            let body = match ArrayBuffer::new(ctx.clone(), r.body) {
                Ok(b) => b.into_value(),
                Err(_) => Value::new_null(ctx.clone()),
            };
            call(ctx, "__platform", "dispatchHttp", (r.id, meta, body));
        }
        Completion::Script { id, url, result } => {
            let (ok, err) = match result {
                Ok(bytes) => {
                    let len = bytes.len();
                    let t = Instant::now();
                    match ctx.eval_with_options::<(), _>(bytes, opts(&url)).catch(ctx) {
                        Ok(()) => {
                            n.borrow_mut().script_log.push(json!({"id": id, "url": url, "phase": "evaluated", "bytes": len, "evalMs": t.elapsed().as_secs_f64() * 1000.0}));
                            (true, String::new())
                        }
                        Err(e) => {
                            n.borrow_mut().script_log.push(json!({"id": id, "url": url, "phase": "eval-error", "error": e.to_string()}));
                            (false, e.to_string())
                        }
                    }
                }
                Err(e) => {
                    n.borrow_mut().script_log.push(json!({"id": id, "url": url, "phase": "fetch-error", "error": e}));
                    (false, e)
                }
            };
            call(ctx, "__platform", "dispatchScript", (id, ok, err));
        }
    }
}

fn dispatch_cdm(ctx: &Ctx, keys_id: u32, e: CdmEvent) {
    let (meta, body): (J, Option<Vec<u8>>) = match e {
        CdmEvent::Initialized(ok) => (json!({"type": "initialized", "ok": ok}), None),
        CdmEvent::NewSession { promise, sid } => (json!({"type": "newSession", "promise": promise, "sid": sid}), None),
        CdmEvent::Resolve(p) => (json!({"type": "resolve", "promise": p}), None),
        CdmEvent::Reject { promise, exception, system_code, message } => (json!({"type": "reject", "promise": promise, "exception": exception, "code": system_code, "message": message}), None),
        CdmEvent::Message { sid, kind, body } => (json!({"type": "message", "sid": sid, "kind": kind}), Some(body)),
        CdmEvent::Keys { sid, keys } => (json!({"type": "keys", "sid": sid, "keys": keys.iter().map(|(k, s)| json!({"kid": mp4::hex(k), "status": s})).collect::<Vec<_>>()}), None),
        CdmEvent::Expiration { sid, expiry } => (json!({"type": "expiration", "sid": sid, "expiry": expiry}), None),
        CdmEvent::Closed { sid } => (json!({"type": "closed", "sid": sid}), None),
    };
    let body = match body {
        Some(b) => ArrayBuffer::new(ctx.clone(), b).map(|x| x.into_value()).unwrap_or_else(|_| Value::new_null(ctx.clone())),
        None => Value::new_null(ctx.clone()),
    };
    call(ctx, "__platform", "dispatchCdm", (keys_id, meta.to_string(), body));
}

fn write_status(path: &str, n: &N, platform: String, app: String, phase: &str) {
    let st = n.borrow();
    let medias: serde_json::Map<String, J> = st
        .medias
        .iter()
        .map(|(id, m)| {
            let c = &m.counters;
            let rate = if m.rate > 0 { m.rate as f64 } else { 48000.0 };
            (
                id.to_string(),
                json!({
                    "currentTime": m.current_time(),
                    "maxCurrentTime": m.max_current_time,
                    "readyState": m.ready_state,
                    "paused": m.paused,
                    "seeking": m.seeking,
                    "ended": m.ended,
                    "waiting": m.waiting,
                    "waitingForKey": m.waiting_for_key,
                    "keysId": m.keys_id,
                    "rate": m.rate,
                    "channels": m.channels,
                    "streamCreated": m.audio.as_ref().map(|a| a.stream_created.load(Ordering::SeqCst)).unwrap_or(false),
                    "clearSamples": c.clear_samples,
                    "encryptedSamples": c.encrypted_samples,
                    "decryptOk": c.decrypt_ok,
                    "decryptFail": c.decrypt_fail.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
                    "decodedClearSeconds": c.decoded_clear_frames as f64 / rate,
                    "decodedDecryptedSeconds": c.decoded_decrypted_frames as f64 / rate,
                    "decodeErr": c.decode_err,
                    "keyWaits": c.key_waits,
                    "underruns": c.underruns,
                    "seeks": c.seeks,
                    "loops": c.loops,
                }),
            )
        })
        .collect();
    let sbs: serde_json::Map<String, J> = st
        .sbs
        .iter()
        .map(|(id, sb)| (id.to_string(), json!({"mime": sb.mime, "appendedBytes": sb.appended_bytes, "frames": sb.frames.len(), "buffered": sb.buffered(), "init": sb.init.as_ref().map(|i| json!({"codec": i.codec, "timescale": i.timescale, "channels": i.channels, "sampleRate": i.sample_rate, "protected": i.protection.as_ref().map(|p| json!({"scheme": p.scheme, "kid": mp4::hex(&p.kid), "ivSize": p.iv_size})), "psshBytes": i.pssh.len()}))})))
        .collect();
    let cdm_logs: Vec<String> = st.cdms.values().flat_map(|c| c.logs().iter().cloned()).collect();
    let v = json!({
        "pid": std::process::id(),
        "phase": phase,
        "elapsedSec": st.start.elapsed().as_secs_f64(),
        "opCounts": st.op_counts,
        "medias": medias,
        "sourceBuffers": sbs,
        "cdm": st.cdm_info,
        "cdmLogs": cdm_logs,
        "emeRequests": st.eme_requests,
        "scripts": st.script_log,
        "http": {"count": st.http_count, "bytes": st.http_bytes},
        "platform": serde_json::from_str::<J>(&platform).unwrap_or(J::Null),
        "app": serde_json::from_str::<J>(&app).unwrap_or(J::Null),
    });
    let tmp = format!("{path}.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn main() -> Result<()> {
    let args = parse_args();
    let (net_tx, net_rx): (Sender<Completion>, Receiver<Completion>) = channel();
    let n: N = Rc::new(RefCell::new(Native {
        start: Instant::now(),
        next_id: 0,
        mses: HashMap::new(),
        sbs: HashMap::new(),
        medias: HashMap::new(),
        cdms: HashMap::new(),
        cdm_info: J::Null,
        net_tx,
        aborted: HashSet::new(),
        op_counts: BTreeMap::new(),
        sink: args.sink.clone(),
        eme_requests: Vec::new(),
        script_log: Vec::new(),
        http_count: 0,
        http_bytes: 0,
    }));
    println!("PID {}", std::process::id());
    let rt = Runtime::new()?;
    rt.set_max_stack_size(4 * 1024 * 1024);
    let ctx = Context::full(&rt)?;
    let config = json!({
        "manifest": args.manifest,
        "licenseUrl": args.license,
        "hlsUrl": args.hls_url,
        "loop": args.looping,
        "hlsDebug": args.hls_debug,
        "audioMasterCodecs": if args.audio_master_codecs.is_empty() { J::Null } else { json!(args.audio_master_codecs) },
    })
    .to_string();
    ctx.with(|ctx| -> Result<()> {
        install_ops(&ctx, &n).map_err(|e| anyhow::anyhow!("install ops: {e}"))?;
        let cfg: Value = ctx.json_parse(config.clone()).map_err(|e| anyhow::anyhow!("config: {e}"))?;
        ctx.globals().set("__spike_config", cfg).map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Err(e) = ctx.eval_with_options::<(), _>(PLATFORM, opts("platform.js")).catch(&ctx) {
            anyhow::bail!("platform.js failed: {e}");
        }
        if let Err(e) = ctx.eval_with_options::<(), _>(APP, opts("app.js")).catch(&ctx) {
            anyhow::bail!("app.js failed: {e}");
        }
        Ok(())
    })?;
    drain(&rt);
    let mut last_status = Instant::now() - Duration::from_secs(5);
    let start = Instant::now();
    loop {
        drain(&rt);
        while let Ok(c) = net_rx.try_recv() {
            ctx.with(|ctx| handle_completion(&ctx, &n, c));
            drain(&rt);
        }
        let cdm_events: Vec<(u32, CdmEvent)> = {
            let mut st = n.borrow_mut();
            let mut out = Vec::new();
            for (id, c) in st.cdms.iter_mut() {
                for e in c.take_events() {
                    out.push((*id, e));
                }
                for e in c.pump() {
                    out.push((*id, e));
                }
            }
            out
        };
        for (id, e) in cdm_events {
            ctx.with(|ctx| dispatch_cdm(&ctx, id, e));
            drain(&rt);
        }
        let mut next_ms = 5.0f64;
        for _ in 0..500 {
            let r: f64 = ctx.with(|ctx| call(&ctx, "__platform", "runTimers", ()).and_then(|v| v.as_number()).unwrap_or(-1.0));
            drain(&rt);
            if r != -2.0 {
                if r >= 0.0 {
                    next_ms = next_ms.min(r);
                }
                break;
            }
        }
        let media_events: Vec<(u32, String)> = {
            let mut guard = n.borrow_mut();
            let st = &mut *guard;
            let mut out = Vec::new();
            for (id, m) in st.medias.iter_mut() {
                m.pump(&mut st.sbs, &st.mses, &mut st.cdms);
                if !m.events.is_empty() {
                    let evs = std::mem::take(&mut m.events);
                    out.push((*id, json!({"events": evs, "state": serde_json::from_str::<J>(&media_state(m)).unwrap()}).to_string()));
                }
            }
            out
        };
        for (id, payload) in media_events {
            ctx.with(|ctx| {
                call(&ctx, "__platform", "dispatchMedia", (id, payload));
            });
            drain(&rt);
        }
        if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            let (platform, app, phase) = ctx.with(|ctx| {
                let p = call(&ctx, "__platform", "statusJson", ()).and_then(|v| v.as_string().and_then(|s| s.to_string().ok())).unwrap_or_else(|| "null".into());
                let a = call(&ctx, "__app", "statusJson", ()).and_then(|v| v.as_string().and_then(|s| s.to_string().ok())).unwrap_or_else(|| "null".into());
                let ph = serde_json::from_str::<J>(&a).ok().and_then(|v| v.get("phase").and_then(|x| x.as_str()).map(String::from)).unwrap_or_default();
                (p, a, ph)
            });
            write_status(&args.status, &n, platform, app, &phase);
        }
        if start.elapsed().as_secs_f64() > args.max_seconds {
            break;
        }
        let wait = next_ms.clamp(1.0, 5.0);
        std::thread::sleep(Duration::from_micros((wait * 1000.0) as u64));
    }
    Ok(())
}
