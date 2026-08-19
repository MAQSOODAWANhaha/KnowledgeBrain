//! Convert-layer engine catalog. Brain `ListAllEngines` + remote ListEngines.

use std::collections::HashMap;

use crate::http_engine::{mineru_endpoint, paddle_endpoint};
use crate::{anydoc, grpc};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EngineInfo {
    pub name: String,
    pub description: String,
    pub file_types: Vec<String>,
    pub available: bool,
    pub unavailable_reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineCatalog {
    pub data: Vec<EngineInfo>,
    pub docreader_addr: String,
    pub docreader_transport: String,
    pub connected: bool,
}

pub async fn list_all_engines(overrides: &HashMap<String, String>) -> EngineCatalog {
    let addr = grpc::reader_addr().unwrap_or_default();
    let (connected, remote) = if addr.is_empty() {
        (false, Vec::new())
    } else {
        match grpc::list_engines(overrides).await {
            Ok(engines) => (true, engines),
            Err(_) => (false, Vec::new()),
        }
    };
    EngineCatalog {
        data: merge_engines(local_engines(connected, overrides), remote),
        docreader_addr: addr,
        docreader_transport: "grpc".into(),
        connected,
    }
}

const MINERU_TYPES: &[&str] = &[
    "pdf", "jpg", "jpeg", "png", "bmp", "tiff", "doc", "docx", "ppt", "pptx",
];
const PADDLE_TYPES: &[&str] = &["pdf", "jpg", "jpeg", "png", "bmp", "tiff"];

struct HttpEngineSpec {
    name: &'static str,
    description: &'static str,
    file_types: &'static [&'static str],
    override_key: &'static str,
    fallback: fn() -> String,
    missing: &'static str,
}

const HTTP_ENGINES: &[HttpEngineSpec] = &[
    HttpEngineSpec {
        name: "mineru",
        description: "MinerU self-hosted service",
        file_types: MINERU_TYPES,
        override_key: "mineru_endpoint",
        fallback: mineru_endpoint,
        missing: "MinerU service not configured",
    },
    HttpEngineSpec {
        name: "mineru_cloud",
        description: "MinerU Cloud API",
        file_types: MINERU_TYPES,
        override_key: "mineru_api_key",
        fallback: mineru_api_key,
        missing: "MinerU API Key not configured",
    },
    HttpEngineSpec {
        name: "paddleocr_vl",
        description: "PaddleOCR-VL self-hosted service",
        file_types: PADDLE_TYPES,
        override_key: "paddleocr_vl_endpoint",
        fallback: paddle_endpoint,
        missing: "PaddleOCR-VL service not configured",
    },
    HttpEngineSpec {
        name: "paddleocr_vl_cloud",
        description: "PaddleOCR-VL Cloud API",
        file_types: PADDLE_TYPES,
        override_key: "paddleocr_vl_cloud_token",
        fallback: paddleocr_vl_cloud_token,
        missing: "PaddleOCR-VL Cloud Token not configured",
    },
];

#[derive(Clone, Copy)]
struct Availability {
    available: bool,
    reason: &'static str,
}

pub fn local_engines(
    docreader_connected: bool,
    overrides: &HashMap<String, String>,
) -> Vec<EngineInfo> {
    let builtin = if docreader_connected {
        Availability {
            available: true,
            reason: "",
        }
    } else {
        Availability {
            available: false,
            reason: "DocReader service not connected",
        }
    };
    let always = Availability {
        available: true,
        reason: "",
    };
    let mut out = vec![
        engine(
            "builtin",
            "DocReader built-in parser engine",
            &[
                "docx", "doc", "pdf", "md", "markdown", "xlsx", "xls", "epub", "html", "htm",
                "mhtml", "jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp", "mp3", "wav", "m4a",
                "flac", "ogg",
            ],
            builtin,
        ),
        engine(
            "simple",
            "Simple format & image parsing (no external service required)",
            &[
                "md", "markdown", "txt", "csv", "json", "jpg", "jpeg", "png", "gif", "bmp", "tiff",
                "webp", "mp3", "wav", "m4a", "flac", "ogg",
            ],
            always,
        ),
        engine(
            "anydoc",
            "anydoc in-process office document converter (no external service required)",
            anydoc::supported_file_types(),
            always,
        ),
    ];
    out.extend(HTTP_ENGINES.iter().map(|spec| {
        engine(
            spec.name,
            spec.description,
            spec.file_types,
            configured(
                first_nonempty(&[
                    override_value(overrides, spec.override_key),
                    (spec.fallback)(),
                ]),
                spec.missing,
            ),
        )
    }));
    out
}

