//! Phase 1: project a completed tender outline into chapters and template slots.

use super::chapters::AttachmentBinding;
use super::tools::{Draft, unmapped_forms};
use super::{OutlineArtifact, SCHEMA_VERSION, TemplateBody, validate_artifact};
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
    super::tools::validate_final_outline(input, &draft.required_requirement_ids, draft)?;
    if frozen_input_sha256 != super::evidence::input_digest(input)? {
        return Err("outline frozen input digest does not match".into());
    }
    let semantic_review_complete = draft.reviewed_requirement_ids == draft.required_requirement_ids
        && draft.reviewed_pack_ids == draft.required_pack_ids;
    if !semantic_review_complete {
        return Err("outline semantic review is incomplete".into());
    }
    let artifact = OutlineArtifact {
        schema_version: SCHEMA_VERSION,
        project_id: input.project_id.clone(),
        frozen_input_sha256: frozen_input_sha256.to_string(),
        chapters: draft.chapters.clone(),
        required_requirement_ids: draft.required_requirement_ids.clone(),
        requirements: draft.requirements.clone(),
        fulfillments: draft.fulfillments.clone(),
        review_issues: draft.review_issues.clone(),
        needs_review: super::tools::needs_semantic_review(draft),
        semantic_review_complete,
        templates: draft
            .slots
            .iter()
            .map(|slot| {
                let mut slot = slot.clone();
                if let TemplateBody::SourceCopy { refs } = &slot.content {
                    slot.text = super::tools::source_copy(input, refs)?;
                }
                Ok(slot)
            })
            .collect::<Result<Vec<_>, String>>()?,
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
/// The direct store route must perform the same validation as tool completion.
/// Deserialized artifacts do not inherit the validity of their former draft.
pub fn validate_publication(
    input: &FrozenInput,
    artifact: &OutlineArtifact,
    bindings: &[AttachmentBinding],
) -> Result<(), String> {
    validate_artifact(artifact)?;
    if artifact.project_id != input.project_id
        || artifact.frozen_input_sha256 != super::evidence::input_digest(input)?
    {
        return Err("published outline input identity mismatch".into());
    }
    if !artifact.semantic_review_complete {
        return Err("published outline semantic review is incomplete".into());
    }
    let draft = Draft {
        chapters: artifact.chapters.clone(),
        bindings: bindings.to_vec(),
        slots: artifact.templates.clone(),
        required_requirement_ids: artifact.required_requirement_ids.clone(),
        requirements: artifact.requirements.clone(),
        fulfillments: artifact.fulfillments.clone(),
        review_issues: artifact.review_issues.clone(),
        slots_submitted: true,
        finished: true,
        ..Draft::default()
    };
    super::tools::validate_final_outline(input, &artifact.required_requirement_ids, &draft)?;
    for issue in &artifact.review_issues {
        if issue.evidence.is_empty() {
            return Err("review issue has no source evidence".into());
        }
        super::evidence::resolve_evidence(input, &issue.evidence)?;
    }
    let needs_review = !artifact.review_issues.is_empty()
        || artifact.fulfillments.iter().any(|f| {
            f.target_refs
                .iter()
                .any(|t| matches!(t, super::TargetRef::ManualTask { .. }))
        });
    if artifact.needs_review != needs_review {
        return Err("published outline review status is inconsistent".into());
    }
    Ok(())
}
