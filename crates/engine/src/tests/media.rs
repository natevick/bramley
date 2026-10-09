use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::{FakeCdm, FakeSink, KID, XOR_KEY, fixture_segments};
use crate::audio::apply_gain;
use crate::buffer::{MediaSourceNative, SourceBufferNative};
use crate::cdm::CdmBackend;
use crate::media::MediaEngine;
use crate::mp4::{Protection, SampleEnc};

const SB: u32 = 1;
const MS: u32 = 2;
const KEYS: u32 = 3;
const STEP: Duration = Duration::from_millis(10);

struct Rig {
    m: MediaEngine,
    sink: FakeSink,
    cdm: FakeCdm,
    sbs: HashMap<u32, SourceBufferNative>,
    mses: HashMap<u32, MediaSourceNative>,
    cdms: HashMap<u32, Box<dyn CdmBackend>>,
    now: Instant,
}

impl Rig {
    fn new(segments: usize, ended: bool) -> Rig {
        let sink = FakeSink::default();
        let factory = sink.clone();
        let mut m = MediaEngine::new(Rc::new(move || Box::new(factory.clone())));
        let (init, media) = fixture_segments();
        let mut sb = SourceBufferNative::new("audio/mp4".into(), usize::MAX);
        sb.append(&init, 0.0).unwrap();
        for seg in &media[..segments] {
            sb.append(seg, 0.0).unwrap();
        }
        let duration = if ended { sb.end_us().unwrap() as f64 / 1e6 } else { f64::NAN };
        let cdm = FakeCdm::default();
        let mut cdms: HashMap<u32, Box<dyn CdmBackend>> = HashMap::new();
        cdms.insert(KEYS, Box::new(cdm.clone()));
        m.attach(MS);
        Rig { m, sink, cdm, sbs: HashMap::from([(SB, sb)]), mses: HashMap::from([(MS, MediaSourceNative { duration, ended, sbs: vec![SB] })]), cdms, now: Instant::now() }
    }

    fn pump(&mut self) -> Vec<&'static str> {
        self.now += STEP;
        self.m.pump(self.now, &self.sbs, &self.mses, &mut self.cdms);
        std::mem::take(&mut self.m.events)
    }

    fn play_for(&mut self, seconds: f64) -> Vec<&'static str> {
        let mut events = Vec::new();
        for _ in 0..(seconds * 100.0).round() as usize {
            events.extend(self.pump());
            self.sink.advance(480);
        }
        events.extend(self.pump());
        events
    }

    fn take(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.m.events)
    }

    fn sb(&mut self) -> &mut SourceBufferNative {
        self.sbs.get_mut(&SB).unwrap()
    }

    fn first_write_after(&self, n: usize) -> i64 {
        self.sink.state().writes[n].0
    }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn readiness_events_follow_spec_order_while_paused() {
    let mut r = Rig::new(5, false);
    assert_eq!(r.pump(), ["loadedmetadata", "loadeddata", "canplay", "canplaythrough"]);
    assert_eq!(r.m.ready_state, 4);
    r.pump();
    assert!(r.sink.state().writes.is_empty());
}

#[test]
fn play_before_data_waits_then_fires_playing() {
    let mut r = Rig::new(0, false);
    assert_eq!(r.pump(), ["loadedmetadata"]);
    r.m.play();
    assert_eq!(r.take(), ["play", "waiting"]);
    let (_, media) = fixture_segments();
    for seg in &media {
        r.sb().append(seg, 0.0).unwrap();
    }
    assert_eq!(r.pump(), ["loadeddata", "canplay", "playing", "canplaythrough", "timeupdate"]);
}

#[test]
fn current_time_follows_rendered_audio() {
    let mut r = Rig::new(5, false);
    r.m.play();
    r.play_for(1.0);
    let t = r.m.current_time();
    assert!(near(t, 1.0, 0.03), "{t}");
    assert!(near(t, r.m.output_position().unwrap(), 0.001));
}

