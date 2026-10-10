use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions};
use symphonia::core::formats::Packet;

use crate::audio::AudioSink;
use crate::buffer::{Frame, MediaSourceNative, SourceBufferNative};
use crate::cdm::{CdmBackend, Decrypted, EncryptedSample};
use crate::mp4::TrackInit;

pub type SinkFactory = Rc<dyn Fn() -> Box<dyn AudioSink>>;

pub const HAVE_NOTHING: u8 = 0;
pub const HAVE_METADATA: u8 = 1;
pub const HAVE_CURRENT_DATA: u8 = 2;
pub const HAVE_FUTURE_DATA: u8 = 3;
pub const HAVE_ENOUGH_DATA: u8 = 4;

const DECODE_AHEAD_US: i64 = 1_500_000;
const ENOUGH_AHEAD_US: i64 = 3_000_000;
const FUTURE_AHEAD_US: i64 = 100_000;
const GAP_US: i64 = 50_000;
const END_SLACK_US: i64 = 250_000;
const TIMEUPDATE_INTERVAL: Duration = Duration::from_millis(250);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

struct Chunk {
    pts_us: i64,
    rate: u32,
    channels: u16,
    frames: usize,
    samples: Vec<i16>,
    encrypted: bool,
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
    pub drains_acked: u64,
    pub drain_timeouts: u64,
    pub sink_restarts: u64,
}

pub struct MediaEngine {
    pub ms_id: Option<u32>,
    pub keys_id: Option<u32>,
    pub paused: bool,
    pub seeking: bool,
    pub ended: bool,
    pub looping: bool,
    pub ready_state: u8,
    pub waiting_for_key: bool,
    pub rate: u32,
    pub channels: u16,
    pub events: Vec<&'static str>,
    pub counters: Counters,
    pub max_current_time_us: i64,
    position_us: i64,
    base_us: i64,
    anchor_played: u64,
    sent_end_us: i64,
    sent_since_anchor: bool,
    cursor_us: i64,
    skip_until_us: i64,
    decoder: Option<(Vec<u8>, Box<dyn Decoder>)>,
    sink: Option<Box<dyn AudioSink>>,
    sink_format: Option<(u32, u16)>,
    held: Option<Chunk>,
    drain_started: Option<Instant>,
    output_failed: bool,
    sink_factory: SinkFactory,
    gain: f32,
    last_timeupdate: Option<Instant>,
    ever_loaded_data: bool,
    out_buf: Vec<u8>,
}

fn frames_to_us(frames: u64, rate: u32) -> i64 {
    (frames as i128 * 1_000_000 / rate.max(1) as i128) as i64
}

impl MediaEngine {
    pub fn new(sink_factory: SinkFactory) -> Self {
        MediaEngine {
            ms_id: None,
            keys_id: None,
            paused: true,
            seeking: false,
            ended: false,
            looping: false,
            ready_state: HAVE_NOTHING,
            waiting_for_key: false,
            rate: 0,
            channels: 0,
            events: Vec::new(),
            counters: Counters::default(),
            max_current_time_us: 0,
            position_us: 0,
            base_us: 0,
            anchor_played: 0,
            sent_end_us: 0,
            sent_since_anchor: false,
            cursor_us: 0,
            skip_until_us: i64::MIN,
            decoder: None,
            sink: None,
            sink_format: None,
            held: None,
            drain_started: None,
            output_failed: false,
            sink_factory,
            gain: 1.0,
            last_timeupdate: None,
            ever_loaded_data: false,
            out_buf: Vec::new(),
        }
    }

    fn emit(&mut self, e: &'static str) {
        self.events.push(e);
    }

    fn played(&self) -> u64 {
        self.sink.as_ref().map(|s| s.played_frames()).unwrap_or(0)
    }

    pub fn current_time_us(&self) -> i64 {
        if self.paused || self.seeking || self.rate == 0 {
            return self.position_us;
        }
        let played = self.played().saturating_sub(self.anchor_played);
        (self.base_us + frames_to_us(played, self.rate)).min(self.sent_end_us.max(self.base_us))
    }

    pub fn output_failed(&self) -> bool {
        self.output_failed
    }

