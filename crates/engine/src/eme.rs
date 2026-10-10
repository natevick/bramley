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
                .filter(|t| matches!(t.as_str(), "cenc" | "keyids" | "webm"))
                .cloned()
                .collect()
        };
        if !init_types.is_empty() && supported_init.is_empty() {
            continue;
        }
        if c.get("distinctiveIdentifier").and_then(|x| x.as_str()) == Some("required") {
            continue;
        }
        let persistent_state =
            if c.get("persistentState").and_then(|x| x.as_str()) == Some("required") {
                "required"
            } else {
                "not-allowed"
            };
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
        let supported = |caps: &[J], type_ok: fn(&str) -> bool| -> Vec<J> {
            caps.iter()
                .filter(|cap| {
                    let ct = cap
                        .get("contentType")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    let rob = cap.get("robustness").and_then(|x| x.as_str()).unwrap_or("");
                    let scheme = cap.get("encryptionScheme").and_then(|x| x.as_str());
                    type_ok(ct)
                        && (rob.is_empty() || rob == "SW_SECURE_CRYPTO")
                        && matches!(scheme, None | Some("cenc") | Some("cbcs"))
                })
                .cloned()
                .collect()
        };
        let audio = supported(&audio_in, audio_type_supported);
        let video: Vec<J> = supported(&video_in, video_type_answer)
            .into_iter()
            .filter(|cap| {
                cap.get("robustness").and_then(|x| x.as_str()) == Some("SW_SECURE_CRYPTO")
            })
            .collect();
        if (!audio_in.is_empty() && audio.is_empty()) || (!video_in.is_empty() && video.is_empty())
        {
            continue;
        }
        if audio.is_empty() {
            continue;
        }
        return Some(json!({
            "label": c.get("label").cloned().unwrap_or(json!("")),
            "initDataTypes": supported_init,
            "audioCapabilities": audio,
            "videoCapabilities": video,
            "distinctiveIdentifier": "not-allowed",
            "persistentState": persistent_state,
            "sessionTypes": ["temporary"],
        }));
    }
    None
}

fn is_aac_lc(codec: &str) -> bool {
    codec == "mp4a.40.2" || codec == "mp4a.40"
}

pub fn audio_type_supported(ct: &str) -> bool {
    let (mime, codecs) = parse_type(ct);
    mime == "audio/mp4" && codecs.iter().all(|c| is_aac_lc(c))
}

fn video_type_answer(ct: &str) -> bool {
    let (mime, codecs) = parse_type(ct);
    mime == "video/mp4" && codecs == ["avc1.42e01e"]
}

pub fn can_play_type(ct: &str) -> &'static str {
    if audio_type_supported(ct) {
        "probably"
    } else if video_type_answer(ct) {
        "maybe"
    } else {
        ""
    }
}

fn parse_type(ct: &str) -> (String, Vec<String>) {
    let lower = ct.to_ascii_lowercase();
    let mut parts = lower.split(';');
    let mime = parts.next().unwrap_or("").trim().to_string();
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
    (mime, codecs)
}
