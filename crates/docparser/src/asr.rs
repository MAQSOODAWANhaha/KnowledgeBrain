//! After convert: audio bytes → transcription (brain knowledge_process step 1.5).

use std::time::Duration;

use crate::ReadResult;

pub const ASR_NOT_CONFIGURED: &str = "ASR model is not configured for audio transcription";
pub const ASR_EMPTY: &str = "[No speech detected in audio file]";
pub const ASR_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Default)]
pub struct AsrSettings {
    pub enabled: bool,
    pub model_id: String,
    pub language: String,
    pub base_url: String,
    pub api_key: String,
}

impl AsrSettings {
    pub fn from_version(enabled: bool, model_id: &str) -> Self {
        Self {
            enabled,
            model_id: model_id.to_string(),
            language: std::env::var("KNOWLEDGEBRAIN_ASR_LANGUAGE").unwrap_or_default(),
            base_url: std::env::var("KNOWLEDGEBRAIN_ASR_BASE_URL").unwrap_or_default(),
            api_key: std::env::var("KNOWLEDGEBRAIN_ASR_API_KEY").unwrap_or_default(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.model_id.is_empty()
    }

    pub fn is_stub(&self) -> bool {
        self.model_id == "stub-asr"
    }
}

/// If not audio, returns `result` unchanged. Missing ASR config → `error` set (no retry).
/// HTTP/stub failure → `Err` (retryable).
pub async fn apply(
    mut result: ReadResult,
    file_name: &str,
    cfg: &AsrSettings,
) -> Result<ReadResult, String> {
    if !result.is_audio || result.audio_data.is_empty() {
        return Ok(result);
    }
    if !cfg.is_enabled() {
        result.error = ASR_NOT_CONFIGURED.into();
        return Ok(result);
    }
    let text = if cfg.is_stub() {
        stub_text(file_name, &result.audio_data)
    } else {
        transcribe_http(cfg, &result.audio_data, file_name).await?
    };
    let text = if text.trim().is_empty() {
        ASR_EMPTY.to_string()
    } else {
        text
    };
    result.markdown = text;
    result.is_audio = false;
    result.audio_data.clear();
    Ok(result)
}

pub fn apply_stub(mut result: ReadResult, file_name: &str) -> ReadResult {
    if result.is_audio {
        result.markdown = stub_text(file_name, &result.audio_data);
        result.is_audio = false;
        result.audio_data.clear();
    }
    result
}

fn stub_text(file_name: &str, audio: &[u8]) -> String {
    format!("[stub-asr:{file_name}:{}]", audio.len())
}

fn transcriptions_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/audio/transcriptions")
    } else if b.ends_with("/audio/transcriptions") {
        b.to_string()
    } else {
        format!("{b}/v1/audio/transcriptions")
    }
}

async fn transcribe_http(
    cfg: &AsrSettings,
    audio: &[u8],
    file_name: &str,
) -> Result<String, String> {
    if cfg.base_url.trim().is_empty() {
        return Err("failed to get ASR model: no KNOWLEDGEBRAIN_ASR_BASE_URL".into());
    }
    if audio.is_empty() {
        return Err("audio bytes are empty".into());
    }
    let name = if file_name.is_empty() {
        "audio.mp3"
    } else {
        file_name
    };
    let part = reqwest::multipart::Part::bytes(audio.to_vec())
        .file_name(name.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| e.to_string())?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", cfg.model_id.clone())
        .text("response_format", "json");
    if !cfg.language.is_empty() {
        form = form.text("language", cfg.language.clone());
    }
    let mut req = reqwest::Client::builder()
        .timeout(ASR_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?
        .post(transcriptions_url(&cfg.base_url))
        .multipart(form);
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("ASR transcription request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("ASR transcription request failed: {status} {body}"));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("ASR transcription request failed: {e}"))?;
    Ok(v.get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_sets_error_no_retry_signal() {
        let r = ReadResult {
            markdown: "[Audio file: a.mp3]".into(),
            is_audio: true,
            audio_data: vec![1, 2, 3],
            ..ReadResult::default()
        };
        let out = apply(r, "a.mp3", &AsrSettings::default()).await.unwrap();
        assert_eq!(out.error, ASR_NOT_CONFIGURED);
        assert!(out.is_audio);
    }

    #[tokio::test]
    async fn stub_writes_markdown() {
        let r = ReadResult {
            markdown: "[Audio file: a.wav]".into(),
            is_audio: true,
            audio_data: vec![9; 4],
            ..ReadResult::default()
        };
        let cfg = AsrSettings {
            enabled: true,
            model_id: "stub-asr".into(),
            ..AsrSettings::default()
        };
        let out = apply(r, "a.wav", &cfg).await.unwrap();
        assert!(!out.is_audio);
        assert!(out.audio_data.is_empty());
        assert_eq!(out.markdown, "[stub-asr:a.wav:4]");
        assert!(out.error.is_empty());
    }
}
