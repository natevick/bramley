use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use anyhow::{Result, anyhow};
use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions};
use symphonia::core::formats::Packet;

use crate::audio::AudioOut;
use crate::cdm::Cdm;
use crate::mp4::{self, SampleEnc, TrackInit};

pub struct Frame {
    pub pts_us: i64,
    pub dur_us: i64,
    pub data: Vec<u8>,
    pub enc: Option<SampleEnc>,
}

pub struct SourceBufferNative {
    pub mime: String,
    pending: Vec<u8>,
    pub init: Option<TrackInit>,
    pub frames: BTreeMap<i64, Frame>,
    pub init_data: Option<Vec<u8>>,
    pub appended_bytes: u64,
}

pub struct AppendResult {
    pub init_segment: bool,
    pub media_frames: usize,
}

impl SourceBufferNative {
    pub fn new(mime: String) -> Self {
        SourceBufferNative { mime, pending: Vec::new(), init: None, frames: BTreeMap::new(), init_data: None, appended_bytes: 0 }
    }

    pub fn abort(&mut self) {
        self.pending.clear();
    }

    pub fn append(&mut self, bytes: &[u8], ts_offset: f64) -> Result<AppendResult> {
        self.appended_bytes += bytes.len() as u64;
        self.pending.extend_from_slice(bytes);
        let complete = mp4::complete_top_level(&self.pending);
        let buf = std::mem::take(&mut self.pending);
        let tops = mp4::boxes(&buf, 0, complete);
        let mut consumed = 0usize;
        let mut result = AppendResult { init_segment: false, media_frames: 0 };
        let offset_us = (ts_offset * 1e6).round() as i64;
        let mut i = 0;
        while i < tops.len() {
            let b = &tops[i];
            if b.is("moov") {
                let init = mp4::parse_init(&buf, b)?;
                if !init.pssh.is_empty() {
                    self.init_data = Some(init.pssh.clone());
                }
                self.init = Some(init);
                result.init_segment = true;
                consumed = b.end;
            } else if b.is("moof") {
                let Some(mdat) = tops.get(i + 1).filter(|x| x.is("mdat")) else {
                    if i + 1 >= tops.len() {
                        break;
                    }
                    consumed = b.end;
                    i += 1;
                    continue;
                };
                let init = self.init.as_ref().ok_or_else(|| anyhow!("media segment before init segment"))?;
                let ts = init.timescale.max(1) as i64;
                let samples = mp4::parse_fragment(&buf, b, init);
                if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
                    let start = first.dts * 1_000_000 / ts + offset_us;
                    let end = (last.dts + last.dur as i64) * 1_000_000 / ts + offset_us;
                    let stale: Vec<i64> = self.frames.range(start..end).map(|(k, _)| *k).collect();
                    for k in stale {
                        self.frames.remove(&k);
                    }
                }
                for s in samples {
                    let pts_us = s.dts * 1_000_000 / ts + offset_us;
                    let dur_us = s.dur as i64 * 1_000_000 / ts;
                    self.frames.insert(pts_us, Frame { pts_us, dur_us, data: s.data, enc: s.enc });
                    result.media_frames += 1;
                }
                consumed = mdat.end;
                i += 2;
                continue;
            } else {
                consumed = b.end;
            }
            i += 1;
        }
        self.pending = buf[consumed..].to_vec();
        Ok(result)
    }

    pub fn remove(&mut self, start: f64, end: f64) {
        let s = (start * 1e6) as i64;
        let e = if end.is_finite() { (end * 1e6) as i64 } else { i64::MAX };
        let keys: Vec<i64> = self.frames.range(s..e).map(|(k, _)| *k).collect();
        for k in keys {
            self.frames.remove(&k);
        }
    }

    pub fn buffered(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        for f in self.frames.values() {
            let end = f.pts_us + f.dur_us;
            match out.last_mut() {
                Some(last) if f.pts_us - last.1 <= 50_000 => last.1 = last.1.max(end),
                _ => out.push((f.pts_us, end)),
            }
        }
        out.into_iter().map(|(a, b)| (a as f64 / 1e6, b as f64 / 1e6)).collect()
    }
}

#[derive(Default, Clone)]
pub struct Counters {
    pub clear_samples: u64,
    pub encrypted_samples: u64,
    pub decrypt_ok: u64,
    pub decrypt_fail: BTreeMap<u32, u64>,
    pub decoded_clear_frames: u64,
    pub decoded_decrypted_frames: u64,
    pub decode_err: u64,
    pub key_waits: u64,
    pub underruns: u64,
    pub seeks: u64,
    pub loops: u64,
}

