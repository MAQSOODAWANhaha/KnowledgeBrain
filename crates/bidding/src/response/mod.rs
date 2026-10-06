//! Response: match knowledge-base evidence and publish response data.
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | `agent` | Response duty only. No tender parse, no new chapters, no template edits. |
//! | `bind` | Bind knowledge hits to outline response slots. |
//!
//! Input is a [`crate::outline::OutlineArtifact`]. Unmatched slots stay
//! `【待人工补充】`.

pub mod agent;
mod bind;

pub use bind::{EvidenceHit, MatchQuery, NO_EVIDENCE_TEXT, match_queries, respond};

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

/// Response data for outline slots. `outline_sha256` binds it to one artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseSet {
    pub schema_version: u32,
    pub outline_sha256: String,
    pub responses: Vec<SlotResponse>,
}
