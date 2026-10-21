use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        other => other,
    }
    .to_owned();
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    let library = out.join("libbramley_musickit.a");
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-emit-library",
            "-static",
            "-parse-as-library",
            "-swift-version",
            "5",
            "-O",
        ])
        .args(["-module-name", "BramleyMusicKit", "-sdk", &sdk])
        .args(["-target", &format!("{arch}-apple-macos14.0")])
        .arg("-o")
        .arg(&library)
        .arg("swift/MusicKitPlayer.swift")
        .status()
        .expect("xcrun swiftc");
    assert!(status.success(), "swiftc failed");
    let toolchain = xcrun(&["--find", "swiftc"]);
    let toolchain = PathBuf::from(toolchain);
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=bramley_musickit");
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    println!(
        "cargo:rustc-link-search=native={}",
        toolchain
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("lib/swift/macosx")
            .display()
    );
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn xcrun(args: &[&str]) -> String {
    let out = Command::new("xcrun").args(args).output().expect("xcrun");
    assert!(out.status.success(), "xcrun {args:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
