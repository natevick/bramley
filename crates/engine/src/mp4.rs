use anyhow::{Result, anyhow, bail};

fn bytes<const N: usize>(b: &[u8], o: usize) -> Result<[u8; N]> {
    o.checked_add(N)
        .and_then(|e| b.get(o..e))
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| anyhow!("truncated box at {o}"))
}
fn byte(b: &[u8], o: usize) -> Result<u8> {
    Ok(bytes::<1>(b, o)?[0])
}
fn slice(b: &[u8], o: usize, len: usize) -> Result<&[u8]> {
    o.checked_add(len)
        .and_then(|e| b.get(o..e))
        .ok_or_else(|| anyhow!("truncated box at {o}"))
}
pub fn be16(b: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(bytes(b, o)?))
}
pub fn be32(b: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes(b, o)?))
}
pub fn be64(b: &[u8], o: usize) -> Result<u64> {
    Ok(u64::from_be_bytes(bytes(b, o)?))
}

pub fn ticks_to_us(ticks: i64, timescale: u32) -> i64 {
    (ticks as i128 * 1_000_000 / timescale.max(1) as i128) as i64
}

pub struct BoxRef {
    pub typ: [u8; 4],
    pub start: usize,
    pub body: usize,
    pub end: usize,
}

impl BoxRef {
    pub fn is(&self, t: &str) -> bool {
        self.typ.as_slice() == t.as_bytes()
    }
}

fn box_size(b: &[u8], off: usize, end: usize) -> Option<(usize, usize)> {
    let mut size = be32(b, off).ok()? as usize;
    let mut hdr = 8;
    if size == 1 {
        size = usize::try_from(be64(b, off + 8).ok()?).ok()?;
        hdr = 16;
    } else if size == 0 {
        size = end - off;
    }
    (size >= hdr && off.checked_add(size)? <= end).then_some((size, hdr))
}

pub fn boxes(b: &[u8], start: usize, end: usize) -> Vec<BoxRef> {
    let end = end.min(b.len());
    let mut out = Vec::new();
    let mut off = start;
    while off + 8 <= end {
        let Some((size, hdr)) = box_size(b, off, end) else {
            break;
        };
        let typ: [u8; 4] = b[off + 4..off + 8].try_into().unwrap();
        out.push(BoxRef {
            typ,
            start: off,
            body: off + hdr,
            end: off + size,
        });
        off += size;
    }
    out
}