#[test]
fn pause_freezes_time_corks_output_and_resumes_without_discarding() {
    let mut r = Rig::new(5, false);
    r.m.play();
    r.play_for(1.0);
    r.take();
    r.m.pause();
    assert_eq!(r.take(), ["timeupdate", "pause"]);
    let frozen = r.m.current_time();
    let writes = r.sink.state().writes.len();
    r.sink.0.borrow_mut().played += 2048;
    assert!(r.sink.state().paused);
    assert!(r.play_for(0.5).is_empty());
    assert_eq!(r.m.current_time(), frozen);
    assert_eq!(r.sink.state().writes.len(), writes);
    r.m.play();
    assert_eq!(r.take(), ["play", "playing"]);
    assert!(!r.sink.state().paused && r.sink.state().flushes == 0);
    assert_eq!(r.m.current_time(), frozen);
    r.play_for(0.5);
    assert!(near(r.m.current_time(), frozen + 0.5, 0.03));
}

#[test]
fn seek_within_buffer_while_paused_lands_exactly_and_discards_queued_audio() {
    let mut r = Rig::new(5, false);
    r.m.play();
    r.play_for(0.5);
    r.m.pause();
    r.take();
    r.m.seek(2_500_000);
    assert_eq!(r.take(), ["seeking"]);
    assert!(r.m.seeking && r.sink.state().queue.is_empty() && r.sink.state().flushes == 1);
    assert_eq!(r.m.current_time(), 2.5);
    assert_eq!(r.pump(), ["timeupdate", "seeked", "canplay"]);
    assert_eq!(r.m.current_time(), 2.5);
    let writes = r.sink.state().writes.len();
    r.m.play();
    assert_eq!(r.take(), ["play", "playing"]);
    r.play_for(0.2);
    assert_eq!(r.first_write_after(writes), 2_500_000);
    assert!(near(r.m.current_time(), 2.7, 0.03));
}

#[test]
fn seek_while_playing_reports_waiting_then_playing() {
    let mut r = Rig::new(5, false);
    r.m.play();
    r.play_for(0.5);
    r.take();
    r.m.seek(1_000_000);
    assert_eq!(r.take(), ["seeking", "timeupdate", "waiting"]);
    assert_eq!(r.pump()[..4], ["timeupdate", "seeked", "canplay", "playing"]);
}

#[test]
fn seek_outside_buffer_waits_for_appended_data() {
    let mut r = Rig::new(2, false);
    r.m.play();
    r.play_for(0.2);
    r.m.seek(3_500_000);
    r.take();
    assert!(!r.play_for(0.3).contains(&"seeked"));
    assert!(r.m.seeking);
    assert_eq!(r.m.current_time(), 3.5);
    let (_, media) = fixture_segments();
    for seg in &media[2..] {
        r.sb().append(seg, 0.0).unwrap();
    }
    let writes = r.sink.state().writes.len();
    assert!(r.pump().contains(&"seeked"));
    r.pump();
    assert_eq!(r.first_write_after(writes), 3_500_000);
}

#[test]
fn end_of_stream_fires_timeupdate_pause_ended_and_play_restarts() {
    let mut r = Rig::new(5, true);
    r.m.play();
    let events = r.play_for(5.0);
    assert_eq!(events[events.len() - 3..], ["timeupdate", "pause", "ended"]);
    assert!(r.m.ended && r.m.paused && r.sink.state().paused);
    let end = r.sbs[&SB].end_us().unwrap() as f64 / 1e6;
    assert!(near(r.m.current_time(), end, 0.03), "{} vs {end}", r.m.current_time());
    assert!(r.play_for(0.2).is_empty());
    r.m.play();
    assert_eq!(r.take(), ["play", "seeking", "timeupdate", "waiting"]);
    assert!(r.pump().contains(&"seeked"));
    assert!(!r.m.ended && !r.sink.state().paused);
    assert_eq!(r.m.current_time(), 0.0);
}

