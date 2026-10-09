#[derive(Debug, Clone, PartialEq)]
pub enum CdmEvent {
    Initialized(bool),
    NewSession {
        promise: u32,
        sid: String,
    },
    Resolve(u32),
    Reject {
        promise: u32,
        exception: u32,
        system_code: u32,
        message: String,
    },
    Message {
        sid: String,
        kind: u32,
        body: Vec<u8>,
    },
    Keys {
        sid: String,
        keys: Vec<(Vec<u8>, u32)>,
    },
    Expiration {
        sid: String,
        expiry: f64,
    },
    Closed {
        sid: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decrypted {
    Ok,
    NoKey,
    Failed(u32),
}

impl Decrypted {
    pub fn from_status(status: u32) -> Decrypted {
        match status {
            0 => Decrypted::Ok,
            2 => Decrypted::NoKey,
            s => Decrypted::Failed(s),
        }
    }
}

pub struct EncryptedSample<'a> {
    pub cbcs: bool,
    pub pattern: (u32, u32),
    pub data: &'a [u8],
    pub kid: &'a [u8],
    pub iv: [u8; 16],
    pub subsamples: &'a [(u32, u32)],
    pub pts_us: i64,
}

pub trait CdmBackend {
    fn set_server_certificate(&mut self, promise: u32, cert: &[u8]);
    fn create_session(&mut self, promise: u32, init_data_type: u32, init: &[u8]);
    fn update_session(&mut self, promise: u32, sid: &str, response: &[u8]);
    fn close_session(&mut self, promise: u32, sid: &str);
    fn remove_session(&mut self, promise: u32, sid: &str);
    fn decrypt(&mut self, sample: &EncryptedSample, out: &mut Vec<u8>) -> Decrypted;
    fn poll(&mut self) -> Vec<CdmEvent>;
    fn describe(&self) -> serde_json::Value;
}

#[cfg(unix)]
pub use widevine::Widevine;

#[cfg(not(unix))]
pub struct Widevine;

#[cfg(not(unix))]
impl Widevine {
    pub fn create(_path: &std::path::Path) -> anyhow::Result<Box<dyn CdmBackend>> {
        anyhow::bail!("the Widevine CDM host is only built for unix targets")
    }
}

#[cfg(unix)]
mod widevine {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::Path;

    use anyhow::{Result, bail};

    use super::{CdmBackend, CdmEvent, Decrypted, EncryptedSample};

    const MAX_LOGS: usize = 64;

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
        fn shim_set_server_certificate(
            handle: *mut c_void,
            promise_id: u32,
            data: *const u8,
            size: u32,
        );
        fn shim_create_session(
            handle: *mut c_void,
            promise_id: u32,
            init_data_type: u32,
            init_data: *const u8,
            init_data_size: u32,
        );
        fn shim_update_session(
            handle: *mut c_void,
            promise_id: u32,
            session_id: *const c_char,
            session_id_size: u32,
            response: *const u8,
            response_size: u32,
        );
        fn shim_close_session(
            handle: *mut c_void,
            promise_id: u32,
            session_id: *const c_char,
            session_id_size: u32,
        );
        fn shim_remove_session(
            handle: *mut c_void,
            promise_id: u32,
            session_id: *const c_char,
            session_id_size: u32,
        );
        fn shim_decrypt(
            handle: *mut c_void,
            scheme: u32,
            crypt: u32,
            skip: u32,
            data: *const u8,
            data_size: u32,
            key_id: *const u8,
            key_id_size: u32,
            iv: *const u8,
            iv_size: u32,
            subsample_pairs: *const u32,
            num_subsamples: u32,
            timestamp: i64,
            out: *mut u8,
            out_capacity: u32,
            out_size: *mut u32,
        ) -> u32;
        fn shim_pump(handle: *mut c_void);
        fn shim_destroy(handle: *mut c_void);
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
        sink(ctx).events.push(CdmEvent::NewSession {
            promise,
            sid: text(sid, len),
        });
    }
    extern "C" fn cb_resolve(ctx: *mut c_void, promise: u32) {
        sink(ctx).events.push(CdmEvent::Resolve(promise));
    }
    extern "C" fn cb_reject(
        ctx: *mut c_void,
        promise: u32,
        exception: u32,
        code: u32,
        msg: *const c_char,
        len: u32,
    ) {
        sink(ctx).events.push(CdmEvent::Reject {
            promise,
            exception,
            system_code: code,
            message: text(msg, len),
        });
    }
    extern "C" fn cb_message(
        ctx: *mut c_void,
        sid: *const c_char,
        sid_len: u32,
        kind: u32,
        msg: *const c_char,
        len: u32,
    ) {
        sink(ctx).events.push(CdmEvent::Message {
            sid: text(sid, sid_len),
            kind,
            body: bytes(msg, len).to_vec(),
        });
    }
    extern "C" fn cb_key(
        ctx: *mut c_void,
        sid: *const c_char,
        sid_len: u32,
        kid: *const u8,
        len: u32,
        status: u32,
        _code: u32,
    ) {
        let kid = bytes(kid as *const c_char, len).to_vec();
        sink(ctx)
            .pending_keys
            .push((text(sid, sid_len), kid, status));
    }
    extern "C" fn cb_keys_done(ctx: *mut c_void, sid: *const c_char, sid_len: u32, _has_new: bool) {
        let s = sink(ctx);
        let sid = text(sid, sid_len);
        let keys: Vec<(Vec<u8>, u32)> = s
            .pending_keys
            .drain(..)
            .filter(|k| k.0 == sid)
            .map(|k| (k.1, k.2))
            .collect();
        s.events.push(CdmEvent::Keys { sid, keys });
    }
    extern "C" fn cb_expiration(ctx: *mut c_void, sid: *const c_char, sid_len: u32, expiry: f64) {
        sink(ctx).events.push(CdmEvent::Expiration {
            sid: text(sid, sid_len),
            expiry,
        });
    }
    extern "C" fn cb_closed(ctx: *mut c_void, sid: *const c_char, sid_len: u32) {
        sink(ctx).events.push(CdmEvent::Closed {
            sid: text(sid, sid_len),
        });
    }
    extern "C" fn cb_log(ctx: *mut c_void, msg: *const c_char) {
        let text = unsafe { CStr::from_ptr(msg) }
            .to_string_lossy()
            .into_owned();
        eprintln!("CDM-HOST {text}");
        let logs = &mut sink(ctx).logs;
        if logs.len() < MAX_LOGS {
            logs.push(text);
        }
    }

