use super::{FIXTURE, fixture_segments};
use crate::buffer::SourceBufferNative;
use crate::mp4::{self, Protection, SampleEnc};

fn bx(typ: &str, parts: &[&[u8]]) -> Vec<u8> {
    let body: Vec<u8> = parts.concat();
    [
        &((body.len() + 8) as u32).to_be_bytes()[..],
        typ.as_bytes(),
        &body,
    ]
    .concat()
}

fn protected_init(kid: &[u8; 16]) -> Vec<u8> {
    let asc = [0x12u8, 0x10];
    let esds = bx(
        "esds",
        &[
            &[0; 4],
            &[0x03, 3 + 2 + 13 + 2 + asc.len() as u8, 0, 1, 0],
            &[0x04, 13],
            &[0; 13],
            &[0x05, asc.len() as u8],
            &asc,
        ],
    );
    let tenc = bx("tenc", &[&[1, 0, 0, 0, 0, 0x19, 1, 8], kid]);
    let sinf = bx(
        "sinf",
        &[
            &bx("frma", &[b"mp4a"]),
            &bx("schm", &[&[0; 4], b"cbcs", &[0, 1, 0, 0]]),
            &bx("schi", &[&tenc]),
        ],
    );
    let entry_fields = [
        &[0u8; 6][..],
        &[0, 1],
        &[0; 8],
        &[0, 2],
        &[0, 16],
        &[0; 4],
        &(48000u32 << 16).to_be_bytes(),
    ]
    .concat();
    let enca = bx("enca", &[&entry_fields, &esds, &sinf]);
    let stsd = bx("stsd", &[&[0; 4], &1u32.to_be_bytes(), &enca]);
    let mdhd = bx("mdhd", &[&[0; 12], &48000u32.to_be_bytes(), &[0; 8]]);
    let trak = bx(
        "trak",
        &[&bx("mdia", &[&mdhd, &bx("minf", &[&bx("stbl", &[&stsd])])])],
    );
    let pssh = bx("pssh", &[&[0; 4], &[0xed; 16], &0u32.to_be_bytes()]);
    bx(
        "moov",
        &[
            &trak,
            &pssh,
            &bx(
                "mvex",
                &[&bx(
                    "trex",
                    &[
                        &[0; 12],
                        &1024u32.to_be_bytes(),
                        &0u32.to_be_bytes(),
                        &[0; 4],
                    ],
                )],
            ),
        ],
    )
}

fn protected_fragment(samples: &[&[u8]]) -> Vec<u8> {
    let mut trun_entries = Vec::new();
    let mut senc_entries = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        trun_entries.extend(1024u32.to_be_bytes());
        trun_entries.extend((s.len() as u32).to_be_bytes());
        senc_entries.extend([i as u8; 8]);
        senc_entries.extend([0, 1, 0, 2]);
        senc_entries.extend((s.len() as u32 - 2).to_be_bytes());
    }
    let count = (samples.len() as u32).to_be_bytes();
    let build = |data_offset: u32| {
        let tfhd = bx("tfhd", &[&[0, 2, 0, 0], &1u32.to_be_bytes()]);
        let tfdt = bx("tfdt", &[&[1, 0, 0, 0], &96000u64.to_be_bytes()]);
        let trun = bx(
            "trun",
            &[
                &[0, 0, 3, 1],
                &count,
                &data_offset.to_be_bytes(),
                &trun_entries,
            ],
        );
        let senc = bx("senc", &[&[0, 0, 0, 2], &count, &senc_entries]);
        bx("moof", &[&bx("traf", &[&tfhd, &tfdt, &trun, &senc])])
    };
    let moof_len = build(0).len() as u32;
    [build(moof_len + 8), bx("mdat", &[&samples.concat()])].concat()
}

#[test]
fn parses_the_clear_fixture_init_segment() {
    let (init, _) = fixture_segments();
    let moov = mp4::boxes(&init, 0, init.len())
        .into_iter()
        .find(|b| b.is("moov"))
        .unwrap();
    let t = mp4::parse_init(&init, &moov).unwrap();
    assert_eq!(
        (t.timescale, t.channels, t.sample_rate, t.codec.as_str()),
        (48000, 1, 48000, "mp4a.40.2")
    );
    assert_eq!(t.asc[0] >> 3, 2);
    assert!(t.protection.is_none() && t.pssh.is_empty());
}

