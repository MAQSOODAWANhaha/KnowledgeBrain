//! Convert-layer engine catalog. Brain `ListAllEngines` + remote ListEngines.

use std::collections::HashMap;

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
}

const HTTP_ENGINES: &[HttpEngineSpec] = &[
    HttpEngineSpec {
        name: "mineru",
        description: "MinerU self-hosted service",
        file_types: MINERU_TYPES,
    },
    HttpEngineSpec {
        name: "mineru_cloud",
        description: "MinerU Cloud API",
        file_types: MINERU_TYPES,
    },
    HttpEngineSpec {
        name: "paddleocr_vl",
        description: "PaddleOCR-VL self-hosted service",
        file_types: PADDLE_TYPES,
    },
    HttpEngineSpec {
        name: "paddleocr_vl_cloud",
        description: "PaddleOCR-VL Cloud API",
        file_types: PADDLE_TYPES,
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
        let resolved = resolve_effective_engine_config(spec.name, overrides);
        EngineInfo {
            name: spec.name.into(),
            description: spec.description.into(),
            file_types: spec.file_types.iter().map(|kind| (*kind).into()).collect(),
            available: resolved.is_ok(),
            unavailable_reason: resolved.err().unwrap_or_default(),
        }
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

/// The same immutable resolution is used by availability and the HTTP sender.
/// Do not derive Debug/Serialize: bearer credentials must never enter logs.
pub struct EffectiveEngineConfig {
    pub engine: String,
    pub endpoint: String,
    pub bearer_token: Option<String>,
}

pub fn resolve_effective_engine_config(
    engine: &str,
    overrides: &HashMap<String, String>,
) -> Result<EffectiveEngineConfig, String> {
    resolve_with_env(engine, overrides, |key| std::env::var(key).ok())
}

fn resolve_with_env(
    engine: &str,
    overrides: &HashMap<String, String>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<EffectiveEngineConfig, String> {
    let (endpoint_key, endpoint_env, token_key, token_env) = match engine {
        "mineru" => (
            "mineru_endpoint",
            ["KNOWLEDGEBRAIN_MINERU_ENDPOINT", "MINERU_ENDPOINT"],
            "mineru_api_key",
            ["KNOWLEDGEBRAIN_MINERU_API_KEY", "MINERU_API_KEY"],
        ),
        "paddleocr_vl" => (
            "paddleocr_vl_endpoint",
            ["KNOWLEDGEBRAIN_PADDLE_ENDPOINT", "PADDLEOCR_VL_ENDPOINT"],
            "paddleocr_vl_token",
            ["KNOWLEDGEBRAIN_PADDLE_TOKEN", "PADDLEOCR_VL_TOKEN"],
        ),
        "mineru_cloud" | "paddleocr_vl_cloud" => {
            return Err(format!(
                "unsupported parser engine: {engine}; cloud protocol is not implemented"
            ));
        }
        _ => return Err("unsupported HTTP parser engine".into()),
    };
    let value = |key: &str, fallback: [&str; 2]| -> Option<String> {
        // An explicit empty override disables a global setting, rather than
        // silently routing data to a different configured destination.
        overrides
            .get(key)
            .cloned()
            .or_else(|| {
                fallback
                    .into_iter()
                    .find_map(|key| env(key).filter(|v| !v.trim().is_empty()))
            })
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let endpoint = value(endpoint_key, endpoint_env)
        .ok_or_else(|| format!("{engine} service not configured"))?;
    let url = reqwest::Url::parse(&endpoint).map_err(|_| "invalid parser endpoint".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "invalid parser endpoint: use HTTP(S) without credentials, query or fragment".into(),
        );
    }
    let bearer_token = value(token_key, token_env);
    if bearer_token.as_ref().is_some_and(|token| {
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).is_err()
    }) {
        return Err("invalid parser authentication token".into());
    }
    Ok(EffectiveEngineConfig {
        engine: engine.into(),
        endpoint,
        bearer_token,
    })
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
    #[test]
    fn effective_config_prefers_overrides_and_never_aliases_cloud() {
        let overrides = HashMap::from([
            (
                "mineru_endpoint".into(),
                "http://override.example/base".into(),
            ),
            ("mineru_api_key".into(), "override-token".into()),
        ]);
        let config = resolve_with_env("mineru", &overrides, |key| {
            Some(if key.contains("ENDPOINT") {
                "http://global.example".into()
            } else {
                "global-token".into()
            })
        })
        .unwrap();
        assert_eq!(config.endpoint, "http://override.example/base");
        assert_eq!(config.bearer_token.as_deref(), Some("override-token"));
        let cloud = HashMap::from([
            ("mineru_api_key".into(), "cloud-secret".into()),
            ("paddleocr_vl_cloud_token".into(), "cloud-secret".into()),
        ]);
        for engine in ["mineru_cloud", "paddleocr_vl_cloud"] {
            let error = resolve_with_env(engine, &cloud, |_| Some("configured".into()))
                .err()
                .unwrap();
            assert!(error.contains("unsupported"));
            assert!(!error.contains("cloud-secret"));
            assert!(
                !local_engines(true, &cloud)
                    .iter()
                    .find(|item| item.name == engine)
                    .unwrap()
                    .available
            );
        }
    }

    #[test]
    fn explicit_empty_endpoint_disables_global_fallback_and_invalid_secrets_are_redacted() {
        let overrides = HashMap::from([("mineru_endpoint".into(), " ".into())]);
        assert!(
            resolve_with_env("mineru", &overrides, |_| Some(
                "https://global.example".into()
            ))
            .is_err()
        );
        let overrides = HashMap::from([(
            "mineru_endpoint".into(),
            "https://user:secret@host.example".into(),
        )]);
        let error = resolve_with_env("mineru", &overrides, |_| None)
            .err()
            .unwrap();
        assert!(!error.contains("secret"));
    }
}
