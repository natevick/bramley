use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use pulseaudio::protocol;
use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions};
use symphonia::core::formats::Packet;

#[repr(C)]
struct ShimCallbacks {
    ctx: *mut c_void,
    on_initialized: extern "C" fn(*mut c_void, bool),
    on_new_session: extern "C" fn(*mut c_void, u32, *const c_char, u32),
    on_resolve: extern "C" fn(*mut c_void, u32),
    on_reject: extern "C" fn(*mut c_void, u32, u32, u32, *const c_char, u32),
    on_message: extern "C" fn(*mut c_void, *const c_char, u32, u32, *const c_char, u32),
    on_key: extern "C" fn(*mut c_void, *const u8, u32, u32, u32),
    on_keys_done: extern "C" fn(*mut c_void, bool),
    on_log: extern "C" fn(*mut c_void, *const c_char),
}

unsafe extern "C" {
    fn shim_load(path: *const c_char, err: *mut c_char, err_cap: u32) -> i32;
    fn shim_cdm_version() -> *const c_char;
    fn shim_requested_host_version() -> i32;
    fn shim_create(interface_version: i32, callbacks: *const ShimCallbacks) -> *mut c_void;
    fn shim_initialize(handle: *mut c_void);
    fn shim_create_session(handle: *mut c_void, promise_id: u32, init_data: *const u8, init_data_size: u32);
    fn shim_update_session(handle: *mut c_void, promise_id: u32, session_id: *const c_char, session_id_size: u32, response: *const u8, response_size: u32);
    fn shim_decrypt(handle: *mut c_void, data: *const u8, data_size: u32, key_id: *const u8, key_id_size: u32, iv: *const u8, iv_size: u32, subsample_pairs: *const u32, num_subsamples: u32, timestamp: i64, out: *mut u8, out_capacity: u32, out_size: *mut u32) -> u32;
    fn shim_pump(handle: *mut c_void);
}

#[derive(Default)]
struct CdmEvents {
    initialized: Option<bool>,
    session_id: Option<String>,
    messages: Vec<(u32, Vec<u8>)>,
    keys: BTreeMap<String, u32>,
    resolved: Vec<u32>,
    rejects: Vec<String>,
    logs: Vec<String>,
}

fn events<'a>(ctx: *mut c_void) -> &'a mut CdmEvents {
    unsafe { &mut *(ctx as *mut CdmEvents) }
}

fn bytes<'a>(ptr: *const c_char, len: u32) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }
}

