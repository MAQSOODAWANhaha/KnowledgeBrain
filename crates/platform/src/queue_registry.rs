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
        BID_CONTENT_GENERATE_V2_TASK, BID_OUTLINE_GENERATE_V2_TASK,
        BID_REQUIREMENT_SET_COMPILE_V2_TASK, BID_SUBMISSION_EXPORT_V2_TASK,
        BID_TENDER_DOCUMENT_PROCESS_V2_TASK, BidAuthoringRequestIdentityV2, ContentGenerateJobV2,
        ContentGenerateOperationV2, OutlineGenerateJobV2, RequirementSetCompileJobV2,
        SubmissionExportJobV2, SubmissionOutputModeV2, TenderDocumentProcessJobV2,
    };
    use oxana::Job;
    use uuid::Uuid;

    fn loaded() -> QueueRegistry {
        QueueRegistry::load().expect("repo-relative deploy/queue-registry.toml")
    }

    #[test]
    fn active_registry_has_only_v2_bidding_tasks() {
        let registry = loaded();
        assert_eq!(registry.format, 1);
        let bid: Vec<_> = registry
            .entries()
            .iter()
            .filter(|entry| entry.task_type.starts_with("bid:"))
            .collect();
        assert_eq!(bid.len(), 5);
        for task in [
            BID_TENDER_DOCUMENT_PROCESS_V2_TASK,
            BID_REQUIREMENT_SET_COMPILE_V2_TASK,
            BID_OUTLINE_GENERATE_V2_TASK,
            BID_CONTENT_GENERATE_V2_TASK,
            BID_SUBMISSION_EXPORT_V2_TASK,
        ] {
            let entry = registry.entry_for_task(task).expect("V2 task declared");
            assert_eq!(entry.physical_queue, "bid-authoring-v2");
            assert_eq!(entry.payload_schema, "bid-authoring/v2");
        }
    }

    #[test]
    fn bidding_identity_formulas_equal_oxana_unique_ids() {
        let registry = loaded();
        let request = BidAuthoringRequestIdentityV2 {
            request_artifact_id: Uuid::from_u128(1),
            request_revision: 7,
            frozen_input_sha256: "a".repeat(64),
        };
        let project_id = Uuid::from_u128(2);
        let document_id = Uuid::from_u128(3);
        let workspace_id = Uuid::from_u128(4);
        let disposition_id = Uuid::from_u128(5);
        let request_formula =
            |kind: &str| format!("{kind}:{{request_artifact_id}}:{{request_revision}}");
        let request_unique = |kind: &str| format!("{kind}:{}:7", request.request_artifact_id);
        let tender = TenderDocumentProcessJobV2 {
            request: request.clone(),
            project_id,
            document_revision_id: document_id,
        };
        let outline = OutlineGenerateJobV2 {
            request: request.clone(),
            project_id,
            workspace_id,
            base_workspace_revision_id: document_id,
        };
        let content = ContentGenerateJobV2 {
            request: request.clone(),
            project_id,
            workspace_id,
            base_workspace_revision_id: document_id,
            operation: ContentGenerateOperationV2::Generate,
        };
        let export = SubmissionExportJobV2 {
            request: request.clone(),
            project_id,
            workspace_id,
            workspace_revision_id: document_id,
            output_mode: SubmissionOutputModeV2::Submission,
        };
        for (task, kind, actual) in [
            (
                BID_TENDER_DOCUMENT_PROCESS_V2_TASK,
                "tender_document_process",
                tender.unique_id(),
            ),
            (
                BID_OUTLINE_GENERATE_V2_TASK,
                "outline_generate",
                outline.unique_id(),
            ),
            (
                BID_CONTENT_GENERATE_V2_TASK,
                "content_generate",
                content.unique_id(),
            ),
            (
                BID_SUBMISSION_EXPORT_V2_TASK,
                "submission_export",
                export.unique_id(),
            ),
        ] {
            assert_eq!(
                registry.entry_for_task(task).unwrap().identity_formula,
                request_formula(kind)
            );
            assert_eq!(actual.as_deref(), Some(request_unique(kind).as_str()));
        }
        let compile = RequirementSetCompileJobV2 {
            request,
            project_id,
            document_set_revision_id: document_id,
            disposition_set_revision_id: disposition_id,
        };
        let formula = "requirement_set_compile:{project_id}:{document_set_revision_id}:{disposition_set_revision_id}";
        assert_eq!(
            registry
                .entry_for_task(BID_REQUIREMENT_SET_COMPILE_V2_TASK)
                .unwrap()
                .identity_formula,
            formula
        );
        assert_eq!(
            compile.unique_id(),
            Some(format!(
                "requirement_set_compile:{project_id}:{document_id}:{disposition_id}"
            ))
        );
    }

    #[test]
    fn all_implemented_v2_workers_are_required() {
        let registry = loaded();
        for task in [
            BID_TENDER_DOCUMENT_PROCESS_V2_TASK,
            BID_REQUIREMENT_SET_COMPILE_V2_TASK,
            BID_OUTLINE_GENERATE_V2_TASK,
            BID_CONTENT_GENERATE_V2_TASK,
            BID_SUBMISSION_EXPORT_V2_TASK,
        ] {
            assert_eq!(
                registry.launch_mode(task),
                Some(LaunchMode::RequiredEnabled)
            );
        }
    }

    #[test]
    fn no_bidding_task_uses_default_queue() {
        for entry in loaded().entries() {
            if entry.task_type.starts_with("bid:") {
                assert_ne!(entry.physical_queue, "default");
            }
        }
    }
}
