//! Signed first-launch intended feature state.
//!
//! Each registry task has exactly one lane. Lane `state` must be compatible
//! with that task's `launch_mode`. This reader does not invent handlers or SQL.

use crate::queue_registry::{LaunchMode, QueueRegistry, QueueRegistryError};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

const STATE_RELATIVE: &str = "deploy/first-launch/intended-state.toml";
const STATE_PATH_ENV: &str = "KNOWLEDGEBRAIN_INTENDED_STATE_PATH";
const EMBEDDED_STATE: &str = include_str!("../../../deploy/first-launch/intended-state.toml");

#[derive(Debug, Error)]
pub enum IntendedStateError {
    #[error("intended state not found at {STATE_RELATIVE}")]
    NotFound,
    #[error("intended state unreadable: {0}")]
    Io(String),
    #[error("intended state parse failed: {0}")]
    Parse(String),
    #[error("intended state invalid: {0}")]
    Invalid(String),
    #[error("queue registry: {0}")]
    Registry(#[from] QueueRegistryError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureState {
    Enabled,
    Disabled,
    MaintenanceOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntendedLane {
    pub task_type: String,
    pub state: FeatureState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntendedState {
    pub format_version: u32,
    pub contract: String,
    pub lanes: Vec<IntendedLane>,
}

impl IntendedState {
    pub fn parse(source: &str) -> Result<Self, IntendedStateError> {
        let state: Self =
            toml::from_str(source).map_err(|error| IntendedStateError::Parse(error.to_string()))?;
        state.validate(&QueueRegistry::load()?)?;
        Ok(state)
    }

    pub fn load() -> Result<Self, IntendedStateError> {
        if let Some(path) = env_override_path(STATE_PATH_ENV) {
            return Self::load_from_path(path);
        }
        match locate_state_path() {
            Ok(path) => Self::load_from_path(path),
            Err(_) => Self::parse(EMBEDDED_STATE),
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, IntendedStateError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .map_err(|error| IntendedStateError::Io(format!("{}: {error}", path.display())))?;
        Self::parse(&source)
    }

    pub fn lanes(&self) -> &[IntendedLane] {
        &self.lanes
    }

    pub fn lane_for_task(&self, task_type: &str) -> Option<&IntendedLane> {
        self.lanes.iter().find(|lane| lane.task_type == task_type)
    }

    pub fn disabled_tasks(&self) -> Vec<&str> {
        self.lanes
            .iter()
            .filter(|lane| lane.state == FeatureState::Disabled)
            .map(|lane| lane.task_type.as_str())
            .collect()
    }

    fn validate(&self, registry: &QueueRegistry) -> Result<(), IntendedStateError> {
        if self.format_version != 1 {
            return Err(invalid("format_version must be 1"));
        }
        if self.contract != "bid-matching-v1" {
            return Err(invalid("contract must be bid-matching-v1"));
        }
        if self.lanes.is_empty() {
            return Err(invalid("lanes must not be empty"));
        }

        let mut seen = BTreeSet::new();
        for lane in &self.lanes {
            if lane.task_type.trim().is_empty() {
                return Err(invalid("lane task_type must be non-empty"));
            }
            if !seen.insert(lane.task_type.as_str()) {
                return Err(invalid("duplicate lane task_type"));
            }
            let Some(entry) = registry.entry_for_task(&lane.task_type) else {
                return Err(invalid("lane task_type is not in the queue registry"));
            };
            if !state_compatible(entry.launch_mode, lane.state) {
                return Err(invalid("lane state is incompatible with launch_mode"));
            }
        }

        for entry in registry.entries() {
            if !seen.contains(entry.task_type.as_str()) {
                return Err(invalid("every registry task must have exactly one lane"));
            }
        }
        if self.lanes.len() != registry.entries().len() {
            return Err(invalid("every registry task must have exactly one lane"));
        }
        Ok(())
    }
}

fn state_compatible(launch_mode: LaunchMode, state: FeatureState) -> bool {
    matches!(
        (launch_mode, state),
        (LaunchMode::RequiredEnabled, FeatureState::Enabled)
            | (LaunchMode::DeclaredDisabled, FeatureState::Disabled)
            | (LaunchMode::MaintenanceOnly, FeatureState::MaintenanceOnly)
    )
}

fn invalid(message: &str) -> IntendedStateError {
    IntendedStateError::Invalid(message.to_string())
}

fn env_override_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn locate_state_path() -> Result<PathBuf, IntendedStateError> {
    let mut candidates = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(STATE_RELATIVE),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd;
        loop {
            candidates.push(dir.join(STATE_RELATIVE));
            if !dir.pop() {
                break;
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(IntendedStateError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TYPE_BID_CONVERT, TYPE_BID_EXTRACT, TYPE_BID_MATCH_ROUTE_V1, TYPE_BID_SECTION_RETRY,
        TYPE_CHUNK_EXTRACT, TYPE_IMAGE_MULTIMODAL, TYPE_QUESTION,
    };

    fn loaded() -> IntendedState {
        IntendedState::load().expect("repo-relative deploy/first-launch/intended-state.toml")
    }

    fn source_with_states(state_for: impl Fn(&str, LaunchMode) -> &'static str) -> String {
        let registry = QueueRegistry::load().expect("queue registry");
        let mut source = String::from("format_version = 1\ncontract = \"bid-matching-v1\"\n");
        for entry in registry.entries() {
            source.push_str(&format!(
                "\n[[lanes]]\ntask_type = \"{}\"\nstate = \"{}\"\n",
                entry.task_type,
                state_for(&entry.task_type, entry.launch_mode)
            ));
        }
        source
    }

    fn compatible_state(_task_type: &str, launch_mode: LaunchMode) -> &'static str {
        match launch_mode {
            LaunchMode::RequiredEnabled => "enabled",
            LaunchMode::DeclaredDisabled => "disabled",
            LaunchMode::MaintenanceOnly => "maintenance_only",
        }
    }

    #[test]
    fn parse_ok() {
        let state = loaded();
        assert_eq!(state.format_version, 1);
        assert_eq!(state.contract, "bid-matching-v1");
        let registry = QueueRegistry::load().expect("queue registry");
        assert_eq!(state.lanes().len(), registry.entries().len());
        assert_eq!(
            state
                .lane_for_task("system:live-recovery:v1")
                .map(|lane| lane.state),
            Some(FeatureState::Enabled)
        );
        assert_eq!(
            state
                .lane_for_task("system:maintenance-housekeep:v1")
                .map(|lane| lane.state),
            Some(FeatureState::MaintenanceOnly)
        );
    }

    #[test]
    fn disabled_includes_multimodal_graph_question() {
        let state = loaded();
        let disabled = state.disabled_tasks();
        assert!(disabled.contains(&TYPE_IMAGE_MULTIMODAL));
        assert!(disabled.contains(&TYPE_CHUNK_EXTRACT));
        assert!(disabled.contains(&TYPE_QUESTION));
        assert_eq!(disabled.len(), 3);
    }

    #[test]
    fn bid_four_enabled() {
        let state = loaded();
        for task_type in [
            TYPE_BID_CONVERT,
            TYPE_BID_EXTRACT,
            TYPE_BID_SECTION_RETRY,
            TYPE_BID_MATCH_ROUTE_V1,
        ] {
            assert_eq!(
                state.lane_for_task(task_type).map(|lane| lane.state),
                Some(FeatureState::Enabled),
                "{task_type} must be enabled"
            );
        }
    }

    #[test]
    fn incompatible_state_rejected() {
        let source = source_with_states(|task_type, launch_mode| {
            if task_type == "document:process" {
                "disabled"
            } else {
                compatible_state(task_type, launch_mode)
            }
        });
        assert!(IntendedState::parse(&source).is_err());

        let source = source_with_states(|task_type, launch_mode| {
            if task_type == TYPE_IMAGE_MULTIMODAL {
                "enabled"
            } else {
                compatible_state(task_type, launch_mode)
            }
        });
        assert!(IntendedState::parse(&source).is_err());

        let source = source_with_states(|task_type, launch_mode| {
            if task_type == "system:maintenance-housekeep:v1" {
                "enabled"
            } else {
                compatible_state(task_type, launch_mode)
            }
        });
        assert!(IntendedState::parse(&source).is_err());
    }

    #[test]
    fn embedded_intended_state_matches_checked_in() {
        let embedded = IntendedState::parse(EMBEDDED_STATE).expect("embedded intended state");
        assert_eq!(embedded, loaded());
    }

    #[test]
    fn load_from_missing_path_is_unreadable() {
        let error = IntendedState::load_from_path("/no/such/knowledgebrain-intended-state.toml")
            .expect_err("missing override path");
        assert!(matches!(error, IntendedStateError::Io(_)));
    }
}
