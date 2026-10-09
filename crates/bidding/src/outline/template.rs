//! Phase 1: project a completed tender outline into chapters and template slots.

use super::chapters::AttachmentBinding;
use super::tools::{Draft, unmapped_forms};
use super::{OutlineArtifact, SCHEMA_VERSION, validate_artifact};
use crate::analysis::FrozenInput;

pub const COVER_CHAPTER_ID: &str = "cover";

/// Chapters, slots, and attachment bindings projected from a finished tool draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedOutline {
    pub artifact: OutlineArtifact,
    pub bindings: Vec<AttachmentBinding>,
}

/// Project a finished tool draft. An unfinished draft cannot be published.
pub fn project_draft(
    input: &FrozenInput,
    frozen_input_sha256: &str,
    draft: &Draft,
) -> Result<ProjectedOutline, String> {
    if !draft.finished {
        return Err("outline draft is not finished".into());
    }
    let artifact = OutlineArtifact {
        schema_version: SCHEMA_VERSION,
        project_id: input.project_id.clone(),
        frozen_input_sha256: frozen_input_sha256.to_string(),
        chapters: draft.chapters.clone(),
        templates: draft.slots.clone(),
        open_issues: Vec::new(),
    };
    validate_artifact(&artifact)?;
    if let Some(form_id) = unmapped_forms(input, draft).into_iter().next() {
        return Err(format!(
            "attachment table {form_id} is not mapped to a chapter"
        ));
    }
    Ok(ProjectedOutline {
        artifact,
        bindings: draft.bindings.clone(),
    })
}
