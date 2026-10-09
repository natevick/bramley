#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const SINK: &str = "engine-itest-null";

fn runtime_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap().trim()))
}

fn tool(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd).args(args).env("XDG_RUNTIME_DIR", runtime_dir()).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Group(Child);

impl Drop for Group {
    fn drop(&mut self) {
        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{}", self.0.id())]).status();
        let _ = self.0.wait();
    }
}

fn null_sink() -> Group {
    let node = format!(
        "create-node adapter {{ factory.name=support.null-audio-sink node.name={SINK} node.description={SINK} media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }}"
    );
    let child = Command::new("bash").args(["-c", &format!("{{ echo '{node}'; exec sleep 100000; }} | pw-cli >/dev/null 2>&1")]).env("XDG_RUNTIME_DIR", runtime_dir()).process_group(0).spawn().unwrap();
    let sink = Group(child);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !tool("wpctl", &["status"]).contains(SINK) {
        assert!(Instant::now() < deadline, "null sink {SINK} never appeared; refusing to play audio");
        std::thread::sleep(Duration::from_millis(100));
    }
    sink
}

fn default_sink() -> String {
    tool("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"]).lines().find(|l| l.contains("node.name")).unwrap_or_default().to_string()
}

fn linked_targets(pid: u32) -> Vec<String> {
    let dump: Vec<Value> = serde_json::from_str(&tool("pw-dump", &[])).unwrap();
    let props = |o: &Value| o["info"]["props"].clone();
    let ours: Vec<u64> = dump
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Node" && props(o)["application.process.id"].to_string().trim_matches('"') == pid.to_string())
        .filter_map(|o| o["id"].as_u64())
        .collect();
    let name_of = |id: &Value| dump.iter().find(|o| o["id"] == *id).and_then(|o| props(o)["node.name"].as_str().map(String::from));
    dump.iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Link" && o["info"]["output-node-id"].as_u64().is_some_and(|id| ours.contains(&id)))
        .filter_map(|o| name_of(&o["info"]["input-node-id"]))
        .collect()
}

#[test]
#[ignore = "plays the Shaka Angel One Widevine stream through the real CDM into a PipeWire null sink"]
fn plays_widevine_hls_end_to_end_into_a_null_sink() {
    let _sink = null_sink();
    let before = default_sink();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let status = dir.join("itest-status.json");
    let _ = std::fs::remove_file(&status);
    let log = std::fs::File::create(dir.join("itest-engine.log")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_engine-play"))
        .args(["--output-device", SINK, "--no-loop", "--max-seconds", "45", "--status"])
        .arg(&status)
        .env("XDG_RUNTIME_DIR", runtime_dir())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0)
        .spawn()
        .unwrap();
    let mut engine = Group(child);
    std::thread::sleep(Duration::from_secs(25));
    let mut targets = linked_targets(engine.0.id());
    targets.dedup();
    let finished = engine.0.wait().unwrap();
    let s: Value = serde_json::from_slice(&std::fs::read(&status).unwrap()).unwrap();
    let media = s["medias"].as_object().unwrap().values().max_by(|a, b| a["decodedDecryptedSeconds"].as_f64().partial_cmp(&b["decodedDecryptedSeconds"].as_f64()).unwrap()).unwrap().clone();
    assert!(finished.success(), "engine-play exited with {finished}");
    assert_eq!(targets, [SINK], "our stream must be linked to the null sink only");
    assert_eq!(default_sink(), before, "the default sink must not change");
    assert_eq!(s["platform"]["eme"]["usableSeen"], true, "no usable key: {}", s["app"]["errors"]);
    assert!(media["decodedDecryptedSeconds"].as_f64().unwrap() > 25.0, "{media}");
    assert!(s["app"]["maxCurrentTime"].as_f64().unwrap() > 30.0, "{}", s["app"]);
    assert_eq!((media["decodeErr"].as_u64(), media["decryptFail"].as_object().map(|m| m.len())), (Some(0), Some(0)), "{media}");
}
