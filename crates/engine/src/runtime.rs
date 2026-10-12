use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use rquickjs::{
    ArrayBuffer, CatchResultExt, Context, Ctx, Function, Object, Runtime, Value,
    context::EvalOptions, function::IntoArgs,
};
use serde_json::{Value as J, json};

use crate::EngineConfig;
use crate::audio::{AudioSink, PulseSink};
use crate::buffer::{MediaSourceNative, SOURCE_BUFFER_QUOTA_BYTES, SourceBufferNative};
use crate::cdm::{CdmBackend, CdmEvent, Widevine};
use crate::eme::{audio_type_supported, can_play_type, eme_check};
use crate::media::{MediaEngine, SinkFactory};
use crate::mp4;
use crate::net::{self, Completion, HttpRequest};

const PLATFORM: &str = include_str!("../js/platform.js");

pub type CdmFactory = Box<dyn Fn(bool) -> Result<Box<dyn CdmBackend>>>;

struct Native {
    start: Instant,
    next_id: u32,
    mses: HashMap<u32, MediaSourceNative>,
    sbs: HashMap<u32, SourceBufferNative>,
    medias: HashMap<u32, MediaEngine>,
    cdms: HashMap<u32, Box<dyn CdmBackend>>,
    cdm_factory: CdmFactory,
    sink_factory: SinkFactory,
    cdm_error: Option<String>,
    net_tx: Sender<Completion>,
    aborted: HashSet<u32>,
    op_counts: BTreeMap<&'static str, u64>,
    eme_requests: Vec<J>,
    script_log: Vec<J>,
    http_count: u64,
    http_bytes: u64,
    redact: Vec<String>,
    truncate_scripts: bool,
    retired_output: OutputCounts,
    in_flight: usize,
}

impl Native {
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
    fn count(&mut self, op: &'static str) {
        *self.op_counts.entry(op).or_default() += 1;
    }
    fn position_for_buffer(&self, sb_id: u32) -> i64 {
        self.medias
            .values()
            .find(|m| {
                m.ms_id
                    .and_then(|id| self.mses.get(&id))
                    .is_some_and(|ms| ms.sbs.contains(&sb_id))
            })
            .map(|m| m.current_time_us())
            .unwrap_or(0)
    }
}

type N = Rc<RefCell<Native>>;

pub struct Engine {
    ctx: Context,
    rt: Runtime,
    n: N,
    net_rx: Receiver<Completion>,
    last_gc: Instant,
}

const GC_INTERVAL: Duration = Duration::from_secs(2);
const ACTIVE_WAIT_MS: f64 = 5.0;
const IDLE_WAIT: Duration = Duration::from_secs(60);

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

fn media_state(m: &MediaEngine) -> String {
    json!({"currentTime": m.current_time(), "readyState": m.ready_state, "paused": m.paused, "seeking": m.seeking, "ended": m.ended}).to_string()
}

