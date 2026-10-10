//! Outline: parse the tender, then publish chapters and template content.
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | `parse` | Complete structured parse. Image OCR runs concurrently. |
//! | `discover` | Reading packs follow section boundaries. |
//! | `chapters` | Stable chapter identity and attachment-table mapping. |
//! | `template` | Prescribed template slots. Bidder blanks stay empty. |
//! | `tools` | The six outline tools. A turn sees only its duty's tools. |
//! | `agent` | One duty per turn. Owns the prompt, tool schema, and tool application. |
//!
//! This package does not read the company knowledge base.

pub mod agent;
pub mod chapters;
pub mod claim_review;
pub mod discover;
pub mod evidence;
pub mod frozen;
pub(crate) mod metadata_fragments;
pub(crate) mod model_wire;
pub mod parse;
pub(crate) mod read_receipts;
pub(crate) mod source_wire;
pub mod store;
mod template;
pub mod tools;
pub mod visual;

#[cfg(test)]
mod acceptance;
#[cfg(test)]
pub(crate) use acceptance::discovered as fixture_discovered;
#[cfg(test)]
pub(crate) use acceptance::organized as fixture_organized;
#[cfg(test)]
mod tests;

pub use template::{COVER_CHAPTER_ID, ProjectedOutline, project_draft, validate_publication};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChapterPurpose {
    Group,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    FixedText,
    TenderValue,
    Instruction,
    BidderBlank,
    Signature,
    Preserved,
}

impl SlotKind {
    pub fn accepts_knowledge_response(self) -> bool {
        matches!(self, Self::BidderBlank | Self::Signature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChapterOutline {
    pub id: String,
    pub parent_id: Option<String>,
    pub order: usize,
    pub title: String,
    pub purpose: ChapterPurpose,
    pub requirement_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateContent {
    pub slot_id: String,
    pub chapter_id: String,
    pub kind: SlotKind,
    pub content: TemplateBody,
    /// Host-projected tender wording. Empty when the response package may fill it.
    pub text: String,
    pub response_required: bool,
    /// Knowledge-base query. Empty unless `response_required`.
    pub match_query: String,
}

/// The model chooses a source or editable position; the host owns fixed text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TemplateBody {
    SourceCopy {
        refs: Vec<evidence::EvidenceRef>,
    },
    EditableBlank,
    GeneratedExplanation {
        supporting_refs: Vec<evidence::EvidenceRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetRef {
    TextSlot {
        slot_id: String,
    },
    FormBinding {
        form_id: String,
    },
    ManualTask {
        task_id: String,
        description: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fulfillment {
    pub requirement_id: String,
    pub primary_response_chapter_id: String,
    pub target_refs: Vec<TargetRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewIssue {
    pub id: String,
    pub code: String,
    pub description: String,
    pub requirement_ids: Vec<String>,
    pub evidence: Vec<evidence::EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenIssue {
    pub id: String,
    pub code: String,
    pub description: String,
    pub chapter_ids: Vec<String>,
}

/// Frozen outline and template. The response package must not read past this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineArtifact {
    pub schema_version: u32,
    pub project_id: String,
    pub frozen_input_sha256: String,
    pub chapters: Vec<ChapterOutline>,
    pub required_requirement_ids: std::collections::BTreeSet<String>,
    pub requirements: std::collections::BTreeMap<String, discover::RequirementRecord>,
    pub fulfillments: Vec<Fulfillment>,
    pub review_issues: Vec<ReviewIssue>,
    pub needs_review: bool,
    pub semantic_review_complete: bool,
    pub templates: Vec<TemplateContent>,
    pub open_issues: Vec<OpenIssue>,
}

pub fn canonical_sha256(value: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json_canonicalizer::to_vec(value)
        .map_err(|error| format!("outline canonical JSON: {error}"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn validate_artifact(artifact: &OutlineArtifact) -> Result<(), String> {
    if artifact.schema_version != SCHEMA_VERSION {
        return Err("outline schema version is not supported".into());
    }
    if artifact.project_id.is_empty() || artifact.frozen_input_sha256.is_empty() {
        return Err("outline artifact is missing project identity".into());
    }
    if artifact.chapters.is_empty() && artifact.templates.is_empty() {
        return Err("outline published no chapters or template content".into());
    }
    let mut chapter_ids = std::collections::BTreeSet::new();
    for chapter in &artifact.chapters {
        if chapter.id.is_empty() || chapter.title.is_empty() {
            return Err("chapter id and title are required".into());
        }
        if chapter.id == COVER_CHAPTER_ID {
            return Err(format!("chapter id {COVER_CHAPTER_ID} is reserved"));
        }
        if !chapter_ids.insert(chapter.id.as_str()) {
            return Err(format!("duplicate chapter {}", chapter.id));
        }
        if chapter.purpose == ChapterPurpose::Group && !chapter.requirement_ids.is_empty() {
            return Err(format!(
                "group chapter {} cannot own requirements",
                chapter.id
            ));
        }
        if chapter.purpose == ChapterPurpose::Group
            && artifact
                .templates
                .iter()
                .any(|slot| slot.chapter_id == chapter.id)
        {
            return Err(format!(
                "group chapter {} cannot carry a knowledge response",
                chapter.id
            ));
        }
    }
    let mut slot_ids = std::collections::BTreeSet::new();
    for slot in &artifact.templates {
        if slot.slot_id.is_empty() || !slot_ids.insert(slot.slot_id.as_str()) {
            return Err("outline template slot identity is invalid".into());
        }
        let known_chapter =
            slot.chapter_id == COVER_CHAPTER_ID || chapter_ids.contains(slot.chapter_id.as_str());
        if !known_chapter {
            return Err(format!(
                "template slot {} is not on a published chapter",
                slot.slot_id
            ));
        }
        match (&slot.content, slot.kind) {
            (TemplateBody::EditableBlank, SlotKind::BidderBlank | SlotKind::Signature) => {}
            (
                TemplateBody::SourceCopy { refs },
                SlotKind::FixedText | SlotKind::TenderValue | SlotKind::Preserved,
            ) if !refs.is_empty() => {}
            (TemplateBody::GeneratedExplanation { .. }, SlotKind::Instruction) => {}
            _ => {
                return Err(format!(
                    "template slot {} content disagrees with its role",
                    slot.slot_id
                ));
            }
        }
        if slot.response_required != slot.kind.accepts_knowledge_response() {
            return Err(format!(
                "template slot {} disagrees with its response role",
                slot.slot_id
            ));
        }
        if slot.response_required {
            if slot.text.is_empty() && !slot.match_query.trim().is_empty() {
                continue;
            }
            return Err(format!(
                "response slot {} must be empty and carry a match query",
                slot.slot_id
            ));
        } else if !slot.match_query.is_empty() {
            return Err(format!(
                "template slot {} cannot carry a knowledge query",
                slot.slot_id
            ));
        }
    }
    Ok(())
}
