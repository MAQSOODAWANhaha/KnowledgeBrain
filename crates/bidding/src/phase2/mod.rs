//! Phase 2: match knowledge-base evidence and publish response data.
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | `agent` | Response duty only. No tender parse, no new chapters, no template edits. |
//! | `response` | Bind knowledge hits to phase-1 response slots. |
//!
//! Input is a [`crate::phase1::Phase1Artifact`]. Unmatched slots stay
//! `【待人工补充】`.

pub mod agent;
mod response;

pub use response::{EvidenceHit, MatchQuery, NO_EVIDENCE_TEXT, match_queries, respond};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Matched,
    NoEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotResponse {
    pub slot_id: String,
    pub chapter_id: String,
    pub status: ResponseStatus,
    pub text: String,
    pub evidence_ids: Vec<String>,
}

/// Response data for phase-1 slots. `phase1_sha256` binds it to one artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phase2Response {
    pub schema_version: u32,
    pub phase1_sha256: String,
    pub responses: Vec<SlotResponse>,
}
