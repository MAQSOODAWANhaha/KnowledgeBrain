use sha2::{Digest, Sha256};
use std::net::ToSocketAddrs;
use uuid::Uuid;

pub fn new_id() -> Uuid {
    Uuid::new_v4()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub const MAX_FILE_SIZE: usize = 50 * 1024 * 1024;

pub fn max_file_bytes() -> usize {
    std::env::var("MAX_FILE_SIZE_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .map(|n| n * 1024 * 1024)
        .unwrap_or(MAX_FILE_SIZE)
}

pub fn first_env(keys: &[&str]) -> String {
    for k in keys {
        if let Ok(v) = std::env::var(k) {
            let t = v.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    String::new()
}

/// Resolve aliases without exposing values in diagnostics. Secrets retain exact bytes.
fn setting(
    get: &impl Fn(&str) -> Option<String>,
    keys: &[&str],
    secret: bool,
) -> Result<String, String> {
    let mut selected: Option<String> = None;
    for key in keys {
        if let Some(raw) = get(key).filter(|v| !v.trim().is_empty()) {
            let value = if secret { raw } else { raw.trim().to_owned() };
            if selected.as_ref().is_some_and(|prior| prior != &value) {
                return Err(format!("conflicting configuration: {}", keys.join(" / ")));
            }
            selected = Some(value);
        }
    }
    Ok(selected.unwrap_or_default())
}

fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn model_setting(keys: &[&str], secret: bool) -> String {
    setting(&process_env, keys, secret).unwrap_or_else(|error| panic!("{error}"))
}

pub fn chat_base_url() -> String {
    model_setting(&["KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL"], false)
}
pub fn chat_api_key() -> String {
    model_setting(&["KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY"], true)
}
pub fn chat_model() -> String {
    model_setting(&["KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL"], false)
}

fn valid_model_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

pub fn openai_chat_ready(base_url: &str, api_key: &str, model: &str) -> bool {
    valid_model_url(base_url.trim())
        && !api_key.trim().is_empty()
        && !model.trim().is_empty()
        && model.trim() != "stub-chat"
}
pub fn openai_chat_configured() -> bool {
    openai_chat_ready(&chat_base_url(), &chat_api_key(), &chat_model())
}
pub fn require_openai_chat() {
    assert!(
        openai_chat_configured(),
        "Valid KNOWLEDGEBRAIN_CHAT_BASE_URL, KNOWLEDGEBRAIN_CHAT_API_KEY and KNOWLEDGEBRAIN_CHAT_MODEL are required"
    );
    chat_supports_images().unwrap_or_else(|error| panic!("{error}"));
    knowledge_enabled().unwrap_or_else(|error| panic!("{error}"));
}
pub fn embedding_base_url() -> String {
    model_setting(
        &["KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", "EMBEDDING_BASE_URL"],
        false,
    )
}
pub fn embedding_api_key() -> String {
    model_setting(
        &["KNOWLEDGEBRAIN_EMBEDDING_API_KEY", "EMBEDDING_API_KEY"],
        true,
    )
}
pub fn embedding_model() -> String {
    model_setting(
        &["KNOWLEDGEBRAIN_EMBEDDING_MODEL", "EMBEDDING_MODEL"],
        false,
    )
}

fn optional_bool(value: Option<String>, key: &str) -> Result<Option<bool>, String> {
    match value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}
pub fn knowledge_enabled() -> Result<bool, String> {
    Ok(optional_bool(
        process_env("KNOWLEDGEBRAIN_KNOWLEDGE_ENABLED"),
        "KNOWLEDGEBRAIN_KNOWLEDGE_ENABLED",
    )?
    .unwrap_or(false))
}

fn supports_images(get: &impl Fn(&str) -> Option<String>, model: &str) -> Result<bool, String> {
    if let Some(value) = optional_bool(
        get("KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES"),
        "KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES",
    )? {
        return Ok(value);
    }
    // Exact documented OpenAI vision models; no prefix or unknown-alias inference.
    // https://developers.openai.com/api/docs/models/gpt-6-luna (image input)
    // https://developers.openai.com/api/docs/models/gpt-4o
    // https://developers.openai.com/api/docs/models/gpt-4.1
    Ok(matches!(
        model,
        "gpt-6-luna"
            | "gpt-4o"
            | "gpt-4o-2024-05-13"
            | "gpt-4o-2024-08-06"
            | "gpt-4o-2024-11-20"
            | "gpt-4o-mini"
            | "gpt-4o-mini-2024-07-18"
            | "gpt-4.1"
            | "gpt-4.1-2025-04-14"
            | "gpt-4.1-mini"
            | "gpt-4.1-mini-2025-04-14"
            | "gpt-4.1-nano"
            | "gpt-4.1-nano-2025-04-14"
    ))
}
pub fn chat_supports_images() -> Result<bool, String> {
    supports_images(&process_env, &chat_model())
}

/// Deliberately not Debug/Serialize: this snapshot contains a credential.
#[derive(Clone)]
pub struct VisionModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Present only when OCR uses the configured Chat endpoint and model.
    pub chat_context_tokens: Option<usize>,
}

/// Match the actual OpenAI-compatible endpoint used by OCR transports.
/// URL normalization is for budget identity only, never credential inheritance.
pub fn model_chat_completions_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let endpoint = if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else if base.ends_with("/chat/completions") {
        base.to_owned()
    } else {
        format!("{base}/v1/chat/completions")
    };
    reqwest::Url::parse(&endpoint)
        .map(|url| url.to_string())
        .unwrap_or(endpoint)
}

fn resolve_vlm(get: &impl Fn(&str) -> Option<String>) -> Result<VisionModelConfig, String> {
    let chat_url = setting(
        get,
        &["KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL"],
        false,
    )?;
    let chat_key = setting(get, &["KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY"], true)?;
    let chat_model = setting(get, &["KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL"], false)?;
    let url = setting(get, &["KNOWLEDGEBRAIN_VLM_BASE_URL"], false)?;
    let key = setting(get, &["KNOWLEDGEBRAIN_VLM_API_KEY"], true)?;
    let model = setting(get, &["KNOWLEDGEBRAIN_VLM_MODEL"], false)?;
    let independent =
        !url.is_empty() && url.trim_end_matches('/') != chat_url.trim_end_matches('/');
    if independent && (key.is_empty() || model.is_empty()) {
        return Err("Independent KNOWLEDGEBRAIN_VLM_BASE_URL requires KNOWLEDGEBRAIN_VLM_API_KEY and KNOWLEDGEBRAIN_VLM_MODEL; Chat credentials are never forwarded".into());
    }
    let effective_vlm_url = if url.is_empty() { &chat_url } else { &url };
    let same_endpoint =
        model_chat_completions_url(effective_vlm_url) == model_chat_completions_url(&chat_url);
    let uses_chat_model = same_endpoint && (model.is_empty() || model == chat_model);
    if uses_chat_model && !supports_images(get, &chat_model)? {
        return Err("OCR inheritance requires confirmed Chat image support; configure KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES or an independent VLM model".into());
    }
    let chat_context_tokens = if uses_chat_model {
        let raw = setting(get, &["KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS"], false)?;
        Some(raw.parse::<usize>().ok().filter(|n| (1..=4_194_304).contains(n))
            .ok_or("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS must be an integer between 1 and 4194304 for Chat OCR")?)
    } else {
        None
    };
    let config = VisionModelConfig {
        base_url: if url.is_empty() { chat_url } else { url },
        api_key: if key.is_empty() { chat_key } else { key },
        model: if model.is_empty() { chat_model } else { model },
        chat_context_tokens,
    };
    if !vlm_endpoint_ready(&config.base_url, &config.model) || config.api_key.trim().is_empty() {
        return Err("Valid VLM base URL, model and API key are required for OCR".into());
    }
    Ok(config)
}
pub fn vlm_config() -> Result<VisionModelConfig, String> {
    resolve_vlm(&process_env)
}
pub fn vlm_base_url() -> String {
    vlm_config().map(|v| v.base_url).unwrap_or_default()
}
pub fn vlm_api_key() -> String {
    vlm_config().map(|v| v.api_key).unwrap_or_default()
}
pub fn vlm_model() -> String {
    vlm_config().map(|v| v.model).unwrap_or_default()
}
pub fn vlm_configured() -> bool {
    vlm_config().is_ok()
}
pub fn vlm_endpoint_ready(base_url: &str, model: &str) -> bool {
    valid_model_url(base_url.trim()) && !model.trim().is_empty() && model.trim() != "stub-vlm"
}

pub fn is_valid_file_type(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "pdf"
            | "txt"
            | "docx"
            | "doc"
            | "epub"
            | "mhtml"
            | "md"
            | "markdown"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "csv"
            | "xlsx"
            | "xls"
            | "pptx"
            | "ppt"
            | "json"
            | "mp3"
            | "wav"
            | "m4a"
            | "flac"
            | "ogg"
    )
}

pub fn is_video(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm")
}

pub fn is_image_type(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tiff" | "webp"
    )
}

