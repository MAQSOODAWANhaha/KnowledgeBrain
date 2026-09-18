//! Outline discovery and semantic checks share the existing durable turn journal.
//! Delivered evidence and completed scanning are deliberately separate receipts.
//! The stored JSON is the current contract only: unknown or missing fields fail.
use super::{
    agent::Checkpoint,
    draft::{DraftPlanItem, DraftStatus},
    tools, *,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Discover,
    Outline,
    Check,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    Required,
    Conditional,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceImpact {
    Structure,
    Format,
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceStatus {
    Resolved,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    Open,
    Resolved,
}

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
    pub description: String,
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
    pub fragment: u64,
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
            if !visited.insert(id) {
                return Err("outline parent cycle".into());
            }
            parent = plan
                .iter()
                .find(|p| p.id == id)
                .ok_or("outline parent missing")?
                .parent
                .as_deref();
        }
    }
    Ok(())
}

pub fn source_scanned(input: &FrozenInput, state: &OutlineState, id: &str) -> bool {
    let Some(source) = input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == id)
    else {
        return false;
    };
    let text_done = if source.text.is_empty() {
        state.scanned.metadata.contains_key(id)
    } else {
        tools::contains(state.scanned.text.get(id), 0, source.text.len())
    };
    text_done
        && input
            .structured_forms
            .iter()
            .filter(|f| f["source_unit_revision_id"] == id)
            .all(|f| {
                let Some(form_id) = f["form_definition_revision_id"].as_str() else {
                    return false;
                };
                super::relations::form_total(&f["definition"]).is_some_and(|n| {
                    n == 0 || tools::contains(state.scanned.form_cells.get(form_id), 0, n)
                })
            })
}

pub fn scan_complete(input: &FrozenInput, state: &OutlineState) -> bool {
    !input.source_units.is_empty()
        && input.documents.iter().all(|document| {
            !document
                .get("disposition")
                .and_then(Value::as_str)
                .is_some_and(|value| matches!(value, "failed" | "pending" | "unresolved"))
        })
        && [
            ("documents", &input.documents),
            ("document_relations", &input.document_relations),
            ("decisions", &input.decisions),
        ]
        .iter()
        .all(|(kind, values)| {
            values.is_empty() || tools::contains(state.scanned.metadata.get(*kind), 0, values.len())
        })
        && input
            .source_units
            .iter()
            .all(|s| source_scanned(input, state, &s.source_unit_revision_id))
}

fn span_location(span: &Span) -> String {
    if let Some(cell) = &span.grid_cell {
        format!(
            "{} / 表 {} / 行 {} 列 {}",
            span.source_id,
            cell.form_id,
            cell.row + 1,
            cell.column + 1
        )
    } else if let Some(view) = &span.view_id {
        format!("{} / 视图 {}", span.source_id, view)
    } else {
        format!("{}:{}-{}", span.source_id, span.start, span.end)
    }
}

