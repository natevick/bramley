use std::collections::BTreeMap;
use std::rc::Rc;

use anyhow::{Result, anyhow};

use crate::mp4::{self, SampleEnc, TrackInit};

pub const SOURCE_BUFFER_QUOTA_BYTES: usize = 12 << 20;
const EVICTION_MARGIN_US: i64 = 1_000_000;
const RANGE_GAP_US: i64 = 50_000;

pub struct Frame {
    pub pts_us: i64,
    pub dur_us: i64,
    pub offset_us: i64,
    pub data: Vec<u8>,
    pub enc: Option<SampleEnc>,
    pub init: Rc<TrackInit>,
}

impl Frame {
    pub fn end_us(&self) -> i64 {
        self.pts_us.saturating_add(self.dur_us)
    }
}

pub struct SourceBufferNative {
    pub mime: String,
    pending: Vec<u8>,
    pub init: Option<Rc<TrackInit>>,
    pub frames: BTreeMap<i64, Frame>,
    pub init_data: Option<Vec<u8>>,
    pub appended_bytes: u64,
    pub bytes: usize,
    pub evicted_frames: u64,
    pub append_window_us: (i64, i64),
    quota: usize,
}

pub struct AppendResult {
    pub init_segment: bool,
    pub media_frames: usize,
}

impl SourceBufferNative {
    pub fn new(mime: String, quota: usize) -> Self {
        SourceBufferNative {
            mime,
            pending: Vec::new(),
            init: None,
            frames: BTreeMap::new(),
            init_data: None,
            appended_bytes: 0,
            bytes: 0,
            evicted_frames: 0,
            append_window_us: (0, i64::MAX),
            quota,
        }
    }

    pub fn abort(&mut self) {
        self.pending.clear();
    }

    fn insert(&mut self, f: Frame) {
        self.bytes += f.data.len();
        if let Some(old) = self.frames.insert(f.pts_us, f) {
            self.bytes -= old.data.len();
        }
    }

    fn remove_us(&mut self, start: i64, end: i64) -> u64 {
        if start >= end {
            return 0;
        }
        let keys: Vec<i64> = self.frames.range(start..end).map(|(k, _)| *k).collect();
        for k in &keys {
            if let Some(f) = self.frames.remove(k) {
                self.bytes -= f.data.len();
            }
        }
        keys.len() as u64
    }

    pub fn make_room(&mut self, incoming: usize, position_us: i64) -> bool {
        if self.bytes + self.pending.len() + incoming <= self.quota {
            return true;
        }
        let limit = position_us - EVICTION_MARGIN_US;
        let mut cut = None;
        let mut freed = 0usize;
        for f in self.frames.values() {
            if f.end_us() > limit
                || self.bytes - freed + self.pending.len() + incoming <= self.quota
            {
                break;
            }
            freed += f.data.len();
            cut = Some(f.end_us());
        }
        if let Some(cut) = cut {
            self.evicted_frames += self.remove_us(i64::MIN, cut);
        }
        self.bytes + self.pending.len() + incoming <= self.quota
    }

    pub fn append(&mut self, bytes: &[u8], ts_offset: f64) -> Result<AppendResult> {
        self.appended_bytes += bytes.len() as u64;
        self.pending.extend_from_slice(bytes);
        let complete = mp4::complete_top_level(&self.pending);
        let buf = std::mem::take(&mut self.pending);
        let tops = mp4::boxes(&buf, 0, complete);
        let mut consumed = 0usize;
        let mut result = AppendResult {
            init_segment: false,
            media_frames: 0,
        };
        let offset_us = (ts_offset * 1e6).round() as i64;
        let mut i = 0;
        while i < tops.len() {
            let b = &tops[i];
            if b.is("moov") {
                let init = mp4::parse_init(&buf, b)?;
                if !init.pssh.is_empty() {
                    self.init_data = Some(init.pssh.clone());
                }
                self.init = Some(Rc::new(init));
                result.init_segment = true;
            } else if b.is("moof") {
                let Some(mdat) = tops.get(i + 1).filter(|x| x.is("mdat")) else {
                    if i + 1 >= tops.len() {
                        break;
                    }
                    consumed = b.end;
                    i += 1;
                    continue;
                };
                let init = self
                    .init
                    .clone()
                    .ok_or_else(|| anyhow!("media segment before init segment"))?;
                let ts = init.timescale;
                let (window_start, window_end) = self.append_window_us;
                let frames: Vec<Frame> = mp4::parse_fragment(&buf, b, &init)?
                    .into_iter()
                    .map(|s| Frame {
                        pts_us: mp4::ticks_to_us(s.dts, ts).saturating_add(offset_us),
                        dur_us: mp4::ticks_to_us(s.dur as i64, ts),
                        offset_us,
                        data: s.data,
                        enc: s.enc,
                        init: init.clone(),
                    })
                    .filter(|f| f.pts_us >= window_start && f.end_us() <= window_end)
                    .collect();
                if let (Some(first), Some(last)) = (frames.first(), frames.last()) {
                    self.remove_us(first.pts_us, last.end_us());
                }
                for f in frames {
                    self.insert(f);
                    result.media_frames += 1;
                }
                consumed = mdat.end;
                i += 2;
                continue;
            }
            consumed = b.end;
            i += 1;
        }
        self.pending = buf[consumed..].to_vec();
        Ok(result)
    }

    pub fn remove(&mut self, start: f64, end: f64) {
        let e = if end.is_finite() {
            (end * 1e6) as i64
        } else {
            i64::MAX
        };
        self.remove_us((start * 1e6) as i64, e);
    }

    pub fn buffered_us(&self) -> Vec<(i64, i64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        for f in self.frames.values() {
            match out.last_mut() {
                Some(last) if f.pts_us.saturating_sub(last.1) <= RANGE_GAP_US => {
                    last.1 = last.1.max(f.end_us())
                }
                _ => out.push((f.pts_us, f.end_us())),
            }
        }
        out
    }

    pub fn buffered(&self) -> Vec<(f64, f64)> {
        self.buffered_us()
            .into_iter()
            .map(|(a, b)| (a as f64 / 1e6, b as f64 / 1e6))
            .collect()
    }

    pub fn end_us(&self) -> Option<i64> {
        self.frames.values().next_back().map(Frame::end_us)
    }

    pub fn offset_us_at(&self, t_us: i64) -> Option<i64> {
        let frame = self
            .frames
            .range(..=t_us)
            .next_back()
            .map(|(_, f)| f)
            .or_else(|| self.frames.values().next());
        frame.map(|f| f.offset_us)
    }
}

pub struct MediaSourceNative {
    pub duration: f64,
    pub ended: bool,
    pub sbs: Vec<u32>,
}