pub struct MediaSourceNative {
    pub duration: f64,
    pub ended: bool,
    pub sbs: Vec<u32>,
}

pub struct MediaEngine {
    pub ms_id: Option<u32>,
    pub keys_id: Option<u32>,
    pub paused: bool,
    pub seeking: bool,
    pub ended: bool,
    pub looping: bool,
    pub ready_state: u8,
    pub cursor_us: i64,
    pub base_us: i64,
    pub waiting_for_key: bool,
    pub waiting: bool,
    decoder: Option<(Vec<u8>, Box<dyn Decoder>)>,
    pub rate: u32,
    pub channels: u16,
    pub audio: Option<AudioOut>,
    sink: String,
    pub events: Vec<String>,
    last_timeupdate: Instant,
    pub counters: Counters,
    pub max_current_time: f64,
    ever_loaded_data: bool,
    out_buf: Vec<u8>,
}

fn keymap_scheme(scheme: &str) -> u32 {
    match scheme {
        "cbcs" | "cbc1" => 2,
        _ => 1,
    }
}

impl MediaEngine {
    pub fn new(sink: String) -> Self {
        MediaEngine {
            ms_id: None,
            keys_id: None,
            paused: true,
            seeking: false,
            ended: false,
            looping: false,
            ready_state: 0,
            cursor_us: 0,
            base_us: 0,
            waiting_for_key: false,
            waiting: false,
            decoder: None,
            rate: 0,
            channels: 0,
            audio: None,
            sink,
            events: Vec::new(),
            last_timeupdate: Instant::now(),
            counters: Counters::default(),
            max_current_time: 0.0,
            ever_loaded_data: false,
            out_buf: Vec::new(),
        }
    }

    fn emit(&mut self, e: &str) {
        self.events.push(e.to_string());
    }

    pub fn current_time(&self) -> f64 {
        let played = match (&self.audio, self.rate) {
            (Some(a), r) if r > 0 && !self.paused && !self.seeking => a.played() as i64 * 1_000_000 / r as i64,
            _ => 0,
        };
        let queued_limit = match (&self.audio, self.rate) {
            (Some(a), r) if r > 0 => a.sent_frames as i64 * 1_000_000 / r as i64,
            _ => 0,
        };
        let t = self.base_us + played.min(queued_limit);
        t as f64 / 1e6
    }

    pub fn attach(&mut self, ms_id: u32) {
        self.ms_id = Some(ms_id);
        self.ready_state = 0;
        self.cursor_us = 0;
        self.base_us = 0;
        self.ended = false;
        self.ever_loaded_data = false;
    }

    pub fn detach(&mut self) {
        self.ms_id = None;
        self.ready_state = 0;
        if let Some(a) = self.audio.as_mut() {
            a.flush();
        }
        self.decoder = None;
    }

    pub fn play(&mut self) {
        if self.paused {
            self.paused = false;
            self.emit("play");
            if self.ended {
                self.ended = false;
                self.seek_to(0.0);
            }
            if self.ready_state >= 3 && !self.seeking {
                self.emit("playing");
            } else {
                self.waiting = true;
                self.emit("waiting");
            }
            self.reset_base_to_cursor();
        }
    }

    fn reset_base_to_cursor(&mut self) {
        if let Some(a) = self.audio.as_mut() {
            a.flush();
        }
        self.base_us = self.cursor_us;
    }

    pub fn pause(&mut self) {
        if !self.paused {
            let t = self.current_time();
            self.paused = true;
            self.cursor_us = (t * 1e6) as i64;
            self.reset_base_to_cursor();
            self.emit("timeupdate");
            self.emit("pause");
        }
    }

    pub fn seek_to(&mut self, t: f64) {
        self.counters.seeks += 1;
        self.cursor_us = (t.max(0.0) * 1e6) as i64;
        self.base_us = self.cursor_us;
        self.seeking = true;
        self.ended = false;
        if let Some(a) = self.audio.as_mut() {
            a.flush();
        }
        if let Some((_, d)) = self.decoder.as_mut() {
            d.reset();
        }
        self.emit("seeking");
        self.emit("timeupdate");
    }