extern "C" fn cb_initialized(ctx: *mut c_void, success: bool) {
    events(ctx).initialized = Some(success);
}
extern "C" fn cb_new_session(ctx: *mut c_void, promise: u32, sid: *const c_char, len: u32) {
    let e = events(ctx);
    e.session_id = Some(String::from_utf8_lossy(bytes(sid, len)).into_owned());
    e.resolved.push(promise);
}
extern "C" fn cb_resolve(ctx: *mut c_void, promise: u32) {
    events(ctx).resolved.push(promise);
}
extern "C" fn cb_reject(ctx: *mut c_void, promise: u32, exception: u32, code: u32, msg: *const c_char, len: u32) {
    let text = String::from_utf8_lossy(bytes(msg, len)).into_owned();
    events(ctx).rejects.push(format!("promise={promise} exception={exception} system_code={code} {text}"));
}
extern "C" fn cb_message(ctx: *mut c_void, _sid: *const c_char, _sid_len: u32, kind: u32, msg: *const c_char, len: u32) {
    events(ctx).messages.push((kind, bytes(msg, len).to_vec()));
}
extern "C" fn cb_key(ctx: *mut c_void, kid: *const u8, len: u32, status: u32, _code: u32) {
    let kid = unsafe { std::slice::from_raw_parts(kid, len as usize) };
    events(ctx).keys.insert(hex(kid), status);
}
extern "C" fn cb_keys_done(_ctx: *mut c_void, _has_new: bool) {}
extern "C" fn cb_log(ctx: *mut c_void, msg: *const c_char) {
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy().into_owned();
    eprintln!("CDM-HOST {text}");
    events(ctx).logs.push(text);
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Cdm {
    handle: *mut c_void,
    events: Box<CdmEvents>,
}

impl Cdm {
    fn pump_until(&mut self, timeout: Duration, mut done: impl FnMut(&CdmEvents) -> bool) -> bool {
        let start = Instant::now();
        loop {
            unsafe { shim_pump(self.handle) };
            if done(&self.events) {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn decrypt(&mut self, data: &[u8], kid: &[u8], iv16: &[u8; 16], subsamples: &[(u32, u32)], ts: i64, out: &mut Vec<u8>) -> u32 {
        let pairs: Vec<u32> = subsamples.iter().flat_map(|(c, e)| [*c, *e]).collect();
        out.resize(data.len() + 64, 0);
        let mut size = 0u32;
        let status = unsafe { shim_decrypt(self.handle, data.as_ptr(), data.len() as u32, kid.as_ptr(), kid.len() as u32, iv16.as_ptr(), 16, pairs.as_ptr(), subsamples.len() as u32, ts, out.as_mut_ptr(), out.len() as u32, &mut size) };
        out.truncate(size as usize);
        status
    }
}

struct Args {
    mpd: String,
    lic: String,
    sink: String,
    status: String,
    play_seconds: f64,
    iface: i32,
}

fn parse_args() -> Args {
    let mut a = Args {
        mpd: "https://storage.googleapis.com/shaka-demo-assets/sintel-widevine/dash.mpd".into(),
        lic: "https://cwip-shaka-proxy.appspot.com/no_auth".into(),
        sink: "spike3-null".into(),
        status: "status.json".into(),
        play_seconds: 120.0,
        iface: 10,
    };
    let v: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i + 1 < v.len() {
        match v[i].as_str() {
            "--mpd" => a.mpd = v[i + 1].clone(),
            "--lic" => a.lic = v[i + 1].clone(),
            "--sink" => a.sink = v[i + 1].clone(),
            "--status" => a.status = v[i + 1].clone(),
            "--play-seconds" => a.play_seconds = v[i + 1].parse().unwrap_or(120.0),
            "--iface" => a.iface = v[i + 1].parse().unwrap_or(10),
            _ => {}
        }
        i += 2;
    }
    a
}

fn http_get(url: &str, range: Option<(u64, u64)>) -> Result<Vec<u8>> {
    let mut req = ureq::get(url);
    if let Some((a, b)) = range {
        req = req.header("Range", &format!("bytes={a}-{b}"));
    }
    let mut resp = req.call().with_context(|| format!("GET {url}"))?;
    Ok(resp.body_mut().with_config().limit(64 << 20).read_to_vec()?)
}

fn http_post(url: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let result = ureq::post(url).header("Content-Type", "application/octet-stream").config().http_status_as_error(false).build().send(body);
    match result {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            let body = resp.body_mut().read_to_vec().unwrap_or_default();
            (status, body)
        }
        Err(e) => {
            eprintln!("CDM-HOST license POST error: {e}");
            (0, Vec::new())
        }
    }
}

struct Mpd {
    base: String,
    init: (u64, u64),
    index: (u64, u64),
}

fn parse_mpd(url: &str, text: &str) -> Result<Mpd> {
    let rep = regex::Regex::new(r#"(?s)<Representation[^>]*codecs="mp4a[^"]*"[^>]*>(.*?)</Representation>"#)?;
    let body = rep.captures(text).ok_or_else(|| anyhow!("no mp4a representation"))?.get(1).unwrap().as_str();
    let base = regex::Regex::new(r"<BaseURL>([^<]+)</BaseURL>")?.captures(body).ok_or_else(|| anyhow!("no BaseURL"))?[1].to_string();
    let idx = regex::Regex::new(r#"indexRange="(\d+)-(\d+)""#)?.captures(body).ok_or_else(|| anyhow!("no indexRange"))?;
    let init = regex::Regex::new(r#"<Initialization range="(\d+)-(\d+)""#)?.captures(body).ok_or_else(|| anyhow!("no Initialization"))?;
    let dir = &url[..url.rfind('/').unwrap() + 1];
    Ok(Mpd { base: format!("{dir}{base}"), init: (init[1].parse()?, init[2].parse()?), index: (idx[1].parse()?, idx[2].parse()?) })
}

fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn be64(b: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(b[o..o + 8].try_into().unwrap())
}

fn boxes(b: &[u8], start: usize, end: usize) -> Vec<(String, usize, usize, usize)> {
    let mut out = Vec::new();
    let mut off = start;
    while off + 8 <= end {
        let mut size = be32(b, off) as usize;
        let typ = String::from_utf8_lossy(&b[off + 4..off + 8]).into_owned();
        let mut hdr = 8;
        if size == 1 {
            size = be64(b, off + 8) as usize;
            hdr = 16;
        }
        if size == 0 {
            size = end - off;
        }
        if size < hdr || off + size > end {
            break;
        }
        out.push((typ, off, off + hdr, off + size));
        off += size;
    }
    out
}

fn find_path(b: &[u8], start: usize, end: usize, path: &[&str]) -> Option<(usize, usize, usize)> {
    for (typ, s, body, e) in boxes(b, start, end) {
        if typ == path[0] {
            if path.len() == 1 {
                return Some((s, body, e));
            }
            if let Some(r) = find_path(b, body, e, &path[1..]) {
                return Some(r);
            }
        }
    }
    None
}

struct InitInfo {
    pssh: Vec<u8>,
    kid: Vec<u8>,
    iv_size: usize,
    asc: Vec<u8>,
}

fn descriptor_len(b: &[u8], p: &mut usize) -> usize {
    let mut len = 0usize;
    for _ in 0..4 {
        let c = b[*p];
        *p += 1;
        len = (len << 7) | (c & 0x7f) as usize;
        if c & 0x80 == 0 {
            break;
        }
    }
    len
}

fn asc_from_esds(b: &[u8], body: usize, end: usize) -> Option<Vec<u8>> {
    let mut p = body + 4;
    while p < end {
        let tag = b[p];
        p += 1;
        let len = descriptor_len(b, &mut p);
        match tag {
            0x03 => {
                let flags = b[p + 2];
                p += 3;
                if flags & 0x80 != 0 {
                    p += 2;
                }
                if flags & 0x40 != 0 {
                    p += 1 + b[p] as usize;
                }
                if flags & 0x20 != 0 {
                    p += 2;
                }
            }
            0x04 => p += 13,
            0x05 => return Some(b[p..p + len].to_vec()),
            _ => p += len,
        }
    }
    None
}

fn parse_init(b: &[u8]) -> Result<InitInfo> {
    let (_, moov_body, moov_end) = find_path(b, 0, b.len(), &["moov"]).ok_or_else(|| anyhow!("no moov"))?;
    let mut pssh = None;
    for (typ, s, body, e) in boxes(b, moov_body, moov_end) {
        if typ == "pssh" && hex(&b[body + 4..body + 20]) == "edef8ba979d64acea3c827dcd51d21ed" {
            pssh = Some(b[s..e].to_vec());
        }
    }
    let (_, stsd_body, stsd_end) = find_path(b, moov_body, moov_end, &["trak", "mdia", "minf", "stbl", "stsd"]).ok_or_else(|| anyhow!("no stsd"))?;
    let entries = boxes(b, stsd_body + 8, stsd_end);
    let (_, _, enca_body, enca_end) = entries.iter().find(|x| x.0 == "enca").cloned().ok_or_else(|| anyhow!("no enca"))?;
    let inner = enca_body + 28;
    let (_, tenc_body, _) = find_path(b, inner, enca_end, &["sinf", "schi", "tenc"]).ok_or_else(|| anyhow!("no tenc"))?;
    let iv_size = b[tenc_body + 7] as usize;
    let kid = b[tenc_body + 8..tenc_body + 24].to_vec();
    let (_, esds_body, esds_end) = find_path(b, inner, enca_end, &["esds"]).ok_or_else(|| anyhow!("no esds"))?;
    let asc = asc_from_esds(b, esds_body, esds_end).ok_or_else(|| anyhow!("no AudioSpecificConfig"))?;
    Ok(InitInfo { pssh: pssh.ok_or_else(|| anyhow!("no widevine pssh"))?, kid, iv_size, asc })
}

fn parse_sidx(b: &[u8], index_end: u64) -> Vec<(u64, u64)> {
    let (_, body, _) = match find_path(b, 0, b.len(), &["sidx"]) {
        Some(x) => x,
        None => return Vec::new(),
    };
    let version = b[body];
    let mut p = body + 12;
    let first_offset;
    if version == 0 {
        first_offset = be32(b, p + 4) as u64;
        p += 8;
    } else {
        first_offset = be64(b, p + 8);
        p += 16;
    }
    p += 2;
    let count = be16(b, p) as usize;
    p += 2;
    let mut off = index_end + 1 + first_offset;
    let mut out = Vec::new();
    for i in 0..count {
        let size = (be32(b, p + 12 * i) & 0x7fff_ffff) as u64;
        out.push((off, off + size - 1));
        off += size;
    }
    out
}

struct Sample {
    data: Vec<u8>,
    encrypted: bool,
    iv: Vec<u8>,
    subsamples: Vec<(u32, u32)>,
    ts: i64,
}

fn parse_fragment(b: &[u8], iv_size: usize) -> Vec<Sample> {
    let mut out = Vec::new();
    for (typ, moof_start, moof_body, moof_end) in boxes(b, 0, b.len()) {
        if typ != "moof" {
            continue;
        }
        let Some((_, traf_body, traf_end)) = find_path(b, moof_body, moof_end, &["traf"]) else { continue };
        let mut sdi = 1u32;
        let mut default_size = 0u32;
        let mut default_dur = 0u32;
        let mut base = moof_start as i64;
        let mut decode_time = 0u64;
        let mut sizes: Vec<u32> = Vec::new();
        let mut data_offset = 0i64;
        let mut ivs: Vec<(Vec<u8>, Vec<(u32, u32)>)> = Vec::new();
        for (t, _, body, _) in boxes(b, traf_body, traf_end) {
            match t.as_str() {
                "tfhd" => {
                    let flags = be32(b, body) & 0xff_ffff;
                    let mut p = body + 8;
                    if flags & 0x1 != 0 {
                        base = be64(b, p) as i64;
                        p += 8;
                    }
                    if flags & 0x2 != 0 {
                        sdi = be32(b, p);
                        p += 4;
                    }
                    if flags & 0x8 != 0 {
                        default_dur = be32(b, p);
                        p += 4;
                    }
                    if flags & 0x10 != 0 {
                        default_size = be32(b, p);
                    }
                }
                "tfdt" => {
                    decode_time = if b[body] == 1 { be64(b, body + 4) } else { be32(b, body + 4) as u64 };
                }
                "trun" => {
                    let flags = be32(b, body) & 0xff_ffff;
                    let count = be32(b, body + 4) as usize;
                    let mut p = body + 8;
                    if flags & 0x1 != 0 {
                        data_offset = be32(b, p) as i32 as i64;
                        p += 4;
                    }
                    if flags & 0x4 != 0 {
                        p += 4;
                    }
                    for _ in 0..count {
                        if flags & 0x100 != 0 {
                            p += 4;
                        }
                        let size = if flags & 0x200 != 0 {
                            let s = be32(b, p);
                            p += 4;
                            s
                        } else {
                            default_size
                        };
                        if flags & 0x400 != 0 {
                            p += 4;
                        }
                        if flags & 0x800 != 0 {
                            p += 4;
                        }
                        sizes.push(size);
                    }
                }
                "senc" => {
                    let flags = be32(b, body) & 0xff_ffff;
                    let count = be32(b, body + 4) as usize;
                    let mut p = body + 8;
                    for _ in 0..count {
                        let iv = b[p..p + iv_size].to_vec();
                        p += iv_size;
                        let mut subs = Vec::new();
                        if flags & 0x2 != 0 {
                            let n = be16(b, p) as usize;
                            p += 2;
                            for _ in 0..n {
                                subs.push((be16(b, p) as u32, be32(b, p + 2)));
                                p += 6;
                            }
                        }
                        ivs.push((iv, subs));
                    }
                }
                _ => {}
            }
        }
        let mut pos = (base + data_offset) as usize;
        let encrypted = sdi == 1 && !ivs.is_empty();
        for (i, size) in sizes.iter().enumerate() {
            let end = pos + *size as usize;
            if end > b.len() {
                break;
            }
            let (iv, subs) = if encrypted { ivs[i].clone() } else { (Vec::new(), Vec::new()) };
            out.push(Sample { data: b[pos..end].to_vec(), encrypted, iv, subsamples: subs, ts: (decode_time + (i as u64) * default_dur as u64) as i64 });
            pos = end;
        }
    }
    out
}

fn audio_thread(sink: String, rx: Receiver<Vec<i16>>, played: Arc<AtomicU64>, routed: Arc<AtomicBool>) -> Result<()> {
    let socket_path = pulseaudio::socket_path_from_env().context("PulseAudio socket not found")?;
    let mut sock = BufReader::new(UnixStream::connect(socket_path)?);
    let cookie = pulseaudio::cookie_path_from_env().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    protocol::write_command_message(sock.get_mut(), 0, &protocol::Command::Auth(protocol::AuthParams { version: protocol::MAX_VERSION, supports_shm: false, supports_memfd: false, cookie }), protocol::MAX_VERSION)?;
    let (_, auth) = protocol::read_reply_message::<protocol::AuthReply>(&mut sock, protocol::MAX_VERSION)?;
    let version = std::cmp::min(protocol::MAX_VERSION, auth.version);
    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, CString::new("spike3-cdm-host").unwrap());
    props.set(protocol::Prop::ApplicationProcessId, CString::new(std::process::id().to_string()).unwrap());
    protocol::write_command_message(sock.get_mut(), 1, &protocol::Command::SetClientName(props), version)?;
    let _ = protocol::read_reply_message::<protocol::SetClientNameReply>(&mut sock, version)?;
    protocol::write_command_message(
        sock.get_mut(),
        2,
        &protocol::Command::CreatePlaybackStream(protocol::PlaybackStreamParams {
            sample_spec: protocol::SampleSpec { format: protocol::SampleFormat::S16Le, channels: 2, sample_rate: 48000 },
            channel_map: protocol::ChannelMap::stereo(),
            cvolume: Some(protocol::ChannelVolume::norm(2)),
            sink_name: Some(CString::new(sink).unwrap()),
            ..Default::default()
        }),
        version,
    )?;
    let (_, info) = protocol::read_reply_message::<protocol::CreatePlaybackStreamReply>(&mut sock, version)?;
    routed.store(true, Ordering::Relaxed);
    eprintln!("CDM-HOST audio stream created channel={} sink_index={}", info.channel, info.sink_index);
    let mut pending: Vec<u8> = Vec::new();
    let fill = |want: usize, pending: &mut Vec<u8>| -> Option<Vec<u8>> {
        while pending.len() < want {
            match rx.recv() {
                Ok(chunk) => pending.extend(chunk.iter().flat_map(|s| s.to_le_bytes())),
                Err(_) => break,
            }
        }
        if pending.is_empty() {
            return None;
        }
        let n = want.min(pending.len());
        Some(pending.drain(..n).collect())
    };
    if let Some(first) = fill(info.requested_bytes as usize, &mut pending) {
        protocol::write_memblock(sock.get_mut(), info.channel, &first, 0)?;
        played.fetch_add(first.len() as u64, Ordering::Relaxed);
    }
    loop {
        let (_, msg) = protocol::read_command_message(&mut sock, version)?;
        match msg {
            protocol::Command::Request(protocol::Request { channel, length }) if channel == info.channel => {
                match fill(length as usize, &mut pending) {
                    Some(buf) => {
                        protocol::write_memblock(sock.get_mut(), info.channel, &buf, 0)?;
                        played.fetch_add(buf.len() as u64, Ordering::Relaxed);
                    }
                    None => return Ok(()),
                }
            }
            protocol::Command::Error(e) => bail!("pulse error {e:?}"),
            _ => {}
        }
    }
}

fn make_decoder(asc: &[u8]) -> Result<Box<dyn Decoder>> {
    let mut params = CodecParameters::new();
    params.for_codec(CODEC_TYPE_AAC).with_sample_rate(48000).with_channels(Channels::FRONT_LEFT | Channels::FRONT_RIGHT).with_extra_data(asc.to_vec().into_boxed_slice());
    Ok(symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?)
}

#[derive(Default)]
struct Counters {
    clear_samples: u64,
    encrypted_samples: u64,
    decrypt_ok: u64,
    decrypt_fail: BTreeMap<u32, u64>,
    decoded_clear_frames: u64,
    decoded_decrypted_frames: u64,
    decode_err_clear: u64,
    decode_err_decrypted: u64,
    segments: u64,
}

fn write_status(path: &str, phase: &str, cdm: &Cdm, c: &Counters, extra: &serde_json::Value, played: u64, routed: bool) {
    let keys: serde_json::Map<String, serde_json::Value> = cdm.events.keys.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect();
    let v = serde_json::json!({
        "phase": phase,
        "pid": std::process::id(),
        "initialized": cdm.events.initialized,
        "sessionId": cdm.events.session_id,
        "keys": keys,
        "usable": cdm.events.keys.values().any(|s| *s == 0),
        "rejects": cdm.events.rejects,
        "shimLogs": cdm.events.logs,
        "clearSamples": c.clear_samples,
        "encryptedSamples": c.encrypted_samples,
        "decryptOk": c.decrypt_ok,
        "decryptFail": c.decrypt_fail.iter().map(|(k, v)| (k.to_string(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
        "decodedClearFrames": c.decoded_clear_frames,
        "decodedDecryptedFrames": c.decoded_decrypted_frames,
        "decodedDecryptedSeconds": c.decoded_decrypted_frames as f64 / 48000.0,
        "decodeErrClear": c.decode_err_clear,
        "decodeErrDecrypted": c.decode_err_decrypted,
        "segments": c.segments,
        "playedSeconds": played as f64 / (48000.0 * 4.0),
        "audioStreamCreated": routed,
        "extra": extra,
    });
    let tmp = format!("{path}.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn main() -> Result<()> {
    let args = parse_args();
    let lib = CString::new("/opt/google/chrome/WidevineCdm/_platform_specific/linux_x64/libwidevinecdm.so")?;
    let mut err = vec![0 as c_char; 512];
    let rc = unsafe { shim_load(lib.as_ptr(), err.as_mut_ptr(), err.len() as u32) };
    if rc != 0 {
        bail!("load failed rc={rc}: {}", unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy());
    }
    let version = unsafe { CStr::from_ptr(shim_cdm_version()) }.to_string_lossy().into_owned();
    eprintln!("CDM-HOST cdm_version={version}");
    let mut events = Box::new(CdmEvents::default());
    let callbacks = ShimCallbacks {
        ctx: &mut *events as *mut CdmEvents as *mut c_void,
        on_initialized: cb_initialized,
        on_new_session: cb_new_session,
        on_resolve: cb_resolve,
        on_reject: cb_reject,
        on_message: cb_message,
        on_key: cb_key,
        on_keys_done: cb_keys_done,
        on_log: cb_log,
    };
    let handle = unsafe { shim_create(args.iface, &callbacks) };
    let host_version = unsafe { shim_requested_host_version() };
    eprintln!("CDM-HOST create iface={} handle_null={} host_version_requested={host_version}", args.iface, handle.is_null());
    if handle.is_null() {
        bail!("CreateCdmInstance returned null for interface {} (host version requested {host_version})", args.iface);
    }
    let mut cdm = Cdm { handle, events };
    let mut extra = serde_json::json!({ "cdmVersion": version, "iface": args.iface, "hostVersionRequested": host_version, "lic": args.lic, "mpd": args.mpd });
    let mut counters = Counters::default();
    let played = Arc::new(AtomicU64::new(0));
    let routed = Arc::new(AtomicBool::new(false));

    unsafe { shim_initialize(cdm.handle) };
    let ok = cdm.pump_until(Duration::from_secs(10), |e| e.initialized.is_some());
    eprintln!("CDM-HOST initialized={:?} (waited_ok={ok})", cdm.events.initialized);
    write_status(&args.status, "initialized", &cdm, &counters, &extra, 0, false);
    if cdm.events.initialized != Some(true) {
        write_status(&args.status, "init-failed", &cdm, &counters, &extra, 0, false);
        bail!("CDM did not initialize");
    }

    let mpd_text = String::from_utf8(http_get(&args.mpd, None)?)?;
    let mpd = parse_mpd(&args.mpd, &mpd_text)?;
    let init_bytes = http_get(&mpd.base, Some(mpd.init))?;
    let init = parse_init(&init_bytes)?;
    eprintln!("CDM-HOST track kid={} iv_size={} asc={} pssh_len={}", hex(&init.kid), init.iv_size, hex(&init.asc), init.pssh.len());
    extra["kid"] = serde_json::json!(hex(&init.kid));
    extra["asc"] = serde_json::json!(hex(&init.asc));

    unsafe { shim_create_session(cdm.handle, 1, init.pssh.as_ptr(), init.pssh.len() as u32) };
    let got = cdm.pump_until(Duration::from_secs(10), |e| !e.messages.is_empty() || !e.rejects.is_empty());
    eprintln!("CDM-HOST session={:?} messages={} rejects={:?} got={got}", cdm.events.session_id, cdm.events.messages.len(), cdm.events.rejects);
    let mut license_status = 0u16;
    if let (Some((kind, challenge)), Some(sid)) = (cdm.events.messages.first().cloned(), cdm.events.session_id.clone()) {
        let (status, response) = http_post(&args.lic, &challenge);
        license_status = status;
        eprintln!("CDM-HOST license message_type={kind} challenge_bytes={} http_status={status} response_bytes={}", challenge.len(), response.len());
        extra["licenseChallengeBytes"] = serde_json::json!(challenge.len());
        extra["licenseHttpStatus"] = serde_json::json!(status);
        extra["licenseResponseBytes"] = serde_json::json!(response.len());
        if (200..300).contains(&status) && !response.is_empty() {
            let sid_c = sid.as_bytes();
            unsafe { shim_update_session(cdm.handle, 2, sid_c.as_ptr() as *const c_char, sid_c.len() as u32, response.as_ptr(), response.len() as u32) };
            cdm.pump_until(Duration::from_secs(10), |e| e.resolved.contains(&2) || !e.rejects.is_empty());
        }
    }
    eprintln!("CDM-HOST keys={:?} rejects={:?} license_status={license_status}", cdm.events.keys, cdm.events.rejects);
    write_status(&args.status, "licensed", &cdm, &counters, &extra, 0, false);

    let index_bytes = http_get(&mpd.base, Some(mpd.index))?;
    let segments = parse_sidx(&index_bytes, mpd.index.1);
    extra["segmentCount"] = serde_json::json!(segments.len());

    let (tx, rx): (SyncSender<Vec<i16>>, Receiver<Vec<i16>>) = sync_channel(64);
    let sink = args.sink.clone();
    let played_a = played.clone();
    let routed_a = routed.clone();
    let audio = std::thread::spawn(move || {
        if let Err(e) = audio_thread(sink, rx, played_a, routed_a) {
            eprintln!("CDM-HOST audio thread error: {e:#}");
        }
    });

    let mut decoder = make_decoder(&init.asc)?;
    let mut out = Vec::new();
    let mut last_status = Instant::now();
    let mut consecutive_fail = 0u32;
    let mut phase = "playing";
    'outer: for (a, b) in segments {
        let seg = http_get(&mpd.base, Some((a, b)))?;
        counters.segments += 1;
        for s in parse_fragment(&seg, init.iv_size) {
            unsafe { shim_pump(cdm.handle) };
            let payload: Vec<u8> = if s.encrypted {
                counters.encrypted_samples += 1;
                let mut iv16 = [0u8; 16];
                iv16[..s.iv.len().min(16)].copy_from_slice(&s.iv[..s.iv.len().min(16)]);
                let status = cdm.decrypt(&s.data, &init.kid, &iv16, &s.subsamples, s.ts, &mut out);
                if status != 0 {
                    *counters.decrypt_fail.entry(status).or_default() += 1;
                    consecutive_fail += 1;
                    if consecutive_fail >= 50 {
                        phase = "stalled-decrypt";
                        break 'outer;
                    }
                    continue;
                }
                consecutive_fail = 0;
                counters.decrypt_ok += 1;
                out.clone()
            } else {
                counters.clear_samples += 1;
                s.data
            };
            let packet = Packet::new_from_slice(0, s.ts as u64, 1024, &payload);
            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let frames = decoded.frames() as u64;
                    let mut sb = SampleBuffer::<i16>::new(decoded.capacity() as u64, *decoded.spec());
                    sb.copy_interleaved_ref(decoded);
                    if s.encrypted {
                        counters.decoded_decrypted_frames += frames;
                    } else {
                        counters.decoded_clear_frames += frames;
                    }
                    if tx.send(sb.samples().to_vec()).is_err() {
                        phase = "audio-gone";
                        break 'outer;
                    }
                }
                Err(e) => {
                    if s.encrypted {
                        counters.decode_err_decrypted += 1;
                    } else {
                        counters.decode_err_clear += 1;
                    }
                    if counters.decode_err_decrypted + counters.decode_err_clear < 5 {
                        eprintln!("CDM-HOST decode error encrypted={} {e}", s.encrypted);
                    }
                }
            }
            if last_status.elapsed() > Duration::from_secs(1) {
                write_status(&args.status, phase, &cdm, &counters, &extra, played.load(Ordering::Relaxed), routed.load(Ordering::Relaxed));
                last_status = Instant::now();
            }
            if played.load(Ordering::Relaxed) as f64 / (48000.0 * 4.0) >= args.play_seconds {
                phase = "target-reached";
                break 'outer;
            }
        }
    }
    eprintln!("CDM-HOST loop ended phase={phase}");
    write_status(&args.status, phase, &cdm, &counters, &extra, played.load(Ordering::Relaxed), routed.load(Ordering::Relaxed));
    drop(tx);
    loop {
        unsafe { shim_pump(cdm.handle) };
        write_status(&args.status, &format!("{phase}-idle"), &cdm, &counters, &extra, played.load(Ordering::Relaxed), routed.load(Ordering::Relaxed));
        std::thread::sleep(Duration::from_secs(1));
        if audio.is_finished() && phase == "target-reached" {
            break;
        }
    }
    Ok(())
}