    pub struct Widevine {
        handle: *mut c_void,
        sink: Box<Sink>,
        _callbacks: Box<ShimCallbacks>,
        version: String,
        host_version: i32,
    }

    impl Widevine {
        pub fn create(path: &Path) -> Result<Box<dyn CdmBackend>> {
            let lib = CString::new(path.as_os_str().as_encoded_bytes())?;
            let mut err = vec![0 as c_char; 512];
            let rc = unsafe { shim_load(lib.as_ptr(), err.as_mut_ptr(), err.len() as u32) };
            if rc != 0 {
                bail!(
                    "loading {} failed rc={rc}: {}",
                    path.display(),
                    unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy()
                );
            }
            let version = unsafe { CStr::from_ptr(shim_cdm_version()) }
                .to_string_lossy()
                .into_owned();
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
            Ok(Box::new(Widevine {
                handle,
                sink,
                _callbacks: callbacks,
                version,
                host_version,
            }))
        }

        fn session_call(
            &mut self,
            f: unsafe extern "C" fn(*mut c_void, u32, *const c_char, u32),
            promise: u32,
            sid: &str,
        ) {
            unsafe {
                f(
                    self.handle,
                    promise,
                    sid.as_ptr() as *const c_char,
                    sid.len() as u32,
                )
            };
        }
    }

    impl CdmBackend for Widevine {
        fn set_server_certificate(&mut self, promise: u32, cert: &[u8]) {
            unsafe {
                shim_set_server_certificate(self.handle, promise, cert.as_ptr(), cert.len() as u32)
            };
        }
        fn create_session(&mut self, promise: u32, init_data_type: u32, init: &[u8]) {
            unsafe {
                shim_create_session(
                    self.handle,
                    promise,
                    init_data_type,
                    init.as_ptr(),
                    init.len() as u32,
                )
            };
        }
        fn update_session(&mut self, promise: u32, sid: &str, response: &[u8]) {
            unsafe {
                shim_update_session(
                    self.handle,
                    promise,
                    sid.as_ptr() as *const c_char,
                    sid.len() as u32,
                    response.as_ptr(),
                    response.len() as u32,
                )
            };
        }
        fn close_session(&mut self, promise: u32, sid: &str) {
            self.session_call(shim_close_session, promise, sid);
        }
        fn remove_session(&mut self, promise: u32, sid: &str) {
            self.session_call(shim_remove_session, promise, sid);
        }
        fn decrypt(&mut self, s: &EncryptedSample, out: &mut Vec<u8>) -> Decrypted {
            let pairs: Vec<u32> = s.subsamples.iter().flat_map(|(c, e)| [*c, *e]).collect();
            out.resize(s.data.len() + 64, 0);
            let mut size = 0u32;
            let status = unsafe {
                shim_decrypt(
                    self.handle,
                    if s.cbcs { 2 } else { 1 },
                    s.pattern.0,
                    s.pattern.1,
                    s.data.as_ptr(),
                    s.data.len() as u32,
                    s.kid.as_ptr(),
                    s.kid.len() as u32,
                    s.iv.as_ptr(),
                    16,
                    pairs.as_ptr(),
                    s.subsamples.len() as u32,
                    s.pts_us,
                    out.as_mut_ptr(),
                    out.len() as u32,
                    &mut size,
                )
            };
            out.truncate(size as usize);
            Decrypted::from_status(status)
        }
        fn poll(&mut self) -> Vec<CdmEvent> {
            let mut events = std::mem::take(&mut self.sink.events);
            unsafe { shim_pump(self.handle) };
            events.append(&mut self.sink.events);
            events
        }
        fn describe(&self) -> serde_json::Value {
            serde_json::json!({"version": self.version, "hostVersionRequested": self.host_version, "interface": 10, "logs": self.sink.logs})
        }
    }

    impl Drop for Widevine {
        fn drop(&mut self) {
            unsafe { shim_destroy(self.handle) };
        }
    }
}