pub fn is_audio_type(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "flac" | "ogg")
}

pub fn is_simple_format(file_type: &str) -> bool {
    let t = file_type.trim_start_matches('.').to_ascii_lowercase();
    matches!(
        t.as_str(),
        "md" | "markdown"
            | "txt"
            | "text"
            | "csv"
            | "json"
            | "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "bmp"
            | "tiff"
            | "webp"
            | "mp3"
            | "wav"
            | "m4a"
            | "flac"
            | "ogg"
    )
}

/// Host-based SSRF deny list (loopback, link-local, RFC1918, metadata).
/// Resolves hostnames so a public name that points at a private IP is blocked.
pub fn url_blocked(url: &str) -> bool {
    let Some(host) = extract_host(url) else {
        return true;
    };
    if host_blocked(&host) {
        return true;
    }
    if looks_like_ip(&host) {
        return false;
    }
    let Ok(addrs) = (host.as_str(), 80u16).to_socket_addrs() else {
        return false;
    };
    addrs.into_iter().any(|a| ip_blocked(a.ip()))
}

fn extract_host(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    let rest = url.split_once("://")?.1;
    let rest = match rest.rfind('@') {
        Some(i) => &rest[i + 1..],
        None => rest,
    };
    if let Some(inner) = rest.strip_prefix('[') {
        return Some(inner.split(']').next().unwrap_or("").to_ascii_lowercase());
    }
    Some(
        rest.split(['/', ':', '?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase(),
    )
}

fn looks_like_ip(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

fn host_blocked(host: &str) -> bool {
    if host.is_empty()
        || host == "localhost"
        || host == "0.0.0.0"
        || host == "::1"
        || host.ends_with(".local")
        || host.ends_with(".localhost")
    {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip_blocked(ip);
    }
    false
}

fn v4_blocked(v: std::net::Ipv4Addr) -> bool {
    v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified()
}

fn ip_blocked(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => v4_blocked(v),
        std::net::IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || v.is_unique_local()
                || v.is_unicast_link_local()
                || v.to_ipv4_mapped().is_some_and(v4_blocked)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_reject_conflicts_and_preserve_secret_bytes() {
        let get = |key: &str| match key {
            "main" => Some(" secret ".into()),
            "alias" => Some("other-secret".into()),
            _ => None,
        };
        assert_eq!(setting(&get, &["main"], true).unwrap(), " secret ");
        assert_eq!(setting(&get, &["main"], false).unwrap(), "secret");
        let error = setting(&get, &["main", "alias"], true).unwrap_err();
        assert!(!error.contains("secret"));
        assert!(error.contains("main / alias"));
    }

    #[test]
    fn vision_inherits_only_confirmed_chat_and_never_forwards_key_to_other_endpoint() {
        let mut values = std::collections::HashMap::from([
            ("KNOWLEDGEBRAIN_CHAT_BASE_URL", "https://chat.invalid/v1"),
            ("KNOWLEDGEBRAIN_CHAT_API_KEY", " chat-secret "),
            ("KNOWLEDGEBRAIN_CHAT_MODEL", "unknown-model"),
            ("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS", "8192"),
        ]);
        let resolve = |values: &std::collections::HashMap<&str, &str>| {
            resolve_vlm(&|key| values.get(key).map(|v| v.to_string()))
        };
        assert!(resolve(&values).is_err());
        values.insert("KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES", "true");
        let config = resolve(&values).unwrap();
        assert_eq!(config.api_key, " chat-secret ");
        assert_eq!(config.model, "unknown-model");
        assert_eq!(config.chat_context_tokens, Some(8192));
        values.insert("KNOWLEDGEBRAIN_VLM_BASE_URL", "https://other.invalid/v1");
        values.insert("KNOWLEDGEBRAIN_VLM_MODEL", "vision-model");
        assert!(resolve(&values).is_err());
        values.insert("KNOWLEDGEBRAIN_VLM_API_KEY", "other-secret");
        assert_eq!(resolve(&values).unwrap().api_key, "other-secret");
        assert_eq!(resolve(&values).unwrap().chat_context_tokens, None);
    }

    #[test]
    fn explicit_same_endpoint_model_reuses_key_but_partial_foreign_config_is_rejected() {
        let mut values = std::collections::HashMap::from([
            ("KNOWLEDGEBRAIN_CHAT_BASE_URL", "https://chat.invalid/v1"),
            ("KNOWLEDGEBRAIN_CHAT_API_KEY", "chat-secret"),
            ("KNOWLEDGEBRAIN_CHAT_MODEL", "text-model"),
            ("KNOWLEDGEBRAIN_VLM_BASE_URL", "https://chat.invalid/v1/"),
            ("KNOWLEDGEBRAIN_VLM_MODEL", "explicit-vision-model"),
        ]);
        let resolve = |values: &std::collections::HashMap<&str, &str>| {
            resolve_vlm(&|key| values.get(key).map(|v| v.to_string()))
        };
        assert_eq!(resolve(&values).unwrap().api_key, "chat-secret");
        values.insert(
            "KNOWLEDGEBRAIN_VLM_BASE_URL",
            "https://chat.invalid/other-tenant",
        );
        assert!(resolve(&values).is_err());
        values.insert("KNOWLEDGEBRAIN_VLM_API_KEY", " ");
        assert!(resolve(&values).is_err());
        values.insert(
            "KNOWLEDGEBRAIN_VLM_BASE_URL",
            "https://user:password@other.invalid/v1",
        );
        values.insert("KNOWLEDGEBRAIN_VLM_API_KEY", "own-secret");
        assert!(resolve(&values).is_err());
    }

    #[test]
    fn same_chat_model_ocr_requires_valid_context_even_when_explicit() {
        let mut values = std::collections::HashMap::from([
            ("KNOWLEDGEBRAIN_CHAT_BASE_URL", "https://chat.invalid/v1"),
            ("KNOWLEDGEBRAIN_CHAT_API_KEY", "synthetic-key"),
            ("KNOWLEDGEBRAIN_CHAT_MODEL", "gpt-4o"),
            ("KNOWLEDGEBRAIN_VLM_MODEL", "gpt-4o"),
        ]);
        let resolve = |values: &std::collections::HashMap<&str, &str>| {
            resolve_vlm(&|key| values.get(key).map(|value| value.to_string()))
        };
        assert!(resolve(&values).is_err());
        for invalid in ["0", "invalid", "4194305"] {
            values.insert("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS", invalid);
            assert!(resolve(&values).is_err());
        }
        values.insert("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS", "8192");
        assert_eq!(resolve(&values).unwrap().chat_context_tokens, Some(8192));
    }

    #[test]
    fn equivalent_ocr_endpoint_inherits_budget_without_inheriting_credentials() {
        let mut values = std::collections::HashMap::from([
            ("KNOWLEDGEBRAIN_CHAT_BASE_URL", "https://chat.invalid/v1"),
            ("KNOWLEDGEBRAIN_CHAT_API_KEY", "synthetic-chat-key"),
            ("KNOWLEDGEBRAIN_CHAT_MODEL", "gpt-4o"),
            ("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS", "8192"),
            (
                "KNOWLEDGEBRAIN_VLM_BASE_URL",
                "https://chat.invalid/v1/chat/completions",
            ),
            ("KNOWLEDGEBRAIN_VLM_MODEL", "gpt-4o"),
        ]);
        let resolve = |values: &std::collections::HashMap<&str, &str>| {
            resolve_vlm(&|key| values.get(key).map(|value| value.to_string()))
        };
        assert!(resolve(&values).is_err());
        values.insert("KNOWLEDGEBRAIN_VLM_API_KEY", "synthetic-explicit-key");
        for endpoint in [
            "https://chat.invalid/v1/chat/completions",
            "https://chat.invalid",
            "https://CHAT.invalid:443/v1/",
        ] {
            values.insert("KNOWLEDGEBRAIN_VLM_BASE_URL", endpoint);
            let config = resolve(&values).unwrap();
            assert_eq!(config.chat_context_tokens, Some(8192));
            assert_eq!(config.api_key, "synthetic-explicit-key");
        }
        values.insert(
            "KNOWLEDGEBRAIN_VLM_BASE_URL",
            "https://chat.invalid/other-tenant/v1",
        );
        assert_eq!(resolve(&values).unwrap().chat_context_tokens, None);
        values.insert(
            "KNOWLEDGEBRAIN_VLM_BASE_URL",
            "https://chat.invalid/v1/chat/completions",
        );
        values.insert("KNOWLEDGEBRAIN_VLM_MODEL", "independent-model");
        assert_eq!(resolve(&values).unwrap().chat_context_tokens, None);
    }

    #[test]
    fn explicit_chat_ocr_model_cannot_bypass_disabled_image_capability() {
        let mut values = std::collections::HashMap::from([
            ("KNOWLEDGEBRAIN_CHAT_BASE_URL", "https://chat.invalid/v1"),
            ("KNOWLEDGEBRAIN_CHAT_API_KEY", "synthetic-chat-key"),
            ("KNOWLEDGEBRAIN_CHAT_MODEL", "gpt-4o"),
            ("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS", "8192"),
            ("KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES", "false"),
            ("KNOWLEDGEBRAIN_VLM_API_KEY", "synthetic-vision-key"),
            ("KNOWLEDGEBRAIN_VLM_MODEL", "gpt-4o"),
        ]);
        let resolve = |values: &std::collections::HashMap<&str, &str>| {
            resolve_vlm(&|key| values.get(key).map(|value| value.to_string()))
        };
        assert!(resolve(&values).is_err());
        values.insert(
            "KNOWLEDGEBRAIN_VLM_BASE_URL",
            "https://chat.invalid/v1/chat/completions",
        );
        assert!(resolve(&values).is_err());
        values.insert("KNOWLEDGEBRAIN_VLM_MODEL", "independent-vision-model");
        assert!(resolve(&values).is_ok());
        values.insert("KNOWLEDGEBRAIN_VLM_MODEL", "gpt-4o");
        values.insert(
            "KNOWLEDGEBRAIN_VLM_BASE_URL",
            "https://independent.invalid/v1",
        );
        assert!(resolve(&values).is_ok());
    }

    #[test]
    fn image_capability_exact_mapping_and_explicit_override() {
        assert!(supports_images(&|_| None, "gpt-4o").unwrap());
        assert!(!supports_images(&|_| None, "gpt-4o-arbitrary").unwrap());
        assert!(supports_images(&|_| None, "gpt-6-luna").unwrap());
        assert!(!supports_images(&|_| None, "unverified-model").unwrap());
        assert!(!supports_images(&|_| Some("false".into()), "gpt-4o").unwrap());
        assert!(supports_images(&|_| Some("invalid".into()), "gpt-4o").is_err());
    }

    #[test]
    fn openai_chat_requires_url_key_and_real_model() {
        assert!(!openai_chat_ready("", "k", "gpt-4"));
        assert!(!openai_chat_ready("https://example/v1", "", "gpt-4"));
        assert!(!openai_chat_ready("https://example/v1", "k", ""));
        assert!(!openai_chat_ready("https://example/v1", "k", "stub-chat"));
        assert!(openai_chat_ready("https://example/v1", "k", "gpt-4"));
    }
}