    pub fn current_time(&self) -> f64 {
        self.current_time_us() as f64 / 1e6
    }

    pub fn output_position(&self) -> Option<f64> {
        self.sink
            .as_ref()
            .and_then(|s| s.output_position_us())
            .map(|p| p as f64 / 1e6)
    }

    fn anchor(&mut self, position_us: i64, keep_queued: bool) {
        self.position_us = position_us;
        self.base_us = position_us;
        self.anchor_played = self.played();
        if !keep_queued {
            self.sent_end_us = position_us;
            self.sent_since_anchor = false;
        }
    }

    fn discard_output(&mut self) {
        self.held = None;
        self.drain_started = None;
        if let Some(s) = self.sink.as_mut() {
            s.flush();
        }
        if let Some((_, d)) = self.decoder.as_mut() {
            d.reset();
        }
    }

    pub fn attach(&mut self, ms_id: u32) {
        self.events.clear();
        self.output_failed = false;
        self.ms_id = Some(ms_id);
        self.ready_state = HAVE_NOTHING;
        self.ended = false;
        self.seeking = false;
        self.ever_loaded_data = false;
        self.cursor_us = 0;
        self.skip_until_us = i64::MIN;
        self.anchor(0, false);
    }

    pub fn detach(&mut self) {
        self.events.clear();
        self.ms_id = None;
        self.ready_state = HAVE_NOTHING;
        self.paused = true;
        if let Some(s) = self.sink.as_mut() {
            s.set_paused(true);
        }
        self.ended = false;
        self.seeking = false;
        self.discard_output();
        self.anchor(0, false);
        self.decoder = None;
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
        if let Some(s) = self.sink.as_mut() {
            s.set_gain(gain);
        }
    }

    pub fn set_keys(&mut self, keys_id: Option<u32>) {
        self.keys_id = keys_id;
        self.waiting_for_key = false;
    }

    pub fn play(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        self.emit("play");
        if let Some(s) = self.sink.as_mut() {
            s.set_paused(false);
        }
        self.anchor(self.position_us, true);
        self.last_timeupdate = None;
        if self.ended {
            self.seek(0);
        } else if self.ready_state >= HAVE_FUTURE_DATA && !self.seeking {
            self.emit("playing");
        } else {
            self.emit("waiting");
        }
    }

    pub fn pause(&mut self) {
        if self.paused {
            return;
        }
        self.position_us = self.current_time_us();
        self.paused = true;
        self.drain_started = None;
        if let Some(s) = self.sink.as_mut() {
            s.set_paused(true);
        }
        self.emit("timeupdate");
        self.emit("pause");
    }

    fn sink_drained(&mut self, now: Instant) -> bool {
        let Some(s) = self.sink.as_mut() else {
            return true;
        };
        let started = *self.drain_started.get_or_insert_with(|| {
            s.drain();
            now
        });
        if s.drained() {
            self.counters.drains_acked += 1;
        } else if now.duration_since(started) >= DRAIN_TIMEOUT {
            self.counters.drain_timeouts += 1;
        } else {
            return false;
        }
        self.drain_started = None;
        true
    }

    fn restart_output(&mut self) {
        let position = self.current_time_us();
        let ever_played = self.played() > 0;
        self.discard_output();
        self.sink = None;
        self.sink_format = None;
        self.cursor_us = position;
        self.skip_until_us = position;
        self.anchor(position, false);
        self.counters.sink_restarts += 1;
        if !ever_played {
            self.output_failed = true;
            self.emit("error");
        }
    }

    pub fn seek(&mut self, target_us: i64) {
        let target_us = target_us.max(0);
        self.ended = false;
        self.cursor_us = target_us;
        self.skip_until_us = target_us;
        if self.ready_state == HAVE_NOTHING {
            self.anchor(target_us, false);
            return;
        }
        self.counters.seeks += 1;
        self.discard_output();
        self.anchor(target_us, false);
        self.seeking = true;
        self.emit("seeking");
        self.set_ready_state(self.ready_state.min(HAVE_METADATA));
    }

