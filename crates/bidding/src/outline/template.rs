//! Phase 1: project a completed tender outline into chapters and template slots.

use super::chapters::AttachmentBinding;
use super::tools::{Draft, unmapped_forms};
use super::{
    ChapterOutline, ChapterPurpose, OpenIssue, OutlineArtifact, SCHEMA_VERSION, SlotKind,
    TemplateContent, validate_artifact,
};
use crate::analysis::FrozenInput;
use crate::analysis::draft::{BodyStatus, ChapterPurpose as PlanPurpose, DraftStatus};
use crate::analysis::outline_flow::{IssueStatus, Phase};
use crate::analysis::{AnalysisResult, RecordData, RegionRole, Span, TemplateRegion};

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
///
/// Bidder and signature slots stay empty. Fixed wording is copied from the
/// frozen tender span or grid cell, never from company knowledge.
pub fn project(input: &FrozenInput, result: &AnalysisResult) -> Result<OutlineArtifact, String> {
    if result.analysis.outline.phase != Phase::Complete {
        return Err("outline is not complete".into());
    }
    let mut chapters = Vec::new();
    let mut templates = Vec::new();
    if let Some(info) = &result.analysis.outline.project_info {
        for (index, line) in info.cover_lines.iter().enumerate() {
            if line.value.is_empty() {
                return Err("prescribed cover line is empty".into());
            }
            templates.push(TemplateContent {
                slot_id: format!("{COVER_CHAPTER_ID}:{index}"),
                chapter_id: COVER_CHAPTER_ID.into(),
                kind: SlotKind::TenderValue,
                text: line.value.clone(),
                response_required: false,
                match_query: String::new(),
            });
        }
    }
    for item in &result.analysis.draft_plan {
        if item.status == DraftStatus::Omitted {
            continue;
        }
        if item.id.is_empty() || item.id == COVER_CHAPTER_ID || item.title.is_empty() {
            return Err("outline chapter identity is invalid".into());
        }
        let purpose = match item.purpose {
            PlanPurpose::Group => ChapterPurpose::Group,
            PlanPurpose::Response => ChapterPurpose::Response,
        };
        chapters.push(ChapterOutline {
            id: item.id.clone(),
            parent_id: item.parent.clone(),
            order: item.order,
            title: item.title.clone(),
            purpose,
            requirement_ids: item.requirement_ids.clone(),
        });
        if item.preserved.is_empty()
            && !matches!(item.body_status, BodyStatus::User | BodyStatus::Generated)
        {
            push_chapter_slots(
                input,
                result,
                item.id.as_str(),
                &item.title,
                item,
                &mut templates,
            )?;
        } else {
            templates.push(TemplateContent {
                slot_id: format!("{}:preserved", item.id),
                chapter_id: item.id.clone(),
                kind: SlotKind::Preserved,
                text: String::new(),
                response_required: false,
                match_query: String::new(),
            });
        }
    }
    let mut open_issues: Vec<_> = result
        .analysis
        .outline
        .issues
        .iter()
        .filter(|(_, issue)| issue.status == IssueStatus::Open)
        .map(|(id, issue)| OpenIssue {
            id: id.clone(),
            code: issue.code.clone(),
            description: issue.description.clone(),
            chapter_ids: issue.chapter_ids.clone(),
        })
        .collect();
    open_issues.sort_by(|left, right| left.id.cmp(&right.id));
    let artifact = OutlineArtifact {
        schema_version: SCHEMA_VERSION,
        project_id: input.project_id.clone(),
        frozen_input_sha256: result.frozen_input_sha256.clone(),
        chapters,
        templates,
        open_issues,
    };
    validate_artifact(&artifact)?;
    if let Some(form_id) = super::chapters::unmapped_attachment_forms(
        input,
        &result.analysis.draft_plan,
        &result.analysis.records,
    )
    .into_iter()
    .next()
    {
        return Err(format!(
            "attachment table {form_id} is not mapped to a chapter"
        ));
    }
    Ok(artifact)
}
fn push_chapter_slots(
    input: &FrozenInput,
    result: &AnalysisResult,
    chapter_id: &str,
    title: &str,
    item: &crate::analysis::draft::DraftPlanItem,
    templates: &mut Vec<TemplateContent>,
) -> Result<(), String> {
    let Some(template_id) = item.template_id.as_deref() else {
        if item.purpose != PlanPurpose::Response {
            return Ok(());
        }
        templates.push(response_slot(
            format!("{chapter_id}:body"),
            chapter_id,
            SlotKind::BidderBlank,
            title,
            "",
        )?);
        return Ok(());
    };
    let record = result
        .analysis
        .records
        .get(template_id)
        .ok_or_else(|| format!("template {template_id} is missing"))?;
    let RecordData::Template { regions, .. } = &record.data else {
        return Err(format!("template {template_id} is not template content"));
    };
    if regions.is_empty() && item.purpose == PlanPurpose::Response {
        templates.push(response_slot(
            format!("{chapter_id}:body"),
            chapter_id,
            SlotKind::BidderBlank,
            title,
            "",
        )?);
        return Ok(());
    }
    for (index, region) in regions.iter().enumerate() {
        let kind = slot_kind(region.role);
        let slot_id = format!("{chapter_id}:region:{index}");
        if kind.accepts_knowledge_response() {
            if item.purpose != PlanPurpose::Response {
                return Err(format!(
                    "group chapter {chapter_id} cannot carry a knowledge response"
                ));
            }
            templates.push(response_slot(
                slot_id,
                chapter_id,
                kind,
                title,
                region.instruction.as_str(),
            )?);
        } else {
            templates.push(TemplateContent {
                slot_id,
                chapter_id: chapter_id.into(),
                kind,
                text: prescribed_text(input, region)?,
                response_required: false,
                match_query: String::new(),
            });
        }
    }
    Ok(())
}
fn response_slot(
    slot_id: String,
    chapter_id: &str,
    kind: SlotKind,
    title: &str,
    instruction: &str,
) -> Result<TemplateContent, String> {
    let match_query = match instruction.trim() {
        "" => title.trim().to_string(),
        instruction => format!("{}\n{instruction}", title.trim()),
    };
    if match_query.is_empty() {
        return Err(format!("response slot {slot_id} has no match query"));
    }
    Ok(TemplateContent {
        slot_id,
        chapter_id: chapter_id.into(),
        kind,
        text: String::new(),
        response_required: true,
        match_query,
    })
}
fn slot_kind(role: RegionRole) -> SlotKind {
    match role {
        RegionRole::FixedText => SlotKind::FixedText,
        RegionRole::TenderValue => SlotKind::TenderValue,
        RegionRole::Instruction => SlotKind::Instruction,
        RegionRole::BidderBlank => SlotKind::BidderBlank,
        RegionRole::Signature => SlotKind::Signature,
    }
}
fn prescribed_text(input: &FrozenInput, region: &TemplateRegion) -> Result<String, String> {
    if region.role == RegionRole::Instruction && !region.instruction.trim().is_empty() {
        return Ok(region.instruction.clone());
    }
    if let Some(form_id) = region.form_id.as_deref()
        && !region.cells.is_empty()
    {
        let mut parts = Vec::new();
        for cell in &region.cells {
            parts.push(cell_text(input, form_id, cell.row, cell.column)?);
        }
        let text = parts.join("\n");
        if text.trim().is_empty() {
            return Err("prescribed template cell is empty".into());
        }
        return Ok(text);
    }
    let text = span_text(input, &region.source)?;
    if text.trim().is_empty() {
        return Err("prescribed template text is empty".into());
    }
    Ok(text)
}
fn span_text(input: &FrozenInput, span: &Span) -> Result<String, String> {
    if span.view_id.is_some() || span.grid_cell.is_some() {
        return Err("prescribed template text needs a tender text span".into());
    }
    let source = input
        .source_units
        .iter()
        .find(|source| source.source_unit_revision_id == span.source_id)
        .ok_or_else(|| format!("template source {} is missing", span.source_id))?;
    source
        .text
        .get(span.start..span.end)
        .map(str::to_string)
        .ok_or_else(|| format!("template span on {} is out of range", span.source_id))
}
fn cell_text(
    input: &FrozenInput,
    form_id: &str,
    row: usize,
    column: usize,
) -> Result<String, String> {
    let form = input
        .structured_forms
        .iter()
        .find(|form| form["form_definition_revision_id"] == form_id)
        .ok_or_else(|| format!("template form {form_id} is missing"))?;
    let cells = form["definition"]["cells"]
        .as_array()
        .ok_or_else(|| format!("template form {form_id} has no cells"))?;
    cells
        .iter()
        .find(|cell| {
            cell["row"].as_u64() == Some(row as u64)
                && cell["column"].as_u64() == Some(column as u64)
        })
        .and_then(|cell| cell["text"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("template form {form_id} cell {row},{column} is missing"))
}
