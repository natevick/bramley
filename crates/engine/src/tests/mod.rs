mod buffer;
mod element;
mod eme;
mod media;
mod mp4;
mod net;

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
    }
    fn played_frames(&self) -> u64 {
        self.0.borrow().played
    }
    fn output_position_us(&self) -> Option<i64> {
        self.0.borrow().position
    }
    fn flush(&mut self) {
        let mut s = self.0.borrow_mut();
        s.queue.clear();
        s.flushes += 1;
    }
    fn set_paused(&mut self, paused: bool) {
        self.0.borrow_mut().paused = paused;
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
