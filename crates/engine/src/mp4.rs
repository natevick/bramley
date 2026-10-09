use anyhow::{Result, anyhow};

pub fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
pub fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
pub fn be64(b: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(b[o..o + 8].try_into().unwrap())
}

pub struct BoxRef {
    pub typ: [u8; 4],
    pub start: usize,
    pub body: usize,
    pub end: usize,
}

impl BoxRef {
    pub fn is(&self, t: &str) -> bool {
        &self.typ == t.as_bytes()
    }
}

pub fn boxes(b: &[u8], start: usize, end: usize) -> Vec<BoxRef> {
    let mut out = Vec::new();
    let mut off = start;
    while off + 8 <= end {
        let mut size = be32(b, off) as usize;
        let typ: [u8; 4] = b[off + 4..off + 8].try_into().unwrap();
        let mut hdr = 8;
        if size == 1 {
            if off + 16 > end {
                break;
            }
            size = be64(b, off + 8) as usize;
            hdr = 16;
        }
        if size == 0 {
            size = end - off;
        }
        if size < hdr || off + size > end {
            break;
        }
        out.push(BoxRef { typ, start: off, body: off + hdr, end: off + size });
        off += size;
    }
    out
}

pub fn complete_top_level(b: &[u8]) -> usize {
    let mut off = 0usize;
    while off + 8 <= b.len() {
        let mut size = be32(b, off) as usize;
        if size == 1 {
            if off + 16 > b.len() {
                break;
            }
            size = be64(b, off + 8) as usize;
        }
        if size == 0 || size < 8 || off + size > b.len() {
            break;
        }
        off += size;
    }
    off
}

pub fn find(b: &[u8], start: usize, end: usize, path: &[&str]) -> Option<BoxRef> {
    for x in boxes(b, start, end) {
        if x.is(path[0]) {
            if path.len() == 1 {
                return Some(x);
            }
            if let Some(r) = find(b, x.body, x.end, &path[1..]) {
                return Some(r);
            }
        }
    }
    None
}

#[derive(Clone, Debug, Default)]
pub struct Protection {
    pub scheme: String,
    pub kid: Vec<u8>,
    pub iv_size: usize,
    pub constant_iv: Vec<u8>,
    pub crypt: u32,
    pub skip: u32,
}