fn install_ops<'js>(ctx: &Ctx<'js>, n: &N) -> rquickjs::Result<()> {
    let g = ctx.globals();
    macro_rules! op {
        ($name:literal, $f:expr) => {
            g.set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }
    macro_rules! with_media {
        ($name:literal, $count:literal, |$m:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {
            let s = n.clone();
            op!($name, move |id: u32 $(, $arg: $ty)*| {
                let mut st = s.borrow_mut();
                st.count($count);
                if let Some($m) = st.medias.get_mut(&id) {
                    $body;
                }
            });
        };
    }
    macro_rules! with_cdm {
        ($name:literal, $count:literal, |$c:ident, $p:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {
            let s = n.clone();
            op!($name, move |keys_id: u32, $p: u32 $(, $arg: $ty)*| {
                let mut st = s.borrow_mut();
                st.count($count);
                if let Some($c) = st.cdms.get_mut(&keys_id) {
                    $body;
                }
            });
        };
    }
    let s = n.clone();
    op!("__op_now", move || -> f64 {
        s.borrow().start.elapsed().as_secs_f64() * 1000.0
    });
    let s = n.clone();
    op!("__op_print", move |level: String, msg: String| {
        eprintln!("JS[{level}] {}", crate::redact(&msg, &s.borrow().redact));
    });
    op!("__op_user_agent", crate::user_agent);
    op!("__op_random", move |ctx: Ctx<'js>,
                             len: u32|
          -> rquickjs::Result<
        ArrayBuffer<'js>,
    > {
        let mut buf = vec![0u8; len as usize];
        if let Err(e) = getrandom::getrandom(&mut buf) {
            return Err(rquickjs::Exception::throw_internal(
                &ctx,
                &format!("crypto.getRandomValues: no system randomness: {e}"),
            ));
        }
        ArrayBuffer::new(ctx, buf)
    });
    let s = n.clone();
    op!("__op_http", move |id: u32,
                           method: String,
                           url: String,
                           headers: String,
                           body: Value<'js>,
                           timeout: f64,
                           cache: String| {
        let mut st = s.borrow_mut();
        st.count("http");
        st.http_count += 1;
        st.in_flight += 1;
        let headers: Vec<(String, String)> = serde_json::from_str(&headers).unwrap_or_default();
        let req = HttpRequest {
            id,
            method,
            url,
            headers,
            body: bytes_of(&body),
            timeout_ms: if timeout > 0.0 {
                timeout as u64
            } else {
                60_000
            },
            use_cache: cache == "default",
        };
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
        st.in_flight += 1;
        st.script_log
            .push(json!({"id": id, "url": url, "phase": "fetching"}));
        net::spawn_script(id, url, st.net_tx.clone());
    });
    let s = n.clone();
    op!("__op_ms_create", move || -> u32 {
        let mut st = s.borrow_mut();
        st.count("ms_create");
        let id = st.id();
        st.mses.insert(
            id,
            MediaSourceNative {
                duration: f64::NAN,
                ended: false,
                sbs: Vec::new(),
            },
        );
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
        st.sbs
            .insert(id, SourceBufferNative::new(mime, SOURCE_BUFFER_QUOTA_BYTES));
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
    op!("__op_sb_prepare_append", move |sb_id: u32,
                                        len: u32|
          -> bool {
        let mut st = s.borrow_mut();
        st.count("sb_prepare_append");
        let position = st.position_for_buffer(sb_id);
        st.sbs
            .get_mut(&sb_id)
            .is_some_and(|sb| sb.make_room(len as usize, position))
    });
    let s = n.clone();
    op!("__op_sb_append", move |sb_id: u32,
                                data: Value<'js>,
                                ts_offset: f64|
          -> String {
        let mut st = s.borrow_mut();
        st.count("sb_append");
        let Some(bytes) = bytes_of(&data) else {
            return json!({"error": "not an ArrayBuffer"}).to_string();
        };
        let Some(sb) = st.sbs.get_mut(&sb_id) else {
            return json!({"error": "no such SourceBuffer"}).to_string();
        };
        match sb.append(&bytes, if ts_offset.is_finite() { ts_offset } else { 0.0 }) {
            Ok(r) => json!({"init": r.init_segment, "frames": r.media_frames, "hasInitData": r.init_segment && sb.init_data.is_some(), "initDuration": sb.init.as_ref().and_then(|i| i.duration_secs), "end": sb.end_us().map(|e| e as f64 / 1e6)}).to_string(),
            Err(e) => json!({"error": format!("{e:#}")}).to_string(),
        }
    });
    let s = n.clone();
    op!(
        "__op_sb_set_append_window",
        move |sb_id: u32, start: f64, end: f64| {
            if let Some(sb) = s.borrow_mut().sbs.get_mut(&sb_id) {
                let us = |t: f64| (t * 1e6).round() as i64;
                sb.append_window_us = (us(start), us(end));
            }
        }
    );
    let s = n.clone();
    op!("__op_sb_init_data", move |ctx: Ctx<'js>,
                                   sb_id: u32|
          -> rquickjs::Result<
        Value<'js>,
    > {
        match s
            .borrow()
            .sbs
            .get(&sb_id)
            .and_then(|sb| sb.init_data.clone())
        {
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
        if let Some(sb) = s.borrow_mut().sbs.get_mut(&sb_id) {
            sb.abort();
        }
    });
    let s = n.clone();
    op!("__op_sb_change_type", move |sb_id: u32,
                                     mime: String|
          -> bool {
        if !audio_type_supported(&mime) {
            return false;
        }
        if let Some(sb) = s.borrow_mut().sbs.get_mut(&sb_id) {
            sb.mime = mime;
        }
        true
    });
    let s = n.clone();
    op!("__op_sb_buffered", move |sb_id: u32| -> String {
        let r: Vec<[f64; 2]> = s
            .borrow()
            .sbs
            .get(&sb_id)
            .map(|sb| sb.buffered().into_iter().map(|(a, b)| [a, b]).collect())
            .unwrap_or_default();
        serde_json::to_string(&r).unwrap()
    });
    let s = n.clone();
    op!("__op_ms_set_duration", move |ms_id: u32, d: f64| {
        if let Some(ms) = s.borrow_mut().mses.get_mut(&ms_id) {
            ms.duration = d;
        }
    });
    let s = n.clone();
    op!("__op_ms_end", move |ms_id: u32, ended: bool| {
        if let Some(ms) = s.borrow_mut().mses.get_mut(&ms_id) {
            ms.ended = ended;
        }
    });
    let s = n.clone();
    op!("__op_media_create", move || -> u32 {
        let mut st = s.borrow_mut();
        st.count("media_create");
        let id = st.id();
        let m = MediaEngine::new(st.sink_factory.clone());
        st.medias.insert(id, m);
        id
    });
    with_media!("__op_media_attach", "media_attach", |m, ms_id: u32| m
        .attach(ms_id));
    with_media!("__op_media_detach", "media_detach", |m| m.detach());
    with_media!("__op_media_play", "media_play", |m| m.play());
    with_media!("__op_media_pause", "media_pause", |m| m.pause());
    with_media!("__op_media_seek", "media_seek", |m, t: f64| m
        .seek((t * 1e6).round() as i64));
    with_media!(
        "__op_media_set_loop",
        "media_set_loop",
        |m, looping: bool| m.looping = looping
    );
    with_media!("__op_media_set_gain", "media_set_gain", |m, gain: f64| m
        .set_gain(gain as f32));
    with_media!(
        "__op_media_set_keys",
        "media_set_keys",
        |m, keys_id: u32| m.set_keys(Some(keys_id).filter(|k| *k != 0))
    );
    let s = n.clone();
    op!("__op_media_state", move |id: u32| -> String {
        s.borrow()
            .medias
            .get(&id)
            .map(media_state)
            .unwrap_or_else(|| "{}".into())
    });
    let s = n.clone();
    op!("__op_destroy", move |kind: String, id: u32| {
        let mut st = s.borrow_mut();
        st.count("destroy");
        match kind.as_str() {
            "ms" => {
                for sb in st.mses.remove(&id).map(|ms| ms.sbs).unwrap_or_default() {
                    st.sbs.remove(&sb);
                }
            }
            "media" => {
                if let Some(m) = st.medias.remove(&id) {
                    let counts = output_counts(&m.counters);
                    st.retired_output = add(st.retired_output, counts);
                }
            }
            "cdm" => drop(st.cdms.remove(&id)),
            _ => {}
        }
    });
    op!("__op_can_play_type", |mime: String| can_play_type(&mime));
    op!("__op_type_supported", |mime: String| audio_type_supported(
        &mime
    ));
    let s = n.clone();
    op!("__op_eme_check", move |key_system: String,
                                configs: String|
          -> String {
        let mut st = s.borrow_mut();
        st.count("eme_check");
        let r = eme_check(&key_system, &configs);
        st.eme_requests.push(json!({"keySystem": key_system, "requested": serde_json::from_str::<J>(&configs).unwrap_or(J::Null), "granted": r.clone()}));
        r.map(|x| x.to_string()).unwrap_or_default()
    });
    let s = n.clone();
    op!(
        "__op_cdm_create",
        move |allow_persistent_state: bool| -> i32 {
            let mut st = s.borrow_mut();
            st.count("cdm_create");
            match (st.cdm_factory)(allow_persistent_state) {
                Ok(c) => {
                    let id = st.id();
                    st.cdms.insert(id, c);
                    id as i32
                }
                Err(e) => {
                    eprintln!("ENGINE cdm create failed: {e:#}");
                    st.cdm_error = Some(format!("{e:#}"));
                    -1
                }
            }
        }
    );
    with_cdm!(
        "__op_cdm_set_cert",
        "cdm_set_cert",
        |c, promise, data: Value<'js>| c
            .set_server_certificate(promise, &bytes_of(&data).unwrap_or_default())
    );
    with_cdm!(
        "__op_cdm_generate",
        "cdm_generate",
        |c, promise, init_type: u32, data: Value<'js>| c.create_session(
            promise,
            init_type,
            &bytes_of(&data).unwrap_or_default()
        )
    );
    with_cdm!(
        "__op_cdm_update",
        "cdm_update",
        |c, promise, sid: String, data: Value<'js>| c.update_session(
            promise,
            &sid,
            &bytes_of(&data).unwrap_or_default()
        )
    );
    with_cdm!("__op_cdm_close", "cdm_close", |c, promise, sid: String| c
        .close_session(promise, &sid));
    with_cdm!(
        "__op_cdm_remove",
        "cdm_remove",
        |c, promise, sid: String| c.remove_session(promise, &sid)
    );
    Ok(())
}

fn call<'js, A: IntoArgs<'js>>(
    ctx: &Ctx<'js>,
    n: &N,
    obj: &str,
    name: &str,
    args: A,
) -> Option<Value<'js>> {
    let r = (|| -> rquickjs::Result<Value<'js>> {
        let p: Object = ctx.globals().get(obj)?;
        let f: Function = p.get(name)?;
        f.call::<_, Value>(args)
    })();
    match r.catch(ctx) {
        Ok(v) => Some(v),
        Err(e) => {
            let msg = crate::redact(&e.to_string(), &n.borrow().redact);
            eprintln!("ENGINE call {obj}.{name} failed: {msg}");
            None
        }
    }
}

fn buffer_or_null<'js>(ctx: &Ctx<'js>, body: Option<Vec<u8>>) -> Value<'js> {
    body.and_then(|b| ArrayBuffer::new(ctx.clone(), b).ok())
        .map(|b| b.into_value())
        .unwrap_or_else(|| Value::new_null(ctx.clone()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    mp4::hex(&Sha256::digest(bytes))
}

fn handle_completion(ctx: &Ctx, n: &N, c: Completion) {
    n.borrow_mut().in_flight -= 1;
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
            call(
                ctx,
                n,
                "__platform",
                "dispatchHttp",
                (r.id, meta, buffer_or_null(ctx, Some(r.body))),
            );
        }
        Completion::Script { id, url, result } => {
            let mut entry = json!({"id": id, "url": url});
            let outcome = result.and_then(|mut bytes| {
                entry["fetchedBytes"] = json!(bytes.len());
                entry["fetchedSha256"] = json!(sha256_hex(&bytes));
                if n.borrow().truncate_scripts {
                    bytes.truncate(bytes.len() / 2);
                }
                entry["evaluatedBytes"] = json!(bytes.len());
                entry["evaluatedSha256"] = json!(sha256_hex(&bytes));
                let t = Instant::now();
                ctx.eval_with_options::<(), _>(bytes, opts(&url))
                    .catch(ctx)
                    .map(|_| t.elapsed())
                    .map_err(|e| e.to_string())
            });
            let (ok, err) = match outcome {
                Ok(took) => {
                    entry["phase"] = json!("evaluated");
                    entry["evalMs"] = json!(took.as_secs_f64() * 1000.0);
                    (true, String::new())
                }
                Err(e) => {
                    entry["phase"] = json!("error");
                    entry["error"] = json!(e);
                    (false, e)
                }
            };
            n.borrow_mut().script_log.push(entry);
            call(ctx, n, "__platform", "dispatchScript", (id, ok, err));
        }
    }
}