pub fn notices(state: &OutlineState) -> Vec<String> {
    let mut notes = Vec::new();
    for (id, issue) in &state.issues {
        if issue.status == IssueStatus::Open {
            let sources: Vec<_> = issue.grounds.iter().map(span_location).collect();
            notes.push(format!(
                "待核实 [{id}] {}（{}）；依据：{}",
                issue.description,
                issue.code,
                sources.join("、")
            ));
        }
    }
    for (id, reference) in &state.references {
        if reference.status == ReferenceStatus::Unresolved {
            notes.push(format!(
                "待核实引用 [{id}] {}；依据：{}",
                reference.target_description,
                reference
                    .grounds
                    .iter()
                    .map(span_location)
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
    }
    notes
}

pub fn snapshot(state: &Checkpoint) -> Result<String, String> {
    digest(
        &json!({"requirements":state.analysis.outline.requirements,"chapters":state.analysis.draft_plan,"references":state.analysis.outline.references,"issues":state.analysis.outline.issues,"fragments":state.analysis.outline.review_fragments}),
    )
}

fn identified<T: Serialize>(items: &BTreeMap<String, T>) -> Vec<Value> {
    items
        .iter()
        .map(|(id, value)| {
            let mut value = serde_json::to_value(value).expect("outline value serialization");
            value["id"] = json!(id);
            value
        })
        .collect()
}

fn span_text(input: &FrozenInput, span: &Span) -> Result<String, String> {
    if let Some(cell) = &span.grid_cell {
        tools::validate_grid_span(input, span)?;
        let form = input
            .structured_forms
            .iter()
            .find(|form| form["form_definition_revision_id"] == cell.form_id)
            .ok_or("fragment form missing")?;
        return form["definition"]["cells"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["row"] == cell.row && entry["column"] == cell.column)
            .and_then(|entry| entry["text"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| "fragment grid text missing".into());
    }
    if span.view_id.is_some() {
        return Err("check fragments require text or grid evidence".into());
    }
    let source = input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == span.source_id)
        .ok_or_else(|| format!("unknown source {}", span.source_id))?;
    if span.start > span.end || span.end > source.text.len() {
        return Err("fragment span out of range".into());
    }
    if !source.text.is_char_boundary(span.start) || !source.text.is_char_boundary(span.end) {
        return Err("fragment span splits a UTF-8 character".into());
    }
    Ok(source.text[span.start..span.end].to_string())
}

fn volume_ids(state: &Checkpoint) -> BTreeSet<String> {
    let mut volumes = BTreeSet::new();
    for node in &state.analysis.draft_plan {
        if node.parent.is_none() && node.status != DraftStatus::Omitted {
            volumes.insert(node.id.clone());
        }
    }
    volumes
}

fn chapter_subtree<'a>(plan: &'a [DraftPlanItem], root: &str) -> Vec<&'a DraftPlanItem> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_string()];
    while let Some(id) = stack.pop() {
        if let Some(node) = plan.iter().find(|n| n.id == id) {
            out.push(node);
            for child in plan.iter().filter(|n| n.parent.as_deref() == Some(&id)) {
                stack.push(child.id.clone());
            }
        }
    }
    out
}

pub fn packet_snapshot(
    input: &FrozenInput,
    state: &Checkpoint,
    packet_id: &str,
) -> Result<String, String> {
    let check = state
        .analysis
        .outline
        .checks
        .get(packet_id)
        .ok_or_else(|| format!("unknown check packet {packet_id}"))?;
    let mut fragments = BTreeMap::new();
    for id in &check.fragment_ids {
        let fragment = state
            .analysis
            .outline
            .review_fragments
            .get(id)
            .ok_or_else(|| format!("missing fragment {id}"))?;
        fragments.insert(
            id,
            json!({
                "kind": fragment.kind,
                "span": fragment.span,
                "document_id": fragment.document_id,
                "volume_ids": fragment.volume_ids,
                "text_sha256": digest(&span_text(input, &fragment.span)?)?,
            }),
        );
    }
    let chapters: Vec<Value> = if packet_id == "composition" {
        state
            .analysis
            .draft_plan
            .iter()
            .filter(|n| n.parent.is_none())
            .map(|n| {
                json!({
                    "id": n.id,
                    "parent": n.parent,
                    "order": n.order,
                    "title": n.title,
                    "requirement_ids": n.requirement_ids,
                    "purpose": n.purpose,
                    "grounds": n.grounds,
                    "format_refs": n.format_refs,
                    "status": n.status,
                })
            })
            .collect()
    } else if let Some(volume) = packet_id.strip_prefix("volume:") {
        chapter_subtree(&state.analysis.draft_plan, volume)
            .into_iter()
            .map(|n| {
                json!({
                    "id": n.id,
                    "parent": n.parent,
                    "order": n.order,
                    "title": n.title,
                    "requirement_ids": n.requirement_ids,
                    "purpose": n.purpose,
                    "grounds": n.grounds,
                    "format_refs": n.format_refs,
                    "status": n.status,
                })
            })
            .collect()
    } else {
        return Err(format!("unsupported check packet {packet_id}"));
    };
    let mut requirement_ids = BTreeSet::new();
    for chapter in &chapters {
        if let Some(ids) = chapter["requirement_ids"].as_array() {
            for id in ids {
                if let Some(id) = id.as_str() {
                    requirement_ids.insert(id.to_string());
                }
            }
        }
    }
    let requirements: BTreeMap<_, _> = requirement_ids
        .into_iter()
        .filter_map(|id| {
            state
                .analysis
                .outline
                .requirements
                .get(&id)
                .map(|need| (id, need))
        })
        .collect();
    digest(&json!({
        "packet_id": packet_id,
        "scope": check.scope,
        "fragments": fragments,
        "chapters": chapters,
        "requirements": requirements,
        "references": state.analysis.outline.references,
    }))
}

fn global_fragment(fragment: &ReviewFragment) -> bool {
    matches!(fragment.kind.as_str(), "composition" | "order" | "global")
}

fn assign_discovered_fragments(state: &mut Checkpoint) {
    let volumes = volume_ids(state);
    let inferred: Vec<_> = state
        .analysis
        .outline
        .review_fragments
        .iter()
        .filter(|(_, f)| !global_fragment(f) && f.volume_ids.is_empty())
        .map(|(id, fragment)| {
            let targets = volumes
                .iter()
                .filter(|volume| {
                    chapter_subtree(&state.analysis.draft_plan, volume)
                        .iter()
                        .any(|node| {
                            node.grounds
                                .iter()
                                .chain(&node.format_refs)
                                .chain(
                                    node.requirement_ids
                                        .iter()
                                        .filter_map(|id| {
                                            state.analysis.outline.requirements.get(id)
                                        })
                                        .flat_map(|need| {
                                            need.grounds.iter().chain(&need.format_grounds)
                                        }),
                                )
                                .any(|span| {
                                    span.source_id == fragment.span.source_id
                                        && if span.grid_cell.is_some()
                                            || fragment.span.grid_cell.is_some()
                                        {
                                            span.grid_cell == fragment.span.grid_cell
                                        } else {
                                            span.start < fragment.span.end
                                                && fragment.span.start < span.end
                                        }
                                })
                        })
                })
                .cloned()
                .collect::<Vec<_>>();
            (id.clone(), targets)
        })
        .collect();
    for (id, targets) in inferred {
        state
            .analysis
            .outline
            .review_fragments
            .get_mut(&id)
            .unwrap()
            .volume_ids = targets;
    }
}

pub fn compose_check_packets(input: &FrozenInput, state: &mut Checkpoint) {
    assign_discovered_fragments(state);
    // Derive mandatory review evidence from every saved requirement and chapter.
    // Discovery fragments additionally retain relevant clauses with no extracted need.
    state
        .analysis
        .outline
        .review_fragments
        .retain(|id, _| !id.starts_with("basis-"));
    let volumes = volume_ids(state);
    let mut bases = Vec::new();
    for (id, need) in &state.analysis.outline.requirements {
        let affected: Vec<String> = volumes
            .iter()
            .filter(|volume| {
                chapter_subtree(&state.analysis.draft_plan, volume)
                    .iter()
                    .any(|node| node.requirement_ids.contains(id))
            })
            .cloned()
            .collect();
        for span in need.grounds.iter().chain(&need.format_grounds) {
            bases.push((
                span.clone(),
                affected.clone(),
                matches!(need.kind.as_str(), "composition" | "order"),
            ));
        }
    }
    for node in &state.analysis.draft_plan {
        let affected: Vec<String> = volumes
            .iter()
            .filter(|volume| {
                chapter_subtree(&state.analysis.draft_plan, volume)
                    .iter()
                    .any(|child| child.id == node.id)
            })
            .cloned()
            .collect();
        for span in node.grounds.iter().chain(&node.format_refs) {
            bases.push((span.clone(), affected.clone(), node.parent.is_none()));
        }
    }
    for (span, volumes, composition) in bases {
        let id = format!("basis-{}", digest(&span).expect("span serializes"));
        let document_id = input
            .source_units
            .iter()
            .find(|s| s.source_unit_revision_id == span.source_id)
            .map(|s| s.document_id.clone())
            .unwrap_or_default();
        let fragment =
            state
                .analysis
                .outline
                .review_fragments
                .entry(id)
                .or_insert(ReviewFragment {
                    kind: "basis".into(),
                    span,
                    document_id,
                    volume_ids: vec![],
                });
        if composition {
            fragment.kind = "composition".into();
        }
        for volume in volumes {
            if !fragment.volume_ids.contains(&volume) {
                fragment.volume_ids.push(volume);
            }
        }
    }
    let mut checks = BTreeMap::new();
    let composition_fragments: Vec<String> = state
        .analysis
        .outline
        .review_fragments
        .iter()
        .filter(|(_, fragment)| {
            fragment.kind == "global" || fragment.kind == "composition" || fragment.kind == "order"
        })
        .map(|(id, _)| id.clone())
        .collect();
    checks.insert(
        "composition".into(),
        OutlineCheck {
            scope: "composition".into(),
            fragment_ids: composition_fragments,
            snapshot_sha256: String::new(),
            finding_ids: state
                .analysis
                .outline
                .issues
                .iter()
                .filter(|(_, issue)| issue.status == IssueStatus::Open)
                .map(|(id, _)| id.clone())
                .collect(),
            status: "pending".into(),
        },
    );
    for volume in volume_ids(state) {
        let fragment_ids: Vec<String> = state
            .analysis
            .outline
            .review_fragments
            .iter()
            .filter(|(_, fragment)| fragment.volume_ids.contains(&volume))
            .map(|(id, _)| id.clone())
            .collect();
        let id = format!("volume:{volume}");
        checks.insert(
            id.clone(),
            OutlineCheck {
                scope: id,
                fragment_ids,
                snapshot_sha256: String::new(),
                finding_ids: vec![],
                status: "pending".into(),
            },
        );
    }
    state.analysis.outline.checks = checks;
    state.outline_run.active_check_packet = Some("composition".into());
}

pub fn invalidate_checks(state: &mut Checkpoint, composition_changed: bool, volume_ids: &[String]) {
    state.analysis.outline.checked_sha256 = None;
    if composition_changed {
        for check in state.analysis.outline.checks.values_mut() {
            check.status = "stale".into();
            check.snapshot_sha256.clear();
        }
    } else {
        for volume in volume_ids {
            let id = format!("volume:{volume}");
            if let Some(check) = state.analysis.outline.checks.get_mut(&id) {
                check.status = "stale".into();
                check.snapshot_sha256.clear();
            }
            if state
                .analysis
                .outline
                .review_fragments
                .values()
                .any(|f| f.volume_ids.len() > 1 && f.volume_ids.iter().any(|v| v == volume))
                && let Some(check) = state.analysis.outline.checks.get_mut("composition")
            {
                check.status = "stale".into();
                check.snapshot_sha256.clear();
            }
        }
    }
    if state.analysis.outline.phase == Phase::Complete {
        state.analysis.outline.phase = Phase::Check;
        state.outline_run.phase = Phase::Check;
    }
    if state.analysis.outline.phase == Phase::Check {
        state.outline_run.active_check_packet = next_pending_packet(state);
    }
}

fn issue_signature(
    input: &FrozenInput,
    state: &Checkpoint,
    issue: &OutlineIssue,
) -> Result<String, String> {
    let mut grounds = Vec::new();
    for span in &issue.grounds {
        grounds.push(digest(&span_text(input, span)?)?);
    }
    let requirements: Vec<_> = issue
        .requirement_ids
        .iter()
        .filter_map(|id| state.analysis.outline.requirements.get(id))
        .map(|need| {
            json!({
                "description": need.description,
                "applicability": need.applicability,
                "condition": need.condition,
                "grounds": need.grounds,
                "format_grounds": need.format_grounds,
            })
        })
        .collect();
    let chapters: Vec<_> = issue
        .chapter_ids
        .iter()
        .filter_map(|id| state.analysis.draft_plan.iter().find(|n| n.id == *id))
        .map(|node| {
            json!({
                "title": node.title,
                "parent": node.parent,
                "order": node.order,
                "requirement_ids": node.requirement_ids,
                "purpose": node.purpose,
            })
        })
        .collect();
    let references: Vec<_> = issue
        .reference_ids
        .iter()
        .filter_map(|id| state.analysis.outline.references.get(id))
        .map(|reference| {
            json!({
                "status": reference.status,
                "impact": reference.impact,
                "target_ids": reference.target_ids,
                "target_description": reference.target_description,
                "resolution_grounds": reference.resolution_grounds,
            })
        })
        .collect();
    digest(&json!({
        "code": issue.code,
        "grounds": grounds,
        "requirements": requirements,
        "chapters": chapters,
        "references": references,
    }))
}

fn record_repair_signature(run: &mut OutlineRun, issue_id: &str, signature: String) -> bool {
    let history = run
        .repair_signatures
        .entry(issue_id.to_string())
        .or_default();
    let same_twice = history.last() == Some(&signature);
    let aba = history.len() >= 2 && history[history.len() - 2] == signature;
    history.push(signature);
    if history.len() > 2 {
        history.remove(0);
    }
    same_twice || aba
}

fn is_blocker_issue(issue: &OutlineIssue) -> bool {
    matches!(
        issue.code.as_str(),
        "B_SCAN_INCOMPLETE"
            | "B_REQUIREMENT_UNMAPPED"
            | "B_TREE_INVALID"
            | "B_CONFLICT_FALSELY_RESOLVED"
            | "B_FORMAT_EVIDENCE_MISSING"
            | "checklist_gap"
            | "unmapped_requirement"
            | "missing_format"
            | "false_resolution"
    )
}

fn next_pending_packet(state: &Checkpoint) -> Option<String> {
    let mut order: Vec<_> = state.analysis.outline.checks.keys().cloned().collect();
    order.sort();
    if let Some(composition) = order.iter().position(|id| id == "composition") {
        let composition = order.remove(composition);
        order.insert(0, composition);
    }
    order.into_iter().find(|id| {
        state
            .analysis
            .outline
            .checks
            .get(id)
            .is_some_and(|check| check.status != "pass")
    })
}

/// Host-derived publish blockers. Open issues that are not in this list do not block.
pub fn blockers(input: &FrozenInput, state: &Checkpoint) -> Vec<&'static str> {
    let flow = &state.analysis.outline;
    let mut codes = Vec::new();
    if !scan_complete(input, flow) {
        codes.push("B_SCAN_INCOMPLETE");
    }
    if tree_valid(&state.analysis.draft_plan).is_err()
        || state.analysis.draft_plan.iter().any(|node| {
            node.status != DraftStatus::Omitted
                && node.grounds.is_empty()
                && node.requirement_ids.is_empty()
                && !state
                    .analysis
                    .draft_plan
                    .iter()
                    .any(|child| child.parent.as_deref() == Some(&node.id))
        })
    {
        codes.push("B_TREE_INVALID");
    }
    let unmapped = flow.requirements.iter().any(|(id, need)| {
        need.applicability != Applicability::NotApplicable
            && !state.analysis.draft_plan.iter().any(|node| {
                node.status != DraftStatus::Omitted && node.requirement_ids.contains(id)
            })
    }) || flow.references.values().any(|reference| {
        reference.status == ReferenceStatus::Unresolved
            && reference.impact == ReferenceImpact::Structure
    });
    if unmapped {
        codes.push("B_REQUIREMENT_UNMAPPED");
    }
    if flow.references.values().any(|reference| {
        reference.status == ReferenceStatus::Resolved && reference.resolution_grounds.is_empty()
    }) {
        codes.push("B_CONFLICT_FALSELY_RESOLVED");
    }
    let missing_format = flow.requirements.iter().any(|(id, need)| {
        need.applicability != Applicability::NotApplicable
            && ((need.kind == "format" && need.format_grounds.is_empty())
                || need.format_grounds.iter().any(|span| {
                    !state.analysis.draft_plan.iter().any(|node| {
                        node.status != DraftStatus::Omitted
                            && node.requirement_ids.contains(id)
                            && node.format_refs.contains(span)
                    })
                }))
    });
    if missing_format
        || flow.references.values().any(|reference| {
            reference.impact == ReferenceImpact::Format
                && reference.status == ReferenceStatus::Unresolved
        })
    {
        codes.push("B_FORMAT_EVIDENCE_MISSING");
    }
    for issue in flow
        .issues
        .values()
        .filter(|issue| issue.status == IssueStatus::Open)
    {
        let code = match issue.code.as_str() {
            "B_SCAN_INCOMPLETE" => Some("B_SCAN_INCOMPLETE"),
            "B_REQUIREMENT_UNMAPPED" | "checklist_gap" | "unmapped_requirement" => {
                Some("B_REQUIREMENT_UNMAPPED")
            }
            "B_TREE_INVALID" => Some("B_TREE_INVALID"),
            "B_CONFLICT_FALSELY_RESOLVED" | "false_resolution" => {
                Some("B_CONFLICT_FALSELY_RESOLVED")
            }
            "B_FORMAT_EVIDENCE_MISSING" | "missing_format" => Some("B_FORMAT_EVIDENCE_MISSING"),
            _ => None,
        };
        if let Some(code) = code
            && !codes.contains(&code)
        {
            codes.push(code);
        }
    }
    codes
}

