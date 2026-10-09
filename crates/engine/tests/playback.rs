#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const SINK: &str = "engine-itest-null";

fn runtime_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        format!(
            "/run/user/{}",
            String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout)
                .unwrap()
                .trim()
        )
    })
}

fn tool(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd)
        .args(args)
        .env("XDG_RUNTIME_DIR", runtime_dir())
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Group(Child);

impl Drop for Group {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{}", self.0.id())])
            .status();
        let _ = self.0.wait();
    }
}

fn null_sink() -> Group {
    let node = format!(
        "create-node adapter {{ factory.name=support.null-audio-sink node.name={SINK} node.description={SINK} media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }}"
    );
    let child = Command::new("bash")
        .args([
            "-c",
            &format!("{{ echo '{node}'; exec sleep 100000; }} | pw-cli >/dev/null 2>&1"),
        ])
        .env("XDG_RUNTIME_DIR", runtime_dir())
        .process_group(0)
        .spawn()
        .unwrap();
    let sink = Group(child);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !tool("wpctl", &["status"]).contains(SINK) {
        assert!(
            Instant::now() < deadline,
            "null sink {SINK} never appeared; refusing to play audio"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    sink
}

fn default_sink() -> String {
    tool("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"])
        .lines()
        .find(|l| l.contains("node.name"))
        .unwrap_or_default()
        .to_string()
}

fn linked_targets(pid: u32) -> Vec<String> {
    let dump: Vec<Value> = serde_json::from_str(&tool("pw-dump", &[])).unwrap();
    let props = |o: &Value| o["info"]["props"].clone();
    let ours: Vec<u64> = dump
        .iter()
        .filter(|o| {
            o["type"] == "PipeWire:Interface:Node"
                && props(o)["application.process.id"]
                    .to_string()
                    .trim_matches('"')
                    == pid.to_string()
        })
        .filter_map(|o| o["id"].as_u64())
        .collect();
    let name_of = |id: &Value| {
        dump.iter()
            .find(|o| o["id"] == *id)
            .and_then(|o| props(o)["node.name"].as_str().map(String::from))
    };
    dump.iter()
        .filter(|o| {
            o["type"] == "PipeWire:Interface:Link"
                && o["info"]["output-node-id"]
                    .as_u64()
                    .is_some_and(|id| ours.contains(&id))
        })
        .filter_map(|o| name_of(&o["info"]["input-node-id"]))
        .collect()
}

fn engine_play(
    name: &str,
    args: &[&str],
    during: impl FnOnce(u32),
) -> (std::process::ExitStatus, Value) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let status = dir.join(format!("{name}-status.json"));
    let _ = std::fs::remove_file(&status);
    let log = std::fs::File::create(dir.join(format!("{name}-engine.log"))).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_bramley-play"))
        .args(["--output-device", SINK, "--no-loop", "--status"])
        .arg(&status)
        .args(args)
        .env("XDG_RUNTIME_DIR", runtime_dir())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0)
        .spawn()
        .unwrap();
    let mut engine = Group(child);
    during(engine.0.id());
    let finished = engine.0.wait().unwrap();
    (
        finished,
        serde_json::from_slice(&std::fs::read(&status).unwrap()).unwrap(),
    )
}

#[test]
#[ignore = "plays the Shaka Angel One Widevine stream through the real CDM into a PipeWire null sink"]
fn plays_widevine_hls_end_to_end_into_a_null_sink() {
    let _sink = null_sink();
    let before = default_sink();
    let mut targets = Vec::new();
    let (finished, s) = engine_play("itest", &["--max-seconds", "70"], |pid| {
        std::thread::sleep(Duration::from_secs(25));
        targets = linked_targets(pid);
        targets.dedup();
    });
    let media = s["medias"]
        .as_object()
        .unwrap()
        .values()
        .max_by(|a, b| {
            a["decodedDecryptedSeconds"]
                .as_f64()
                .partial_cmp(&b["decodedDecryptedSeconds"].as_f64())
                .unwrap()
        })
        .unwrap()
        .clone();
    assert!(finished.success(), "bramley-play exited with {finished}");
    assert_eq!(
        targets,
        [SINK],
        "our stream must be linked to the null sink only"
    );
    assert_eq!(default_sink(), before, "the default sink must not change");
    assert_eq!(
        s["platform"]["eme"]["usableSeen"], true,
        "no usable key: {}",
        s["app"]["errors"]
    );
    assert!(
        media["decodedDecryptedSeconds"].as_f64().unwrap() > 25.0,
        "{media}"
    );
    assert!(
        s["app"]["maxCurrentTime"].as_f64().unwrap() > 30.0,
        "{}",
        s["app"]
    );
    assert_eq!(
        (
            media["decodeErr"].as_u64(),
            media["decryptFail"].as_object().map(|m| m.len())
        ),
        (Some(0), Some(0)),
        "{media}"
    );
    assert_eq!(s["app"]["mediaEvents"]["ended"], 1, "{}", s["app"]);
    assert_eq!(
        (
            media["drainsAcked"].as_u64(),
            media["drainTimeouts"].as_u64()
        ),
        (Some(1), Some(0)),
        "ended must wait for PipeWire's drain ack: {media}"
    );
}