    fn set_ready_state(&mut self, new: u8) {
        let old = self.ready_state;
        if new == old {
            return;
        }
        self.ready_state = new;
        let potentially_playing = !self.paused && !self.ended;
        if old == HAVE_NOTHING {
            self.emit("loadedmetadata");
        }
        if new >= HAVE_CURRENT_DATA && !self.ever_loaded_data {
            self.ever_loaded_data = true;
            self.emit("loadeddata");
        }
        if old < HAVE_FUTURE_DATA && new >= HAVE_FUTURE_DATA {
            self.emit("canplay");
            if potentially_playing {
                self.emit("playing");
            }
        }
        if old < HAVE_ENOUGH_DATA && new == HAVE_ENOUGH_DATA {
            self.emit("canplaythrough");
        }
        if old >= HAVE_FUTURE_DATA && new < HAVE_FUTURE_DATA && potentially_playing {
            if !self.seeking {
                self.counters.underruns += 1;
            }
            self.emit("timeupdate");
            self.emit("waiting");
        }
    }

    fn computed_ready_state(
        &self,
        sbs: &HashMap<u32, SourceBufferNative>,
        ms: &MediaSourceNative,
    ) -> u8 {
        if ms.sbs.is_empty()
            || !ms
                .sbs
                .iter()
                .all(|id| sbs.get(id).is_some_and(|s| s.init.is_some()))
        {
            return HAVE_NOTHING;
        }
        if self.seeking {
            return HAVE_METADATA;
        }
        let t = self.current_time_us();
        let duration_us = if ms.duration.is_finite() {
            (ms.duration * 1e6) as i64
        } else {
            i64::MAX
        };
        let mut ahead = i64::MAX;
        for sb in ms.sbs.iter().filter_map(|id| sbs.get(id)) {
            let Some((_, end)) = sb
                .buffered_us()
                .into_iter()
                .find(|(s, e)| t >= s - GAP_US && t <= *e)
            else {
                return HAVE_METADATA;
            };
            let reaches_end = ms.ended && end >= duration_us.saturating_sub(END_SLACK_US);
            ahead = ahead.min(if reaches_end { i64::MAX } else { end - t });
        }
        match ahead {
            a if a >= ENOUGH_AHEAD_US => HAVE_ENOUGH_DATA,
            a if a > FUTURE_AHEAD_US => HAVE_FUTURE_DATA,
            _ => HAVE_CURRENT_DATA,
        }
    }

    fn at_stream_end(&self, sb: &SourceBufferNative) -> bool {
        let Some(end_us) = sb.end_us() else {
            return false;
        };
        self.cursor_us >= end_us - 1_000 && self.current_time_us() >= self.sent_end_us
    }

    fn reach_end(&mut self) {
        if self.looping {
            self.counters.loops += 1;
            self.seek(0);
            return;
        }
        self.position_us = self.current_time_us();
        self.ended = true;
        self.emit("timeupdate");
        self.paused = true;
        if let Some(s) = self.sink.as_mut() {
            s.set_paused(true);
        }
        self.emit("pause");
        self.emit("ended");
    }

    fn try_complete_seek(&mut self, sb: &SourceBufferNative, ms: &MediaSourceNative, now: Instant) {
        let mut t = self.position_us;
        let in_range = sb
            .buffered_us()
            .iter()
            .any(|(s, e)| t >= s - GAP_US && t < *e);
        let past_end = ms.ended && sb.end_us().is_some_and(|e| t >= e - GAP_US);
        if !in_range && !past_end {
            return;
        }
        let at = sb
            .frames
            .range(..=t)
            .next_back()
            .map(|(_, f)| f)
            .or_else(|| sb.frames.values().next());
        if let Some(f) = at {
            t = t.max(f.pts_us);
            let preroll = sb
                .frames
                .range(..f.pts_us)
                .next_back()
                .map(|(_, p)| p)
                .filter(|p| f.pts_us - p.end_us() <= GAP_US && Rc::ptr_eq(&p.init, &f.init));
            self.cursor_us = preroll.unwrap_or(f).pts_us;
        }
        self.skip_until_us = t;
        self.seeking = false;
        self.anchor(t, false);
        self.emit("timeupdate");
        self.emit("seeked");
        self.last_timeupdate = Some(now);
    }