fn blocker_details(input: &FrozenInput, state: &Checkpoint) -> Vec<Value> {
    let flow = &state.analysis.outline;
    let mut rows = Vec::new();
    for (id, need) in &flow.requirements {
        if need.applicability == Applicability::NotApplicable {
            continue;
        }
        let chapters: Vec<_> = state
            .analysis
            .draft_plan
            .iter()
            .filter(|node| node.status != DraftStatus::Omitted && node.requirement_ids.contains(id))
            .collect();
        if chapters.is_empty() {
            rows.push(json!({"code":"B_REQUIREMENT_UNMAPPED","requirement_id":id,"action":"map this requirement with put_outline_items"}));
        }
        if need.kind == "format" && need.format_grounds.is_empty() {
            rows.push(json!({"code":"B_FORMAT_EVIDENCE_MISSING","requirement_id":id,"action":"update this requirement's format_grounds with submit_outline_scan, using actual read evidence"}));
        }
        for span in &need.format_grounds {
            if !chapters.iter().any(|node| node.format_refs.contains(span)) {
                rows.push(json!({"code":"B_FORMAT_EVIDENCE_MISSING","requirement_id":id,
                    "chapter_ids":chapters.iter().map(|node| &node.id).collect::<Vec<_>>(),
                    "required_format_ref":span,"action":"attach this exact saved format basis to a mapped chapter"}));
            }
        }
    }
    for (id, reference) in &flow.references {
        if reference.status == ReferenceStatus::Unresolved
            && reference.impact != ReferenceImpact::Content
        {
            rows.push(json!({"code":if reference.impact == ReferenceImpact::Format {"B_FORMAT_EVIDENCE_MISSING"} else {"B_REQUIREMENT_UNMAPPED"},
                "reference_id":id,"action":"resolve the saved reference with submit_outline_scan and actual resolution_grounds; chapter edits alone cannot resolve it"}));
        }
    }
    for (id, issue) in &flow.issues {
        if issue.status == IssueStatus::Open && is_blocker_issue(issue) {
            rows.push(json!({"code":issue.code,"issue_id":id,"action":"repair the issue and record resolution grounds without changing its blocker code"}));
        }
    }
    for code in blockers(input, state) {
        if !rows.iter().any(|row| row["code"] == code) {
            rows.push(json!({"code":code,"action":"inspect saved scan, tree and reference state"}));
        }
    }
    rows
}

pub fn checked(input: &FrozenInput, state: &Checkpoint) -> bool {
    state.analysis.outline.phase == Phase::Complete
        && blockers(input, state).is_empty()
        && !state.analysis.outline.checks.is_empty()
        && state.analysis.outline.checks.iter().all(|(id, check)| {
            check.status == "pass"
                && packet_snapshot(input, state, id).ok().as_ref() == Some(&check.snapshot_sha256)
        })
        && snapshot(state).ok().as_ref() == state.analysis.outline.checked_sha256.as_ref()
}

/// Delivered evidence is not yet a scan conclusion. Project the exact difference
/// after history eviction as well as during ordinary discovery.
fn pending_scan_ranges(state: &Checkpoint) -> Vec<Value> {
    let mut rows = Vec::new();
    for (kind, delivered, scanned) in [
        (
            "text",
            &state.analysis.coverage.text,
            &state.analysis.outline.scanned.text,
        ),
        (
            "forms",
            &state.analysis.coverage.form_cells,
            &state.analysis.outline.scanned.form_cells,
        ),
        (
            "metadata",
            &state.analysis.coverage.metadata,
            &state.analysis.outline.scanned.metadata,
        ),
    ] {
        for (id, ranges) in delivered {
            if kind == "metadata"
                && !["documents", "document_relations", "decisions"].contains(&id.as_str())
            {
                continue;
            }
            for &(start, end) in ranges {
                let mut cursor = start;
                for &(a, b) in scanned.get(id).into_iter().flatten() {
                    if b <= cursor || a >= end {
                        continue;
                    }
                    if a > cursor {
                        rows.push(json!({"kind":kind,"id":id,"start":cursor,"end":a.min(end)}));
                    }
                    cursor = cursor.max(b).min(end);
                }
                if cursor < end {
                    rows.push(json!({"kind":kind,"id":id,"start":cursor,"end":end}));
                }
            }
        }
    }
    rows
}

pub fn packet(input: &FrozenInput, state: &Checkpoint, budget: usize) -> Result<Value, String> {
    let discovering = state.analysis.outline.phase == Phase::Discover;
    let chapters: Vec<_> = state.analysis.draft_plan.iter().map(|n| json!({"id":n.id,"parent":n.parent,"order":n.order,"title":n.title,"requirement_ids":n.requirement_ids,"status":n.status,"purpose":n.purpose,"body_status":n.body_status})).collect();
    let mut out = json!({"phase":state.analysis.outline.phase,"snapshot_sha256":snapshot(state)?,
        "scanned_sources":input.source_units.iter().filter(|s| source_scanned(input, &state.analysis.outline, &s.source_unit_revision_id)).count(),
        "total_sources":input.source_units.len(),"chapters":tools::bounded_page(&chapters, 0, chapters.len().max(1), budget / 2)?,
        "requirement_count":state.analysis.outline.requirements.len(),
        "blockers":blockers(input, state),"instruction":"Use read_outline pagination for remaining saved chapters and requirements. Scanning receipts describe only actual inspected ranges."});
    if !discovering {
        out["blocker_details"] =
            tools::bounded_page(&blocker_details(input, state), 0, usize::MAX, budget / 4)?;
        out["requirements"] = tools::bounded_page(
            &identified(&state.analysis.outline.requirements),
            0,
            usize::MAX,
            budget / 2,
        )?;
    }
    if discovering {
        out["cursor_chunk"] =
            json!(super::draft::outline_chunks(input).get(state.outline_run.chunk_cursor));
        out["pending_scan"] =
            tools::bounded_page(&pending_scan_ranges(state), 0, usize::MAX, budget / 4)?;
        out["discovery_watch"] = json!(state.main_progress.watch);
        out["instruction"] = json!(
            "cursor_chunk is the earliest incomplete accounting range; close its missing scan ranges first. pending_scan lists delivered ranges without scan conclusions, not unread ranges. Submit inspected ranges with submit_outline_scan, including empty requirements when appropriate. Partial table/text conclusions are allowed; do not wait to reread the entire document or table. If original text is no longer visible, reread only the needed pending range. Targeted cross-reference reads remain allowed. Repeating delivered reads is not progress."
        );
    }
    if state.analysis.outline.phase == Phase::Check
        && let Some(packet_id) = &state.outline_run.active_check_packet
    {
        out["packet_id"] = json!(packet_id);
        out["packet_snapshot_sha256"] = json!(packet_snapshot(input, state, packet_id)?);
        out["packets"] = json!(
            state
                .analysis
                .outline
                .checks
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        );
        out["instruction"] = json!(
            "Check the active packet against its saved review fragments only. submit_outline_check with packet_id and packet_snapshot_sha256."
        );
    }
    Ok(out)
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
    description: String,
    requirement_ids: Vec<String>,
    chapter_ids: Vec<String>,
    reference_ids: Vec<String>,
    grounds: Vec<Span>,
    status: IssueStatus,
    resolution_grounds: Vec<Span>,
}

