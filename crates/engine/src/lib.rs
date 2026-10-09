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

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub cdm_path: PathBuf,
    pub audio_sink_name: String,
    pub output_device: Option<String>,
}

impl EngineConfig {
    pub fn default_cdm_path() -> PathBuf {
        if cfg!(target_os = "linux") {
            PathBuf::from("/opt/google/chrome/WidevineCdm/_platform_specific/linux_x64/libwidevinecdm.so")
        } else {
            PathBuf::from(format!("{}widevinecdm{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX))
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig { cdm_path: Self::default_cdm_path(), audio_sink_name: "engine".into(), output_device: None }
    }
}

#[cfg(test)]
mod tests;
