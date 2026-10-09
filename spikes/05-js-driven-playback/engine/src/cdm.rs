use std::ffi::{CStr, CString, c_char, c_void};

use anyhow::{Result, bail};

pub const CDM_PATH: &str = "/opt/google/chrome/WidevineCdm/_platform_specific/linux_x64/libwidevinecdm.so";

#[repr(C)]
struct ShimCallbacks {
    ctx: *mut c_void,
    on_initialized: extern "C" fn(*mut c_void, bool),
    on_new_session: extern "C" fn(*mut c_void, u32, *const c_char, u32),
    on_resolve: extern "C" fn(*mut c_void, u32),
    on_reject: extern "C" fn(*mut c_void, u32, u32, u32, *const c_char, u32),
    on_message: extern "C" fn(*mut c_void, *const c_char, u32, u32, *const c_char, u32),
    on_key: extern "C" fn(*mut c_void, *const c_char, u32, *const u8, u32, u32, u32),
    on_keys_done: extern "C" fn(*mut c_void, *const c_char, u32, bool),
    on_expiration: extern "C" fn(*mut c_void, *const c_char, u32, f64),
    on_closed: extern "C" fn(*mut c_void, *const c_char, u32),
    on_log: extern "C" fn(*mut c_void, *const c_char),
}

unsafe extern "C" {
    fn shim_load(path: *const c_char, err: *mut c_char, err_cap: u32) -> i32;
    fn shim_cdm_version() -> *const c_char;
    fn shim_requested_host_version() -> i32;
    fn shim_create(interface_version: i32, callbacks: *const ShimCallbacks) -> *mut c_void;
    fn shim_initialize(handle: *mut c_void);
    fn shim_set_server_certificate(handle: *mut c_void, promise_id: u32, data: *const u8, size: u32);
    fn shim_create_session(handle: *mut c_void, promise_id: u32, init_data_type: u32, init_data: *const u8, init_data_size: u32);
    fn shim_update_session(handle: *mut c_void, promise_id: u32, session_id: *const c_char, session_id_size: u32, response: *const u8, response_size: u32);
    fn shim_close_session(handle: *mut c_void, promise_id: u32, session_id: *const c_char, session_id_size: u32);
    fn shim_remove_session(handle: *mut c_void, promise_id: u32, session_id: *const c_char, session_id_size: u32);
    fn shim_decrypt(handle: *mut c_void, scheme: u32, crypt: u32, skip: u32, data: *const u8, data_size: u32, key_id: *const u8, key_id_size: u32, iv: *const u8, iv_size: u32, subsample_pairs: *const u32, num_subsamples: u32, timestamp: i64, out: *mut u8, out_capacity: u32, out_size: *mut u32) -> u32;
    fn shim_pump(handle: *mut c_void);
    fn shim_destroy(handle: *mut c_void);
}

#[derive(Debug, Clone)]
pub enum CdmEvent {
    Initialized(bool),
    NewSession { promise: u32, sid: String },
    Resolve(u32),
    Reject { promise: u32, exception: u32, system_code: u32, message: String },
    Message { sid: String, kind: u32, body: Vec<u8> },
    Keys { sid: String, keys: Vec<(Vec<u8>, u32)> },
    Expiration { sid: String, expiry: f64 },
    Closed { sid: String },
}

#[derive(Default)]
struct Sink {
    events: Vec<CdmEvent>,
    pending_keys: Vec<(String, Vec<u8>, u32)>,
    logs: Vec<String>,
}

fn sink<'a>(ctx: *mut c_void) -> &'a mut Sink {
    unsafe { &mut *(ctx as *mut Sink) }
}

fn bytes<'a>(ptr: *const c_char, len: u32) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }
}

fn text(ptr: *const c_char, len: u32) -> String {
    String::from_utf8_lossy(bytes(ptr, len)).into_owned()
}

extern "C" fn cb_initialized(ctx: *mut c_void, success: bool) {
    sink(ctx).events.push(CdmEvent::Initialized(success));
}
extern "C" fn cb_new_session(ctx: *mut c_void, promise: u32, sid: *const c_char, len: u32) {
    sink(ctx).events.push(CdmEvent::NewSession { promise, sid: text(sid, len) });
}
extern "C" fn cb_resolve(ctx: *mut c_void, promise: u32) {
    sink(ctx).events.push(CdmEvent::Resolve(promise));
}
extern "C" fn cb_reject(ctx: *mut c_void, promise: u32, exception: u32, code: u32, msg: *const c_char, len: u32) {
    sink(ctx).events.push(CdmEvent::Reject { promise, exception, system_code: code, message: text(msg, len) });
}
extern "C" fn cb_message(ctx: *mut c_void, sid: *const c_char, sid_len: u32, kind: u32, msg: *const c_char, len: u32) {
    sink(ctx).events.push(CdmEvent::Message { sid: text(sid, sid_len), kind, body: bytes(msg, len).to_vec() });
}
extern "C" fn cb_key(ctx: *mut c_void, sid: *const c_char, sid_len: u32, kid: *const u8, len: u32, status: u32, _code: u32) {
    let kid = unsafe { std::slice::from_raw_parts(kid, len as usize) }.to_vec();
    sink(ctx).pending_keys.push((text(sid, sid_len), kid, status));
}
extern "C" fn cb_keys_done(ctx: *mut c_void, sid: *const c_char, sid_len: u32, _has_new: bool) {
    let s = sink(ctx);
    let sid = text(sid, sid_len);
    let keys: Vec<(Vec<u8>, u32)> = s.pending_keys.drain(..).filter(|k| k.0 == sid).map(|k| (k.1, k.2)).collect();
    s.events.push(CdmEvent::Keys { sid, keys });
}
extern "C" fn cb_expiration(ctx: *mut c_void, sid: *const c_char, sid_len: u32, expiry: f64) {
    sink(ctx).events.push(CdmEvent::Expiration { sid: text(sid, sid_len), expiry });
}
extern "C" fn cb_closed(ctx: *mut c_void, sid: *const c_char, sid_len: u32) {
    sink(ctx).events.push(CdmEvent::Closed { sid: text(sid, sid_len) });
}
extern "C" fn cb_log(ctx: *mut c_void, msg: *const c_char) {
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy().into_owned();
    eprintln!("CDM-HOST {text}");
    sink(ctx).logs.push(text);
}