fn validate_issue_update(prior: Option<&OutlineIssue>, next: &ScanIssue) -> Result<(), String> {
    if let Some(prior) = prior.filter(|issue| is_blocker_issue(issue)) {
        if prior.code != next.code
            || !prior
                .requirement_ids
                .iter()
                .all(|id| next.requirement_ids.contains(id))
            || !prior
                .chapter_ids
                .iter()
                .all(|id| next.chapter_ids.contains(id))
            || !prior
                .reference_ids
                .iter()
                .all(|id| next.reference_ids.contains(id))
            || !prior.grounds.iter().all(|span| next.grounds.contains(span))
        {
            return Err("retain the blocker code, affected IDs and original grounds; resolve it with evidence instead of reclassifying it".into());
        }
    }
    Ok(())
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
struct ScanFragment {
    id: String,
    kind: String,
    span: Span,
    document_id: String,
    volume_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scan {
    text: BTreeMap<String, Vec<(usize, usize)>>,
    forms: BTreeMap<String, Vec<(usize, usize)>>,
    empty_sources: Vec<String>,
    metadata: BTreeMap<String, Vec<(usize, usize)>>,
    requirements: Vec<ScanNeed>,
    references: Vec<ScanReference>,
    issues: Vec<ScanIssue>,
    review_fragments: Vec<ScanFragment>,
}

fn allocate(seq: &mut u64, prefix: &str, id: String, known: bool) -> Result<String, String> {
    if id.is_empty() {
        *seq += 1;
        return Ok(format!("{prefix}-{}", *seq));
    }
    if !known {
        return Err(format!("unknown {prefix} ID; use empty ID to allocate"));
    }
    Ok(id)
}

fn fragment_receipt(input: &FrozenInput, state: &Checkpoint, id: &str) -> Result<String, String> {
    let packet = state
        .outline_run
        .active_check_packet
        .as_deref()
        .ok_or("no active check packet")?;
    if !state.analysis.outline.checks[packet]
        .fragment_ids
        .iter()
        .any(|item| item == id)
    {
        return Err("fragment is outside the active check packet".into());
    }
    Ok(format!(
        "outline-check:{}:{id}",
        packet_snapshot(input, state, packet)?
    ))
}

fn record_fragment_delivery(
    input: &FrozenInput,
    state: &mut Checkpoint,
    id: &str,
    start: usize,
    end: usize,
) -> Result<(), String> {
    if state.analysis.outline.phase == Phase::Check {
        let key = fragment_receipt(input, state, id)?;
        tools::cover(
            state.analysis.coverage.metadata.entry(key).or_default(),
            start,
            end.max(1),
        );
    }
    Ok(())
}

pub fn apply(
    input: &FrozenInput,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
    budget: usize,
) -> Result<Value, String> {
    if name == "read_outline_fragment" {
        let id = args["fragment_id"].as_str().ok_or("fragment_id required")?;
        if state.analysis.outline.phase == Phase::Check {
            fragment_receipt(input, state, id)?;
        }
        let fragment = state
            .analysis
            .outline
            .review_fragments
            .get(id)
            .ok_or("unknown fragment")?;
        let text = span_text(input, &fragment.span)?;
        let start = args["start"].as_u64().ok_or("start required")? as usize;
        let max_bytes = args["max_bytes"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("positive max_bytes required")? as usize;
        if start > text.len() || !text.is_char_boundary(start) {
            return Err("invalid fragment byte offset".into());
        }
        let mut end = start.saturating_add(max_bytes).min(text.len());
        loop {
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let result = json!({"fragment_id":id,"span":fragment.span,"start":start,"next":end,"total_bytes":text.len(),"text":&text[start..end]});
            if serde_json::to_vec(&result)
                .map_err(|e| e.to_string())?
                .len()
                <= budget
            {
                if start == end && start < text.len() {
                    return Err("fragment result budget too small".into());
                }
                record_fragment_delivery(input, state, id, start, end)?;
                return Ok(result);
            }
            if end == start {
                return Err("fragment result budget too small".into());
            }
            end = start + (end - start) / 2;
        }
    }
    if name == "read_outline" {
        let offset = args["offset"].as_u64().ok_or("offset required")? as usize;
        let limit = args["limit"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("positive limit required")? as usize;
        return match args["kind"].as_str() {
            Some("pending_scan") if state.analysis.outline.phase == Phase::Discover => {
                tools::bounded_page(&pending_scan_ranges(state), offset, limit, budget)
            }
            Some("chapters") => {
                tools::bounded_page(&state.analysis.draft_plan, offset, limit, budget)
            }
            Some("blockers") => {
                tools::bounded_page(&blocker_details(input, state), offset, limit, budget)
            }
            Some("requirements") => tools::bounded_page(
                &identified(&state.analysis.outline.requirements),
                offset,
                limit,
                budget,
            ),
            Some("references") => tools::bounded_page(
                &identified(&state.analysis.outline.references),
                offset,
                limit,
                budget,
            ),
            Some("issues") => tools::bounded_page(
                &identified(&state.analysis.outline.issues),
                offset,
                limit,
                budget,
            ),
            Some("fragments") => {
                let active = state
                    .outline_run
                    .active_check_packet
                    .as_ref()
                    .and_then(|id| state.analysis.outline.checks.get(id));
                let fragments = state
                    .analysis
                    .outline
                    .review_fragments
                    .iter()
                    .filter(|(id, _)| {
                        state.analysis.outline.phase != Phase::Check
                            || active.is_some_and(|check| check.fragment_ids.contains(id))
                    })
                    .map(|(id, fragment)| {
                        let text = span_text(input, &fragment.span)?;
                        let mut row = json!({"id":id,"kind":fragment.kind,"span":fragment.span,
                            "document_id":fragment.document_id,"volume_ids":fragment.volume_ids,
                            "text":text,"total_bytes":text.len()});
                        if serde_json::to_vec(&row).map_err(|e| e.to_string())?.len() + 128 > budget
                        {
                            row.as_object_mut().unwrap().remove("text");
                            row["instruction"] =
                                json!("Use read_outline_fragment for bounded continuation.");
                        }
                        Ok(row)
                    })
                    .collect::<Result<Vec<Value>, String>>()?;
                let page = tools::bounded_page(&fragments, offset, limit, budget)?;
                for row in page["items"].as_array().unwrap() {
                    if let Some(text) = row["text"].as_str() {
                        record_fragment_delivery(
                            input,
                            state,
                            row["id"].as_str().unwrap(),
                            0,
                            text.len(),
                        )?;
                    }
                }
                Ok(page)
            }

            _ => {
                Err("kind must be chapters, requirements, references, issues, or fragments".into())
            }
        };
    }
    if name == "assign_outline_fragments" {
        if !matches!(
            state.analysis.outline.phase,
            Phase::Outline | Phase::Discover
        ) || !scan_complete(input, &state.analysis.outline)
        {
            return Err("fragment assignment requires completed discovery and organization".into());
        }
        let volumes = volume_ids(state);
        let mut fragments = state.analysis.outline.review_fragments.clone();
        for assignment in args["assignments"]
            .as_array()
            .ok_or("assignments required")?
        {
            let id = assignment["fragment_id"]
                .as_str()
                .ok_or("fragment_id required")?;
            let targets: Vec<String> = serde_json::from_value(assignment["volume_ids"].clone())
                .map_err(|e| e.to_string())?;
            let global = assignment["global"].as_bool().ok_or("global required")?;
            if (global && !targets.is_empty())
                || (!global && targets.is_empty())
                || targets.iter().any(|id| !volumes.contains(id))
            {
                return Err("choose global scope or existing root chapter IDs".into());
            }
            let fragment = fragments.get_mut(id).ok_or("unknown fragment")?;
            if global {
                fragment.kind = "global".into();
            } else if global_fragment(fragment) {
                fragment.kind = "scoped".into();
            }
            fragment.volume_ids = targets;
        }
        state.analysis.outline.review_fragments = fragments;
        invalidate_checks(state, true, &[]);
        return Ok(json!({"assigned":true}));
    }
    if name == "finish_outline" {
        if !matches!(
            state.analysis.outline.phase,
            Phase::Outline | Phase::Discover
        ) {
            return Err("organization or directed discovery phase required".into());
        }
        let missing = blockers(input, state);
        if !missing.is_empty() {
            let details = tools::bounded_page(&blocker_details(input, state), 0, 8, budget / 2)?;
            return Err(format!(
                "{}; read_outline(kind=blockers) for remaining actionable details: {}",
                missing.join("; "),
                details
            ));
        }
        assign_discovered_fragments(state);
        let volumes = volume_ids(state);
        let unassigned: Vec<_> = state
            .analysis
            .outline
            .review_fragments
            .iter()
            .filter(|(id, f)| {
                !id.starts_with("basis-")
                    && !global_fragment(f)
                    && (f.volume_ids.is_empty()
                        || f.volume_ids.iter().any(|id| !volumes.contains(id)))
            })
            .map(|(id, _)| id)
            .collect();
        if !unassigned.is_empty() {
            return Err(format!(
                "assign_outline_fragments requires explicit scope for: {}",
                unassigned
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let previous = state.analysis.outline.checks.clone();
        let repairing = state.outline_run.active_check_packet.clone();
        compose_check_packets(input, state);
        for (id, old) in previous {
            if let Some(check) = state.analysis.outline.checks.get_mut(&id) {
                for finding in &old.finding_ids {
                    if !check.finding_ids.contains(finding) {
                        check.finding_ids.push(finding.clone());
                    }
                }
            }
            if repairing.as_ref() != Some(&id)
                && old.status == "pass"
                && packet_snapshot(input, state, &id).ok().as_ref() == Some(&old.snapshot_sha256)
            {
                state.analysis.outline.checks.insert(id, old);
            }
        }
        state.outline_run.active_check_packet = next_pending_packet(state);
        state.analysis.outline.phase = Phase::Check;
        state.outline_run.phase = Phase::Check;
        let packet_id = state
            .outline_run
            .active_check_packet
            .clone()
            .ok_or("check packet missing")?;
        let packet_sha = packet_snapshot(input, state, &packet_id)?;
        return Ok(json!({
            "snapshot_sha256": snapshot(state)?,
            "packet_id": packet_id,
            "packet_snapshot_sha256": packet_sha,
            "packets": state.analysis.outline.checks.keys().cloned().collect::<Vec<_>>(),
            "instruction":"Check the active packet against its saved review fragments only. submit_outline_check with that packet_id. Do not rescan the tender."
        }));
    }
    if name == "submit_outline_scan" {
        if !matches!(
            state.analysis.outline.phase,
            Phase::Discover | Phase::Outline
        ) {
            return Err("scanning is closed; submit check findings to reopen discovery".into());
        }
        let batch: Scan = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
        if state.analysis.outline.phase == Phase::Outline
            && (!batch.text.is_empty()
                || !batch.forms.is_empty()
                || !batch.metadata.is_empty()
                || !batch.empty_sources.is_empty()
                || batch
                    .requirements
                    .iter()
                    .any(|entry| !state.analysis.outline.requirements.contains_key(&entry.id))
                || batch
                    .references
                    .iter()
                    .any(|entry| !state.analysis.outline.references.contains_key(&entry.id)))
        {
            return Err("organization may update saved requirements and references only; discovery ranges are already closed".into());
        }
        let mut next = state.analysis.outline.clone();
        for (id, ranges) in batch.text {
            for (start, end) in ranges {
                tools::validate_span(
                    input,
                    state.coverage(),
                    &Span {
                        source_id: id.clone(),
                        start,
                        end,
                        view_id: None,
                        grid_cell: None,
                    },
                )?;
                tools::cover(next.scanned.text.entry(id.clone()).or_default(), start, end);
            }
        }
        for (id, ranges) in batch.forms {
            let form = input
                .structured_forms
                .iter()
                .find(|f| f["form_definition_revision_id"] == id)
                .ok_or("unknown form")?;
            let total = super::relations::form_total(&form["definition"]).ok_or("invalid form")?;
            for (start, end) in ranges {
                if start >= end
                    || end > total
                    || !tools::contains(state.coverage().form_cells.get(&id), start, end)
                {
                    return Err("form scan exceeds delivered cells".into());
                }
                tools::cover(
                    next.scanned.form_cells.entry(id.clone()).or_default(),
                    start,
                    end,
                );
            }
        }
        for (kind, ranges) in batch.metadata {
            let total = match kind.as_str() {
                "documents" => input.documents.len(),
                "document_relations" => input.document_relations.len(),
                "decisions" => input.decisions.len(),
                _ => return Err("unknown collection metadata kind".into()),
            };
            for (start, end) in ranges {
                if start >= end
                    || end > total
                    || !tools::contains(state.coverage().metadata.get(&kind), start, end)
                {
                    return Err("metadata scan exceeds delivered ranges".into());
                }
                tools::cover(
                    next.scanned.metadata.entry(kind.clone()).or_default(),
                    start,
                    end,
                );
            }
        }
        for id in batch.empty_sources {
            let source = input
                .source_units
                .iter()
                .find(|s| s.source_unit_revision_id == id)
                .ok_or("unknown empty source")?;
            if !source.text.is_empty() {
                return Err("nonempty source needs explicit scanned ranges".into());
            }
            let has_grid = input
                .structured_forms
                .iter()
                .any(|form| form["source_unit_revision_id"] == id);
            if !has_grid && !state.coverage().views.values().any(|v| v.source_id == id) {
                return Err("empty text is not evidence of a blank page; inspect the original view before disposition".into());
            }
            next.scanned.metadata.entry(id).or_default();
        }
        let mut saved = Vec::new();
        for need in batch.requirements {
            if need.description.trim().is_empty() || need.grounds.is_empty() {
                return Err("submission requirement needs description and exact grounds".into());
            }
            if need.applicability != Applicability::Required && need.condition.trim().is_empty() {
                return Err(
                    "conditional/not-applicable requirement needs its source condition".into(),
                );
            }
            for span in need.grounds.iter().chain(&need.format_grounds) {
                tools::validate_span(input, state.coverage(), span)?;
            }
            let value = SubmissionNeed {
                description: need.description,
                kind: need.kind,
                applicability: need.applicability,
                condition: need.condition,
                grounds: need.grounds,
                format_grounds: need.format_grounds,
                order_constraints: need.order_constraints,
            };
            if need.id.is_empty()
                && let Some((id, _)) = next.requirements.iter().find(|(_, prior)| **prior == value)
            {
                saved.push(id.clone());
                continue;
            }
            let known = next.requirements.contains_key(&need.id);
            let id = allocate(
                &mut next.id_sequences.requirement,
                "requirement",
                need.id,
                known,
            )?;
            saved.push(id.clone());
            next.requirements.insert(id, value);
        }

        for reference in batch.references {
            if reference.grounds.is_empty() || reference.target_description.trim().is_empty() {
                return Err("reference needs description and original grounds".into());
            }
            if reference
                .requirement_ids
                .iter()
                .any(|id| !next.requirements.contains_key(id))
                || reference.target_ids.iter().any(|id| {
                    !input
                        .source_units
                        .iter()
                        .any(|s| s.source_unit_revision_id == *id)
                })
            {
                return Err(
                    "reference targets must be frozen source IDs and requirement IDs must exist"
                        .into(),
                );
            }
            if reference.status == ReferenceStatus::Resolved
                && reference.resolution_grounds.is_empty()
            {
                return Err("resolved reference needs resolution grounds".into());
            }
            for span in reference
                .grounds
                .iter()
                .chain(&reference.resolution_grounds)
            {
                tools::validate_span(input, state.coverage(), span)?;
            }
            let known = next.references.contains_key(&reference.id);
            let id = allocate(
                &mut next.id_sequences.reference,
                "reference",
                reference.id,
                known,
            )?;
            next.references.insert(
                id,
                OutlineReference {
                    grounds: reference.grounds,
                    target_description: reference.target_description,
                    target_ids: reference.target_ids,
                    requirement_ids: reference.requirement_ids,
                    impact: reference.impact,
                    status: reference.status,
                    resolution_grounds: reference.resolution_grounds,
                },
            );
        }
        for issue in batch.issues {
            validate_issue_update(next.issues.get(&issue.id), &issue)?;
            if issue.description.trim().is_empty() {
                return Err("issue description required".into());
            }
            if issue.status == IssueStatus::Resolved && issue.resolution_grounds.is_empty() {
                return Err("resolved issue needs resolution grounds".into());
            }
            for span in issue.grounds.iter().chain(&issue.resolution_grounds) {
                tools::validate_span(input, state.coverage(), span)?;
            }
            let known = next.issues.contains_key(&issue.id);
            let id = allocate(&mut next.id_sequences.issue, "issue", issue.id, known)?;
            next.issues.insert(
                id,
                OutlineIssue {
                    code: issue.code,
                    description: issue.description,
                    requirement_ids: issue.requirement_ids,
                    chapter_ids: issue.chapter_ids,
                    reference_ids: issue.reference_ids,
                    grounds: issue.grounds,
                    status: issue.status,
                    resolution_grounds: issue.resolution_grounds,
                },
            );
        }
        for fragment in batch.review_fragments {
            tools::validate_span(input, state.coverage(), &fragment.span)?;
            span_text(input, &fragment.span)?;
            let known = next.review_fragments.contains_key(&fragment.id);
            let id = allocate(
                &mut next.id_sequences.fragment,
                "fragment",
                fragment.id,
                known,
            )?;
            next.review_fragments.insert(
                id,
                ReviewFragment {
                    kind: fragment.kind,
                    span: fragment.span,
                    document_id: fragment.document_id,
                    volume_ids: fragment.volume_ids,
                },
            );
        }
        next.checked_sha256 = None;
        state.analysis.outline = next;
        invalidate_checks(state, true, &[]);
        return Ok(
            json!({"saved":saved,"scan_complete":scan_complete(input, &state.analysis.outline)}),
        );
    }
    if name == "submit_outline_check" {
        if state.analysis.outline.phase != Phase::Check {
            return Err("finish discovery and organization before checking".into());
        }
        let packet_id = args["packet_id"]
            .as_str()
            .ok_or("packet_id required")?
            .to_string();
        if state.outline_run.active_check_packet.as_ref() != Some(&packet_id) {
            return Err("only the active check packet may be submitted".into());
        }
        if !state.analysis.outline.checks.contains_key(&packet_id) {
            return Err(format!("unknown check packet {packet_id}"));
        }
        let expected = packet_snapshot(input, state, &packet_id)?;
        let provided = args
            .get("packet_snapshot_sha256")
            .ok_or("packet_snapshot_sha256 required")?;
        if provided != &json!(expected) {
            return Err("check packet changed; use the current packet snapshot".into());
        }
        if state.analysis.outline.checks[&packet_id]
            .fragment_ids
            .is_empty()
        {
            return Err(
                "check packet has no original evidence; return to organization and supply grounds"
                    .into(),
            );
        }
        for id in &state.analysis.outline.checks[&packet_id].fragment_ids {
            let key = fragment_receipt(input, state, id)?;
            let text = span_text(input, &state.analysis.outline.review_fragments[id].span)?;
            if !tools::contains(state.coverage().metadata.get(&key), 0, text.len().max(1)) {
                return Err(format!(
                    "check fragment {id} has not been delivered and confirmed in full"
                ));
            }
        }
        let issues: Vec<ScanIssue> =
            serde_json::from_value(args["issues"].clone()).map_err(|e| e.to_string())?;
        let mut finding_ids = state.analysis.outline.checks[&packet_id]
            .finding_ids
            .clone();
        let mut reopen = false;
        let mut blocker_stalled = false;
        for mut issue in issues {
            if issue.description.trim().is_empty() {
                return Err("issue description required".into());
            }
            // A repeated finding is an update, even if the model omitted its ID.
            if issue.id.is_empty()
                && let Some((id, _)) = state.analysis.outline.issues.iter().find(|(_, prior)| {
                    prior.code == issue.code
                        && prior.grounds == issue.grounds
                        && prior.requirement_ids == issue.requirement_ids
                        && prior.chapter_ids == issue.chapter_ids
                        && prior.reference_ids == issue.reference_ids
                })
            {
                issue.id = id.clone();
            }
            for span in issue.grounds.iter().chain(&issue.resolution_grounds) {
                tools::validate_span(input, state.coverage(), span)?;
            }
            if issue.status == IssueStatus::Resolved && issue.resolution_grounds.is_empty() {
                return Err("resolved issue needs resolution grounds".into());
            }
            validate_issue_update(state.analysis.outline.issues.get(&issue.id), &issue)?;
            let known = state.analysis.outline.issues.contains_key(&issue.id);
            let id = allocate(
                &mut state.analysis.outline.id_sequences.issue,
                "issue",
                issue.id,
                known,
            )?;
            let saved = OutlineIssue {
                code: issue.code,
                description: issue.description,
                requirement_ids: issue.requirement_ids,
                chapter_ids: issue.chapter_ids,
                reference_ids: issue.reference_ids,
                grounds: issue.grounds,
                status: issue.status,
                resolution_grounds: issue.resolution_grounds,
            };
            if !finding_ids.contains(&id) {
                finding_ids.push(id.clone());
            }
            state.analysis.outline.issues.insert(id, saved);
        }
        finding_ids.retain(|id| {
            state
                .analysis
                .outline
                .issues
                .get(id)
                .is_some_and(|issue| issue.status == IssueStatus::Open)
        });
        for id in &finding_ids {
            let issue = &state.analysis.outline.issues[id];
            let signature = issue_signature(input, state, issue)?;
            let is_blocker = is_blocker_issue(issue);
            let stalled = record_repair_signature(&mut state.outline_run, id, signature);
            if stalled {
                blocker_stalled |= is_blocker;
            } else {
                reopen = true;
            }
        }
        let blocked = blockers(input, state);
        if !blocked.is_empty() {
            if blocker_stalled {
                state.outline_run.no_progress_rounds = 3;
            }
            state.analysis.outline.phase = Phase::Discover;
            state.outline_run.phase = Phase::Discover;
            if let Some(check) = state.analysis.outline.checks.get_mut(&packet_id) {
                check.status = "fail".into();
                check.snapshot_sha256 = expected;
                check.finding_ids = finding_ids;
            }
            return Ok(json!({"repair_required":true,"blockers":blocked,"packet_id":packet_id}));
        }
        if blocker_stalled {
            state.outline_run.no_progress_rounds = 3;
            if let Some(check) = state.analysis.outline.checks.get_mut(&packet_id) {
                check.status = "fail".into();
                check.snapshot_sha256 = expected;
                check.finding_ids = finding_ids.clone();
            }
            return Ok(json!({
                "repair_required":true,
                "packet_id":packet_id,
                "no_progress":true,
                "finding_ids":finding_ids,
            }));
        }
        if reopen {
            if let Some(check) = state.analysis.outline.checks.get_mut(&packet_id) {
                check.status = "fail".into();
                check.snapshot_sha256 = expected;
                check.finding_ids = finding_ids.clone();
            }
            state.analysis.outline.phase = Phase::Discover;
            state.outline_run.phase = Phase::Discover;
            return Ok(json!({
                "repair_required":true,
                "packet_id":packet_id,
                "finding_ids":finding_ids,
                "instruction":"Directed discovery only for the cited grounds; do not restart from the first window."
            }));
        }
        if let Some(check) = state.analysis.outline.checks.get_mut(&packet_id) {
            check.status = "pass".into();
            check.snapshot_sha256 = expected;
            check.finding_ids = finding_ids;
        }
        state.outline_run.active_check_packet = next_pending_packet(state);
        if let Some(next) = state.outline_run.active_check_packet.clone() {
            let next_sha = packet_snapshot(input, state, &next)?;
            return Ok(json!({
                "checked":false,
                "packet_id":packet_id,
                "next_packet_id":next,
                "packet_snapshot_sha256":next_sha,
            }));
        }
        let global = snapshot(state)?;
        state.analysis.outline.checked_sha256 = Some(global);
        state.analysis.outline.phase = Phase::Complete;
        state.outline_run.phase = Phase::Complete;
        state.outline_run.active_check_packet = None;
        return Ok(json!({"checked":true,"packet_id":packet_id}));
    }
    Err("unknown outline flow tool".into())
}

pub fn schemas() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../schemas/tender-outline-flow-v1.schema.json"
    ))
    .expect("outline flow schema")
}

#[cfg(test)]
pub(super) fn read_check_evidence(input: &FrozenInput, state: &mut Checkpoint) {
    let packet = state.outline_run.active_check_packet.clone().unwrap();
    for id in state.analysis.outline.checks[&packet].fragment_ids.clone() {
        let mut start = 0;
        loop {
            let page = apply(
                input,
                state,
                "read_outline_fragment",
                &json!({"fragment_id":id,"start":start,"max_bytes":1000}),
                4096,
            )
            .unwrap();
            start = page["next"].as_u64().unwrap() as usize;
            if start >= page["total_bytes"].as_u64().unwrap() as usize {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_contract_round_trips_and_legacy_fields_are_rejected() {
        let state = OutlineState::default();
        let json = serde_json::to_value(&state).unwrap();
        let back: OutlineState = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.phase, Phase::Discover);
        assert!(json.get("next_requirement").is_none());
        assert!(json["issues"].is_object());
        let mut legacy = json;
        legacy["next_requirement"] = serde_json::json!(1);
        legacy["issues"] = serde_json::json!(["old string issue"]);
        assert!(serde_json::from_value::<OutlineState>(legacy).is_err());
    }

    #[test]
    fn fixture_round_trips_outline_state_and_rejects_legacy_keys() {
        let raw: Value = serde_json::from_str(include_str!(
            "../../schemas/fixtures/outline-checkpoint-v1.json"
        ))
        .unwrap();
        let outline: OutlineState = serde_json::from_value(raw["outline"].clone()).unwrap();
        let plan: Vec<crate::tender_analysis::draft::DraftPlanItem> =
            serde_json::from_value(raw["draft_plan"].clone()).unwrap();
        assert_eq!(outline.phase, Phase::Outline);
        assert_eq!(outline.id_sequences.chapter, 1);
        assert_eq!(
            plan[0].purpose,
            crate::tender_analysis::draft::ChapterPurpose::Response
        );
        assert_eq!(plan[0].requirement_ids, vec!["requirement-1".to_string()]);
        let outline_again: OutlineState =
            serde_json::from_value(serde_json::to_value(&outline).unwrap()).unwrap();
        let plan_again: Vec<crate::tender_analysis::draft::DraftPlanItem> =
            serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
        assert_eq!(outline_again.requirements.len(), 1);
        assert_eq!(plan_again[0].id, "chapter-1");

        let mut legacy_outline = raw["outline"].clone();
        legacy_outline["next_requirement"] = json!(1);
        assert!(serde_json::from_value::<OutlineState>(legacy_outline).is_err());
        let mut string_issues = raw["outline"].clone();
        string_issues["issues"] = json!(["legacy string issue"]);
        assert!(serde_json::from_value::<OutlineState>(string_issues).is_err());
        let mut unresolved = raw["outline"].clone();
        unresolved["unresolved_references"] = json!([]);
        assert!(serde_json::from_value::<OutlineState>(unresolved).is_err());
    }

    fn span(source: &str, end: usize) -> Span {
        Span {
            source_id: source.into(),
            start: 0,
            end,
            view_id: None,
            grid_cell: None,
        }
    }

    fn checked_state(input: &FrozenInput) -> Checkpoint {
        let mut state = Checkpoint {
            journal: Default::default(),
            input_sha256: digest(input).unwrap(),
            config_sha256: "c".repeat(64),
            turn: 0,
            tool_calls: 0,
            read_bytes: 0,
            review_rounds: 0,
            role: crate::tender_analysis::agent::Role::Main,
            analysis: Analysis::default(),
            review: None,
            review_draft: Default::default(),
            source_review: None,
            repair: Default::default(),
            dispatch: Default::default(),
            reviewer_coverage: Coverage::default(),
            pending_coverage: None,
            transcript: vec![],
            main_progress: Default::default(),
            reviewer_progress: Default::default(),
            main_work: None,
            reviewer_work: None,
            done: false,
            source_views: Default::default(),
            draft_stage: Default::default(),
            draft_active_id: None,
            draft_outline_gaps: None,
            draft_outline_stalls: 0,
            draft_outline_window: 0,
            draft_degraded: vec![],
            draft_stopped: false,
            draft_compile_object_id: None,
            draft_docx_base64: None,
            outline_config_sha256: None,
            fill_config_sha256: None,
            outline_run: Default::default(),
        };
        let source = &input.source_units[0];
        tools::cover(
            state
                .analysis
                .outline
                .scanned
                .text
                .entry(source.source_unit_revision_id.clone())
                .or_default(),
            0,
            source.text.len(),
        );
        state.analysis.outline.requirements.insert(
            "requirement-1".into(),
            SubmissionNeed {
                description: "投标函".into(),
                kind: "composition".into(),
                applicability: Applicability::Required,
                condition: String::new(),
                grounds: vec![span(&source.source_unit_revision_id, source.text.len())],
                format_grounds: vec![],
                order_constraints: vec![],
            },
        );
        state
            .analysis
            .draft_plan
            .push(crate::tender_analysis::draft::DraftPlanItem {
                grounds: vec![span(&source.source_unit_revision_id, source.text.len())],
                requirement_ids: vec!["requirement-1".into()],
                id: "chapter-1".into(),
                parent: None,
                order: 0,
                title: "投标函".into(),
                prescribed: true,
                source_ids: vec![source.source_unit_revision_id.clone()],
                windows: vec![vec![source.source_unit_revision_id.clone()]],
                window_index: 0,
                template_id: None,
                status: crate::tender_analysis::draft::DraftStatus::Pending,
                purpose: crate::tender_analysis::draft::ChapterPurpose::Response,
                format_refs: vec![],
                body_status: crate::tender_analysis::draft::BodyStatus::Empty,
                omit_reason: None,
                preserved: vec![],
            });
        state.analysis.outline.phase = Phase::Complete;
        state.analysis.outline.checked_sha256 = Some(snapshot(&state).unwrap());
        compose_check_packets(&input, &mut state);
        for id in state
            .analysis
            .outline
            .checks
            .keys()
            .cloned()
            .collect::<Vec<_>>()
        {
            let sha = packet_snapshot(input, &state, &id).unwrap_or_default();
            if let Some(check) = state.analysis.outline.checks.get_mut(&id) {
                check.status = "pass".into();
                check.snapshot_sha256 = sha;
            }
        }
        state.outline_run.active_check_packet = None;
        state.analysis.outline.checked_sha256 = Some(snapshot(&state).unwrap());
        state
    }

    #[test]
    fn a08_content_only_unresolved_reference_does_not_block() {
        let input = FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "d".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "组成：一、投标函".into(),
                locator: json!({}),
                ordinal: 0,
            }],
            structured_forms: vec![],
            decisions: vec![],
        };
        let mut state = checked_state(&input);
        state.analysis.outline.references.insert(
            "reference-1".into(),
            OutlineReference {
                grounds: vec![span("source", 6)],
                target_description: "外部标准 GB/T 1".into(),
                target_ids: vec![],
                requirement_ids: vec!["requirement-1".into()],
                impact: ReferenceImpact::Content,
                status: ReferenceStatus::Unresolved,
                resolution_grounds: vec![],
            },
        );
        assert!(
            blockers(&input, &state).is_empty(),
            "{:?}",
            blockers(&input, &state)
        );
        assert!(
            !checked(&input, &state),
            "changed references invalidate prior approval"
        );
    }

    #[test]
    fn a08_missing_qualification_attachment_blocks_unmapped() {
        let input = FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "d".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "资格材料见附件清单".into(),
                locator: json!({}),
                ordinal: 0,
            }],
            structured_forms: vec![],
            decisions: vec![],
        };
        let mut state = checked_state(&input);
        state.analysis.outline.references.insert(
            "reference-1".into(),
            OutlineReference {
                grounds: vec![span("source", 8)],
                target_description: "附件列明的全部资格材料".into(),
                target_ids: vec![],
                requirement_ids: vec!["requirement-1".into()],
                impact: ReferenceImpact::Structure,
                status: ReferenceStatus::Unresolved,
                resolution_grounds: vec![],
            },
        );
        assert!(
            blockers(&input, &state).contains(&"B_REQUIREMENT_UNMAPPED"),
            "{:?}",
            blockers(&input, &state)
        );
    }

    #[test]
    fn a08_missing_pricing_format_blocks() {
        let input = FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "d".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "报价表见附件".into(),
                locator: json!({}),
                ordinal: 0,
            }],
            structured_forms: vec![],
            decisions: vec![],
        };
        let mut state = checked_state(&input);
        state.analysis.outline.references.insert(
            "reference-1".into(),
            OutlineReference {
                grounds: vec![span("source", 6)],
                target_description: "指定报价表".into(),
                target_ids: vec![],
                requirement_ids: vec!["requirement-1".into()],
                impact: ReferenceImpact::Format,
                status: ReferenceStatus::Unresolved,
                resolution_grounds: vec![],
            },
        );
        assert!(
            blockers(&input, &state).contains(&"B_FORMAT_EVIDENCE_MISSING"),
            "{:?}",
            blockers(&input, &state)
        );
    }

    #[test]
    fn check_phase_rejects_rescans_and_reads_saved_fragments() {
        let input = FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "d".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "投标函格式".into(),
                locator: json!({}),
                ordinal: 0,
            }],
            structured_forms: vec![],
            decisions: vec![],
        };
        let mut state = checked_state(&input);
        state.analysis.draft_plan.clear();
        state.analysis.outline.requirements.clear();
        state.analysis.outline.checks.clear();
        state.analysis.outline.checked_sha256 = None;
        state.analysis.outline.phase = Phase::Discover;
        state.outline_run.phase = Phase::Discover;
        tools::cover(
            state
                .analysis
                .coverage
                .text
                .entry("source".into())
                .or_default(),
            0,
            input.source_units[0].text.len(),
        );
        let scan = json!({
            "text":{"source":[[0,input.source_units[0].text.len()]]},
            "forms":{},
            "metadata":{},"empty_sources":[],
            "requirements":[{
                "id":"","description":"投标函","kind":"submission","applicability":"required","condition":"",
                "grounds":[{"source_id":"source","start":0,"end":input.source_units[0].text.len(),"view_id":null,"grid_cell":null}],
                "format_grounds":[],"order_constraints":[]
            }],
            "references":[],
            "issues":[],
            "review_fragments":[{
                "id":"","kind":"composition","document_id":"doc","volume_ids":[],
                "span":{"source_id":"source","start":0,"end":input.source_units[0].text.len(),"view_id":null,"grid_cell":null}
            }]
        });
        apply(&input, &mut state, "submit_outline_scan", &scan, 64_000).unwrap();
        assert!(
            state
                .analysis
                .outline
                .review_fragments
                .values()
                .any(|f| f.kind == "composition")
        );
        state.analysis.outline.phase = Phase::Outline;
        state.outline_run.phase = Phase::Outline;
        state
            .analysis
            .draft_plan
            .push(crate::tender_analysis::draft::DraftPlanItem {
                grounds: vec![span("source", input.source_units[0].text.len())],
                requirement_ids: state
                    .analysis
                    .outline
                    .requirements
                    .keys()
                    .cloned()
                    .collect(),
                id: "chapter-1".into(),
                parent: None,
                order: 0,
                title: "投标函".into(),
                prescribed: true,
                source_ids: vec!["source".into()],
                windows: vec![],
                window_index: 0,
                template_id: None,
                status: DraftStatus::Pending,
                purpose: crate::tender_analysis::draft::ChapterPurpose::Response,
                format_refs: vec![],
                body_status: crate::tender_analysis::draft::BodyStatus::Empty,
                omit_reason: None,
                preserved: vec![],
            });
        let finished = apply(&input, &mut state, "finish_outline", &json!({}), 64_000).unwrap();
        assert_eq!(finished["packet_id"], "composition");
        assert!(state.analysis.outline.checks.contains_key("composition"));
        let page = apply(
            &input,
            &mut state,
            "read_outline",
            &json!({"kind":"fragments","offset":0,"limit":10}),
            64_000,
        )
        .unwrap();
        assert!(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["kind"] == "composition")
        );
        assert!(
            apply(&input, &mut state, "submit_outline_scan", &scan, 64_000).is_err(),
            "check phase must not accept a new scan"
        );
    }

    #[test]
    fn same_repair_signature_twice_is_no_progress() {
        let mut run = OutlineRun::default();
        assert!(!record_repair_signature(
            &mut run,
            "issue-1",
            "sig-a".into()
        ));
        assert!(record_repair_signature(&mut run, "issue-1", "sig-a".into()));
        assert!(!record_repair_signature(
            &mut run,
            "issue-1",
            "sig-b".into()
        ));
        assert!(
            record_repair_signature(&mut run, "issue-1", "sig-a".into()),
            "A→B→A is no progress"
        );
    }

    #[test]
    fn checked_requires_all_packets_current() {
        let input = FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "d".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "组成：一、投标函".into(),
                locator: json!({}),
                ordinal: 0,
            }],
            structured_forms: vec![],
            decisions: vec![],
        };
        let mut state = checked_state(&input);
        assert!(checked(&input, &state));
        state
            .analysis
            .outline
            .checks
            .get_mut("composition")
            .unwrap()
            .status = "stale".into();
        assert!(!checked(&input, &state));
    }
    fn review_input() -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "p".into(),
            document_set_id: "d".into(),
            documents: vec![],
            document_relations: vec![],
            decisions: vec![],
            structured_forms: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "投标函与报价表".into(),
                locator: json!({}),
                ordinal: 0,
            }],
        }
    }

    #[test]
    fn recall_retains_ids_and_check_fragments_include_original_text() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.review_fragments.insert(
            "fragment-1".into(),
            ReviewFragment {
                kind: "composition".into(),
                span: span("source", input.source_units[0].text.len()),
                document_id: "doc".into(),
                volume_ids: vec![],
            },
        );
        compose_check_packets(&input, &mut state);
        state.analysis.outline.phase = Phase::Check;
        let needs = apply(
            &input,
            &mut state,
            "read_outline",
            &json!({"kind":"requirements","offset":0,"limit":10}),
            8000,
        )
        .unwrap();
        assert!(needs.to_string().contains("requirement-1"));
        let fragments = apply(
            &input,
            &mut state,
            "read_outline",
            &json!({"kind":"fragments","offset":0,"limit":10}),
            8000,
        )
        .unwrap();
        assert!(fragments.to_string().contains("投标函与报价表"));
        assert!(fragments.to_string().contains("fragment-1"));
    }

    #[test]
    fn open_blocker_cannot_disappear_in_empty_check_submission() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.issues.insert(
            "issue-1".into(),
            OutlineIssue {
                code: "B_REQUIREMENT_UNMAPPED".into(),
                description: "缺少应提交的资格附件".into(),
                requirement_ids: vec![],
                chapter_ids: vec![],
                reference_ids: vec![],
                grounds: vec![],
                status: IssueStatus::Open,
                resolution_grounds: vec![],
            },
        );
        compose_check_packets(&input, &mut state);
        state.analysis.outline.phase = Phase::Check;
        read_check_evidence(&input, &mut state);
        let sha = packet_snapshot(&input, &state, "composition").unwrap();
        let result = apply(
            &input,
            &mut state,
            "submit_outline_check",
            &json!({"packet_id":"composition","packet_snapshot_sha256":sha,"issues":[]}),
            8000,
        )
        .unwrap();
        assert_eq!(result["repair_required"], true);
        super::super::draft::after_batch(&input, &mut state, false, false).unwrap();
        assert_eq!(state.analysis.outline.phase, Phase::Discover);
        assert!(!checked(&input, &state));
    }

    #[test]
    fn stalled_warning_survives_omission_and_finishes_check() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.issues.insert(
            "issue-1".into(),
            OutlineIssue {
                code: "W_CONTENT_REFERENCE".into(),
                description: "外部附件暂未提供".into(),
                requirement_ids: vec![],
                chapter_ids: vec![],
                reference_ids: vec![],
                grounds: vec![],
                status: IssueStatus::Open,
                resolution_grounds: vec![],
            },
        );
        for round in 0..2 {
            compose_check_packets(&input, &mut state);
            state.analysis.outline.phase = Phase::Check;
            state.outline_run.active_check_packet = Some("composition".into());
            read_check_evidence(&input, &mut state);
            let sha = packet_snapshot(&input, &state, "composition").unwrap();
            let result = apply(
                &input,
                &mut state,
                "submit_outline_check",
                &json!({"packet_id":"composition","packet_snapshot_sha256":sha,"issues":[]}),
                8000,
            )
            .unwrap();
            if round == 0 {
                assert_eq!(result["repair_required"], true);
            } else {
                assert_eq!(state.analysis.outline.checks["composition"].status, "pass");
            }
        }
        assert_eq!(
            state.analysis.outline.issues["issue-1"].status,
            IssueStatus::Open
        );
        assert!(
            notices(&state.analysis.outline)
                .join(" ")
                .contains("外部附件暂未提供")
        );
    }

    #[test]
    fn scan_schema_accepts_evidence_bearing_records() {
        let schema = schemas()
            .into_iter()
            .find(|s| s["function"]["name"] == "submit_outline_scan")
            .unwrap();
        let validator = jsonschema::JSONSchema::compile(&schema["function"]["parameters"]).unwrap();
        let ground = json!({"source_id":"s","start":0,"end":12,"view_id":null,"grid_cell":null});
        let payload = json!({
            "text":{"s":[[0,12]]},"forms":{},"metadata":{},"empty_sources":[],
            "requirements":[],
            "issues":[{"id":"","code":"W_CONTENT_REFERENCE","description":"附件缺失",
                "requirement_ids":[],"chapter_ids":[],"reference_ids":[],"grounds":[ground.clone()],
                "status":"open","resolution_grounds":[]}],
            "references":[{"id":"","grounds":[ground.clone()],"target_description":"附件一",
                "target_ids":[],"requirement_ids":[],"impact":"content","status":"unresolved","resolution_grounds":[]}],
            "review_fragments":[{"id":"","kind":"composition","span":ground,"document_id":"doc","volume_ids":[]}]
        });
        if let Err(errors) = validator.validate(&payload) {
            panic!(
                "{}",
                errors.map(|e| e.to_string()).collect::<Vec<_>>().join("; ")
            );
        }
        let mut invalid = payload;
        invalid["issues"][0]
            .as_object_mut()
            .unwrap()
            .remove("description");
        assert!(!validator.is_valid(&invalid));
    }

    #[test]
    fn failed_document_cannot_become_complete_by_reading_metadata() {
        let mut input = review_input();
        let mut state = checked_state(&input);
        input
            .documents
            .push(json!({"document_id":"missing","disposition":"failed"}));
        state
            .analysis
            .outline
            .scanned
            .metadata
            .insert("documents".into(), vec![(0, input.documents.len())]);
        assert!(!scan_complete(&input, &state.analysis.outline));
        assert!(blockers(&input, &state).contains(&"B_SCAN_INCOMPLETE"));
    }

    #[test]
    fn long_fragment_requires_all_current_packet_ranges_before_checking() {
        let mut input = review_input();
        input.source_units[0].text = "技术响应内容".repeat(2000);
        let mut state = checked_state(&input);
        state.analysis.outline.review_fragments.insert(
            "long".into(),
            ReviewFragment {
                kind: "composition".into(),
                span: span("source", input.source_units[0].text.len()),
                document_id: "doc".into(),
                volume_ids: vec![],
            },
        );
        compose_check_packets(&input, &mut state);
        state.analysis.outline.phase = Phase::Check;
        state.outline_run.active_check_packet = Some("composition".into());
        let check = json!({"packet_id":"composition","packet_snapshot_sha256":packet_snapshot(&input,&state,"composition").unwrap(),"issues":[]});
        assert!(
            apply(&input, &mut state, "submit_outline_check", &check, 2048)
                .unwrap_err()
                .contains("not been delivered")
        );
        let index = apply(
            &input,
            &mut state,
            "read_outline",
            &json!({"kind":"fragments","offset":0,"limit":10}),
            2048,
        )
        .unwrap();
        assert!(index["items"][0].get("text").is_none());
        let mut start = 0;
        while start < input.source_units[0].text.len() {
            let page = apply(
                &input,
                &mut state,
                "read_outline_fragment",
                &json!({"fragment_id":"long","start":start,"max_bytes":1000}),
                2048,
            )
            .unwrap();
            let next = page["next"].as_u64().unwrap() as usize;
            assert!(next > start);
            start = next;
        }
        read_check_evidence(&input, &mut state);
        assert!(apply(&input, &mut state, "submit_outline_check", &check, 2048).is_ok());
    }

    #[test]
    fn collection_metadata_is_part_of_scan_completion() {
        let mut input = review_input();
        let mut state = checked_state(&input);
        input
            .decisions
            .push(json!({"description":"补遗替代原报价格式"}));
        assert!(!scan_complete(&input, &state.analysis.outline));
        tools::cover(
            state
                .analysis
                .outline
                .scanned
                .metadata
                .entry("decisions".into())
                .or_default(),
            0,
            1,
        );
        assert!(scan_complete(&input, &state.analysis.outline));
    }
    #[test]
    fn missing_discovery_fragments_cannot_bypass_requirement_evidence() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.review_fragments.clear();
        compose_check_packets(&input, &mut state);
        state.analysis.outline.phase = Phase::Check;
        let check = json!({"packet_id":"composition", "packet_snapshot_sha256":
            packet_snapshot(&input, &state, "composition").unwrap(), "issues":[]});
        assert!(
            !state.analysis.outline.checks["composition"]
                .fragment_ids
                .is_empty()
        );
        assert!(
            apply(&input, &mut state, "submit_outline_check", &check, 8000)
                .unwrap_err()
                .contains("not been delivered")
        );
        read_check_evidence(&input, &mut state);
        assert!(apply(&input, &mut state, "submit_outline_check", &check, 8000).is_ok());
    }

    #[test]
    fn blocker_cannot_be_reclassified_without_resolution() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.phase = Phase::Discover;
        state.analysis.outline.issues.insert(
            "issue-1".into(),
            OutlineIssue {
                code: "B_REQUIREMENT_UNMAPPED".into(),
                description: "未列出必需附件".into(),
                requirement_ids: vec![],
                chapter_ids: vec![],
                reference_ids: vec![],
                grounds: vec![],
                status: IssueStatus::Open,
                resolution_grounds: vec![],
            },
        );
        assert!(blockers(&input, &state).contains(&"B_REQUIREMENT_UNMAPPED"));
        apply(&input,&mut state,"submit_outline_scan",&json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[],"references":[],"review_fragments":[],"issues":[{"id":"issue-1","code":"W_CONTENT_REFERENCE","description":"未列出必需附件","requirement_ids":[],"chapter_ids":[],"reference_ids":[],"grounds":[],"status":"open","resolution_grounds":[]}]}),16000).unwrap_err();
        assert!(blockers(&input, &state).contains(&"B_REQUIREMENT_UNMAPPED"));
    }
    #[test]
    fn unassigned_technical_fragment_follows_volume_grounds() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.review_fragments.insert(
            "fragment-1".into(),
            ReviewFragment {
                kind: "technical_response".into(),
                span: span("source", 3),
                document_id: "doc".into(),
                volume_ids: vec![],
            },
        );
        compose_check_packets(&input, &mut state);
        assert!(
            !state.analysis.outline.checks["composition"]
                .fragment_ids
                .contains(&"fragment-1".to_string())
        );
        assert!(
            state.analysis.outline.checks["volume:chapter-1"]
                .fragment_ids
                .contains(&"fragment-1".to_string())
        );
    }

    #[test]
    fn organization_repairs_saved_reference_and_requirement_without_reopening_scan() {
        let input = review_input();
        let mut state = checked_state(&input);
        state.analysis.outline.phase = Phase::Outline;
        state.outline_run.phase = Phase::Outline;
        state.analysis.outline.references.insert(
            "reference-1".into(),
            OutlineReference {
                grounds: vec![span("source", 3)],
                target_description: "组成条款".into(),
                target_ids: vec![],
                requirement_ids: vec!["requirement-1".into()],
                impact: ReferenceImpact::Structure,
                status: ReferenceStatus::Unresolved,
                resolution_grounds: vec![],
            },
        );
        let details = apply(
            &input,
            &mut state,
            "read_outline",
            &json!({"kind":"blockers","offset":0,"limit":20}),
            16000,
        )
        .unwrap();
        assert!(
            details["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["reference_id"] == "reference-1")
        );
        let scan_before = serde_json::to_value(&state.analysis.outline.scanned).unwrap();
        let mut reference =
            serde_json::to_value(&state.analysis.outline.references["reference-1"]).unwrap();
        reference["id"] = json!("reference-1");
        reference["status"] = json!("resolved");
        reference["target_ids"] = json!(["source"]);
        reference["resolution_grounds"] = json!([span("source", 3)]);
        let mut request = json!({"text":{},"forms":{},"metadata":{},"empty_sources":[],"requirements":[],"references":[reference],"issues":[],"review_fragments":[]});
        assert!(apply(&input, &mut state, "submit_outline_scan", &request, 16000).is_err());
        tools::cover(
            state
                .analysis
                .coverage
                .text
                .entry("source".into())
                .or_default(),
            0,
            3,
        );
        apply(&input, &mut state, "submit_outline_scan", &request, 16000).unwrap();
        assert!(!blockers(&input, &state).contains(&"B_REQUIREMENT_UNMAPPED"));
        assert_eq!(
            serde_json::to_value(&state.analysis.outline.scanned).unwrap(),
            scan_before
        );
        request["text"] = json!({"source":[[0,3]]});
        assert!(apply(&input, &mut state, "submit_outline_scan", &request, 16000).is_err());
        state.analysis.outline.phase = Phase::Check;
        request["text"] = json!({});
        assert!(apply(&input, &mut state, "submit_outline_scan", &request, 16000).is_err());
    }
}