pub fn complete_top_level(b: &[u8]) -> usize {
    let mut off = 0usize;
    while off + 8 <= b.len() {
        if be32(b, off).ok() == Some(0) {
            break;
        }
        let Some((size, _)) = box_size(b, off, b.len()) else {
            break;
        };
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

#[derive(Clone, Debug, Default, PartialEq)]
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

fn descriptor_len(b: &[u8], p: &mut usize) -> Result<usize> {
    let mut len = 0usize;
    for _ in 0..4 {
        let c = byte(b, *p)?;
        *p += 1;
        len = (len << 7) | (c & 0x7f) as usize;
        if c & 0x80 == 0 {
            break;
        }
    }
    Ok(len)
}

fn asc_from_esds(b: &[u8], body: usize, end: usize) -> Result<Vec<u8>> {
    let mut p = body + 4;
    while p < end {
        let tag = byte(b, p)?;
        p += 1;
        let len = descriptor_len(b, &mut p)?;
        match tag {
            0x03 => {
                let flags = byte(b, p + 2)?;
                p += 3;
                if flags & 0x80 != 0 {
                    p += 2;
                }
                if flags & 0x40 != 0 {
                    p += 1 + byte(b, p)? as usize;
                }
                if flags & 0x20 != 0 {
                    p += 2;
                }
            }
            0x04 => p += 13,
            0x05 => return Ok(slice(b, p, len)?.to_vec()),
            _ => p += len,
        }
    }
    bail!("no AudioSpecificConfig")
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn versioned32(b: &[u8], body: usize, v0: usize, v1: usize) -> Result<u32> {
    be32(b, body + if byte(b, body)? == 1 { v1 } else { v0 })
}

pub fn parse_init(b: &[u8], moov: &BoxRef) -> Result<TrackInit> {
    let mut t = TrackInit::default();
    for x in boxes(b, moov.body, moov.end) {
        if x.is("pssh") {
            t.pssh.extend_from_slice(&b[x.start..x.end]);
        }
    }
    if let Some(mvhd) = find(b, moov.body, moov.end, &["mvhd"]) {
        let (ts, dur) = if byte(b, mvhd.body)? == 1 {
            (be32(b, mvhd.body + 20)?, be64(b, mvhd.body + 24)?)
        } else {
            (be32(b, mvhd.body + 12)?, be32(b, mvhd.body + 16)? as u64)
        };
        if ts > 0 && dur > 0 && dur != u32::MAX as u64 && dur != u64::MAX {
            t.duration_secs = Some(dur as f64 / ts as f64);
        }
    }
    if let Some(trex) = find(b, moov.body, moov.end, &["mvex", "trex"]) {
        t.default_duration = be32(b, trex.body + 12)?;
        t.default_size = be32(b, trex.body + 16)?;
    }
    let trak = find(b, moov.body, moov.end, &["trak"]).ok_or_else(|| anyhow!("no trak"))?;
    if let Some(tkhd) = find(b, trak.body, trak.end, &["tkhd"]) {
        t.track_id = versioned32(b, tkhd.body, 12, 20)?;
    }
    let mdhd = find(b, trak.body, trak.end, &["mdia", "mdhd"]).ok_or_else(|| anyhow!("no mdhd"))?;
    t.timescale = versioned32(b, mdhd.body, 12, 20)?;
    let stsd = find(b, trak.body, trak.end, &["mdia", "minf", "stbl", "stsd"])
        .ok_or_else(|| anyhow!("no stsd"))?;
    let entries = boxes(b, stsd.body + 8, stsd.end);
    let entry = entries
        .iter()
        .find(|e| e.is("enca") || e.is("mp4a"))
        .ok_or_else(|| anyhow!("no mp4a/enca sample entry"))?;
    t.channels = be16(b, entry.body + 16)?;
    t.sample_rate = be32(b, entry.body + 24)? >> 16;
    let inner = entry.body + 28;
    let esds = find(b, inner, entry.end, &["esds"]).ok_or_else(|| anyhow!("no esds"))?;
    t.asc = asc_from_esds(b, esds.body, esds.end)?;
    t.codec = format!("mp4a.40.{}", t.asc.first().map(|x| x >> 3).unwrap_or(2));
    if entry.is("enca") {
        let sinf =
            find(b, inner, entry.end, &["sinf"]).ok_or_else(|| anyhow!("enca without sinf"))?;
        let mut p = Protection {
            scheme: "cenc".into(),
            ..Default::default()
        };
        if let Some(schm) = find(b, sinf.body, sinf.end, &["schm"]) {
            p.scheme = String::from_utf8_lossy(slice(b, schm.body + 4, 4)?).into_owned();
        }
        let tenc =
            find(b, sinf.body, sinf.end, &["schi", "tenc"]).ok_or_else(|| anyhow!("no tenc"))?;
        if byte(b, tenc.body)? >= 1 {
            let pat = byte(b, tenc.body + 5)?;
            p.crypt = (pat >> 4) as u32;
            p.skip = (pat & 0x0f) as u32;
        }
        p.iv_size = byte(b, tenc.body + 7)? as usize;
        p.kid = slice(b, tenc.body + 8, 16)?.to_vec();
        if p.iv_size == 0 {
            let n = byte(b, tenc.body + 24)? as usize;
            p.constant_iv = slice(b, tenc.body + 25, n)?.to_vec();
        }
        t.protection = Some(p);
    }
    Ok(t)
}

#[derive(Clone, Debug, PartialEq)]
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

fn take32(b: &[u8], p: &mut usize) -> Result<u32> {
    let v = be32(b, *p)?;
    *p += 4;
    Ok(v)
}

fn bounded_count(b: &[u8], body: usize) -> Result<usize> {
    let count = be32(b, body + 4)? as usize;
    if count > b.len() {
        bail!("sample count {count} exceeds buffer");
    }
    Ok(count)
}

fn parse_senc(b: &[u8], x: &BoxRef, iv_size: usize) -> Result<Vec<SampleEnc>> {
    let flags = be32(b, x.body)? & 0xff_ffff;
    let count = bounded_count(b, x.body)?;
    let mut p = x.body + 8;
    let mut out = Vec::new();
    for _ in 0..count {
        let iv = slice(b, p, iv_size)?.to_vec();
        p += iv_size;
        let mut subsamples = Vec::new();
        if flags & 0x2 != 0 {
            let n = be16(b, p)? as usize;
            p += 2;
            for _ in 0..n {
                subsamples.push((be16(b, p)? as u32, be32(b, p + 2)?));
                p += 6;
            }
        }
        out.push(SampleEnc { iv, subsamples });
    }
    Ok(out)
}

pub fn parse_fragment(b: &[u8], moof: &BoxRef, track: &TrackInit) -> Result<Vec<Sample>> {
    let mut out = Vec::new();
    let protection = track.protection.as_ref();
    for traf in boxes(b, moof.body, moof.end)
        .into_iter()
        .filter(|x| x.is("traf"))
    {
        let mut default_size = track.default_size;
        let mut default_dur = track.default_duration;
        let mut base = moof.start as i64;
        let mut decode_time = 0u64;
        let mut runs: Vec<(i64, Vec<(u32, u32)>)> = Vec::new();
        let mut encs: Option<Vec<SampleEnc>> = None;
        for x in boxes(b, traf.body, traf.end) {
            if x.is("tfhd") {
                let flags = be32(b, x.body)? & 0xff_ffff;
                let mut p = x.body + 8;
                if flags & 0x1 != 0 {
                    base = be64(b, p)? as i64;
                    p += 8;
                }
                if flags & 0x2 != 0 {
                    p += 4;
                }
                if flags & 0x8 != 0 {
                    default_dur = take32(b, &mut p)?;
                }
                if flags & 0x10 != 0 {
                    default_size = be32(b, p)?;
                }
            } else if x.is("tfdt") {
                decode_time = if byte(b, x.body)? == 1 {
                    be64(b, x.body + 4)?
                } else {
                    be32(b, x.body + 4)? as u64
                };
            } else if x.is("trun") {
                let flags = be32(b, x.body)? & 0xff_ffff;
                let count = bounded_count(b, x.body)?;
                let mut p = x.body + 8;
                let data_offset = if flags & 0x1 != 0 {
                    take32(b, &mut p)? as i32 as i64
                } else {
                    0
                };
                if flags & 0x4 != 0 {
                    p += 4;
                }
                let mut entries = Vec::new();
                for _ in 0..count {
                    let dur = if flags & 0x100 != 0 {
                        take32(b, &mut p)?
                    } else {
                        default_dur
                    };
                    let size = if flags & 0x200 != 0 {
                        take32(b, &mut p)?
                    } else {
                        default_size
                    };
                    p += 4 * ((flags & 0x400 != 0) as usize + (flags & 0x800 != 0) as usize);
                    entries.push((dur, size));
                }
                runs.push((data_offset, entries));
            } else if x.is("senc") && protection.is_some() {
                encs = Some(parse_senc(
                    b,
                    &x,
                    protection.map(|p| p.iv_size).unwrap_or(0),
                )?);
            }
        }
        let mut dts = decode_time as i64;
        let mut idx = 0usize;
        for (data_offset, entries) in runs {
            let mut pos = base
                .checked_add(data_offset)
                .and_then(|p| usize::try_from(p).ok())
                .ok_or_else(|| anyhow!("bad data offset"))?;
            for (dur, size) in entries {
                let data = slice(b, pos, size as usize)?.to_vec();
                let enc = encs
                    .as_ref()
                    .and_then(|e| e.get(idx))
                    .cloned()
                    .map(|mut e| {
                        if e.iv.is_empty() {
                            e.iv = protection
                                .map(|p| p.constant_iv.clone())
                                .unwrap_or_default();
                        }
                        e
                    });
                out.push(Sample {
                    dts,
                    dur,
                    data,
                    enc,
                });
                dts = dts.wrapping_add(dur as i64);
                pos += size as usize;
                idx += 1;
            }
        }
    }
    Ok(out)
}