pub struct Cdm {
    handle: *mut c_void,
    sink: Box<Sink>,
    _callbacks: Box<ShimCallbacks>,
    pub version: String,
    pub host_version: i32,
}

pub fn load() -> Result<String> {
    let lib = CString::new(CDM_PATH)?;
    let mut err = vec![0 as c_char; 512];
    let rc = unsafe { shim_load(lib.as_ptr(), err.as_mut_ptr(), err.len() as u32) };
    if rc != 0 {
        bail!("load failed rc={rc}: {}", unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy());
    }
    Ok(unsafe { CStr::from_ptr(shim_cdm_version()) }.to_string_lossy().into_owned())
}

impl Cdm {
    pub fn create() -> Result<Cdm> {
        let version = load()?;
        let mut sink = Box::new(Sink::default());
        let callbacks = Box::new(ShimCallbacks {
            ctx: &mut *sink as *mut Sink as *mut c_void,
            on_initialized: cb_initialized,
            on_new_session: cb_new_session,
            on_resolve: cb_resolve,
            on_reject: cb_reject,
            on_message: cb_message,
            on_key: cb_key,
            on_keys_done: cb_keys_done,
            on_expiration: cb_expiration,
            on_closed: cb_closed,
            on_log: cb_log,
        });
        let handle = unsafe { shim_create(10, &*callbacks) };
        let host_version = unsafe { shim_requested_host_version() };
        if handle.is_null() {
            bail!("CreateCdmInstance returned null (host version requested {host_version})");
        }
        unsafe { shim_initialize(handle) };
        Ok(Cdm { handle, sink, _callbacks: callbacks, version, host_version })
    }

    pub fn pump(&mut self) -> Vec<CdmEvent> {
        unsafe { shim_pump(self.handle) };
        std::mem::take(&mut self.sink.events)
    }

    pub fn take_events(&mut self) -> Vec<CdmEvent> {
        std::mem::take(&mut self.sink.events)
    }

    pub fn logs(&self) -> &[String] {
        &self.sink.logs
    }

    pub fn set_server_certificate(&mut self, promise: u32, cert: &[u8]) {
        unsafe { shim_set_server_certificate(self.handle, promise, cert.as_ptr(), cert.len() as u32) };
    }

    pub fn create_session(&mut self, promise: u32, init_data_type: u32, init: &[u8]) {
        unsafe { shim_create_session(self.handle, promise, init_data_type, init.as_ptr(), init.len() as u32) };
    }

    pub fn update_session(&mut self, promise: u32, sid: &str, response: &[u8]) {
        unsafe { shim_update_session(self.handle, promise, sid.as_ptr() as *const c_char, sid.len() as u32, response.as_ptr(), response.len() as u32) };
    }

    pub fn close_session(&mut self, promise: u32, sid: &str) {
        unsafe { shim_close_session(self.handle, promise, sid.as_ptr() as *const c_char, sid.len() as u32) };
    }

    pub fn remove_session(&mut self, promise: u32, sid: &str) {
        unsafe { shim_remove_session(self.handle, promise, sid.as_ptr() as *const c_char, sid.len() as u32) };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decrypt(&mut self, scheme: u32, pattern: (u32, u32), data: &[u8], kid: &[u8], iv16: &[u8; 16], subsamples: &[(u32, u32)], ts: i64, out: &mut Vec<u8>) -> u32 {
        let pairs: Vec<u32> = subsamples.iter().flat_map(|(c, e)| [*c, *e]).collect();
        out.resize(data.len() + 64, 0);
        let mut size = 0u32;
        let status = unsafe { shim_decrypt(self.handle, scheme, pattern.0, pattern.1, data.as_ptr(), data.len() as u32, kid.as_ptr(), kid.len() as u32, iv16.as_ptr(), 16, pairs.as_ptr(), subsamples.len() as u32, ts, out.as_mut_ptr(), out.len() as u32, &mut size) };
        out.truncate(size as usize);
        status
    }
}

impl Drop for Cdm {
    fn drop(&mut self) {
        unsafe { shim_destroy(self.handle) };
    }
}