    fn ensure_decoder(&mut self, init: &TrackInit) -> Result<()> {
        if self.decoder.as_ref().map(|(asc, _)| asc == &init.asc).unwrap_or(false) {
            return Ok(());
        }
        let ch = if init.channels == 1 { Channels::FRONT_LEFT } else { Channels::FRONT_LEFT | Channels::FRONT_RIGHT };
        let mut params = CodecParameters::new();
        params.for_codec(CODEC_TYPE_AAC).with_sample_rate(if init.sample_rate > 0 { init.sample_rate } else { 44100 }).with_channels(ch).with_extra_data(init.asc.clone().into_boxed_slice());
        let dec = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;
        self.decoder = Some((init.asc.clone(), dec));
        Ok(())
    }

    fn update_ready_state(&mut self, sbs: &HashMap<u32, SourceBufferNative>, ms: Option<&MediaSourceNative>) {
        let Some(ms) = ms else { return };
        if ms.ended && self.at_stream_end(sbs, ms) {
            return;
        }
        let have_init = !ms.sbs.is_empty() && ms.sbs.iter().all(|id| sbs.get(id).map(|s| s.init.is_some()).unwrap_or(false));
        if !have_init {
            return;
        }
        let t = if self.seeking || self.paused { self.cursor_us as f64 / 1e6 } else { self.current_time() };
        let mut ahead: Option<f64> = None;
        let mut reaches_end = false;
        for id in &ms.sbs {
            let Some(sb) = sbs.get(id) else { continue };
            let r = sb.buffered().into_iter().find(|(s, e)| t >= *s - 0.05 && t < *e);
            let a = r.map(|(_, e)| {
                if ms.ended && ms.duration.is_finite() && e >= ms.duration - 0.25 {
                    reaches_end = true;
                }
                e - t
            });
            ahead = match (ahead, a) {
                (None, x) => x,
                (Some(p), Some(x)) => Some(p.min(x)),
                (Some(_), None) => None,
            };
        }
        let new_state = match ahead {
            None => 1,
            Some(a) if a >= 3.0 || reaches_end => 4,
            Some(a) if a > 0.1 => 3,
            Some(_) => 2,
        };
        let old = self.ready_state;
        if new_state == old {
            return;
        }
        self.ready_state = new_state;
        if old == 0 {
            self.emit("loadedmetadata");
        }
        if new_state >= 2 && !self.ever_loaded_data {
            self.ever_loaded_data = true;
            self.emit("loadeddata");
        }
        if new_state >= 3 && old < 3 {
            self.emit("canplay");
            if !self.paused && !self.seeking {
                self.waiting = false;
                self.emit("playing");
            }
        }
        if new_state == 4 && old < 4 {
            self.emit("canplaythrough");
        }
        if new_state < 3 && old >= 3 && !self.paused && !self.ended {
            self.waiting = true;
            self.counters.underruns += 1;
            self.emit("waiting");
        }
    }

