//! Outline discovery and semantic checks share the existing durable turn journal.
//! Delivered evidence and completed scanning are deliberately separate receipts.
//! The stored JSON is the current contract only: unknown or missing fields fail.
use super::{agent::Checkpoint, draft::{DraftPlanItem, DraftStatus}, tools, *};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase { #[default] Discover, Outline, Check, Complete }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability { Required, Conditional, NotApplicable }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceImpact { Structure, Format, Content }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceStatus { Resolved, Unresolved }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus { Open, Resolved }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionNeed {
    pub description: String,
    pub kind: String,
    pub applicability: Applicability,
    pub condition: String,
    pub grounds: Vec<Span>,
    pub format_grounds: Vec<Span>,
    pub order_constraints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineReference {
    pub grounds: Vec<Span>,
    pub target_description: String,
    pub target_ids: Vec<String>,
    pub requirement_ids: Vec<String>,
    pub impact: ReferenceImpact,
    pub status: ReferenceStatus,
    pub resolution_grounds: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFragment {
    pub kind: String,
    pub span: Span,
    pub document_id: String,
    pub volume_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineIssue {
    pub code: String,
    pub requirement_ids: Vec<String>,
    pub chapter_ids: Vec<String>,
    pub reference_ids: Vec<String>,
    pub grounds: Vec<Span>,
    pub status: IssueStatus,
    pub resolution_grounds: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineCheck {
    pub scope: String,
    pub fragment_ids: Vec<String>,
    pub snapshot_sha256: String,
    pub finding_ids: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdSequences {
    pub requirement: u64,
    pub reference: u64,
    pub issue: u64,
    pub chapter: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineState {
    pub phase: Phase,
    pub scanned: Coverage,
    pub requirements: BTreeMap<String, SubmissionNeed>,
    pub references: BTreeMap<String, OutlineReference>,
    pub review_fragments: BTreeMap<String, ReviewFragment>,
    pub issues: BTreeMap<String, OutlineIssue>,
    pub checks: BTreeMap<String, OutlineCheck>,
    pub id_sequences: IdSequences,
    pub checked_sha256: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineRun {
    pub phase: Phase,
    pub chunk_plan_sha256: String,
    pub chunk_cursor: usize,
    pub active_check_packet: Option<String>,
    pub repair_signatures: BTreeMap<String, Vec<String>>,
    pub no_progress_rounds: usize,
}

pub fn tree_valid(plan: &[DraftPlanItem]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut orders = BTreeSet::new();
    for node in plan {
        if node.id.is_empty() || node.title.trim().is_empty() || !ids.insert(&node.id) {
            return Err("outline IDs and titles must be nonempty and unique".into());
        }
        if !orders.insert((&node.parent, node.order)) {
            return Err("outline sibling order must be unique".into());
        }
    }
    for node in plan {
        let mut visited = BTreeSet::from([node.id.as_str()]);
        let mut parent = node.parent.as_deref();
        while let Some(id) = parent {
            if !visited.insert(id) { return Err("outline parent cycle".into()); }
            parent = plan.iter().find(|p| p.id == id)
                .ok_or("outline parent missing")?.parent.as_deref();
        }
    }
    Ok(())
}

pub fn source_scanned(input: &FrozenInput, state: &OutlineState, id: &str) -> bool {
    let Some(source) = input.source_units.iter().find(|s| s.source_unit_revision_id == id) else { return false; };
    let text_done = if source.text.is_empty() { state.scanned.metadata.contains_key(id) }
        else { tools::contains(state.scanned.text.get(id), 0, source.text.len()) };
    text_done && input.structured_forms.iter().filter(|f| f["source_unit_revision_id"] == id).all(|f| {
        let Some(form_id) = f["form_definition_revision_id"].as_str() else { return false; };
        super::relations::form_total(&f["definition"]).is_some_and(|n| n == 0 || tools::contains(state.scanned.form_cells.get(form_id), 0, n))
    })
}

pub fn scan_complete(input: &FrozenInput, state: &OutlineState) -> bool {
    !input.source_units.is_empty() && input.source_units.iter()
        .all(|s| source_scanned(input, state, &s.source_unit_revision_id))
}

pub fn snapshot(state: &Checkpoint) -> Result<String, String> {
    digest(&json!({"requirements":state.analysis.outline.requirements,"chapters":state.analysis.draft_plan}))
}

/// Host-derived publish blockers. Open issues that are not in this list do not block.
pub fn blockers(input: &FrozenInput, state: &Checkpoint) -> Vec<&'static str> {
    let flow = &state.analysis.outline;
    let mut codes = Vec::new();
    if !scan_complete(input, flow) { codes.push("B_SCAN_INCOMPLETE"); }
    if tree_valid(&state.analysis.draft_plan).is_err()
        || state.analysis.draft_plan.iter().any(|node| node.grounds.is_empty() && node.requirement_ids.is_empty() && !state.analysis.draft_plan.iter().any(|child| child.parent.as_deref() == Some(&node.id)))
    {
        codes.push("B_TREE_INVALID");
    }
    let unmapped = flow.requirements.iter().any(|(id, need)| {
        need.applicability != Applicability::NotApplicable
            && !state.analysis.draft_plan.iter().any(|node| node.status != DraftStatus::Omitted && node.requirement_ids.contains(id))
    }) || flow.references.values().any(|reference| {
        reference.status == ReferenceStatus::Unresolved
            && reference.impact == ReferenceImpact::Structure
            && reference.target_ids.is_empty()
    });
    if unmapped { codes.push("B_REQUIREMENT_UNMAPPED"); }
    if flow.references.values().any(|reference| reference.status == ReferenceStatus::Resolved && reference.resolution_grounds.is_empty()) {
        codes.push("B_CONFLICT_FALSELY_RESOLVED");
    }
    if flow.references.values().any(|reference| reference.impact == ReferenceImpact::Format && reference.status == ReferenceStatus::Unresolved && reference.target_ids.is_empty()) {
        codes.push("B_FORMAT_EVIDENCE_MISSING");
    }
    codes
}

pub fn checked(input: &FrozenInput, state: &Checkpoint) -> bool {
    state.analysis.outline.phase == Phase::Complete && blockers(input, state).is_empty()
        && snapshot(state).ok().as_ref() == state.analysis.outline.checked_sha256.as_ref()
}

pub fn packet(input: &FrozenInput, state: &Checkpoint, budget: usize) -> Result<Value, String> {
    let chapters: Vec<_> = state.analysis.draft_plan.iter().map(|n| json!({"id":n.id,"parent":n.parent,"order":n.order,"title":n.title,"requirement_ids":n.requirement_ids,"status":n.status,"purpose":n.purpose,"body_status":n.body_status})).collect();
    Ok(json!({"phase":state.analysis.outline.phase,"snapshot_sha256":snapshot(state)?,
        "scanned_sources":input.source_units.iter().filter(|s| source_scanned(input, &state.analysis.outline, &s.source_unit_revision_id)).count(),
        "total_sources":input.source_units.len(),"chapters":tools::bounded_page(&chapters, 0, chapters.len().max(1), budget / 2)?,
        "requirements":tools::bounded_page(&state.analysis.outline.requirements.values().collect::<Vec<_>>(), 0, usize::MAX, budget / 2)?,
        "blockers":blockers(input, state),"instruction":"Use read_outline pagination for remaining saved chapters and requirements. Scanning receipts describe only actual inspected ranges."}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanNeed {
    id: String,
    description: String,
    kind: String,
    applicability: Applicability,
    condition: String,
    grounds: Vec<Span>,
    format_grounds: Vec<Span>,
    order_constraints: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanIssue {
    id: String,
    code: String,
    requirement_ids: Vec<String>,
    chapter_ids: Vec<String>,
    reference_ids: Vec<String>,
    grounds: Vec<Span>,
    status: IssueStatus,
    resolution_grounds: Vec<Span>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanReference {
    id: String,
    grounds: Vec<Span>,
    target_description: String,
    target_ids: Vec<String>,
    requirement_ids: Vec<String>,
    impact: ReferenceImpact,
    status: ReferenceStatus,
    resolution_grounds: Vec<Span>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scan {
    text: BTreeMap<String, Vec<(usize, usize)>>,
    forms: BTreeMap<String, Vec<(usize, usize)>>,
    empty_sources: Vec<String>,
    requirements: Vec<ScanNeed>,
    references: Vec<ScanReference>,
    issues: Vec<ScanIssue>,
}

fn allocate(seq: &mut u64, prefix: &str, id: String, known: bool) -> Result<String, String> {
    if id.is_empty() {
        *seq += 1;
        return Ok(format!("{prefix}-{}", *seq));
    }
    if !known { return Err(format!("unknown {prefix} ID; use empty ID to allocate")); }
    Ok(id)
}

pub fn apply(input: &FrozenInput, state: &mut Checkpoint, name: &str, args: &Value, budget: usize) -> Result<Value, String> {
    if name == "read_outline" {
        let offset = args["offset"].as_u64().ok_or("offset required")? as usize;
        let limit = args["limit"].as_u64().filter(|n| *n > 0).ok_or("positive limit required")? as usize;
        return match args["kind"].as_str() {
            Some("chapters") => tools::bounded_page(&state.analysis.draft_plan, offset, limit, budget),
            Some("requirements") => tools::bounded_page(&state.analysis.outline.requirements.values().collect::<Vec<_>>(), offset, limit, budget),
            Some("references") => tools::bounded_page(&state.analysis.outline.references.values().collect::<Vec<_>>(), offset, limit, budget),
            Some("issues") => tools::bounded_page(&state.analysis.outline.issues.values().collect::<Vec<_>>(), offset, limit, budget),
            _ => Err("kind must be chapters, requirements, references, or issues".into()),
        };
    }
    if name == "finish_outline" {
        if state.analysis.outline.phase != Phase::Outline { return Err("organization phase required".into()); }
        let missing = blockers(input, state);
        if !missing.is_empty() { return Err(missing.join("; ")); }
        state.analysis.outline.phase = Phase::Check;
        state.outline_run.phase = Phase::Check;
        return Ok(json!({"snapshot_sha256":snapshot(state)?,"instruction":"Check the saved requirements against the saved review fragments. Do not rescan the tender."}));
    }
    if name == "submit_outline_scan" {
        if state.analysis.outline.phase != Phase::Discover { return Err("scanning is closed; submit check findings to reopen discovery".into()); }
        let batch: Scan = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
        let mut next = state.analysis.outline.clone();
        for (id, ranges) in batch.text {
            for (start, end) in ranges {
                tools::validate_span(input, state.coverage(), &Span { source_id: id.clone(), start, end, view_id: None, grid_cell: None })?;
                tools::cover(next.scanned.text.entry(id.clone()).or_default(), start, end);
            }
        }
        for (id, ranges) in batch.forms {
            let form = input.structured_forms.iter().find(|f| f["form_definition_revision_id"] == id).ok_or("unknown form")?;
            let total = super::relations::form_total(&form["definition"]).ok_or("invalid form")?;
            for (start, end) in ranges {
                if start >= end || end > total || !tools::contains(state.coverage().form_cells.get(&id), start, end) { return Err("form scan exceeds delivered cells".into()); }
                tools::cover(next.scanned.form_cells.entry(id.clone()).or_default(), start, end);
            }
        }
        for id in batch.empty_sources {
            let source = input.source_units.iter().find(|s| s.source_unit_revision_id == id).ok_or("unknown empty source")?;
            if !source.text.is_empty() { return Err("nonempty source needs explicit scanned ranges".into()); }
            if !state.coverage().text.contains_key(&id) && !state.coverage().views.values().any(|v| v.source_id == id) { return Err("read empty source or its original view before disposition".into()); }
            next.scanned.metadata.entry(id).or_default();
        }
        let mut saved = Vec::new();
        for need in batch.requirements {
            if need.description.trim().is_empty() || need.grounds.is_empty() { return Err("submission requirement needs description and exact grounds".into()); }
            if need.applicability != Applicability::Required && need.condition.trim().is_empty() { return Err("conditional/not-applicable requirement needs its source condition".into()); }
            for span in need.grounds.iter().chain(&need.format_grounds) { tools::validate_span(input, state.coverage(), span)?; }
            let known = next.requirements.contains_key(&need.id);
            let id = allocate(&mut next.id_sequences.requirement, "requirement", need.id, known)?;
            saved.push(id.clone());
            next.requirements.insert(id, SubmissionNeed {
                description: need.description, kind: need.kind, applicability: need.applicability, condition: need.condition,
                grounds: need.grounds, format_grounds: need.format_grounds, order_constraints: need.order_constraints,
            });
        }
        for reference in batch.references {
            for span in reference.grounds.iter().chain(&reference.resolution_grounds) { tools::validate_span(input, state.coverage(), span)?; }
            let known = next.references.contains_key(&reference.id);
            let id = allocate(&mut next.id_sequences.reference, "reference", reference.id, known)?;
            next.references.insert(id, OutlineReference {
                grounds: reference.grounds, target_description: reference.target_description, target_ids: reference.target_ids,
                requirement_ids: reference.requirement_ids, impact: reference.impact, status: reference.status, resolution_grounds: reference.resolution_grounds,
            });
        }
        for issue in batch.issues {
            if issue.status == IssueStatus::Resolved && issue.resolution_grounds.is_empty() { return Err("resolved issue needs resolution grounds".into()); }
            for span in issue.grounds.iter().chain(&issue.resolution_grounds) { tools::validate_span(input, state.coverage(), span)?; }
            let known = next.issues.contains_key(&issue.id);
            let id = allocate(&mut next.id_sequences.issue, "issue", issue.id, known)?;
            next.issues.insert(id, OutlineIssue {
                code: issue.code, requirement_ids: issue.requirement_ids, chapter_ids: issue.chapter_ids, reference_ids: issue.reference_ids,
                grounds: issue.grounds, status: issue.status, resolution_grounds: issue.resolution_grounds,
            });
        }
        next.checked_sha256 = None;
        state.analysis.outline = next;
        return Ok(json!({"saved":saved,"scan_complete":scan_complete(input, &state.analysis.outline)}));
    }
    if name == "submit_outline_check" {
        if state.analysis.outline.phase != Phase::Check { return Err("finish discovery and organization before checking".into()); }
        let expected = snapshot(state)?;
        if args["snapshot_sha256"] != expected { return Err("outline changed; check current snapshot".into()); }
        let issues: Vec<ScanIssue> = serde_json::from_value(args["issues"].clone()).map_err(|e| e.to_string())?;
        for issue in issues {
            if issue.status == IssueStatus::Resolved && issue.resolution_grounds.is_empty() { return Err("resolved issue needs resolution grounds".into()); }
            let known = state.analysis.outline.issues.contains_key(&issue.id);
            let id = allocate(&mut state.analysis.outline.id_sequences.issue, "issue", issue.id, known)?;
            state.analysis.outline.issues.insert(id, OutlineIssue {
                code: issue.code, requirement_ids: issue.requirement_ids, chapter_ids: issue.chapter_ids, reference_ids: issue.reference_ids,
                grounds: issue.grounds, status: issue.status, resolution_grounds: issue.resolution_grounds,
            });
        }
        let blocked = blockers(input, state);
        if !blocked.is_empty() {
            state.outline_run.no_progress_rounds += 1;
            state.analysis.outline.phase = Phase::Discover;
            state.outline_run.phase = Phase::Discover;
            return Ok(json!({"repair_required":true,"blockers":blocked}));
        }
        state.analysis.outline.checked_sha256 = Some(expected);
        state.analysis.outline.phase = Phase::Complete;
        state.outline_run.phase = Phase::Complete;
        return Ok(json!({"checked":true}));
    }
    Err("unknown outline flow tool".into())
}

pub fn schemas() -> Vec<Value> {
    serde_json::from_str(include_str!("../../schemas/tender-outline-flow-v1.schema.json")).expect("outline flow schema")
}