#[test]
fn looping_seeks_to_start_instead_of_ending() {
    let mut r = Rig::new(5, true);
    r.m.looping = true;
    r.m.play();
    let events = r.play_for(5.0);
    assert!(!events.contains(&"ended") && events.contains(&"seeked"));
    assert_eq!(r.m.counters.loops, 1);
    assert!(r.m.current_time() < 2.0);
}

#[test]
fn running_out_of_data_mid_stream_fires_timeupdate_then_waiting() {
    let mut r = Rig::new(2, false);
    r.m.play();
    let events = r.play_for(2.5);
    assert!(events.windows(2).any(|w| w == ["timeupdate", "waiting"]), "{events:?}");
    assert_eq!(r.m.counters.underruns, 1);
    assert!(r.m.current_time() <= r.sbs[&SB].end_us().unwrap() as f64 / 1e6);
}

#[test]
fn timeupdate_fires_at_most_every_250ms_while_playing() {
    let mut r = Rig::new(5, false);
    r.pump();
    r.m.play();
    r.take();
    let count = |e: &[&str]| e.iter().filter(|x| **x == "timeupdate").count();
    assert_eq!(count(&r.pump()), 1);
    assert_eq!(count(&r.play_for(0.2)), 0);
    assert_eq!(count(&r.play_for(0.1)), 1);
}

#[test]
fn gain_reaches_the_sink_before_and_after_it_exists() {
    let mut r = Rig::new(5, false);
    r.m.set_gain(0.25);
    r.m.play();
    r.play_for(0.1);
    assert_eq!(r.sink.state().gain, 0.25);
    r.m.set_gain(0.0);
    assert_eq!(r.sink.state().gain, 0.0);
}

#[test]
fn apply_gain_scales_and_clamps_pcm() {
    let pcm = |v: &[i16]| v.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>();
    let mut half = pcm(&[1000, -1000, 32767]);
    apply_gain(&mut half, 0.5);
    assert_eq!(half, pcm(&[500, -500, 16384]));
    let mut loud = pcm(&[30000, -30000]);
    apply_gain(&mut loud, 2.0);
    assert_eq!(loud, pcm(&[32767, -32768]));
}

#[test]
fn encrypted_frames_wait_for_a_key_without_skipping_audio() {
    let mut r = Rig::new(5, false);
    let sb = r.sb();
    sb.init.as_mut().unwrap().protection = Some(Protection { scheme: "cenc".into(), kid: KID.to_vec(), iv_size: 8, ..Default::default() });
    for f in sb.frames.values_mut() {
        f.data.iter_mut().for_each(|b| *b ^= XOR_KEY);
        f.enc = Some(SampleEnc { iv: vec![1; 8], subsamples: vec![] });
    }
    let first_pts = *sb.frames.keys().next().unwrap();
    r.m.set_keys(Some(KEYS));
    r.m.play();
    let events = r.play_for(0.3);
    assert_eq!(events.iter().filter(|e| **e == "waitingforkey").count(), 1);
    assert!(r.sink.state().writes.is_empty() && r.m.counters.decrypt_fail.is_empty());
    r.cdm.0.borrow_mut().has_key = true;
    r.play_for(0.3);
    assert_eq!(r.first_write_after(0), first_pts);
    assert!(!r.m.waiting_for_key && r.m.counters.decode_err == 0 && r.m.counters.decoded_decrypted_frames > 0);
    assert!(r.cdm.0.borrow().decrypts > 0);
}

#[test]
fn seeking_before_metadata_sets_the_start_position_without_events() {
    let sink = FakeSink::default();
    let mut m = MediaEngine::new(Rc::new(move || Box::new(sink.clone())));
    m.attach(MS);
    m.seek(1_500_000);
    assert!(m.events.is_empty() && !m.seeking && m.counters.seeks == 0);
    assert_eq!(m.current_time(), 1.5);
}

#[test]
fn cdm_status_two_is_no_key_and_other_failures_are_distinct() {
    use crate::cdm::Decrypted;
    assert_eq!([0, 1, 2, 4].map(Decrypted::from_status), [Decrypted::Ok, Decrypted::Failed(1), Decrypted::NoKey, Decrypted::Failed(4)]);
}
