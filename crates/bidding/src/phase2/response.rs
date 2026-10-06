//! Phase 2: bind knowledge-base hits to phase-1 response slots.

use super::{Phase2Response, ResponseStatus, SlotResponse};
use crate::phase1::{Phase1Artifact, SCHEMA_VERSION, canonical_sha256, validate_artifact};
use serde::{Deserialize, Serialize};

pub const NO_EVIDENCE_TEXT: &str = "【待人工补充】";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchQuery {
    pub slot_id: String,
    pub chapter_id: String,
    pub query: String,
}

/// One knowledge-base passage already attached to a phase-1 response slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceHit {
    pub evidence_id: String,
    pub document_id: String,
    pub slot_id: String,
    pub text: String,
}

/// Queries phase 2 may send to the knowledge base. Fixed template slots are absent.
pub fn match_queries(artifact: &Phase1Artifact) -> Result<Vec<MatchQuery>, String> {
    validate_artifact(artifact)?;
    Ok(artifact
        .templates
        .iter()
        .filter(|slot| slot.response_required)
        .map(|slot| MatchQuery {
            slot_id: slot.slot_id.clone(),
            chapter_id: slot.chapter_id.clone(),
            query: slot.match_query.clone(),
        })
        .collect())
}

/// Write response data for every response slot. Unmatched slots stay placeholders.
pub fn respond(artifact: &Phase1Artifact, hits: &[EvidenceHit]) -> Result<Phase2Response, String> {
    validate_artifact(artifact)?;
    let response_slots: Vec<_> = artifact
        .templates
        .iter()
        .filter(|slot| slot.response_required)
        .collect();
    let allowed: std::collections::BTreeSet<_> = response_slots
        .iter()
        .map(|slot| slot.slot_id.as_str())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for hit in hits {
        if hit.evidence_id.is_empty() || hit.document_id.is_empty() {
            return Err("knowledge hit is missing identity".into());
        }
        if !allowed.contains(hit.slot_id.as_str()) {
            return Err(format!(
                "knowledge hit {} is not a response slot",
                hit.slot_id
            ));
        }
        if hit.text.trim().is_empty() || hit.text == NO_EVIDENCE_TEXT {
            return Err(format!(
                "knowledge hit {} has no evidence text",
                hit.evidence_id
            ));
        }
        if !seen.insert((hit.slot_id.as_str(), hit.evidence_id.as_str())) {
            return Err(format!(
                "knowledge hit {} is repeated for {}",
                hit.evidence_id, hit.slot_id
            ));
        }
    }
    let responses = response_slots
        .into_iter()
        .map(|slot| {
            let matched: Vec<_> = hits
                .iter()
                .filter(|hit| hit.slot_id == slot.slot_id)
                .collect();
            if matched.is_empty() {
                SlotResponse {
                    slot_id: slot.slot_id.clone(),
                    chapter_id: slot.chapter_id.clone(),
                    status: ResponseStatus::NoEvidence,
                    text: NO_EVIDENCE_TEXT.into(),
                    evidence_ids: Vec::new(),
                }
            } else {
                SlotResponse {
                    slot_id: slot.slot_id.clone(),
                    chapter_id: slot.chapter_id.clone(),
                    status: ResponseStatus::Matched,
                    text: matched
                        .iter()
                        .map(|hit| hit.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    evidence_ids: matched.iter().map(|hit| hit.evidence_id.clone()).collect(),
                }
            }
        })
        .collect();
    Ok(Phase2Response {
        schema_version: SCHEMA_VERSION,
        phase1_sha256: canonical_sha256(artifact)?,
        responses,
    })
}
