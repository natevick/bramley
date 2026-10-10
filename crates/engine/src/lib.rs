mod audio;
mod buffer;
mod cdm;
mod eme;
mod media;
mod mp4;
mod net;
mod player;
mod runtime;

use std::path::PathBuf;

#[cfg(feature = "diagnostics")]
pub use player::Diagnostics;
pub use player::{Command, PlayState, Player, PlayerConfig, PlayerError, Queue, Snapshot, Tokens};
pub use runtime::Engine;

#[cfg(feature = "sabotage")]
#[used]
static SABOTAGE_MARKER: &str = "bramley-unshipped-feature:engine/sabotage\0";
#[cfg(feature = "diagnostics")]
#[used]
static DIAGNOSTICS_MARKER: &str = "bramley-unshipped-feature:engine/diagnostics\0";

#[used]
static LINKED_MARKER: &str = "bramley-linked:engine\0";

pub fn user_agent() -> String {
    format!(
        "Bramley/{} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    )
}

#[derive(Clone)]
pub struct EngineConfig {
    pub cdm_path: PathBuf,
    pub audio_sink_name: String,
    pub output_device: Option<String>,
    pub redact: Vec<String>,
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

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("cdm_path", &self.cdm_path)
            .field("audio_sink_name", &self.audio_sink_name)
            .field("output_device", &self.output_device)
            .finish_non_exhaustive()
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            cdm_path: Self::default_cdm_path(),
            audio_sink_name: "engine".into(),
            output_device: None,
            redact: Vec::new(),
        }
    }
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        let encoded = secret
            .replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D");
        mask_from_prefix(&mut out, &[secret, &encoded]);
    }
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

fn mask_from_prefix(text: &mut String, forms: &[&str]) {
    const MIN_PREFIX: usize = 16;
    for head in forms.iter().map(|f| f.get(..MIN_PREFIX).unwrap_or(f)) {
        let mut from = 0;
        while let Some(i) = text[from..].find(head).map(|i| i + from) {
            let len = forms
                .iter()
                .map(|f| {
                    text[i..]
                        .bytes()
                        .zip(f.bytes())
                        .take_while(|(a, b)| a == b)
                        .count()
                })
                .max()
                .unwrap_or(0);
            text.replace_range(i..i + len, "<redacted>");
            from = i + "<redacted>".len();
        }
    }
}

#[cfg(test)]
mod tests;