fn dispatch_cdm(ctx: &Ctx, n: &N, keys_id: u32, e: CdmEvent) {
    let (meta, body) = match e {
        CdmEvent::Initialized(ok) => (json!({"type": "initialized", "ok": ok}), None),
        CdmEvent::NewSession { promise, sid } => (
            json!({"type": "newSession", "promise": promise, "sid": sid}),
            None,
        ),
        CdmEvent::Resolve(p) => (json!({"type": "resolve", "promise": p}), None),
        CdmEvent::Reject {
            promise,
            exception,
            system_code,
            message,
        } => (
            json!({"type": "reject", "promise": promise, "exception": exception, "code": system_code, "message": message}),
            None,
        ),
        CdmEvent::Message { sid, kind, body } => (
            json!({"type": "message", "sid": sid, "kind": kind}),
            Some(body),
        ),
        CdmEvent::Keys { sid, keys } => (
            json!({"type": "keys", "sid": sid, "keys": keys.iter().map(|(k, s)| json!({"kid": mp4::hex(k), "status": s})).collect::<Vec<_>>()}),
            None,
        ),
        CdmEvent::Expiration { sid, expiry } => (
            json!({"type": "expiration", "sid": sid, "expiry": expiry}),
            None,
        ),
        CdmEvent::Closed { sid } => (json!({"type": "closed", "sid": sid}), None),
    };
    call(
        ctx,
        n,
        "__platform",
        "dispatchCdm",
        (keys_id, meta.to_string(), buffer_or_null(ctx, body)),
    );
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Engine> {
        let EngineConfig {
            cdm_path,
            audio_sink_name,
            output_device,
            redact,
        } = config;
        let cdm: CdmFactory = Box::new(move |persistent| Widevine::create(&cdm_path, persistent));
        let sink: SinkFactory = Rc::new(move || {
            Box::new(PulseSink::start(
                audio_sink_name.clone(),
                output_device.clone(),
            )) as Box<dyn AudioSink>
        });
        crate::net::clear_cache();
        let engine = Engine::with_backends(cdm, sink)?;
        {
            let mut st = engine.n.borrow_mut();
            st.redact = redact;
        }
        Ok(engine)
    }

    pub(crate) fn with_backends(
        cdm_factory: CdmFactory,
        sink_factory: SinkFactory,
    ) -> Result<Engine> {
        let (net_tx, net_rx) = channel();
        let n: N = Rc::new(RefCell::new(Native {
            start: Instant::now(),
            next_id: 0,
            mses: HashMap::new(),
            sbs: HashMap::new(),
            medias: HashMap::new(),
            cdms: HashMap::new(),
            cdm_factory,
            sink_factory,
            cdm_error: None,
            net_tx,
            aborted: HashSet::new(),
            op_counts: BTreeMap::new(),
            eme_requests: Vec::new(),
            script_log: Vec::new(),
            http_count: 0,
            http_bytes: 0,
            redact: Vec::new(),
            truncate_scripts: false,
            retired_output: [0; OUTPUT_COUNTS],
            in_flight: 0,
        }));
        let rt = Runtime::new()?;
        rt.set_max_stack_size(4 * 1024 * 1024);
        let ctx = Context::full(&rt)?;
        ctx.with(|ctx| install_ops(&ctx, &n))
            .map_err(|e| anyhow!("install ops: {e}"))?;
        let engine = Engine {
            ctx,
            rt,
            n,
            net_rx,
            last_gc: Instant::now(),
        };
        engine.eval_script("platform.js", PLATFORM)?;
        #[cfg(feature = "trace")]
        engine.eval_script("trace.js", include_str!("../js/trace.js"))?;
        Ok(engine)
    }

    fn drain(&self) {
        for _ in 0..100_000 {
            match self.rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => eprintln!("ENGINE job error: {e:?}"),
            }
        }
    }

    #[cfg(any(test, feature = "sabotage"))]
    pub fn sabotage_truncate_scripts(&self) {
        self.n.borrow_mut().truncate_scripts = true;
    }

    #[cfg(any(test, feature = "diagnostics"))]
    pub fn enable_diagnostics(&self) -> Result<()> {
        self.eval_script("diagnostics.js", include_str!("../js/diagnostics.js"))
    }

    pub fn eval_script(&self, name: &str, source: &str) -> Result<()> {
        let r = self.ctx.with(|ctx| {
            ctx.eval_with_options::<(), _>(source, opts(name))
                .catch(&ctx)
                .map_err(|e| anyhow!("{name} failed: {e}"))
        });
        self.drain();
        r
    }

    pub fn eval_json(&self, expression: &str) -> Result<J> {
        let src = format!("JSON.stringify(({expression}))");
        let r = self.ctx.with(|ctx| {
            ctx.eval_with_options::<Option<String>, _>(src, opts("eval"))
                .catch(&ctx)
                .map_err(|e| anyhow!("eval failed: {e}"))
        });
        self.drain();
        Ok(r?
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or(J::Null))
    }

    pub(crate) fn global_number(&self, object: &str, key: &str) -> Option<f64> {
        self.ctx
            .with(|ctx| ctx.globals().get::<_, Object>(object)?.get::<_, f64>(key))
            .ok()
    }

    pub fn run_gc(&mut self) {
        self.rt.run_gc();
        self.last_gc = Instant::now();
        self.drain();
    }

    fn run_tasks(&self) -> (bool, Option<f64>) {
        for ran in 0..500 {
            let r: f64 = self.ctx.with(|ctx| {
                call(&ctx, &self.n, "__platform", "runTimers", ())
                    .and_then(|v| v.as_number())
                    .unwrap_or(-1.0)
            });
            self.drain();
            if r != -2.0 {
                return (ran > 0, (r >= 0.0).then_some(r));
            }
        }
        (true, None)
    }

    pub fn tick(&mut self) -> Duration {
        if self.last_gc.elapsed() >= GC_INTERVAL {
            self.run_gc();
        }
        self.drain();
        let mut busy = false;
        while let Ok(c) = self.net_rx.try_recv() {
            busy = true;
            self.ctx.with(|ctx| handle_completion(&ctx, &self.n, c));
            self.drain();
            self.run_tasks();
        }
        let cdm_events: Vec<(u32, CdmEvent)> = self
            .n
            .borrow_mut()
            .cdms
            .iter_mut()
            .flat_map(|(id, c)| c.poll().into_iter().map(move |e| (*id, e)))
            .collect();
        busy |= !cdm_events.is_empty();
        for (id, e) in cdm_events {
            self.ctx.with(|ctx| dispatch_cdm(&ctx, &self.n, id, e));
            self.drain();
        }
        let (ran, next_ms) = self.run_tasks();
        busy |= ran;
        let now = Instant::now();
        let media_events: Vec<(u32, String)> = {
            let mut guard = self.n.borrow_mut();
            let st = &mut *guard;
            let mut out = Vec::new();
            for (id, m) in st.medias.iter_mut() {
                m.pump(now, &st.sbs, &st.mses, &mut st.cdms);
                if !m.events.is_empty() {
                    let events = std::mem::take(&mut m.events);
                    out.push((
                        *id,
                        format!(
                            "{{\"events\":{},\"state\":{}}}",
                            json!(events),
                            media_state(m)
                        ),
                    ));
                }
            }
            out
        };
        busy |= !media_events.is_empty();
        for (id, payload) in media_events {
            self.ctx.with(|ctx| {
                call(&ctx, &self.n, "__platform", "dispatchMedia", (id, payload));
            });
            self.drain();
            self.run_tasks();
        }
        let st = self.n.borrow();
        let active = busy
            || st.in_flight > st.aborted.len()
            || self.rt.is_job_pending()
            || st.medias.values().any(|m| !m.idle());
        if active {
            let ms = next_ms.unwrap_or(ACTIVE_WAIT_MS).clamp(1.0, ACTIVE_WAIT_MS);
            return Duration::from_micros((ms * 1000.0) as u64);
        }
        let js = next_ms.map(|ms| Duration::from_micros((ms.max(1.0) * 1000.0) as u64));
        let cdm = st.cdms.values().filter_map(|c| c.next_timer()).min();
        js.into_iter().chain(cdm).min().unwrap_or(IDLE_WAIT)
    }

    pub fn run_for(&mut self, d: Duration) {
        let end = Instant::now() + d;
        while Instant::now() < end {
            let wait = self.tick();
            std::thread::sleep(wait.min(end.saturating_duration_since(Instant::now())));
        }
    }

    pub fn current_time(&self) -> Option<f64> {
        let st = self.n.borrow();
        st.medias
            .iter()
            .filter(|(_, m)| m.keys_id.is_some())
            .min_by_key(|(id, _)| **id)
            .map(|(_, m)| m.current_time())
    }

    pub fn output_failed(&self) -> bool {
        self.n.borrow().medias.values().any(|m| m.output_failed())
    }

    pub fn output_counts(&self) -> OutputCounts {
        let st = self.n.borrow();
        let live = st.medias.values().map(|m| output_counts(&m.counters));
        live.fold(st.retired_output, add)
    }

    pub fn status(&self) -> J {
        let st = self.n.borrow();
        let medias: serde_json::Map<String, J> = st
            .medias
            .iter()
            .map(|(id, m)| {
                let c = &m.counters;
                let rate = if m.rate > 0 { m.rate as f64 } else { 48000.0 };
                let v = json!({
                    "currentTime": m.current_time(), "maxCurrentTime": m.max_current_time_us as f64 / 1e6, "outputPosition": m.output_position(),
                    "readyState": m.ready_state, "paused": m.paused, "seeking": m.seeking, "ended": m.ended, "waitingForKey": m.waiting_for_key, "keysId": m.keys_id,
                    "rate": m.rate, "channels": m.channels, "clearSamples": c.clear_samples, "encryptedSamples": c.encrypted_samples, "decryptOk": c.decrypt_ok,
                    "decryptFail": c.decrypt_fail.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
                    "decodedClearSeconds": c.decoded_clear_frames as f64 / rate, "decodedDecryptedSeconds": c.decoded_decrypted_frames as f64 / rate,
                    "decodeErr": c.decode_err, "keyWaits": c.key_waits, "underruns": c.underruns, "seeks": c.seeks, "loops": c.loops,
                    "drainsAcked": c.drains_acked, "drainTimeouts": c.drain_timeouts, "sinkRestarts": c.sink_restarts,
                });
                (id.to_string(), v)
            })
            .collect();
        let sbs: serde_json::Map<String, J> = st
            .sbs
            .iter()
            .map(|(id, sb)| {
                let init = sb.init.as_ref().map(|i| json!({"codec": i.codec, "timescale": i.timescale, "channels": i.channels, "sampleRate": i.sample_rate, "protected": i.protection.as_ref().map(|p| json!({"scheme": p.scheme, "kid": mp4::hex(&p.kid), "ivSize": p.iv_size})), "psshBytes": i.pssh.len()}));
                (id.to_string(), json!({"mime": sb.mime, "appendedBytes": sb.appended_bytes, "bytes": sb.bytes, "evictedFrames": sb.evicted_frames, "frames": sb.frames.len(), "buffered": sb.buffered(), "init": init}))
            })
            .collect();
        let platform = self
            .ctx
            .with(|ctx| {
                call(&ctx, &self.n, "__platform", "statusJson", ())
                    .and_then(|v| v.as_string().and_then(|s| s.to_string().ok()))
            })
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(J::Null);
        json!({
            "pid": std::process::id(),
            "elapsedSec": st.start.elapsed().as_secs_f64(),
            "opCounts": st.op_counts,
            "live": {"medias": st.medias.len(), "mediaSources": st.mses.len(), "sourceBuffers": st.sbs.len(), "cdms": st.cdms.len()},
            "medias": medias,
            "sourceBuffers": sbs,
            "cdm": st.cdm_error.as_ref().map(|e| json!({"error": e})).or_else(|| st.cdms.values().next().map(|c| c.describe())),
            "emeRequests": st.eme_requests,
            "scripts": st.script_log,
            "http": {"count": st.http_count, "bytes": st.http_bytes},
            "platform": platform,
        })
    }
}

pub const OUTPUT_COUNTS: usize = 7;
pub type OutputCounts = [u64; OUTPUT_COUNTS];

pub(crate) fn output_counts(c: &crate::media::Counters) -> OutputCounts {
    let decrypt_fails = c.decrypt_fail.values().sum();
    [
        c.drains_acked,
        c.drain_timeouts,
        c.sent_us,
        c.skipped_us,
        c.decrypted_us,
        c.underruns,
        decrypt_fails,
    ]
}

fn add(a: OutputCounts, b: OutputCounts) -> OutputCounts {
    std::array::from_fn(|i| a[i] + b[i])
}
