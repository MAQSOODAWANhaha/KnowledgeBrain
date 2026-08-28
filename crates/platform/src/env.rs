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

pub fn chat_base_url() -> String {
    first_env(&["KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL"])
}

pub fn chat_api_key() -> String {
    first_env(&["KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY"])
}

pub fn chat_model() -> String {
    first_env(&["KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL"])
}

pub fn embedding_base_url() -> String {
    first_env(&["KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", "EMBEDDING_BASE_URL"])
}

pub fn embedding_api_key() -> String {
    first_env(&["KNOWLEDGEBRAIN_EMBEDDING_API_KEY", "EMBEDDING_API_KEY"])
}

pub fn embedding_model() -> String {
    first_env(&["KNOWLEDGEBRAIN_EMBEDDING_MODEL", "EMBEDDING_MODEL"])
}

pub fn vlm_base_url() -> String {
    first_env(&["KNOWLEDGEBRAIN_VLM_BASE_URL"])
}

pub fn vlm_api_key() -> String {
    first_env(&["KNOWLEDGEBRAIN_VLM_API_KEY"])
}

pub fn vlm_model() -> String {
    first_env(&["KNOWLEDGEBRAIN_VLM_MODEL"])
}

pub fn vlm_configured() -> bool {
    vlm_endpoint_ready(&vlm_base_url(), &vlm_model())
}

/// Base URL plus a real vision model. `stub-vlm` is not configured.
pub fn vlm_endpoint_ready(base_url: &str, model: &str) -> bool {
    let model = model.trim();
    !base_url.trim().is_empty() && !model.is_empty() && model != "stub-vlm"
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
