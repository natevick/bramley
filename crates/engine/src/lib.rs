mod audio;
mod buffer;
mod cdm;
mod eme;
mod media;
mod mp4;
mod net;
mod runtime;

use std::path::PathBuf;

pub use runtime::Engine;

pub fn user_agent() -> String {
    format!(
        "Bramley/{} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    )
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub cdm_path: PathBuf,
    pub audio_sink_name: String,
    pub output_device: Option<String>,
    pub redact: Vec<String>,
    pub truncate_scripts: bool,
}

impl EngineConfig {
    pub fn default_cdm_path() -> PathBuf {
        if cfg!(target_os = "linux") {
            PathBuf::from(
                "/opt/google/chrome/WidevineCdm/_platform_specific/linux_x64/libwidevinecdm.so",
            )
        } else {
            PathBuf::from(format!(
                "{}widevinecdm{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ))
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            cdm_path: Self::default_cdm_path(),
            audio_sink_name: "engine".into(),
            output_device: None,
            redact: Vec::new(),
            truncate_scripts: false,
        }
    }
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = secrets
        .iter()
        .filter(|s| !s.is_empty())
        .fold(text.to_string(), |t, s| t.replace(s.as_str(), "<redacted>"));
    let jwt_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    let mut from = 0;
    while let Some(i) = out[from..].find("eyJ").map(|i| i + from) {
        let end = out[i..].find(|c| !jwt_char(c)).map_or(out.len(), |e| i + e);
        if out[i..end].matches('.').count() >= 2 {
            out.replace_range(i..end, "<jwt>");
            from = i + 5;
        } else {
            from = end;
        }
    }
    out
}

#[cfg(test)]
mod tests;
