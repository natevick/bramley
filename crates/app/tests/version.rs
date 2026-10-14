#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
fn version_flag_prints_the_crate_version_without_a_session() {
    let output = Command::new(env!("CARGO_BIN_EXE_bramley"))
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("bramley ", env!("CARGO_PKG_VERSION"), "\n")
    );
}