/// Local engines first. Matching remote names take remote file_types /
/// description. Remote-only engines are appended.
pub fn merge_engines(local: Vec<EngineInfo>, remote: Vec<EngineInfo>) -> Vec<EngineInfo> {
    let remote_map: HashMap<String, EngineInfo> = remote
        .iter()
        .cloned()
        .map(|e| (e.name.clone(), e))
        .collect();
    let mut seen = HashMap::new();
    let mut out = Vec::with_capacity(local.len() + remote.len());
    for mut engine in local {
        seen.insert(engine.name.clone(), true);
        if let Some(re) = remote_map.get(&engine.name) {
            if !re.file_types.is_empty() {
                engine.file_types = re.file_types.clone();
            }
            if !re.description.is_empty() {
                engine.description = re.description.clone();
            }
        }
        out.push(engine);
    }
    for engine in remote {
        if seen.contains_key(&engine.name) {
            continue;
        }
        out.push(engine);
    }
    out
}

fn engine(name: &str, description: &str, file_types: &[&str], avail: Availability) -> EngineInfo {
    EngineInfo {
        name: name.into(),
        description: description.into(),
        file_types: file_types.iter().map(|s| (*s).to_string()).collect(),
        available: avail.available,
        unavailable_reason: avail.reason.into(),
    }
}

fn configured(value: String, missing: &'static str) -> Availability {
    if value.trim().is_empty() {
        Availability {
            available: false,
            reason: missing,
        }
    } else {
        Availability {
            available: true,
            reason: "",
        }
    }
}

fn mineru_api_key() -> String {
    env_value("MINERU_API_KEY")
}

fn paddleocr_vl_cloud_token() -> String {
    env_value("PADDLEOCR_VL_CLOUD_TOKEN")
}

fn override_value(overrides: &HashMap<String, String>, key: &str) -> String {
    overrides.get(key).cloned().unwrap_or_default()
}

fn env_value(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

fn first_nonempty(values: &[String]) -> String {
    values
        .iter()
        .find(|v| !v.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_catalog_includes_anydoc_and_simple() {
        let engines = local_engines(false, &HashMap::new());
        let names: Vec<&str> = engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "builtin",
                "simple",
                "anydoc",
                "mineru",
                "mineru_cloud",
                "paddleocr_vl",
                "paddleocr_vl_cloud"
            ]
        );
        let anydoc = engines.iter().find(|e| e.name == "anydoc").unwrap();
        assert!(anydoc.available);
        assert!(anydoc.file_types.contains(&"docx".into()));
        assert!(anydoc.file_types.contains(&"xlsx".into()));
        let simple = engines.iter().find(|e| e.name == "simple").unwrap();
        assert!(simple.available);
        let builtin = engines.iter().find(|e| e.name == "builtin").unwrap();
        assert!(!builtin.available);
        assert!(builtin.unavailable_reason.contains("not connected"));
    }

    #[test]
    fn merge_prefers_remote_types_and_appends_unknown() {
        let local = local_engines(true, &HashMap::new());
        let remote = vec![
            EngineInfo {
                name: "builtin".into(),
                description: "内置解析引擎".into(),
                file_types: vec!["pdf".into(), "docx".into()],
                available: true,
                unavailable_reason: String::new(),
            },
            EngineInfo {
                name: "markitdown".into(),
                description: "MarkItDown".into(),
                file_types: vec!["pptx".into()],
                available: true,
                unavailable_reason: String::new(),
            },
        ];
        let merged = merge_engines(local, remote);
        let builtin = merged.iter().find(|e| e.name == "builtin").unwrap();
        assert_eq!(builtin.description, "内置解析引擎");
        assert_eq!(builtin.file_types, vec!["pdf", "docx"]);
        assert!(merged.iter().any(|e| e.name == "markitdown"));
        assert!(merged.iter().any(|e| e.name == "anydoc"));
    }

    #[test]
    fn overrides_mark_http_engines_available() {
        let mut ov = HashMap::new();
        ov.insert("mineru_endpoint".into(), "http://mineru".into());
        ov.insert("paddleocr_vl_endpoint".into(), "http://paddle".into());
        let engines = local_engines(true, &ov);
        assert!(
            engines
                .iter()
                .find(|e| e.name == "mineru")
                .unwrap()
                .available
        );
        assert!(
            engines
                .iter()
                .find(|e| e.name == "paddleocr_vl")
                .unwrap()
                .available
        );
        assert!(
            !engines
                .iter()
                .find(|e| e.name == "mineru_cloud")
                .unwrap()
                .available
        );
    }
}
