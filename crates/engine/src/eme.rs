use serde_json::{Value as J, json};

pub fn eme_check(key_system: &str, configs: &str) -> Option<J> {
    if key_system != "com.widevine.alpha" {
        return None;
    }
    let configs: Vec<J> = serde_json::from_str(configs).ok()?;
    for c in configs {
        let init_types: Vec<String> = c
            .get("initDataTypes")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let supported_init: Vec<String> = if init_types.is_empty() {
            Vec::new()
        } else {
            init_types
                .iter()
                .filter(|t| *t == "cenc" || *t == "webm")
                .cloned()
                .collect()
        };
        if !init_types.is_empty() && supported_init.is_empty() {
            continue;
        }
        if c.get("distinctiveIdentifier").and_then(|x| x.as_str()) == Some("required")
            || c.get("persistentState").and_then(|x| x.as_str()) == Some("required")
        {
            continue;
        }
        let session_types: Vec<String> = c
            .get("sessionTypes")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["temporary".into()]);
        if session_types.iter().any(|t| t != "temporary") {
            continue;
        }
        let audio_in = c
            .get("audioCapabilities")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let video_in = c
            .get("videoCapabilities")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let audio: Vec<J> = audio_in
            .iter()
            .filter(|cap| {
                let ct = cap
                    .get("contentType")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let rob = cap.get("robustness").and_then(|x| x.as_str()).unwrap_or("");
                let scheme = cap.get("encryptionScheme").and_then(|x| x.as_str());
                audio_type_supported(ct)
                    && (rob.is_empty() || rob == "SW_SECURE_CRYPTO")
                    && matches!(scheme, None | Some("cenc") | Some("cbcs"))
            })
            .cloned()
            .collect();
        if !audio_in.is_empty() && audio.is_empty() {
            continue;
        }
        if !video_in.is_empty() {
            continue;
        }
        if audio.is_empty() {
            continue;
        }
        return Some(json!({
            "label": c.get("label").cloned().unwrap_or(json!("")),
            "initDataTypes": supported_init,
            "audioCapabilities": audio,
            "videoCapabilities": [],
            "distinctiveIdentifier": "not-allowed",
            "persistentState": "not-allowed",
            "sessionTypes": ["temporary"],
        }));
    }
    None
}

pub fn audio_type_supported(ct: &str) -> bool {
    let lower = ct.to_ascii_lowercase();
    let mut parts = lower.split(';');
    let mime = parts.next().unwrap_or("").trim();
    if mime != "audio/mp4" {
        return false;
    }
    let mut codecs: Vec<String> = Vec::new();
    for p in parts {
        let p = p.trim();
        if let Some(v) = p.strip_prefix("codecs=") {
            codecs = v
                .trim_matches('"')
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }
    }
    codecs
        .iter()
        .all(|c| c == "mp4a.40.2" || c == "mp4a.40.5" || c == "mp4a.40.29" || c == "mp4a.40")
}
