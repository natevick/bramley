use super::{FIXTURE, fixture_segments};
use crate::buffer::SourceBufferNative;

fn long_timeline(
    quota: usize,
    seconds: u32,
    position_us: impl Fn(f64) -> i64,
) -> (SourceBufferNative, usize, bool) {
    let (init, media) = fixture_segments();
    let mut sb = SourceBufferNative::new("audio/mp4".into(), quota);
    sb.append(&init, 0.0).unwrap();
    let mut peak = 0;
    let mut refused = false;
    for round in 0..seconds / 4 {
        let offset = round as f64 * 4.0;
        for seg in &media[..4] {
            if !sb.make_room(seg.len(), position_us(offset)) {
                refused = true;
                continue;
            }
            sb.append(seg, offset).unwrap();
            peak = peak.max(sb.bytes);
        }
    }
    (sb, peak, refused)
}

#[test]
fn remove_drops_frames_and_their_bytes() {
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    sb.append(FIXTURE, 0.0).unwrap();
    let before = sb.bytes;
    sb.remove(0.0, 2.0);
    let ranges = sb.buffered();
    assert!(ranges[0].0 >= 2.0 && ranges[0].0 < 2.03, "{ranges:?}");
    assert!(sb.bytes < before * 2 / 3);
    sb.remove(0.0, f64::INFINITY);
    assert_eq!((sb.bytes, sb.frames.len()), (0, 0));
}

#[test]
fn remove_with_an_inverted_range_is_a_no_op() {
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    sb.append(FIXTURE, 0.0).unwrap();
    let frames = sb.frames.len();
    sb.remove(3.0, 1.0);
    assert_eq!(sb.frames.len(), frames);
}

#[test]
fn reappending_the_same_range_replaces_frames_without_double_counting() {
    let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
    sb.append(FIXTURE, 0.0).unwrap();
    let (frames, bytes) = (sb.frames.len(), sb.bytes);
    let (_, media) = fixture_segments();
    for seg in &media {
        sb.append(seg, 0.0).unwrap();
    }
    assert_eq!((sb.frames.len(), sb.bytes), (frames, bytes));
}

#[test]
fn eviction_keeps_a_long_track_under_quota_while_playback_advances() {
    let quota = 40_000;
    let (sb, peak, refused) =
        long_timeline(quota, 400, |offset| ((offset - 2.0).max(0.0) * 1e6) as i64);
    assert!(!refused);
    assert!(peak <= quota, "peak {peak}");
    assert!(sb.evicted_frames > 15_000, "evicted {}", sb.evicted_frames);
    let ranges = sb.buffered();
    assert!(
        ranges[0].0 > 380.0 && ranges.last().unwrap().1 > 399.9,
        "{ranges:?}"
    );
}

#[test]
fn eviction_never_removes_data_at_or_ahead_of_the_playback_position() {
    let (sb, _, refused) = long_timeline(40_000, 40, |_| 0);
    assert!(
        refused,
        "appends beyond quota with nothing behind the playhead must be refused"
    );
    assert_eq!(sb.evicted_frames, 0);
    assert!(sb.bytes <= 40_000);
}
