//! Canonical queue-registry declarations and typed reader.
//!
//! Phase 1B/1D covers schema/data declarations plus static equality for the
//! implemented producer/queue subset. This module does not claim Redis,
//! handler, subscription, or readiness closure.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use thiserror::Error;

const REGISTRY_RELATIVE: &str = "deploy/queue-registry.toml";
const REGISTRY_PATH_ENV: &str = "KNOWLEDGEBRAIN_QUEUE_REGISTRY_PATH";
const EMBEDDED_REGISTRY: &str = include_str!("../../../deploy/queue-registry.toml");

#[derive(Debug, Error)]
pub enum QueueRegistryError {
    #[error("queue registry not found at {REGISTRY_RELATIVE}")]
    NotFound,
    #[error("queue registry unreadable: {0}")]
    Io(String),
    #[error("queue registry parse failed: {0}")]
    Parse(String),
    #[error("queue registry invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    RequiredEnabled,
    DeclaredDisabled,
    MaintenanceOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueRegistryEntry {
    pub physical_queue: String,
    pub task_type: String,
    pub payload_schema: String,
    pub payload_version: u32,
    pub identity_formula: String,
    pub protocol: u32,
    pub handler: String,
    pub snapshots: Vec<String>,
    pub capabilities: Vec<String>,
    pub launch_mode: LaunchMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueRegistry {
    pub format: u32,
    pub schema_version: u32,
    pub release_id: String,
    pub minimum_worker_protocol: u32,
    pub entries: Vec<QueueRegistryEntry>,
}

impl QueueRegistry {
    pub fn parse(source: &str) -> Result<Self, QueueRegistryError> {
        let registry: Self =
            toml::from_str(source).map_err(|error| QueueRegistryError::Parse(error.to_string()))?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn load() -> Result<Self, QueueRegistryError> {
        if let Some(path) = env_override_path(REGISTRY_PATH_ENV) {
            return Self::load_from_path(path);
        }
        match locate_registry_path() {
            Ok(path) => Self::load_from_path(path),
            Err(_) => Self::parse(EMBEDDED_REGISTRY),
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, QueueRegistryError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .map_err(|error| QueueRegistryError::Io(format!("{}: {error}", path.display())))?;
        Self::parse(&source)
    }

    pub fn entries(&self) -> &[QueueRegistryEntry] {
        &self.entries
    }

    pub fn entry_for_task(&self, task_type: &str) -> Option<&QueueRegistryEntry> {
        self.entries
            .iter()
            .find(|entry| entry.task_type == task_type)
    }

    pub fn launch_mode(&self, task_type: &str) -> Option<LaunchMode> {
        self.entry_for_task(task_type)
            .map(|entry| entry.launch_mode)
    }

    pub fn required_enabled_tasks(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.launch_mode == LaunchMode::RequiredEnabled)
            .map(|entry| entry.task_type.as_str())
            .collect()
    }

    pub fn declared_disabled_tasks(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.launch_mode == LaunchMode::DeclaredDisabled)
            .map(|entry| entry.task_type.as_str())
            .collect()
    }

    fn validate(&self) -> Result<(), QueueRegistryError> {
        if self.format != 1 {
            return Err(invalid("format must be 1"));
        }
        if self.schema_version != 1 {
            return Err(invalid("schema_version must be 1"));
        }
        if self.release_id.trim().is_empty() {
            return Err(invalid("release_id must be a non-empty literal"));
        }
        if self.minimum_worker_protocol != 1 {
            return Err(invalid("minimum_worker_protocol must be 1"));
        }
        if self.entries.is_empty() {
            return Err(invalid("entries must not be empty"));
        }

        let mut task_types = BTreeSet::new();
        for entry in &self.entries {
            if entry.physical_queue.trim().is_empty()
                || entry.task_type.trim().is_empty()
                || entry.payload_schema.trim().is_empty()
                || entry.identity_formula.trim().is_empty()
                || entry.handler.trim().is_empty()
            {
                return Err(invalid("entry fields must be non-empty"));
            }
            if entry.physical_queue == "sync" {
                return Err(invalid("sync queue is forbidden"));
            }
            if entry.payload_version != 1 {
                return Err(invalid("payload_version must be 1"));
            }
            if entry.protocol != 1 {
                return Err(invalid("protocol must be 1"));
            }
            if entry.snapshots.is_empty() || entry.capabilities.is_empty() {
                return Err(invalid("snapshots and capabilities must be declared"));
            }
            if !task_types.insert(entry.task_type.as_str()) {
                return Err(invalid("duplicate task_type"));
            }
        }
        Ok(())
    }
}

fn invalid(message: &str) -> QueueRegistryError {
    QueueRegistryError::Invalid(message.to_string())
}

fn env_override_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn locate_registry_path() -> Result<PathBuf, QueueRegistryError> {
    let mut candidates = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(REGISTRY_RELATIVE),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd;
        loop {
            candidates.push(dir.join(REGISTRY_RELATIVE));
            if !dir.pop() {
                break;
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(QueueRegistryError::NotFound)
}

fn cached() -> Result<&'static QueueRegistry, QueueRegistryError> {
    static REGISTRY: OnceLock<QueueRegistry> = OnceLock::new();
    if let Some(registry) = REGISTRY.get() {
        return Ok(registry);
    }
    let loaded = QueueRegistry::load()?;
    Ok(REGISTRY.get_or_init(|| loaded))
}

pub fn entries() -> Result<&'static [QueueRegistryEntry], QueueRegistryError> {
    Ok(cached()?.entries())
}

pub fn entry_for_task(
    task_type: &str,
) -> Result<Option<&'static QueueRegistryEntry>, QueueRegistryError> {
    Ok(cached()?.entry_for_task(task_type))
}

pub fn launch_mode(task_type: &str) -> Result<Option<LaunchMode>, QueueRegistryError> {
    Ok(cached()?.launch_mode(task_type))
}

pub fn required_enabled_tasks() -> Result<Vec<&'static str>, QueueRegistryError> {
    Ok(cached()?.required_enabled_tasks())
}

