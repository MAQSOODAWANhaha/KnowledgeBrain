//! Durable main-dispatch root stored on the checkpoint.
use super::*;
use crate::agent_runtime::progress::ProgressWatch;
use std::collections::BTreeSet;

pub(super) const POLICY: &str = "main-dispatch-v1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub active: Option<Active>,
    pub entries: BTreeMap<String, Entry>,
    pub last_committed_turn: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Active {
    Ordinary(String),
    Repair(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub source_id: Option<String>,
    pub spent_batches: usize,
    pub spent_replans: usize,
    pub watch: ProgressWatch,
    pub attempted_dependencies: BTreeSet<String>,
    pub completed_dependencies: Option<String>,
}
