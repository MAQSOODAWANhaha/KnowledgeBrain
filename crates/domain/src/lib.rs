//! Domain types, parse_status machine, and post_process formula.

mod formula;
mod process;
mod status;
mod store;

pub use formula::*;
pub use process::*;
pub use status::*;
pub use store::*;

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

pub fn vlm_configured() -> bool {
    let vlm = std::env::var("KNOWLEDGEBRAIN_VLM_BASE_URL").unwrap_or_default();
    if !vlm.trim().is_empty() {
        return true;
    }
    !std::env::var("KNOWLEDGEBRAIN_CHAT_BASE_URL")
        .unwrap_or_default()
        .trim()
        .is_empty()
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
    #[test]
    fn workspace_members_match_spec() {
        let manifest = include_str!("../../../Cargo.toml");
        for member in [
            "crates/domain",
            "crates/auth",
            "crates/models",
            "crates/runtime",
            "crates/api",
            "crates/docparser",
            "crates/chunker",
            "crates/index",
            "crates/enrichment",
            "crates/graph",
            "crates/wiki",
            "crates/clone",
            "crates/search",
            "crates/obs",
            "crates/storage",
            "crates/worker",
        ] {
            assert!(
                manifest.contains(&format!("\"{member}\"")),
                "missing workspace member {member}"
            );
        }
        assert!(manifest.contains("edition = \"2024\""));
        assert!(manifest.contains("rust-version = \"1.97\""));
    }

    #[test]
    fn url_blocked_covers_private_and_loopback() {
        assert!(super::url_blocked("ftp://example.com"));
        assert!(super::url_blocked("http://localhost/x"));
        assert!(super::url_blocked("https://127.0.0.1/x"));
        assert!(super::url_blocked("http://10.1.2.3/a"));
        assert!(super::url_blocked("http://192.168.0.4/a"));
        assert!(super::url_blocked("http://172.31.0.1/a"));
        assert!(super::url_blocked("http://169.254.169.254/latest"));
        assert!(super::url_blocked("http://[::1]/"));
        assert!(super::url_blocked("http://svc.local/path"));
        assert!(!super::url_blocked("https://example.com/doc"));
        assert!(super::url_blocked("http://172.17.0.1/x"));
        assert!(super::url_blocked("http://169.254.1.1/x"));
        assert!(super::url_blocked("https://user:pass@127.0.0.1/secret"));
        assert!(super::url_blocked("http://[::ffff:127.0.0.1]/"));
    }
}