#[derive(Clone, Debug, Default)]
pub struct TrackInit {
    pub track_id: u32,
    pub timescale: u32,
    pub codec: String,
    pub asc: Vec<u8>,
    pub channels: u16,
    pub sample_rate: u32,
    pub default_duration: u32,
    pub default_size: u32,
    pub protection: Option<Protection>,
    pub pssh: Vec<u8>,
    pub duration_secs: Option<f64>,
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

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn parse_init(b: &[u8], moov: &BoxRef) -> Result<TrackInit> {
    let mut t = TrackInit::default();
    for x in boxes(b, moov.body, moov.end) {
        if x.is("pssh") {
            t.pssh.extend_from_slice(&b[x.start..x.end]);
        }
    }
    if let Some(mvhd) = find(b, moov.body, moov.end, &["mvhd"]) {
        let v = b[mvhd.body];
        let (ts, dur) = if v == 1 { (be32(b, mvhd.body + 20), be64(b, mvhd.body + 24)) } else { (be32(b, mvhd.body + 12), be32(b, mvhd.body + 16) as u64) };
        if ts > 0 && dur > 0 && dur != u32::MAX as u64 && dur != u64::MAX {
            t.duration_secs = Some(dur as f64 / ts as f64);
        }
    }
    if let Some(trex) = find(b, moov.body, moov.end, &["mvex", "trex"]) {
        t.default_duration = be32(b, trex.body + 12);
        t.default_size = be32(b, trex.body + 16);
    }
    let trak = find(b, moov.body, moov.end, &["trak"]).ok_or_else(|| anyhow!("no trak"))?;
    if let Some(tkhd) = find(b, trak.body, trak.end, &["tkhd"]) {
        let v = b[tkhd.body];
        t.track_id = if v == 1 { be32(b, tkhd.body + 20) } else { be32(b, tkhd.body + 12) };
    }
    let mdhd = find(b, trak.body, trak.end, &["mdia", "mdhd"]).ok_or_else(|| anyhow!("no mdhd"))?;
    t.timescale = if b[mdhd.body] == 1 { be32(b, mdhd.body + 20) } else { be32(b, mdhd.body + 12) };
    let stsd = find(b, trak.body, trak.end, &["mdia", "minf", "stbl", "stsd"]).ok_or_else(|| anyhow!("no stsd"))?;
    let entries = boxes(b, stsd.body + 8, stsd.end);
    let entry = entries.iter().find(|e| e.is("enca") || e.is("mp4a")).ok_or_else(|| anyhow!("no mp4a/enca sample entry"))?;
    t.channels = be16(b, entry.body + 16);
    t.sample_rate = be32(b, entry.body + 24) >> 16;
    let inner = entry.body + 28;
    let esds = find(b, inner, entry.end, &["esds"]).ok_or_else(|| anyhow!("no esds"))?;
    t.asc = asc_from_esds(b, esds.body, esds.end).ok_or_else(|| anyhow!("no AudioSpecificConfig"))?;
    t.codec = format!("mp4a.40.{}", t.asc.first().map(|x| x >> 3).unwrap_or(2));
    if entry.is("enca") {
        let sinf = find(b, inner, entry.end, &["sinf"]).ok_or_else(|| anyhow!("enca without sinf"))?;
        let mut p = Protection { scheme: "cenc".into(), ..Default::default() };
        if let Some(schm) = find(b, sinf.body, sinf.end, &["schm"]) {
            p.scheme = String::from_utf8_lossy(&b[schm.body + 4..schm.body + 8]).into_owned();
        }
        let tenc = find(b, sinf.body, sinf.end, &["schi", "tenc"]).ok_or_else(|| anyhow!("no tenc"))?;
        let version = b[tenc.body];
        if version >= 1 {
            let pat = b[tenc.body + 5];
            p.crypt = (pat >> 4) as u32;
            p.skip = (pat & 0x0f) as u32;
        }
        p.iv_size = b[tenc.body + 7] as usize;
        p.kid = b[tenc.body + 8..tenc.body + 24].to_vec();
        if p.iv_size == 0 && tenc.end > tenc.body + 25 {
            let n = b[tenc.body + 24] as usize;
            p.constant_iv = b[tenc.body + 25..tenc.body + 25 + n].to_vec();
        }
        t.protection = Some(p);
    }
    Ok(t)
}

#[derive(Clone, Debug)]
pub struct SampleEnc {
    pub iv: Vec<u8>,
    pub subsamples: Vec<(u32, u32)>,
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub dts: i64,
    pub dur: u32,
    pub data: Vec<u8>,
    pub enc: Option<SampleEnc>,
}

pub fn parse_fragment(b: &[u8], moof: &BoxRef, track: &TrackInit) -> Vec<Sample> {
    let mut out = Vec::new();
    let iv_size = track.protection.as_ref().map(|p| p.iv_size).unwrap_or(0);
    for traf in boxes(b, moof.body, moof.end).into_iter().filter(|x| x.is("traf")) {
        let mut default_size = track.default_size;
        let mut default_dur = track.default_duration;
        let mut base = moof.start as i64;
        let mut decode_time = 0u64;
        let mut runs: Vec<(i64, Vec<(u32, u32)>)> = Vec::new();
        let mut ivs: Vec<SampleEnc> = Vec::new();
        let mut have_senc = false;
        for x in boxes(b, traf.body, traf.end) {
            if x.is("tfhd") {
                let flags = be32(b, x.body) & 0xff_ffff;
                let mut p = x.body + 8;
                if flags & 0x1 != 0 {
                    base = be64(b, p) as i64;
                    p += 8;
                }
                if flags & 0x2 != 0 {
                    p += 4;
                }
                if flags & 0x8 != 0 {
                    default_dur = be32(b, p);
                    p += 4;
                }
                if flags & 0x10 != 0 {
                    default_size = be32(b, p);
                }
            } else if x.is("tfdt") {
                decode_time = if b[x.body] == 1 { be64(b, x.body + 4) } else { be32(b, x.body + 4) as u64 };
            } else if x.is("trun") {
                let flags = be32(b, x.body) & 0xff_ffff;
                let count = be32(b, x.body + 4) as usize;
                let mut p = x.body + 8;
                let mut data_offset = 0i64;
                if flags & 0x1 != 0 {
                    data_offset = be32(b, p) as i32 as i64;
                    p += 4;
                }
                if flags & 0x4 != 0 {
                    p += 4;
                }
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let dur = if flags & 0x100 != 0 {
                        let d = be32(b, p);
                        p += 4;
                        d
                    } else {
                        default_dur
                    };
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
                    entries.push((dur, size));
                }
                runs.push((data_offset, entries));
            } else if x.is("senc") {
                have_senc = true;
                let flags = be32(b, x.body) & 0xff_ffff;
                let count = be32(b, x.body + 4) as usize;
                let mut p = x.body + 8;
                for _ in 0..count {
                    let iv = if iv_size > 0 { b[p..p + iv_size].to_vec() } else { Vec::new() };
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
                    ivs.push(SampleEnc { iv, subsamples: subs });
                }
            }
        }
        let mut dts = decode_time as i64;
        let mut idx = 0usize;
        for (data_offset, entries) in runs {
            let mut pos = (base + data_offset) as usize;
            for (dur, size) in entries {
                let end = pos + size as usize;
                if end > b.len() {
                    break;
                }
                let enc = if have_senc && track.protection.is_some() {
                    ivs.get(idx).cloned().map(|mut e| {
                        if e.iv.is_empty() {
                            e.iv = track.protection.as_ref().unwrap().constant_iv.clone();
                        }
                        e
                    })
                } else {
                    None
                };
                out.push(Sample { dts, dur, data: b[pos..end].to_vec(), enc });
                dts += dur as i64;
                pos = end;
                idx += 1;
            }
        }
    }
    out
}