    pub fn pump(&mut self, sbs: &mut HashMap<u32, SourceBufferNative>, mses: &HashMap<u32, MediaSourceNative>, cdms: &mut HashMap<u32, Cdm>) {
        let Some(ms_id) = self.ms_id else { return };
        let ms = mses.get(&ms_id);
        if let Some(ms) = ms {
            if ms.ended && !self.seeking && !self.paused && self.at_stream_end(sbs, ms) {
                if self.looping {
                    self.counters.loops += 1;
                    self.seek_to(0.0);
                } else {
                    self.ended = true;
                    self.emit("timeupdate");
                    self.paused = true;
                    self.emit("pause");
                    self.emit("ended");
                }
                return;
            }
        }
        self.update_ready_state(sbs, ms);
        let Some(ms) = ms else { return };
        let Some(sb_id) = ms.sbs.first().copied() else { return };
        if self.seeking {
            let Some(sb) = sbs.get(&sb_id) else { return };
            let t = self.cursor_us as f64 / 1e6;
            if sb.buffered().iter().any(|(s, e)| t >= *s - 0.05 && t < *e) {
                if let Some((&k, _)) = sb.frames.range(..=self.cursor_us + 1).next_back() {
                    self.cursor_us = k;
                    self.base_us = k;
                }
                self.seeking = false;
                self.emit("seeked");
                self.emit("timeupdate");
                if !self.paused && self.ready_state >= 3 {
                    self.emit("playing");
                }
            }
            return;
        }
        if self.paused || self.ready_state < 3 && !self.ended_ready(sbs, ms, sb_id) {
            return;
        }
        let Some(sb) = sbs.get(&sb_id) else { return };
        let Some(init) = sb.init.clone() else { return };
        if self.ensure_decoder(&init).is_err() {
            return;
        }
        if self.audio.is_none() {
            self.audio = Some(AudioOut::start(self.sink.clone()));
        }
        let target_frames = (self.rate.max(44100) as u64) * 3 / 2;
        loop {
            let queued = self.audio.as_ref().map(|a| a.queued()).unwrap_or(0);
            if self.rate > 0 && queued >= target_frames {
                break;
            }
            let next = sb.frames.range(self.cursor_us - 2_000..).next().map(|(_, f)| f);
            let Some(frame) = next else { break };
            if frame.pts_us - self.cursor_us > 100_000 {
                break;
            }
            let payload: Vec<u8> = if let Some(enc) = &frame.enc {
                let prot = init.protection.clone().unwrap_or_default();
                let Some(keys_id) = self.keys_id else {
                    if !self.waiting_for_key {
                        self.waiting_for_key = true;
                        self.counters.key_waits += 1;
                        self.emit("waitingforkey");
                    }
                    break;
                };
                let Some(cdm) = cdms.get_mut(&keys_id) else { break };
                let mut iv16 = [0u8; 16];
                let n = enc.iv.len().min(16);
                iv16[..n].copy_from_slice(&enc.iv[..n]);
                let status = cdm.decrypt(keymap_scheme(&prot.scheme), (prot.crypt, prot.skip), &frame.data, &prot.kid, &iv16, &enc.subsamples, frame.pts_us, &mut self.out_buf);
                if status == 1 {
                    if !self.waiting_for_key {
                        self.waiting_for_key = true;
                        self.counters.key_waits += 1;
                        self.emit("waitingforkey");
                    }
                    *self.counters.decrypt_fail.entry(status).or_default() += 1;
                    break;
                }
                self.waiting_for_key = false;
                if status != 0 {
                    *self.counters.decrypt_fail.entry(status).or_default() += 1;
                    self.cursor_us = frame.pts_us + frame.dur_us;
                    continue;
                }
                self.counters.encrypted_samples += 1;
                self.counters.decrypt_ok += 1;
                self.out_buf.clone()
            } else {
                self.counters.clear_samples += 1;
                frame.data.clone()
            };
            let encrypted = frame.enc.is_some();
            let (pts_us, dur_us) = (frame.pts_us, frame.dur_us);
            let (_, dec) = self.decoder.as_mut().unwrap();
            let packet = Packet::new_from_slice(0, pts_us as u64, 1024, &payload);
            match dec.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let frames = decoded.frames() as u64;
                    let mut buf = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
                    buf.copy_interleaved_ref(decoded);
                    let rate = spec.rate;
                    let channels = spec.channels.count() as u16;
                    if self.rate != rate || self.channels != channels {
                        self.rate = rate;
                        self.channels = channels;
                    }
                    if encrypted {
                        self.counters.decoded_decrypted_frames += frames;
                    } else {
                        self.counters.decoded_clear_frames += frames;
                    }
                    let audio = self.audio.as_mut().unwrap();
                    if audio.sent_frames == 0 {
                        self.base_us = pts_us;
                    }
                    audio.send(rate, channels, buf.samples().to_vec());
                }
                Err(e) => {
                    self.counters.decode_err += 1;
                    if self.counters.decode_err < 5 {
                        eprintln!("ENGINE decode error encrypted={encrypted} {e}");
                    }
                }
            }
            self.cursor_us = pts_us + dur_us;
        }
        let ct = self.current_time();
        if ct > self.max_current_time {
            self.max_current_time = ct;
        }
        if self.last_timeupdate.elapsed().as_millis() >= 250 {
            self.last_timeupdate = Instant::now();
            self.emit("timeupdate");
        }
    }

    fn at_stream_end(&self, sbs: &HashMap<u32, SourceBufferNative>, ms: &MediaSourceNative) -> bool {
        let Some(sb) = ms.sbs.first().and_then(|id| sbs.get(id)) else { return false };
        let Some(end_us) = sb.frames.values().next_back().map(|f| f.pts_us + f.dur_us) else { return false };
        let queued = self.audio.as_ref().map(|a| a.queued()).unwrap_or(0);
        self.cursor_us >= end_us - 1_000 && queued == 0
    }

    fn ended_ready(&self, sbs: &HashMap<u32, SourceBufferNative>, ms: &MediaSourceNative, sb_id: u32) -> bool {
        if !ms.ended || self.ready_state < 2 {
            return false;
        }
        sbs.get(&sb_id).map(|s| !s.frames.is_empty()).unwrap_or(false)
    }
}