pub fn declared_disabled_tasks() -> Result<Vec<&'static str>, QueueRegistryError> {
    Ok(cached()?.declared_disabled_tasks())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TYPE_BID_CONVERT, TYPE_BID_EXTRACT, TYPE_BID_MATCH_ROUTE_V1,
        TYPE_BID_PREPARE_ATTACHMENT_V1, TYPE_BID_RENDER_SUBMISSION_V1,
    };

    const DOCUMENTED_QUEUES: &[&str] = &[
        "default",
        "postprocess",
        "summary",
        "multimodal",
        "graph",
        "question",
        "wiki",
        "low",
        "bid-convert-v1",
        "bid-extract-v1",
        "bid-matching-v1",
        "bid-render-v1",
    ];

    fn loaded() -> QueueRegistry {
        QueueRegistry::load().expect("repo-relative deploy/queue-registry.toml")
    }

    #[test]
    fn parse_ok() {
        let registry = loaded();
        assert_eq!(registry.format, 1);
        assert_eq!(registry.schema_version, 1);
        assert_eq!(registry.release_id, "kb-queue-registry-v1");
        assert_eq!(registry.minimum_worker_protocol, 1);
        assert_eq!(registry.entries().len(), 22);
    }

    #[test]
    fn exact_bid_entries() {
        let registry = loaded();
        let bid: Vec<_> = registry
            .entries()
            .iter()
            .filter(|entry| entry.task_type.starts_with("bid:"))
            .collect();
        assert_eq!(bid.len(), 5);

        let convert = registry
            .entry_for_task(TYPE_BID_CONVERT)
            .expect("bid:convert");
        assert_eq!(convert.physical_queue, "bid-convert-v1");
        assert_eq!(convert.payload_schema, "bid-convert/v1");
        assert_eq!(convert.identity_formula, "bid:convert:{document_id}");
        assert_eq!(convert.handler, "BidConvertV1Handler");
        assert_eq!(convert.launch_mode, LaunchMode::RequiredEnabled);

        let prepare = registry
            .entry_for_task(TYPE_BID_PREPARE_ATTACHMENT_V1)
            .expect("bid:prepare-attachment:v1");
        assert_eq!(prepare.physical_queue, "bid-convert-v1");
        assert_eq!(prepare.payload_schema, "bid-prepare-attachment/v1");
        assert_eq!(
            prepare.identity_formula,
            "bid:prepare-attachment:v1:{preparation_job_id}"
        );
        assert_eq!(prepare.handler, "BidPrepareAttachmentV1Handler");
        assert_eq!(prepare.launch_mode, LaunchMode::RequiredEnabled);

        let extract = registry
            .entry_for_task(TYPE_BID_EXTRACT)
            .expect("bid:extract");
        assert_eq!(extract.physical_queue, "bid-extract-v1");
        assert_eq!(extract.payload_schema, "bid-extract/v1");
        assert_eq!(extract.identity_formula, "bid:extract:{run_id}");
        assert_eq!(extract.handler, "BidExtractV1Handler");
        assert_eq!(extract.launch_mode, LaunchMode::RequiredEnabled);

        let matching = registry
            .entry_for_task(TYPE_BID_MATCH_ROUTE_V1)
            .expect("bid:match-route:v1");
        assert_eq!(matching.physical_queue, "bid-matching-v1");
        assert_eq!(matching.payload_schema, "bid-match-route/v1");
        assert_eq!(matching.identity_formula, "bid:match-route:v1:{job_id}");
        assert_eq!(matching.handler, "BidMatchRouteV1Handler");
        assert_eq!(matching.launch_mode, LaunchMode::RequiredEnabled);

        let render = registry
            .entry_for_task(TYPE_BID_RENDER_SUBMISSION_V1)
            .expect("bid:render-submission:v1");
        assert_eq!(render.physical_queue, "bid-render-v1");
        assert_eq!(render.payload_schema, "bid-render-submission/v1");
        assert_eq!(
            render.identity_formula,
            "bid:render-submission:v1:{render_job_id}"
        );
        assert_eq!(render.handler, "BidRenderSubmissionV1Handler");
        assert_eq!(render.launch_mode, LaunchMode::RequiredEnabled);
    }

    #[test]
    fn unknown_task_absent() {
        let registry = loaded();
        assert!(registry.entry_for_task("not-a-real-task").is_none());
        assert!(registry.entry_for_task("bid:match").is_none());
        assert!(registry.entry_for_task("bid:convert:v1").is_none());
        assert!(registry.entry_for_task("bid:extract-target:v1").is_none());
        assert!(registry.launch_mode("missing").is_none());
    }

    #[test]
    fn declared_disabled_includes_multimodal_graph_question() {
        let registry = loaded();
        let disabled = registry.declared_disabled_tasks();
        assert!(disabled.contains(&"image:multimodal"));
        assert!(disabled.contains(&"chunk:extract"));
        assert!(disabled.contains(&"question:generation"));
        assert_eq!(disabled.len(), 3);
        let mut queues: Vec<&str> = registry
            .entries()
            .iter()
            .filter(|entry| entry.launch_mode == LaunchMode::DeclaredDisabled)
            .map(|entry| entry.physical_queue.as_str())
            .collect();
        queues.sort_unstable();
        queues.dedup();
        assert_eq!(queues, ["graph", "multimodal", "question"]);
    }

    #[test]
    fn no_default_bid_handler() {
        let registry = loaded();
        for entry in registry.entries() {
            if entry.physical_queue == "default" {
                assert!(
                    !entry.task_type.starts_with("bid:"),
                    "default must not carry Bid task {}",
                    entry.task_type
                );
                assert!(
                    !entry.handler.contains("Bid"),
                    "default must not carry Bid handler {}",
                    entry.handler
                );
            }
        }
        assert!(
            registry
                .entry_for_task(TYPE_BID_CONVERT)
                .unwrap()
                .physical_queue
                != "default"
        );
    }

    #[test]
    fn protocol_is_one() {
        let registry = loaded();
        assert_eq!(registry.minimum_worker_protocol, 1);
        assert!(registry.entries().iter().all(|entry| entry.protocol == 1));
        assert!(
            registry
                .entries()
                .iter()
                .all(|entry| entry.payload_version == 1)
        );
    }

    #[test]
    fn no_extra_queues_beyond_documented_set() {
        let registry = loaded();
        let queues: BTreeSet<&str> = registry
            .entries()
            .iter()
            .map(|entry| entry.physical_queue.as_str())
            .collect();
        let documented: BTreeSet<&str> = DOCUMENTED_QUEUES.iter().copied().collect();
        assert_eq!(queues, documented);
        assert!(!queues.contains("sync"));
        assert!(!queues.contains("bid-conversion-v1"));
        assert!(!queues.contains("bid-extraction-v1"));
    }

    #[test]
    fn embedded_registry_matches_checked_in() {
        let embedded = QueueRegistry::parse(EMBEDDED_REGISTRY).expect("embedded registry");
        assert_eq!(embedded, loaded());
    }

    #[test]
    fn load_from_missing_path_is_unreadable() {
        let error = QueueRegistry::load_from_path("/no/such/knowledgebrain-queue-registry.toml")
            .expect_err("missing override path");
        assert!(matches!(error, QueueRegistryError::Io(_)));
    }

    #[test]
    fn deny_unknown_fields() {
        let source = r#"
format = 1
schema_version = 1
release_id = "x"
minimum_worker_protocol = 1
unexpected = true
[[entries]]
physical_queue = "default"
task_type = "document:process"
payload_schema = "document-process/v1"
payload_version = 1
identity_formula = "document:process:{document_id}:{attempt}"
protocol = 1
handler = "DocumentProcessV1Handler"
snapshots = ["process"]
capabilities = ["postgresql"]
launch_mode = "required_enabled"
"#;
        assert!(QueueRegistry::parse(source).is_err());
    }
}