    fn ensure_decoder(&mut self, init: &TrackInit) -> Result<()> {
        if self
            .decoder
            .as_ref()
            .is_some_and(|(asc, _)| asc == &init.asc)
        {
            return Ok(());
        }
        let ch = if init.channels == 1 {
            Channels::FRONT_LEFT
        } else {
            Channels::FRONT_LEFT | Channels::FRONT_RIGHT
        };
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_AAC)
            .with_sample_rate(if init.sample_rate > 0 {
                init.sample_rate
            } else {
                44100
            })
            .with_channels(ch)
            .with_extra_data(init.asc.clone().into_boxed_slice());
        let dec = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;
        self.decoder = Some((init.asc.clone(), dec));
        Ok(())
    }

    fn wait_for_key(&mut self) {
        if !self.waiting_for_key {
            self.waiting_for_key = true;
            self.counters.key_waits += 1;
            self.emit("waitingforkey");
        }
    }

    fn decrypt(
        &mut self,
        frame: &Frame,
        init: &TrackInit,
        cdms: &mut HashMap<u32, Box<dyn CdmBackend>>,
    ) -> Option<Decrypted> {
        let (Some(enc), Some(prot)) = (&frame.enc, &init.protection) else {
            return Some(Decrypted::Ok);
        };
        let cdm = self.keys_id.and_then(|id| cdms.get_mut(&id))?;
        let mut iv = [0u8; 16];
        let n = enc.iv.len().min(16);
        iv[..n].copy_from_slice(&enc.iv[..n]);
        let sample = EncryptedSample {
            cbcs: matches!(prot.scheme.as_str(), "cbcs" | "cbc1"),
            pattern: (prot.crypt, prot.skip),
            data: &frame.data,
            kid: &prot.kid,
            iv,
            subsamples: &enc.subsamples,
            pts_us: frame.pts_us,
        };
        Some(cdm.decrypt(&sample, &mut self.out_buf))
    }

    fn decode_ahead(
        &mut self,
        sb: &SourceBufferNative,
        cdms: &mut HashMap<u32, Box<dyn CdmBackend>>,
        now: Instant,
    ) {
        if self.output_failed || sb.init.is_none() {
            return;
        }
        if let Some(c) = self.held.take() {
            self.send(c, now);
        }
        while self.held.is_none() && self.sent_end_us - self.current_time_us() < DECODE_AHEAD_US {
            let Some(frame) = sb
                .frames
                .range(self.cursor_us - 2_000..)
                .next()
                .map(|(_, f)| f)
            else {
                break;
            };
            if frame.pts_us - self.cursor_us > 100_000 {
                break;
            }
            if self.ensure_decoder(&frame.init).is_err() {
                return;
            }
            let encrypted = frame.enc.is_some();
            match self.decrypt(frame, &frame.init, cdms) {
                None | Some(Decrypted::NoKey) => {
                    self.wait_for_key();
                    break;
                }
                Some(Decrypted::Failed(code)) => {
                    *self.counters.decrypt_fail.entry(code).or_default() += 1;
                    self.cursor_us = frame.end_us();
                    continue;
                }
                Some(Decrypted::Ok) => self.waiting_for_key = false,
            }
            if encrypted {
                self.counters.encrypted_samples += 1;
                self.counters.decrypt_ok += 1;
            } else {
                self.counters.clear_samples += 1;
            }
            let payload: &[u8] = if encrypted {
                &self.out_buf
            } else {
                &frame.data
            };
            let packet = Packet::new_from_slice(0, frame.pts_us as u64, 1024, payload);
            let decoded = self.decoder.as_mut().map(|(_, d)| {
                d.decode(&packet).map(|buf| {
                    (*buf.spec(), buf.frames(), {
                        let mut sb = SampleBuffer::<i16>::new(buf.capacity() as u64, *buf.spec());
                        sb.copy_interleaved_ref(buf);
                        sb
                    })
                })
            });
            self.cursor_us = frame.end_us();
            match decoded {
                Some(Ok((spec, frames, pcm))) => {
                    let frames = frames.min(
                        (frame.dur_us as i128 * spec.rate as i128 + 500_000).div_euclid(1_000_000)
                            as usize,
                    );
                    let channels = spec.channels.count();
                    let chunk = Chunk {
                        pts_us: frame.pts_us,
                        rate: spec.rate,
                        channels: channels as u16,
                        frames,
                        samples: pcm.samples()[..frames * channels].to_vec(),
                        encrypted,
                    };
                    self.send(chunk, now)
                }
                Some(Err(e)) => {
                    self.counters.decode_err += 1;
                    if self.counters.decode_err < 5 {
                        eprintln!("ENGINE decode error encrypted={encrypted} {e}");
                    }
                }
                None => {}
            }
        }
    }

    fn send(&mut self, c: Chunk, now: Instant) {
        let end_us = c.pts_us + frames_to_us(c.frames as u64, c.rate);
        if end_us <= self.skip_until_us {
            return;
        }
        if self.sink.is_some() && self.sink_format != Some((c.rate, c.channels)) {
            if self.sent_since_anchor && !self.sink_drained(now) {
                self.held = Some(c);
                return;
            }
            let position = self.current_time_us();
            self.sink = None;
            self.anchor(position, false);
        }
        if self.sink.is_none() {
            let mut s = (self.sink_factory)();
            s.set_gain(self.gain);
            self.sink = Some(s);
            self.sink_format = Some((c.rate, c.channels));
        }
        let Chunk {
            pts_us,
            rate,
            channels,
            frames,
            mut samples,
            encrypted,
        } = c;
        self.rate = rate;
        self.channels = channels;
        let skip = if self.skip_until_us > pts_us {
            ((self.skip_until_us - pts_us) as i128 * rate as i128 / 1_000_000) as usize
        } else {
            0
        };
        let skip = skip.min(frames);
        let start_us = pts_us.max(self.skip_until_us);
        self.skip_until_us = i64::MIN;
        let kept = (frames - skip) as u64;
        if encrypted {
            self.counters.decoded_decrypted_frames += kept;
        } else {
            self.counters.decoded_clear_frames += kept;
        }
        if !self.sent_since_anchor && start_us > self.base_us {
            self.base_us = start_us;
            self.position_us = start_us;
        }
        self.sent_since_anchor = true;
        self.sent_end_us = end_us;
        samples.drain(..skip * channels as usize);
        if let Some(s) = self.sink.as_mut() {
            s.write(start_us, rate, channels, samples);
        }
    }

    pub fn pump(
        &mut self,
        now: Instant,
        sbs: &HashMap<u32, SourceBufferNative>,
        mses: &HashMap<u32, MediaSourceNative>,
        cdms: &mut HashMap<u32, Box<dyn CdmBackend>>,
    ) {
        let Some(ms) = self.ms_id.and_then(|id| mses.get(&id)) else {
            return;
        };
        if self.sink.as_ref().is_some_and(|s| s.failed()) {
            self.restart_output();
        }
        let sb = ms.sbs.first().and_then(|id| sbs.get(id));
        if let Some(sb) = sb {
            if ms.ended
                && !self.seeking
                && !self.paused
                && self.ready_state >= HAVE_CURRENT_DATA
                && self.at_stream_end(sb)
            {
                if self.sink_drained(now) {
                    self.reach_end();
                }
                return;
            }
            if self.seeking {
                self.try_complete_seek(sb, ms, now);
            }
        }
        let rs = self.computed_ready_state(sbs, ms);
        self.set_ready_state(rs);
        let Some(sb) = sb else { return };
        if self.paused
            || self.seeking
            || self.ready_state < HAVE_FUTURE_DATA
                && !(ms.ended && self.ready_state >= HAVE_CURRENT_DATA)
        {
            return;
        }
        self.decode_ahead(sb, cdms, now);
        self.max_current_time_us = self.max_current_time_us.max(self.current_time_us());
        if self
            .last_timeupdate
            .is_none_or(|t| now.duration_since(t) >= TIMEUPDATE_INTERVAL)
        {
            self.last_timeupdate = Some(now);
            self.emit("timeupdate");
        }
    }
}
