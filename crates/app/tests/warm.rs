#![cfg(target_os = "linux")]

use std::process::Command;

use fastframe_instance::{Claim, Slot};

fn scratch(name: &str) -> std::path::PathBuf {
    let target =
        std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target")).unwrap();
    let dir = target.join(format!("warm-{name}{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn warm(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bramley"))
        .arg("warm")
        .args(args)
        .env_clear()
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("BRAMLEY_CONFIG_DIR", dir.join("config"))
        .env("HOME", dir)
        .output()
        .unwrap()
}

#[test]
fn bramley_warm_exits_zero_with_a_counts_only_line_and_touches_nothing_while_the_app_runs() {
    let dir = scratch("held");
    let slot = Slot::at(dir.join("run/bramley"), "bramley");
    let held = slot.claim("show", |_| Some("ok".into()));
    assert!(matches!(held, Claim::First(_)));
    let output = warm(&dir, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "bramley warm: requests=0 cached=0 skipped=0 stopped=app-running\n"
    );
    assert!(!dir.join("cache").exists() && !dir.join("config").exists());
}

#[test]
fn bramley_warm_takes_no_arguments_and_says_so() {
    let dir = scratch("usage");
    let output = warm(&dir, &["--now"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: bramley warm"));
}