#[test]
fn demuxes_contiguous_aac_frames_from_every_fragment() {
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    let r = sb.append(FIXTURE, 0.0).unwrap();
    assert!(r.init_segment);
    let frames: Vec<_> = sb.frames.values().collect();
    assert!(frames.len() > 180, "{} frames", frames.len());
    assert!(
        frames[..frames.len() - 1]
            .iter()
            .all(|f| f.dur_us == 21_333 && !f.data.is_empty() && f.enc.is_none())
    );
    assert!(frames.windows(2).all(|w| w[1].pts_us - w[0].end_us() <= 1));
    let ranges = sb.buffered();
    assert_eq!(ranges.len(), 1);
    assert!(ranges[0].0 < 0.05 && ranges[0].1 > 3.9, "{ranges:?}");
}

#[test]
fn byte_by_byte_appends_produce_the_same_frames_as_one_append() {
    let mut whole = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    whole.append(FIXTURE, 0.0).unwrap();
    let mut split = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    for chunk in FIXTURE.chunks(7) {
        split.append(chunk, 0.0).unwrap();
    }
    assert_eq!(whole.frames.len(), split.frames.len());
    assert!(
        whole
            .frames
            .values()
            .zip(split.frames.values())
            .all(|(a, b)| a.pts_us == b.pts_us && a.data == b.data)
    );
}

#[test]
fn timestamp_offset_shifts_frames() {
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    sb.append(FIXTURE, 10.0).unwrap();
    assert!(sb.buffered()[0].0 >= 10.0);
}

#[test]
fn media_segment_before_init_is_an_error() {
    let (_, media) = fixture_segments();
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    assert!(sb.append(&media[0], 0.0).is_err());
}

#[test]
fn corrupt_bytes_return_errors_instead_of_panicking() {
    for pos in 0..FIXTURE.len().min(2000) {
        for value in [0x00, 0xff, 0x7f] {
            let mut bytes = FIXTURE.to_vec();
            bytes[pos] = value;
            let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
            let _ = sb.append(&bytes, 0.0);
        }
    }
    for cut in (0..FIXTURE.len()).step_by(13) {
        let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
        let _ = sb.append(&FIXTURE[..cut], 0.0);
    }
}

#[test]
fn huge_sample_counts_are_rejected() {
    let mut frag = protected_fragment(&[b"abcdef"]);
    let trun = frag.windows(4).position(|w| w == b"trun").unwrap();
    frag[trun + 8..trun + 12].copy_from_slice(&u32::MAX.to_be_bytes());
    let init = protected_init(&[1; 16]);
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    sb.append(&init, 0.0).unwrap();
    assert!(sb.append(&frag, 0.0).is_err());
}

#[test]
fn parses_cbcs_protection_pssh_and_per_sample_subsamples() {
    let kid = [0xab; 16];
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    sb.append(
        &[
            protected_init(&kid),
            protected_fragment(&[b"0123456789", b"abcdefgh"]),
        ]
        .concat(),
        0.0,
    )
    .unwrap();
    let init = sb.init.as_ref().unwrap();
    assert_eq!(
        init.protection,
        Some(Protection {
            scheme: "cbcs".into(),
            kid: kid.to_vec(),
            iv_size: 8,
            constant_iv: vec![],
            crypt: 1,
            skip: 9
        })
    );
    assert_eq!((init.channels, init.default_duration), (2, 1024));
    assert_eq!(sb.init_data.as_ref().map(|p| &p[4..8]), Some(&b"pssh"[..]));
    let frames: Vec<_> = sb.frames.values().collect();
    assert_eq!(
        frames.iter().map(|f| f.pts_us).collect::<Vec<_>>(),
        vec![2_000_000, 2_021_333]
    );
    assert_eq!(frames[0].data, b"0123456789");
    assert_eq!(
        frames[1].enc,
        Some(SampleEnc {
            iv: vec![1; 8],
            subsamples: vec![(2, 6)]
        })
    );
}