#[test]
#[ignore = "plays ten back-to-back loads of the Angel One stream through the real CDM into a PipeWire null sink"]
fn ten_back_to_back_tracks_hold_memory() {
    let _sink = null_sink();
    let args = [
        "--tracks",
        "10",
        "--track-seconds",
        "20",
        "--max-seconds",
        "400",
    ];
    let (finished, s) = engine_play("tracks", &args, |_| {});
    assert!(finished.success(), "bramley-play exited with {finished}");
    let tracks = s["tracks"].as_array().unwrap();
    let field = |i: usize, k: &str| tracks[i][k].as_f64().unwrap_or(0.0);
    let keys = |i: usize| tracks[i]["js"]["keyLoaded"].as_u64().unwrap();
    assert_eq!(tracks.len(), 10, "VOID: {}", s["app"]["errors"]);
    for i in 0..10 {
        let previous = if i == 0 {
            (0, 0.0)
        } else {
            (keys(i - 1), field(i - 1, "decodedDecryptedSeconds"))
        };
        assert!(keys(i) > previous.0, "VOID: track {} loaded no key", i + 1);
        let decrypted = field(i, "decodedDecryptedSeconds") - previous.1;
        assert!(
            decrypted >= 9.0,
            "VOID: track {} decrypted {decrypted:.1} s",
            i + 1
        );
    }
    for op in ["cdm_create", "ms_create", "sb_add"] {
        assert!(
            s["opCounts"][op].as_u64() >= Some(10),
            "VOID: {op} {}",
            s["opCounts"]
        );
    }
    let pss: Vec<f64> = (0..10).map(|i| field(i, "pssMiB")).collect();
    let live: Vec<&Value> = tracks.iter().map(|t| &t["live"]).collect();
    eprintln!("PSS MiB per track {pss:.1?}\nlive native objects {live:?}");
    assert!(
        pss[9] - pss[0] <= 8.0,
        "FAIL: PSS grew {:.1} MiB over ten tracks: {pss:.1?}",
        pss[9] - pss[0]
    );
}

#[test]
#[ignore = "plays Google's public Widevine cbcs Tears of Steel audio through the real CDM into a PipeWire null sink"]
fn plays_widevine_cbcs_audio_end_to_end_into_a_null_sink() {
    let _sink = null_sink();
    let playlist = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tears-audio-cbcs.m3u8"
    );
    let license = "https://proxy.uat.widevine.com/proxy?video_id=2015_tears&provider=widevine_test";
    let args = [
        "--manifest-file",
        playlist,
        "--lic",
        license,
        "--max-seconds",
        "40",
    ];
    let (finished, s) = engine_play("cbcs", &args, |_| {});
    assert!(finished.success(), "bramley-play exited with {finished}");
    let init = s["sourceBuffers"]
        .as_object()
        .unwrap()
        .values()
        .find_map(|sb| sb["init"]["protected"].as_object().cloned())
        .unwrap_or_else(|| panic!("no protected init segment: {}", s["app"]["errors"]));
    assert_eq!(init["scheme"], "cbcs", "{init:?}");
    let media = s["medias"].as_object().unwrap().values().next().unwrap();
    assert_eq!(
        s["platform"]["eme"]["usableSeen"], true,
        "{}",
        s["app"]["errors"]
    );
    assert!(media["encryptedSamples"].as_u64() > Some(1000), "{media}");
    assert!(
        media["decodedDecryptedSeconds"].as_f64() > Some(25.0),
        "{media}"
    );
    assert_eq!(
        (
            media["decodeErr"].as_u64(),
            media["decryptFail"].as_object().map(|m| m.len())
        ),
        (Some(0), Some(0)),
        "{media}"
    );
}
