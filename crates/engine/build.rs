use std::path::PathBuf;

fn main() {
    detect_apple_tokens();
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=cdm-api/content_decryption_module.h");
    if std::env::var("CARGO_CFG_TARGET_FAMILY").as_deref() != Ok("unix") {
        return;
    }
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("src/shim.cc")
        .warnings(true)
        .compile("cdmshim");
    println!("cargo:rustc-link-lib=dl");
}

fn detect_apple_tokens() {
    println!("cargo::rustc-check-cfg=cfg(bramley_tokens)");
    println!("cargo:rerun-if-env-changed=BRAMLEY_CONFIG_DIR");
    println!("cargo:rerun-if-env-changed=HOME");
    let dir = std::env::var_os("BRAMLEY_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/bramley")));
    let Some(dir) = dir else { return };
    let watched = if dir.exists() {
        dir.as_path()
    } else {
        dir.parent().unwrap_or(&dir)
    };
    println!("cargo:rerun-if-changed={}", watched.display());
    if ["developer-token", "music-user-token"]
        .iter()
        .all(|f| dir.join(f).is_file())
    {
        println!("cargo:rustc-cfg=bramley_tokens");
    } else {
        println!(
            "cargo:warning=SKIPPED: the Apple Music end-to-end test is not built; no token files in {}",
            dir.display()
        );
    }
}
