#[cfg(unix)]
mod audio;
mod buffer;
mod cdm;
mod element;
mod eme;
mod media;
mod mp4;
mod net;
mod runtime;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::audio::AudioSink;
use crate::cdm::{CdmBackend, CdmEvent, Decrypted, EncryptedSample};

pub const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/clear-aac.mp4");
pub const XOR_KEY: u8 = 0x5a;

#[derive(Default)]
pub struct SinkState {
    pub writes: Vec<(i64, u64)>,
    pub queue: VecDeque<(i64, u64)>,
    pub played: u64,
    pub paused: bool,
    pub gain: f32,
    pub flushes: u32,
    pub position: Option<i64>,
    pub rate: u32,
    pub written: u64,
    pub latency: u64,
    pub drain_requested: bool,
    pub failed: bool,
    pub cork_positions: Vec<Option<i64>>,
}

#[derive(Clone, Default)]
pub struct FakeSink(pub Rc<RefCell<SinkState>>);

impl FakeSink {
    pub fn advance(&self, mut frames: u64) {
        let mut s = self.0.borrow_mut();
        if s.paused {
            return;
        }
        while frames > 0 {
            let rate = s.rate as i64;
            let Some((pts, n)) = s.queue.front_mut() else {
                break;
            };
            let take = frames.min(*n);
            *pts += take as i64 * 1_000_000 / rate;
            *n -= take;
            let end = *pts;
            if *n == 0 {
                s.queue.pop_front();
            }
            s.played += take;
            s.position = Some(end);
            frames -= take;
        }
    }

    pub fn state(&self) -> std::cell::Ref<'_, SinkState> {
        self.0.borrow()
    }
}

impl AudioSink for FakeSink {
    fn write(&mut self, pts_us: i64, rate: u32, channels: u16, samples: Vec<i16>) {
        let mut s = self.0.borrow_mut();
        let frames = samples.len() as u64 / channels as u64;
        s.rate = rate;
        s.writes.push((pts_us, frames));
        s.queue.push_back((pts_us, frames));
        s.written += frames;
    }
    fn played_frames(&self) -> u64 {
        let s = self.0.borrow();
        s.written.min(s.played + s.latency)
    }
    fn output_position_us(&self) -> Option<i64> {
        self.0.borrow().position
    }
    fn flush(&mut self) {
        let mut s = self.0.borrow_mut();
        s.queue.clear();
        s.flushes += 1;
        s.written = s.played;
        s.drain_requested = false;
    }
    fn set_paused(&mut self, paused: bool) {
        let mut s = self.0.borrow_mut();
        if paused && !s.paused {
            let position = s.position;
            s.cork_positions.push(position);
        }
        s.paused = paused;
    }
    fn drain(&mut self) {
        self.0.borrow_mut().drain_requested = true;
    }
    fn drained(&self) -> bool {
        let s = self.0.borrow();
        s.drain_requested && s.queue.is_empty()
    }
    fn failed(&self) -> bool {
        self.0.borrow().failed
    }
    fn set_gain(&mut self, gain: f32) {
        self.0.borrow_mut().gain = gain;
    }
}

#[derive(Default)]
pub struct CdmState {
    pub message_first: bool,
    pub has_key: bool,
    pub events: Vec<CdmEvent>,
    pub decrypts: u64,
}

#[derive(Clone, Default)]
pub struct FakeCdm(pub Rc<RefCell<CdmState>>);

pub const KID: [u8; 16] = [7; 16];

impl CdmBackend for FakeCdm {
    fn set_server_certificate(&mut self, promise: u32, _cert: &[u8]) {
        self.0.borrow_mut().events.push(CdmEvent::Resolve(promise));
    }
    fn create_session(&mut self, promise: u32, _init_data_type: u32, _init: &[u8]) {
        let mut s = self.0.borrow_mut();
        let message = CdmEvent::Message {
            sid: "s1".into(),
            kind: 0,
            body: b"challenge".to_vec(),
        };
        let session = CdmEvent::NewSession {
            promise,
            sid: "s1".into(),
        };
        let (a, b) = if s.message_first {
            (message, session)
        } else {
            (session, message)
        };
        s.events.extend([a, b]);
    }
    fn update_session(&mut self, promise: u32, sid: &str, response: &[u8]) {
        let mut s = self.0.borrow_mut();
        if response == b"bad" {
            s.events.push(CdmEvent::Reject {
                promise,
                exception: 2,
                system_code: 0,
                message: "rejected license".into(),
            });
            return;
        }
        s.has_key = true;
        s.events.push(CdmEvent::Keys {
            sid: sid.into(),
            keys: vec![(KID.to_vec(), 0)],
        });
        s.events.push(CdmEvent::Resolve(promise));
    }
    fn close_session(&mut self, promise: u32, sid: &str) {
        let mut s = self.0.borrow_mut();
        s.events.push(CdmEvent::Resolve(promise));
        s.events.push(CdmEvent::Closed { sid: sid.into() });
    }
    fn remove_session(&mut self, promise: u32, _sid: &str) {
        self.0.borrow_mut().events.push(CdmEvent::Resolve(promise));
    }
    fn decrypt(&mut self, sample: &EncryptedSample, out: &mut Vec<u8>) -> Decrypted {
        let mut s = self.0.borrow_mut();
        if !s.has_key || sample.kid != KID {
            return Decrypted::NoKey;
        }
        s.decrypts += 1;
        *out = sample.data.iter().map(|b| b ^ XOR_KEY).collect();
        Decrypted::Ok
    }
    fn poll(&mut self) -> Vec<CdmEvent> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }
    fn describe(&self) -> serde_json::Value {
        serde_json::json!({"fake": true})
    }
}

pub fn fixture_segments() -> (Vec<u8>, Vec<Vec<u8>>) {
    let tops = crate::mp4::boxes(FIXTURE, 0, FIXTURE.len());
    let init = tops
        .iter()
        .take_while(|b| !b.is("moof"))
        .flat_map(|b| FIXTURE[b.start..b.end].to_vec())
        .collect();
    let media = tops
        .chunks(2)
        .filter(|p| p[0].is("moof"))
        .map(|p| FIXTURE[p[0].start..p[1].end].to_vec())
        .collect();
    (init, media)
}

pub struct ChildRun {
    pub status: std::process::ExitStatus,
    pub stderr: String,
}

pub fn in_child(test: &str, wrapper: &[&str], body: impl FnOnce()) -> Option<ChildRun> {
    const CHILD: &str = "ENGINE_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(test) {
        body();
        return None;
    }
    let exe = std::env::current_exe().unwrap();
    let mut argv: Vec<&std::ffi::OsStr> = wrapper.iter().map(std::ffi::OsStr::new).collect();
    argv.push(exe.as_os_str());
    let out = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .args(["--exact", test, "--include-ignored", "--nocapture"])
        .env(CHILD, test)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("running 1 test"),
        "child did not run {test}: {stdout}"
    );
    Some(ChildRun {
        status: out.status,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}
