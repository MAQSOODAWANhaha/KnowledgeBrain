//! Independent source-to-result judgments. Tasks are derived from immutable
//! source geometry; neither reading receipts nor candidate checks finish them.
use super::agent::context::WorkStatus;
use super::agent::{Checkpoint, Config, Role, WorkState, context, repair, validate_finding};
use super::*;
use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch, Recovery};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) mod evidence_candidates;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Region {
    Text {
        start: usize,
        end: usize,
    },
    Grid {
        form_id: String,
        start: usize,
        end: usize,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub source_id: String,
    pub region: Region,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    pub source_ids: BTreeSet<String>,
    pub references: BTreeSet<String>,
    /// Unscoped candidate queries include negative results. Any semantic edit
    /// can change their answer, even if no old candidate ID was returned.
    pub global: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentStatus {
    Checked,
    Findings,
    NeedsEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryStatus {
    Complete,
    Continuation,
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub state: BoundaryStatus,
    pub reason: String,
    pub sources: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundaries {
    pub before: Boundary,
    pub after: Boundary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequest {
    pub question: String,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateMapping {
    pub template_id: String,
    pub requirement_ids: Vec<String>,
    pub relation_ids: Vec<String>,
    pub finding_ids: Vec<String>,
    pub reason: String,
    pub sources: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipCheck {
    pub record_id: String,
    pub status: RelationshipStatus,
    pub related_record_ids: Vec<String>,
    pub relation_ids: Vec<String>,
    pub unresolved_record_ids: Vec<String>,
    pub finding_ids: Vec<String>,
    pub reason: String,
    pub sources: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipStatus {
    Resolved,
    NotRequired,
    SourceLimited,
    Findings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgment {
    pub task_id: String,
    pub expected_version: String,
    pub status: JudgmentStatus,
    pub summary: String,
    pub sources: Vec<Span>,
    pub candidate_refs: Vec<String>,
    pub template_mappings: Vec<TemplateMapping>,
    #[serde(default)]
    pub relationship_checks: Vec<RelationshipCheck>,
    pub boundaries: Boundaries,
    pub finding_ids: Vec<String>,
    pub evidence_requests: Vec<EvidenceRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub judgment: Judgment,
    pub dependencies: Dependencies,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub results: BTreeMap<String, Receipt>,
    pub active_task: Option<String>,
    pub dependencies: Dependencies,
    /// Withdrawal must not resurrect an older clean receipt with equal text.
    pub finding_revisions: BTreeMap<String, u64>,
    pub candidate_revisions: BTreeMap<String, u64>,
    pub completed_analysis_sha256: Option<String>,
    /// Batches spent on a reviewer pack key (sorted source_scope).
    #[serde(default)]
    pub pack_batches: BTreeMap<String, usize>,
    // Derived solely from frozen input and the tool budget, never a semantic
    // receipt. Rebuild after restart; do not change the checkpoint contract.
    #[serde(skip)]
    task_inventory: std::sync::OnceLock<std::sync::Arc<TaskInventory>>,
}

#[derive(Debug)]
struct TaskInventory {
    input_sha256: String,
    max_tool_result_bytes: usize,
    tasks: Vec<Task>,
}

fn task_inventory(
    input: &FrozenInput,
    state: &Checkpoint,
    max_tool_result_bytes: usize,
) -> Result<Vec<Task>, String> {
    let Some(review) = &state.source_review else {
        return tasks(input, max_tool_result_bytes);
    };
    let input_sha256 = digest(input)?;
    if let Some(cached) = review.task_inventory.get()
        && cached.input_sha256 == input_sha256
        && cached.max_tool_result_bytes == max_tool_result_bytes
    {
        return Ok(cached.tasks.clone());
    }
    let tasks = tasks(input, max_tool_result_bytes)?;
    // A changed input/budget must use its own fresh partition even when an
    // existing checkpoint is inspected before the normal frozen-input gate.
    let _ = review
        .task_inventory
        .set(std::sync::Arc::new(TaskInventory {
            input_sha256,
            max_tool_result_bytes,
            tasks: tasks.clone(),
        }));
    Ok(tasks)
}

/// Use the existing read envelopes to partition without a second parser.
/// Half the tool allowance leaves room for accompanying candidate evidence.
pub(super) fn tasks(
    input: &FrozenInput,
    max_tool_result_bytes: usize,
) -> Result<Vec<Task>, String> {
    let budget = max_tool_result_bytes / 2;
    let input_sha = digest(input)?;
    let mut tasks = Vec::new();
    let mut append = |source_id: &str, region: Region| -> Result<(), String> {
        let id = digest(&json!(["source_review", 1, input_sha, source_id, region]))?;
        tasks.push(Task {
            id,
            source_id: source_id.into(),
            region,
        });
        Ok(())
    };
    for source in &input.source_units {
        let id = &source.source_unit_revision_id;
        let mut start = 0;
        while start < source.text.len() {
            let page = tools::invoke(
                input,
                &mut Analysis::default(),
                &mut Coverage::default(),
                true,
                "read_source",
                &json!({"source_id":id,"start":start,"max_bytes":budget}),
                budget,
            )?;
            let end = page["end"].as_u64().ok_or("source page end missing")? as usize;
            if end <= start {
                return Err("source review fragment cannot fit the frozen budget".into());
            }
            append(id, Region::Text { start, end })?;
            start = end;
        }
        let mut has_grid = false;
        for form in input
            .structured_forms
            .iter()
            .filter(|f| f["source_unit_revision_id"] == *id)
        {
            let form_id = form["form_definition_revision_id"]
                .as_str()
                .ok_or("form identity missing")?;
            let total =
                relations::form_total(&form["definition"]).ok_or("form grid unavailable")?;
            let mut start = 0;
            while start < total {
                let mut count = total - start;
                loop {
                    let page = tools::invoke(
                        input,
                        &mut Analysis::default(),
                        &mut Coverage::default(),
                        true,
                        "read_form",
                        &json!({"form_id":form_id,"offset":start,"limit":count}),
                        budget,
                    );
                    match page {
                        Ok(page) => {
                            let end =
                                page["next"].as_u64().ok_or("grid page end missing")? as usize;
                            if end <= start {
                                return Err("grid review fragment did not advance".into());
                            }
                            append(
                                id,
                                Region::Grid {
                                    form_id: form_id.into(),
                                    start,
                                    end,
                                },
                            )?;
                            start = end;
                            has_grid = true;
                            break;
                        }
                        Err(error) if count == 1 => return Err(error),
                        Err(_) => count = count.div_ceil(2),
                    }
                }
            }
        }
        if source.text.is_empty() && !has_grid {
            append(id, Region::Empty)?;
        }
    }
    Ok(tasks)
}

pub fn initialize(input: &FrozenInput, config: &Config) -> Result<State, String> {
    Ok(State {
        schema_version: 1,
        manifest_sha256: digest(&tasks(input, config.limits.max_tool_result_bytes)?)?,
        results: BTreeMap::new(),
        active_task: None,
        dependencies: Dependencies::default(),
        finding_revisions: BTreeMap::new(),
        candidate_revisions: BTreeMap::new(),
        completed_analysis_sha256: None,
        pack_batches: BTreeMap::new(),
        task_inventory: Default::default(),
    })
}

fn neighbors<'a>(
    input: &FrozenInput,
    inventory: &'a [Task],
    task: &Task,
) -> Vec<(&'static str, &'a Task)> {
    let Some(source) = input
        .source_units
        .iter()
        .find(|source| source.source_unit_revision_id == task.source_id)
    else {
        return Vec::new();
    };
    let same_document: BTreeSet<_> = input
        .source_units
        .iter()
        .filter(|other| other.document_id == source.document_id)
        .map(|source| source.source_unit_revision_id.as_str())
        .collect();
    let document_tasks: Vec<_> = inventory
        .iter()
        .filter(|task| same_document.contains(task.source_id.as_str()))
        .collect();
    let Some(position) = document_tasks.iter().position(|item| item.id == task.id) else {
        return Vec::new();
    };
    [
        ("before", position.checked_sub(1)),
        ("after", position.checked_add(1)),
    ]
    .into_iter()
    .filter_map(|(side, index)| Some((side, *document_tasks.get(index?)?)))
    .collect()
}

pub(super) fn references(analysis: &Analysis, dependencies: &Dependencies) -> BTreeSet<String> {
    dependency_references(analysis, dependencies)
        .into_iter()
        .filter(|key| context::reference(analysis, key).is_ok())
        .collect()
}

/// Work owned by original geometry, independently of supporting query history.
/// Direct relation endpoints require comparison, but do not bring their other
/// relationships, templates or whole source tasks into this assignment.
pub(super) struct Obligations {
    pub comparisons: BTreeSet<String>,
    pub subjects: BTreeSet<String>,
    sources: BTreeSet<String>,
}

fn obligations(input: &FrozenInput, state: &Checkpoint, task: &Task) -> Obligations {
    let mut owned = task_dependencies(task);
    expand_view_dependencies(input, &state.reviewer_coverage, &mut owned);
    let in_fragment = |span: &Span| {
        if !owned.source_ids.contains(&span.source_id) {
            return false;
        }
        if owned.source_ids.len() > 1 {
            return true; // Preserve joint inspection of an independently viewed page.
        }
        match &task.region {
            Region::Text { start, end } => {
                span.view_id.is_some()
                    || (span.grid_cell.is_none() && span.start < *end && span.end > *start)
            }
            Region::Grid {
                form_id,
                start,
                end,
            } => {
                span.view_id.is_some()
                    || (span
                        .grid_cell
                        .as_ref()
                        .is_some_and(|cell| &cell.form_id == form_id)
                        && tools::validate_grid_span(input, span)
                            .is_ok_and(|offset| offset >= *start && offset < *end))
            }
            Region::Empty => true,
        }
    };
    let subjects: BTreeSet<String> = state
        .analysis
        .records
        .iter()
        .filter(|(_, record)| record.sources.iter().any(&in_fragment))
        .map(|(id, _)| format!("record:{id}"))
        .collect();
    let mut comparisons = subjects.clone();
    for (id, edge) in &state.analysis.relations {
        if subjects.contains(&format!("record:{}", edge.from))
            || subjects.contains(&format!("record:{}", edge.to))
            || edge.grounds.iter().any(&in_fragment)
        {
            comparisons.insert(format!("relation:{id}"));
            comparisons.insert(format!("record:{}", edge.from));
            comparisons.insert(format!("record:{}", edge.to));
        }
    }
    comparisons.extend(
        owned
            .source_ids
            .iter()
            .filter(|id| state.analysis.dispositions.contains_key(*id))
            .map(|id| format!("disposition:{id}")),
    );
    Obligations {
        comparisons,
        subjects,
        sources: owned.source_ids,
    }
}

pub(super) fn active_obligations(
    input: &FrozenInput,
    state: &Checkpoint,
    budget: usize,
) -> Result<Option<Obligations>, String> {
    let Some(review) = state
        .source_review
        .as_ref()
        .filter(|_| state.role == Role::Reviewer)
    else {
        return Ok(None);
    };
    let Some(id) = &review.active_task else {
        return Ok(None);
    };
    let input_sha256 = digest(input)?;
    if let Some(cache) = review
        .task_inventory
        .get()
        .filter(|cache| cache.input_sha256 == input_sha256)
        && let Some(task) = cache.tasks.iter().find(|task| &task.id == id)
    {
        return Ok(Some(obligations(input, state, task)));
    }
    let task = task_inventory(input, state, budget)?
        .into_iter()
        .find(|task| &task.id == id)
        .ok_or("active source task is not in the frozen inventory")?;
    Ok(Some(obligations(input, state, &task)))
}

// Deleted IDs remain version dependencies, but cannot be inspected or compared
// as current candidates. Their findings still require explicit reviewer action.
fn dependency_references(analysis: &Analysis, dependencies: &Dependencies) -> BTreeSet<String> {
    let scope: Vec<_> = dependencies.source_ids.iter().cloned().collect();
    let mut refs: BTreeSet<_> = context::scope_references(analysis, &scope)
        .into_iter()
        .collect();
    refs.extend(dependencies.references.iter().cloned());
    let embedded: Vec<_> = refs
        .iter()
        .filter_map(|key| key.strip_prefix("record:"))
        .filter_map(|id| analysis.records.get(id))
        .flat_map(embedded_relationship_targets)
        .map(|id| format!("record:{id}"))
        .collect();
    refs.extend(embedded);
    // Explicitly inspected endpoints also depend on newly added/removed edges,
    // including edges whose grounds lie outside the original source fragment.
    let incident: Vec<_> = analysis
        .relations
        .iter()
        .filter(|(_, edge)| {
            refs.contains(&format!("record:{}", edge.from))
                || refs.contains(&format!("record:{}", edge.to))
        })
        .map(|(id, _)| format!("relation:{id}"))
        .collect();
    refs.extend(incident);
    let endpoints: Vec<_> = refs
        .iter()
        .filter_map(|key| key.strip_prefix("relation:"))
        .filter_map(|id| analysis.relations.get(id))
        .flat_map(|r| [format!("record:{}", r.from), format!("record:{}", r.to)])
        .collect();
    refs.extend(endpoints);
    refs
}

fn semantic_analysis(analysis: &Analysis) -> Value {
    json!({"records":analysis.records,"relations":analysis.relations,"dispositions":analysis.dispositions})
}

pub(super) fn version(state: &Checkpoint, dependencies: &Dependencies) -> Result<String, String> {
    dependency_version(state, dependencies, true)
}

fn dependency_version(
    state: &Checkpoint,
    dependencies: &Dependencies,
    include_findings: bool,
) -> Result<String, String> {
    let refs: BTreeMap<_, _> = dependency_references(&state.analysis, dependencies)
        .into_iter()
        .map(|key| {
            let value = context::reference(&state.analysis, &key).unwrap_or(Value::Null);
            (key, value)
        })
        .collect();
    let rules: BTreeMap<_, _> = state
        .analysis
        .records
        .iter()
        .filter(|(_, r)| matches!(r.data, RecordData::Rule { .. }))
        .collect();
    let revisions: BTreeMap<_, _> = state
        .source_review
        .as_ref()
        .into_iter()
        .flat_map(|review| &review.finding_revisions)
        .filter(|(source, _)| {
            include_findings && (dependencies.global || dependencies.source_ids.contains(*source))
        })
        .collect();
    let mut value = json!({"input":state.input_sha256,"sources":dependencies.source_ids,"references":refs,
        "rules":rules,"finding_revisions":revisions,
        "global":dependencies.global.then(||semantic_analysis(&state.analysis))});
    if include_findings {
        let repairs = repair::review_dependencies(&state.repair, dependencies, &refs);
        if !repairs.is_empty() {
            value["main_repairs"] = json!(repairs);
        }
        // Explicit cross-source candidates depend on their own finding
        // revisions, without treating every original citation as a query of
        // all candidates sharing that source.
        let revisions: BTreeMap<_, _> = state
            .source_review
            .as_ref()
            .into_iter()
            .flat_map(|review| &review.candidate_revisions)
            .filter(|(key, _)| refs.contains_key(*key))
            .collect();
        if !revisions.is_empty() {
            value["candidate_finding_revisions"] = json!(revisions);
        }
    }
    digest(&value)
}

fn candidate_dependencies(state: &Checkpoint, key: &str) -> Result<Dependencies, String> {
    let candidate = context::reference(&state.analysis, key)?;
    let mut dependencies = Dependencies::default();
    dependencies.references.insert(key.into());
    if key.starts_with("disposition:") {
        // A disposition judges the whole source's candidate collection.
        collect_sources(&candidate, &mut dependencies.source_ids);
    } else if let Some(id) = key.strip_prefix("record:") {
        // A local comparison depends on incident relations, not every other
        // candidate that happens to cite the same page. Recompute membership
        // so adding, removing or moving a relation also invalidates it.
        dependencies.references.extend(
            state
                .analysis
                .relations
                .iter()
                .filter(|(_, relation)| relation.from == id || relation.to == id)
                .map(|(id, _)| format!("relation:{id}")),
        );
    }
    // Include actual endpoints and explicit template parents without walking
    // the entire relation graph. Source judgments retain their broader scope.
    let refs = references(&state.analysis, &dependencies);
    for reference in &refs {
        if let Some(Record {
            data:
                RecordData::Template {
                    parent: Some(parent),
                    ..
                },
            ..
        }) = reference
            .strip_prefix("record:")
            .and_then(|id| state.analysis.records.get(id))
        {
            dependencies.references.insert(format!("record:{parent}"));
        }
    }
    dependencies.references.extend(refs);
    Ok(dependencies)
}

/// Current local dependency values, without the independent review's global
/// rule content or finding revisions. Missing dependent IDs remain explicit.
pub(in crate::tender_analysis) fn candidate_reference_values(
    state: &Checkpoint,
    key: &str,
) -> Result<BTreeMap<String, Value>, String> {
    let dependencies = candidate_dependencies(state, key)?;
    Ok(dependency_references(&state.analysis, &dependencies)
        .into_iter()
        .map(|key| {
            let value = context::reference(&state.analysis, &key).unwrap_or(Value::Null);
            (key, value)
        })
        .collect())
}

pub(in crate::tender_analysis) fn candidate_version(
    state: &Checkpoint,
    key: &str,
) -> Result<String, String> {
    let dependencies = candidate_dependencies(state, key)?;
    let revisions: BTreeMap<_, _> = state
        .source_review
        .as_ref()
        .into_iter()
        .flat_map(|review| &review.candidate_revisions)
        // Rule content participates in dependency_version globally. A finding
        // about an unchanged rule is not a global content edit: retain its
        // revision only for comparisons that actually depend on that record.
        // Saving/withdrawing findings still invalidates related comparisons;
        // actual rule edits still invalidate every potentially governed check.
        .filter(|(reference, _)| dependencies.references.contains(*reference))
        .collect();
    digest(&json!([
        dependency_version(state, &dependencies, false)?,
        revisions
    ]))
}

fn collect_sources(value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("source_id").and_then(Value::as_str) {
                into.insert(id.into());
            }
            for value in object.values() {
                collect_sources(value, into);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_sources(value, into);
            }
        }
        _ => {}
    }
}

pub(super) fn finding_changed(
    state: &mut Checkpoint,
    before: Option<&Finding>,
    after: Option<&Finding>,
) -> Result<(), String> {
    if digest(&before)? == digest(&after)? {
        return Ok(());
    }
    let mut sources = BTreeSet::new();
    let mut references = BTreeSet::new();
    for finding in before.into_iter().chain(after) {
        collect_sources(&json!(finding.sources), &mut sources);
        for affected in &finding.affected {
            if let Some(record) = state.analysis.records.get(&affected.id) {
                references.insert(format!("record:{}", affected.id));
                collect_sources(&json!(record), &mut sources);
            }
            if let Some(relation) = state.analysis.relations.get(&affected.id) {
                references.insert(format!("relation:{}", affected.id));
                collect_sources(&json!(relation), &mut sources);
                for id in [&relation.from, &relation.to] {
                    if let Some(record) = state.analysis.records.get(id) {
                        collect_sources(&json!(record), &mut sources);
                    }
                }
            }
        }
    }
    if let Some(review) = state.source_review.as_mut() {
        for source in sources {
            references.insert(format!("disposition:{source}"));
            let revision = review.finding_revisions.entry(source).or_default();
            *revision = revision.checked_add(1).ok_or("finding revision overflow")?;
        }
        for reference in references {
            let revision = review.candidate_revisions.entry(reference).or_default();
            *revision = revision
                .checked_add(1)
                .ok_or("candidate finding revision overflow")?;
        }
    }
    Ok(())
}

fn task_dependencies(task: &Task) -> Dependencies {
    Dependencies {
        source_ids: BTreeSet::from([task.source_id.clone()]),
        ..Default::default()
    }
}

/// Navigation must describe the assigned fragment, not demand the entire
/// oversized source before each local judgment. Cross-reference citations are
/// checked separately; metadata is required once for independent interpretation.
pub(super) fn reading_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    coverage: &Coverage,
    max_bytes: usize,
) -> Result<Option<Vec<Value>>, String> {
    let Some(id) = state
        .source_review
        .as_ref()
        .and_then(|r| r.active_task.as_ref())
    else {
        return Ok(None);
    };
    let task = task_inventory(input, state, max_bytes)?
        .into_iter()
        .find(|t| &t.id == id)
        .ok_or("source review task identity changed")?;
    let mut gaps = Vec::new();
    for mut gap in tools::reading_gaps(input, coverage) {
        if gap["kind"] == "unread_metadata" {
            gaps.push(gap);
            continue;
        }
        match &task.region {
            Region::Text { start, end }
                if gap["source_id"] == task.source_id && gap["kind"] == "unread_source" =>
            {
                let a =
                    (*start).max(gap["start"].as_u64().ok_or("source gap start missing")? as usize);
                let b = (*end).min(gap["end"].as_u64().ok_or("source gap end missing")? as usize);
                if a < b {
                    gap["start"] = json!(a);
                    gap["end"] = json!(b);
                    gaps.push(gap);
                }
            }
            Region::Grid {
                form_id,
                start,
                end,
            } if gap["form_id"] == *form_id
                && !tools::contains(coverage.form_cells.get(form_id), *start, *end) =>
            {
                gaps.push(json!({"kind":"unread_grid","source_id":task.source_id,"form_id":form_id,"start":start,"end":end,"read_ranges":coverage.form_cells.get(form_id)}));
            }
            _ => {}
        }
    }
    Ok(Some(gaps))
}

// Geometry can establish a distinct continuation target, not its semantic
// relevance. The reviewer still has to explain and independently read it.
fn continuation_target(input: &FrozenInput, task: &Task, span: &Span, before: bool) -> bool {
    if span.source_id != task.source_id {
        return true;
    }
    match &task.region {
        Region::Text { start, end } if span.view_id.is_none() && span.grid_cell.is_none() => {
            if before {
                span.end <= *start
            } else {
                span.start >= *end
            }
        }
        Region::Grid {
            form_id,
            start,
            end,
        } => {
            let Some(cell) = span
                .grid_cell
                .as_ref()
                .filter(|cell| cell.form_id == *form_id)
            else {
                return false;
            };
            input
                .structured_forms
                .iter()
                .find(|form| form["form_definition_revision_id"] == *form_id)
                .and_then(|form| relations::form_dimensions(&form["definition"]))
                .and_then(|(_, columns)| relations::dense_index(cell.row, cell.column, columns))
                .is_some_and(|index| {
                    if before {
                        index < *start
                    } else {
                        index >= *end
                    }
                })
        }
        _ => false,
    }
}

fn reviewed_boundary_gap(
    state: &Checkpoint,
    task: &Task,
    boundary: &Boundary,
    judgment: &Judgment,
) -> Result<bool, String> {
    for check in judgment
        .relationship_checks
        .iter()
        .filter(|check| check.status == RelationshipStatus::SourceLimited)
    {
        for id in &check.unresolved_record_ids {
            let Some(record) = state.analysis.records.get(id) else {
                continue;
            };
            if !matches!(record.data, RecordData::Unresolved { .. }) {
                continue;
            }
            let key = format!("record:{id}");
            if state.reviewer_coverage.candidate.get(&key) != Some(&digest(record)?)
                || !context::has_review_outcome(state, &key)?
            {
                continue;
            }
            if boundary.sources.iter().any(|span| {
                span.source_id == task.source_id
                    && record
                        .sources
                        .iter()
                        .any(|source| shared_mapping_evidence(source, span))
                    && check
                        .sources
                        .iter()
                        .any(|source| shared_mapping_evidence(source, span))
            }) {
                // The normal relationship validator still checks this explicit
                // judgment, subject binding and all available candidate sources.
                // The Unresolved record remains a source report open item.
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn validate_boundaries(
    input: &FrozenInput,
    state: &Checkpoint,
    task: &Task,
    judgment: &Judgment,
) -> Result<(), String> {
    for (name, boundary) in [
        ("before", &judgment.boundaries.before),
        ("after", &judgment.boundaries.after),
    ] {
        let path = format!("/boundaries/{name}");
        if boundary.reason.trim().is_empty() {
            return Err(tools::field_error(
                &format!("{path}/reason"),
                "explain whether the source continues and what evidence establishes the boundary",
            ));
        }
        if judgment.status == JudgmentStatus::Checked
            && boundary.state == BoundaryStatus::Unresolved
            && !reviewed_boundary_gap(state, task, boundary, judgment)?
        {
            return Err(tools::field_error(
                &format!("{path}/state"),
                "An unresolved boundary needs an independently compared current Unresolved record with matching original evidence and a source_limited relationship check naming it in unresolved_record_ids before status=checked. While searching use needs_evidence. If the candidate omits or misstates a genuine source gap, save that candidate error with put_review_finding; once accurately recorded, keep the gap in the source report instead of retaining an extraction error merely because evidence is absent. Do not label an unresolved boundary complete or continuation",
            ));
        }
        for span in &boundary.sources {
            tools::validate_span(input, &state.reviewer_coverage, span)
                .map_err(|error| tools::field_error(&path, error))?;
        }
        if boundary.state == BoundaryStatus::Continuation
            && !boundary
                .sources
                .iter()
                .any(|span| continuation_target(input, task, span, name == "before"))
        {
            return Err(tools::field_error(
                &path,
                "continuation requires a read citation beyond the current fragment on this side or in a distinct source. If no target has been established, keep boundary state=unresolved and use needs_evidence while searching. A verified source gap needs a saved and independently compared Unresolved record plus a source_limited relationship check; only a missing or incorrect candidate representation is an extraction finding. The current fragment alone is not a continuation target",
            ));
        }
    }
    Ok(())
}

fn complete(input: &FrozenInput, state: &Checkpoint, task: &Task) -> Result<bool, String> {
    let Some(receipt) = state
        .source_review
        .as_ref()
        .and_then(|r| r.results.get(&task.id))
    else {
        return Ok(false);
    };
    let mut dependencies = receipt.dependencies.clone();
    expand_view_dependencies(input, &state.reviewer_coverage, &mut dependencies);
    let owned = obligations(input, state, task);
    Ok(receipt.judgment.task_id == task.id
        && receipt.dependencies.source_ids.contains(&task.source_id)
        && receipt.judgment.status != JudgmentStatus::NeedsEvidence
        && validate_boundaries(input, state, task, &receipt.judgment).is_ok()
        && validate_layout_view(input, state, task, &dependencies, &receipt.judgment).is_ok()
        && validate_relationship_subjects(
            input,
            state,
            &receipt.judgment,
            &mut dependencies,
            &owned.subjects,
        )
        .is_ok()
        && validate_template_subjects(
            input,
            state,
            &receipt.judgment,
            &mut dependencies,
            &owned.subjects,
        )
        .is_ok()
        && receipt.version == version(state, &dependencies)?
        && receipt
            .judgment
            .finding_ids
            .iter()
            .all(|id| state.review_draft.contains_key(id)))
}

pub(super) fn pending(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Vec<Task>, String> {
    let tasks = task_inventory(input, state, config.limits.max_tool_result_bytes)?;
    pending_from_inventory(input, state, &tasks)
}

pub(super) fn pending_open(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Vec<Task>, String> {
    let pending = pending(input, config, state)?;
    let mut open = Vec::new();
    for task in pending {
        if !context::scope_is_blocked(state, std::slice::from_ref(&task.source_id))? {
            open.push(task);
        }
    }
    Ok(open)
}

pub(super) fn omitted_sources(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<BTreeMap<String, String>, String> {
    let pending = pending(input, config, state)?;
    let mut omitted = BTreeMap::new();
    for task in pending {
        if context::scope_is_blocked(state, std::slice::from_ref(&task.source_id))? {
            omitted.insert(task.source_id, "pack exhausted or blocked".into());
        }
    }
    Ok(omitted)
}

pub(super) fn charge_pack(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
) -> Result<(), String> {
    if state.role != Role::Reviewer || config.limits.pack_max_turns == 0 {
        return Ok(());
    }
    let Some(scope) = state
        .reviewer_work
        .as_ref()
        .map(|work| work.source_scope.clone())
    else {
        return Ok(());
    };
    if scope.is_empty() {
        return Ok(());
    }
    let mut key = scope.clone();
    key.sort();
    let key = key.join("\n");
    let spent = {
        let review = state
            .source_review
            .as_mut()
            .ok_or("source review state missing")?;
        let spent = review.pack_batches.entry(key).or_insert(0);
        *spent = spent.saturating_add(1);
        *spent
    };
    if spent < config.limits.pack_max_turns {
        return Ok(());
    }
    let leftover: Vec<String> = pending_open(input, config, state)?
        .into_iter()
        .filter(|task| scope.contains(&task.source_id))
        .map(|task| task.source_id)
        .collect();
    if leftover.is_empty() {
        return Ok(());
    }
    let values: Vec<_> = context::scope_references(&state.analysis, &leftover)
        .iter()
        .map(|item| context::reference(&state.analysis, item))
        .collect::<Result<Vec<_>, _>>()?;
    state.reviewer_progress.blockers.push(ExecutionBlocker {
        scope: leftover,
        dependencies_sha256: digest(&values)?,
        watch: ProgressWatch {
            recovery: Recovery::Blocked,
            replans: config.limits.max_focus_replans,
            ..Default::default()
        },
    });
    state.reviewer_work = None;
    state.pending_coverage = None;
    if let Some(review) = state.source_review.as_mut() {
        review.active_task = None;
    }
    Ok(())
}

fn pending_from_inventory(
    input: &FrozenInput,
    state: &Checkpoint,
    tasks: &[Task],
) -> Result<Vec<Task>, String> {
    let review = state
        .source_review
        .as_ref()
        .ok_or("independent source review state missing")?;
    if review.schema_version != 1 || review.manifest_sha256 != digest(&tasks)? {
        return Err("independent source task manifest changed".into());
    }
    let mut remaining: Vec<_> = tasks
        .iter()
        .filter_map(|task| match complete(input, state, task) {
            Ok(true) => None,
            Ok(false) => Some(Ok(task.clone())),
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_, _>>()?;
    if remaining.is_empty() {
        // Aggregation also requires current candidate comparisons. A retained
        // source judgment can outlive a comparison receipt (for example after
        // restoring older dependency versions). Do not leave a reviewer with
        // no assigned task while that final gate still requires work.
        for task in tasks {
            let dependencies = &review.results[&task.id].dependencies;
            for key in references(&state.analysis, dependencies) {
                if !context::has_review_outcome(state, &key)? {
                    remaining.push(task.clone());
                    break;
                }
            }
        }
    }
    Ok(remaining)
}

/// Track the actual scope of successful queries, including zero-result ones.
/// This state is committed with the same tool batch; it is not reading coverage.
/// Text/grid source reads and searches depend only on FrozenInput, already
/// frozen by input_sha256. They do not query the mutable candidate collection.
/// Their matches are navigation; subsequent reads, candidate queries and
/// explicit judgment citations track any mutable result dependencies.
pub(super) fn record_query(input: &FrozenInput, state: &mut Checkpoint, name: &str, args: &Value) {
    if state.role != Role::Reviewer {
        return;
    }
    let scope = state.work().map(|w| w.source_scope.clone());
    let mixed_refs: Vec<_> = if name == "inspect_analysis" && args["kind"] == "all" {
        args["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .flat_map(|id| {
                [
                    state
                        .analysis
                        .records
                        .contains_key(id)
                        .then(|| format!("record:{id}")),
                    state
                        .analysis
                        .relations
                        .contains_key(id)
                        .then(|| format!("relation:{id}")),
                    state
                        .analysis
                        .dispositions
                        .contains_key(id)
                        .then(|| format!("disposition:{id}")),
                ]
                .into_iter()
                .flatten()
            })
            .collect()
    } else {
        vec![]
    };
    let Some(review) = state
        .source_review
        .as_mut()
        .filter(|r| r.active_task.is_some())
    else {
        return;
    };
    let dependencies = &mut review.dependencies;
    match name {
        "read_source_view" => {
            if let Some(id) = args["source_id"].as_str() {
                // A layout review intentionally compares all templates on the
                // same original page, including split text/grid sources.
                dependencies.source_ids.extend(
                    input
                        .source_units
                        .iter()
                        .filter(|source| same_page(input, id, &source.source_unit_revision_id))
                        .map(|source| source.source_unit_revision_id.clone()),
                );
            }
        }
        "inspect_analysis" => {
            if let Some(id) = args["source_id"].as_str() {
                dependencies.source_ids.insert(id.into());
            } else if let Some(ids) = args["ids"].as_array() {
                if args["kind"] == "all" {
                    dependencies.references.extend(mixed_refs);
                    return;
                }
                let kind = match args["kind"].as_str() {
                    Some("relation") => "relation",
                    Some("disposition") => "disposition",
                    _ => "record",
                };
                dependencies.references.extend(
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(|id| format!("{kind}:{id}")),
                );
            } else if args["scope"] == "collection" {
                dependencies.global = true;
            } else if let Some(scope) = scope {
                dependencies.source_ids.extend(scope);
            } else {
                dependencies.global = true;
            }
        }
        _ => {}
    }
    expand_view_dependencies(input, &state.reviewer_coverage, dependencies);
}

fn has_executable_task(state: &Checkpoint, pending: &[Task]) -> Result<bool, String> {
    for task in pending {
        if !context::scope_is_blocked(state, std::slice::from_ref(&task.source_id))? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn select_next(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
) -> Result<(), String> {
    if state.role != Role::Reviewer || state.done {
        return Ok(());
    }
    if let Some(review) = &mut state.source_review {
        expand_view_dependencies(input, &state.reviewer_coverage, &mut review.dependencies);
    }
    let pending = pending(input, config, state)?;
    let mut executable = Vec::new();
    for task in &pending {
        if !context::scope_is_blocked(state, std::slice::from_ref(&task.source_id))? {
            executable.push(task);
        }
    }
    let review = state
        .source_review
        .as_ref()
        .ok_or("source review state missing")?;
    let active_task = review.active_task.clone();
    let seed_id = pending
        .iter()
        .find(|task| active_task.as_ref() == Some(&task.id))
        .map(|task| task.source_id.clone())
        .or_else(|| {
            state
                .reviewer_work
                .as_ref()
                .and_then(|work| work.source_scope.first().cloned())
        });
    if let Some(seed_id) = seed_id {
        let pack = crate::tender_analysis::pack::pack_containing(
            input,
            config.limits.pack_max_units,
            config.limits.pack_max_chars,
            &seed_id,
        );
        let pack_executable: Vec<&Task> = executable
            .iter()
            .copied()
            .filter(|task| pack.contains(&task.source_id))
            .collect();
        if !pack_executable.is_empty() {
            let source_scope: Vec<String> = pack
                .into_iter()
                .filter(|id| executable.iter().any(|task| &task.source_id == id))
                .collect();
            if active_task
                .as_ref()
                .is_some_and(|id| pack_executable.iter().any(|task| &task.id == id))
            {
                if let Some(work) = state.reviewer_work.as_mut() {
                    work.source_scope = source_scope;
                }
                return Ok(());
            }
            let task = pack_executable[0];
            let mut dependencies = review
                .results
                .get(&task.id)
                .map(|result| result.dependencies.clone())
                .unwrap_or_else(|| task_dependencies(task));
            expand_view_dependencies(input, &state.reviewer_coverage, &mut dependencies);
            let review = state.source_review.as_mut().expect("checked review state");
            review.active_task = Some(task.id.clone());
            review.dependencies = dependencies;
            if let Some(work) = state.reviewer_work.as_mut() {
                work.source_scope = source_scope;
            }
            return Ok(());
        }
    }
    if active_task
        .as_ref()
        .is_some_and(|id| executable.iter().any(|task| &task.id == id))
    {
        return Ok(());
    }
    let next = executable.first().copied();
    let mut dependencies = next
        .map(|task| {
            state
                .source_review
                .as_ref()
                .and_then(|review| review.results.get(&task.id))
                .map(|result| result.dependencies.clone())
                .unwrap_or_else(|| task_dependencies(task))
        })
        .unwrap_or_default();
    expand_view_dependencies(input, &state.reviewer_coverage, &mut dependencies);
    let review = state.source_review.as_mut().expect("checked review state");
    review.active_task = next.map(|task| task.id.clone());
    review.dependencies = dependencies;
    if let Some(task) = next {
        if state.reviewer_progress.watch.recovery == Recovery::Blocked {
            let prior = state
                .reviewer_progress
                .blockers
                .iter()
                .find(|b| b.scope.contains(&task.source_id))
                .map(|b| b.watch.clone());
            state.reviewer_progress.resume(prior.as_ref());
        }
        let pack = crate::tender_analysis::pack::pack_containing(
            input,
            config.limits.pack_max_units,
            config.limits.pack_max_chars,
            &task.source_id,
        );
        let mut source_scope: Vec<String> = pack
            .into_iter()
            .filter(|id| executable.iter().any(|pending| &pending.source_id == id))
            .collect();
        if source_scope.is_empty() {
            source_scope = vec![task.source_id.clone()];
        }
        if state
            .reviewer_work
            .as_ref()
            .is_none_or(|work| work.source_scope != source_scope)
        {
            state.pending_coverage = None;
        }
        let mut work = WorkState { source_scope, deferred_sources: Vec::new(),
            objective: "Independently compare the original fragment and all associated candidates, including omissions and continuation boundaries.".into(),
            focus: context::Focus::default(), output_refs: Vec::new(), pending_refs: Vec::new(),
            status: WorkStatus::Active, note: String::new() };
        context::retain_outcomes(&state.analysis, &mut work, None);
        state.reviewer_work = Some(work);
        // Preserve the just-executed complete protocol group. Older source
        // payloads are recoverable by identity and need not follow the task.
        if let Some(start) = state
            .transcript
            .iter()
            .rposition(|m| m["role"] == "assistant")
        {
            state.transcript.drain(..start);
        }
    }
    Ok(())
}

fn required_findings(
    state: &Checkpoint,
    task: &Task,
    sources: &BTreeSet<String>,
    refs: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut findings: BTreeSet<_> = state
        .review_draft
        .iter()
        .filter(|(_, f)| {
            f.sources.iter().any(|s| sources.contains(&s.source_id))
                || f.affected.iter().any(|a| {
                    refs.iter()
                        .any(|r| r.split_once(':').is_some_and(|(_, id)| id == a.id))
                })
        })
        .map(|(id, _)| id.clone())
        .collect();
    // Removing an affected candidate does not erase its previously assigned
    // finding. Keep that obligation on this source task until the reviewer
    // explicitly repairs or withdraws it against the new analysis.
    findings.extend(
        state
            .source_review
            .as_ref()
            .and_then(|review| review.results.get(&task.id))
            .into_iter()
            .flat_map(|r| &r.judgment.finding_ids)
            .filter(|id| state.review_draft.contains_key(*id))
            .cloned(),
    );
    findings
}

fn templates(analysis: &Analysis, refs: &BTreeSet<String>) -> BTreeSet<String> {
    refs.iter()
        .filter_map(|key| key.strip_prefix("record:"))
        .filter(|id| {
            matches!(analysis.records.get(*id).map(|record| &record.data),
            Some(RecordData::Template { applicability, .. })
                if applicability.state != ApplicabilityState::NotApplicable)
        })
        .map(str::to_owned)
        .collect()
}

pub(super) fn same_page(input: &FrozenInput, left: &str, right: &str) -> bool {
    let source = |id: &str| {
        input
            .source_units
            .iter()
            .find(|s| s.source_unit_revision_id == id)
    };
    match (source(left), source(right)) {
        (Some(left), Some(right)) => {
            left.document_id == right.document_id
                && left.locator["page_ordinal"].as_u64().is_some()
                && left.locator["page_ordinal"] == right.locator["page_ordinal"]
        }
        _ => false,
    }
}

// A page image exposes every parsed source on that page. Candidate dependency
// scope must match those pixels; this grants no text/grid/candidate reading.
fn expand_view_dependencies(
    input: &FrozenInput,
    coverage: &Coverage,
    dependencies: &mut Dependencies,
) {
    let pages: Vec<_> = coverage
        .views
        .values()
        .filter(|view| {
            dependencies
                .source_ids
                .iter()
                .any(|id| same_page(input, id, &view.source_id))
        })
        .map(|view| &view.source_id)
        .collect();
    dependencies.source_ids.extend(
        input
            .source_units
            .iter()
            .filter(|source| {
                pages
                    .iter()
                    .any(|id| same_page(input, &source.source_unit_revision_id, id))
            })
            .map(|source| source.source_unit_revision_id.clone()),
    );
}

fn requires_layout_view(
    input: &FrozenInput,
    state: &Checkpoint,
    task: &Task,
    dependencies: &Dependencies,
) -> bool {
    if templates(&state.analysis, &references(&state.analysis, dependencies)).is_empty() {
        return false;
    }
    // Splitting a heading/note and its grid into separate template records
    // does not remove their original page layout. Require the same pixels
    // regardless of how the extractor grouped those regions.
    let (mut text, mut grid) = (false, false);
    for record in state.analysis.records.values() {
        let RecordData::Template {
            regions,
            applicability,
            ..
        } = &record.data
        else {
            continue;
        };
        if applicability.state == ApplicabilityState::NotApplicable {
            continue;
        }
        for region in regions
            .iter()
            .filter(|region| same_page(input, &task.source_id, &region.source.source_id))
        {
            grid |= region.form_id.is_some();
            text |= region.form_id.is_none();
        }
        if text && grid {
            return true;
        }
    }
    false
}

fn validate_layout_view(
    input: &FrozenInput,
    state: &Checkpoint,
    task: &Task,
    dependencies: &Dependencies,
    judgment: &Judgment,
) -> Result<(), String> {
    if judgment.status != JudgmentStatus::NeedsEvidence
        && requires_layout_view(input, state, task, dependencies)
        && !judgment.sources.iter().any(|span| {
            span.view_id.as_ref().is_some_and(|id| {
                state.reviewer_coverage.views.get(id).is_some_and(|view| {
                    view.source_id == span.source_id
                        && same_page(input, &task.source_id, &view.source_id)
                })
            })
        })
    {
        return Err(tools::field_error(
            "/sources",
            format!(
                "Compare the page's text/table template regions, including regions split into separate templates, with original page pixels and cite the delivered visual citation. Use read_source_view with source_id={} or reuse an independently delivered view of the same document page. Parsed source order cannot establish interleaving. If the original view is unavailable, keep status=needs_evidence with the concrete layout question; do not sign a completed layout comparison.",
                task.source_id
            ),
        ));
    }
    Ok(())
}

pub(super) fn shared_mapping_evidence(a: &Span, b: &Span) -> bool {
    if a.source_id != b.source_id {
        return false;
    }
    if a.grid_cell.is_some() || b.grid_cell.is_some() || a.view_id.is_some() || b.view_id.is_some()
    {
        return a == b;
    }
    // A precise finding and a surrounding paragraph can cite the same clause.
    // Do not equate adjacent or merely partially overlapping text ranges.
    a.start < a.end
        && b.start < b.end
        && ((a.start <= b.start && b.end <= a.end) || (b.start <= a.start && a.end <= b.end))
}

fn relationship_records(analysis: &Analysis, refs: &BTreeSet<String>) -> BTreeSet<String> {
    refs.iter()
        .filter_map(|key| key.strip_prefix("record:"))
        .filter(
            |id| match analysis.records.get(*id).map(|record| &record.data) {
                Some(RecordData::Rule { .. } | RecordData::Unresolved { .. }) => true,
                Some(RecordData::Template {
                    parent,
                    applicability,
                    ..
                }) => parent.is_some() || applicability.state != ApplicabilityState::Applicable,
                Some(RecordData::Requirement { applicability, .. }) => {
                    applicability.state != ApplicabilityState::Applicable
                }
                _ => false,
            },
        )
        .map(str::to_owned)
        .collect()
}

/// Existing typed references are relationships, not a request to duplicate
/// them in Analysis.relations. Local RuleItem targets stay in their own record.
fn embedded_relationship_targets(record: &Record) -> BTreeSet<String> {
    match &record.data {
        RecordData::Template { parent, .. } => parent.iter().cloned().collect(),
        RecordData::Rule { items, .. } => items
            .iter()
            .flat_map(|item| &item.targets)
            .filter_map(|target| match target {
                rule_contract::RuleItemTarget::Record { id } if id != &record.id => {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn validate_relationship_subjects(
    input: &FrozenInput,
    state: &Checkpoint,
    judgment: &Judgment,
    dependencies: &mut Dependencies,
    required_subjects: &BTreeSet<String>,
) -> Result<(), String> {
    let scope = references(&state.analysis, dependencies);
    let mut seen = BTreeSet::new();
    for (index, check) in judgment.relationship_checks.iter().enumerate() {
        let path = format!("/relationship_checks/{index}");
        let fail = |message: &str| tools::field_error(&path, message);
        let key = format!("record:{}", check.record_id);
        if !scope.contains(&key) || !seen.insert(check.record_id.clone()) {
            return Err(fail(
                "judge each current task record at most once; inspect cross-source targets before including them",
            ));
        }
        let subject = &state.analysis.records[&check.record_id];
        let parent = match &subject.data {
            RecordData::Template { parent, .. } => parent.as_ref(),
            _ => None,
        };
        let embedded = embedded_relationship_targets(subject);
        if check.reason.trim().is_empty() || check.sources.is_empty() {
            return Err(fail(
                "explain the actual reference targets, selected conditions and continuation interpretation using independently read sources",
            ));
        }
        for source in &check.sources {
            tools::validate_span(input, &state.reviewer_coverage, source)
                .map_err(|error| tools::field_error(&path, error))?;
        }
        if !subject.sources.iter().any(|source| {
            check
                .sources
                .iter()
                .any(|span| shared_mapping_evidence(source, span))
        }) {
            return Err(tools::field_error(
                &format!("{path}/sources"),
                json!({
                    "record_id":check.record_id,
                    "subject_source_example":subject.sources.first().map(|source| evidence_refs::compact(input, source).unwrap_or_else(|| json!(source))),
                    "instruction":"Cite the subject record's original evidence as well as relevant target evidence. subject_source_example is one stored candidate location, not a reading receipt or a complete evidence set. Reuse its original only if independently delivered and relevant; otherwise read the missing range before revising this judgment."
                }),
            ));
        }
        for ids in [
            &check.related_record_ids,
            &check.relation_ids,
            &check.unresolved_record_ids,
            &check.finding_ids,
        ] {
            if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
                return Err(fail("relationship IDs must be distinct within each list"));
            }
        }
        for reference in std::iter::once(key)
            .chain(
                check
                    .related_record_ids
                    .iter()
                    .chain(&check.unresolved_record_ids)
                    .map(|id| format!("record:{id}")),
            )
            .chain(check.relation_ids.iter().map(|id| format!("relation:{id}")))
        {
            let value = context::reference(&state.analysis, &reference)
                .map_err(|error| tools::field_error(&path, error))?;
            if state.reviewer_coverage.candidate.get(&reference) != Some(&digest(&value)?) {
                return Err(fail(&format!(
                    "independently inspect the current relationship evidence: {reference}"
                )));
            }
            dependencies.references.insert(reference);
        }
        for id in &check.relation_ids {
            let edge = &state.analysis.relations[id];
            let target = if edge.from == check.record_id {
                &edge.to
            } else if edge.to == check.record_id {
                &edge.from
            } else {
                return Err(fail(
                    "each relation must connect this subject to a listed related record",
                ));
            };
            if !check.related_record_ids.contains(target) {
                return Err(fail(
                    "include each relation's other endpoint in related_record_ids",
                ));
            }
        }
        for id in &check.unresolved_record_ids {
            let record = &state.analysis.records[id];
            let RecordData::Unresolved { affected, .. } = &record.data else {
                return Err(fail(
                    "unresolved_record_ids must name saved unresolved records",
                ));
            };
            if id != &check.record_id
                && !affected.contains(&check.record_id)
                && !record.sources.iter().any(|source| {
                    subject
                        .sources
                        .iter()
                        .any(|span| shared_mapping_evidence(source, span))
                })
            {
                return Err(fail(
                    "the saved unresolved record must identify this subject or share its original evidence; do not borrow an unrelated unresolved item",
                ));
            }
        }
        for (finding_index, id) in check.finding_ids.iter().enumerate() {
            let finding_path = format!("{path}/finding_ids/{finding_index}");
            let Some(finding) = state.review_draft.get(id) else {
                return Err(tools::field_error(
                    &finding_path,
                    format!("save the relationship or applicability finding first: {id}"),
                ));
            };
            validate_finding(input, state, finding)
                .map_err(|error| tools::field_error(&finding_path, error))?;
            if !judgment.finding_ids.contains(id) {
                return Err(tools::field_error(
                    &finding_path,
                    format!("retain saved finding {id} in the outer finding_ids"),
                ));
            }
            if !(finding
                .affected
                .iter()
                .any(|affected| affected.id == check.record_id)
                || finding.sources.iter().any(|source| {
                    subject
                        .sources
                        .iter()
                        .any(|span| shared_mapping_evidence(source, span))
                        && check
                            .sources
                            .iter()
                            .any(|span| shared_mapping_evidence(source, span))
                }))
            {
                let candidate = state
                    .review_draft
                    .iter()
                    .find(|(other_id, other)| {
                        *other_id != id
                            && other
                                .affected
                                .iter()
                                .any(|affected| affected.id == check.record_id)
                            && validate_finding(input, state, other).is_ok()
                    })
                    .map(|(id, _)| id);
                return Err(tools::field_error(
                    &finding_path,
                    json!({
                        "record_id":check.record_id,"unrelated_finding_id":id,"candidate_finding_id":candidate,
                        "instruction":"This finding neither names this affected subject nor shares its cited original evidence. candidate_finding_id, when present, is an existing finding naming the subject: inspect_review and verify whether it describes this relationship problem before using it. Do not create a duplicate or withdraw a valid finding to finish; governing evidence may be on another page."
                    }),
                ));
            }
            // The finding's original citations were independently validated
            // above. They do not query every candidate on those pages. The
            // subject and explicit endpoints retain their finding revisions.
        }
        if (check.status == RelationshipStatus::Findings) != !check.finding_ids.is_empty() {
            return Err(fail(
                "use findings with saved relationship problems; otherwise leave this check's finding_ids empty",
            ));
        }
        match check.status {
            RelationshipStatus::Resolved => {
                if matches!(subject.data, RecordData::Unresolved { .. })
                    || (check.relation_ids.is_empty() && embedded.is_empty())
                    || embedded
                        .iter()
                        .any(|id| !check.related_record_ids.contains(id))
                    || !check.unresolved_record_ids.is_empty()
                    || check
                        .relation_ids
                        .iter()
                        .any(|id| state.analysis.relations[id].state == RelationState::Unresolved)
                    || check.related_record_ids.iter().any(|id| {
                        !embedded.contains(id)
                            && !check.relation_ids.iter().any(|edge| {
                                let edge = &state.analysis.relations[edge];
                                edge.from == *id || edge.to == *id
                            })
                    })
                {
                    return Err(fail(
                        "resolved requires actual resolved edges or saved typed targets to every listed target; list every external Rule.items target and Template.parent in related_record_ids. These typed links need no duplicate references/contains edge. Independently verify their source meaning; report a stale unresolved record as a finding",
                    ));
                }
                for target in embedded.iter().filter(|id| parent != Some(*id)) {
                    if !state.analysis.records[target].sources.iter().any(|source| {
                        check
                            .sources
                            .iter()
                            .any(|span| shared_mapping_evidence(source, span))
                    }) {
                        return Err(fail(
                            "cite independently read original evidence for each typed Rule target as well as the subject; a saved target is not semantic approval",
                        ));
                    }
                }
                if let Some(parent) = parent
                    && !state.analysis.records[parent].sources.iter().any(|source| {
                        check
                            .sources
                            .iter()
                            .any(|span| shared_mapping_evidence(source, span))
                    })
                {
                    return Err(fail(
                        "cite independently read original evidence for the selected template parent as well as the child; record existence does not establish semantic ownership",
                    ));
                }
            }
            RelationshipStatus::NotRequired => {
                if matches!(subject.data, RecordData::Unresolved { .. })
                    || parent.is_some()
                    || !embedded.is_empty()
                    || !check.related_record_ids.is_empty()
                    || !check.relation_ids.is_empty()
                    || !check.unresolved_record_ids.is_empty()
                    || state
                        .analysis
                        .relations
                        .values()
                        .any(|edge| edge.from == check.record_id || edge.to == check.record_id)
                {
                    return Err(fail(
                        "not_required needs a source-grounded absence of external relationships; existing Rule.items targets, template parents, edges or unresolved records must be examined, not dismissed",
                    ));
                }
            }
            RelationshipStatus::SourceLimited => {
                let unknown = matches!(&subject.data,
                    RecordData::Rule { applicability, .. } | RecordData::Requirement { applicability, .. } | RecordData::Template { applicability, .. }
                    if applicability.state == ApplicabilityState::Unknown);
                if !unknown && check.unresolved_record_ids.is_empty() {
                    return Err(fail(
                        "source_limited requires a saved unresolved record affecting this subject or its explicit unknown applicability; missing extraction is a finding, not missing source evidence",
                    ));
                }
                for id in &check.unresolved_record_ids {
                    if let RecordData::Unresolved { candidates, .. } =
                        &state.analysis.records[id].data
                    {
                        for source_id in candidates.iter().filter(|id| {
                            input
                                .source_units
                                .iter()
                                .any(|source| source.source_unit_revision_id == **id)
                        }) {
                            if !check
                                .sources
                                .iter()
                                .any(|span| span.source_id == *source_id)
                            {
                                return Err(fail(&format!(
                                    "inspect and cite the frozen candidate source {source_id} before retaining source_limited; unread is not unavailable, and reading alone does not resolve ambiguity"
                                )));
                            }
                        }
                    }
                }
            }
            RelationshipStatus::Findings => {}
        }
    }
    if judgment.status != JudgmentStatus::NeedsEvidence
        && let Some(id) = relationship_records(&state.analysis, required_subjects)
            .difference(&seen)
            .next()
    {
        return Err(tools::field_error(
            "/relationship_checks",
            format!(
                "missing explicit relationship/applicability judgment for record:{id}; ordinary candidate comparisons do not establish reference targets, selected conditions or continuation resolution"
            ),
        ));
    }
    Ok(())
}

fn validate_template_subjects(
    input: &FrozenInput,
    state: &Checkpoint,
    judgment: &Judgment,
    dependencies: &mut Dependencies,
    required_subjects: &BTreeSet<String>,
) -> Result<(), String> {
    let available = templates(&state.analysis, &references(&state.analysis, dependencies));
    let expected = templates(&state.analysis, required_subjects);
    let mut seen = BTreeSet::new();
    for (index, mapping) in judgment.template_mappings.iter().enumerate() {
        let path = format!("/template_mappings/{index}");
        if !available.contains(&mapping.template_id) || !seen.insert(mapping.template_id.clone()) {
            return Err(tools::field_error(
                &path,
                "use each current task template once",
            ));
        }
        if mapping.reason.trim().is_empty() || mapping.sources.is_empty() {
            return Err(tools::field_error(
                &path,
                "cite and explain which source obligations use this template; explain an empty requirement_ids list without inventing an obligation",
            ));
        }
        for source in &mapping.sources {
            tools::validate_span(input, &state.reviewer_coverage, source)
                .map_err(|error| tools::field_error(&path, error))?;
        }
        let mut parents = BTreeSet::new();
        let mut current = Some(mapping.template_id.as_str());
        while let Some(id) = current {
            if !parents.insert(id.to_owned()) {
                return Err(tools::field_error(&path, "template parent cycle"));
            }
            let Some(RecordData::Template { parent, .. }) =
                state.analysis.records.get(id).map(|r| &r.data)
            else {
                return Err(tools::field_error(&path, "template parent must exist"));
            };
            current = parent.as_deref();
        }
        let requirements: BTreeSet<_> = mapping.requirement_ids.iter().collect();
        let relations: BTreeSet<_> = mapping.relation_ids.iter().collect();
        let findings: BTreeSet<_> = mapping.finding_ids.iter().collect();
        if requirements.len() != mapping.requirement_ids.len()
            || relations.len() != mapping.relation_ids.len()
            || findings.len() != mapping.finding_ids.len()
        {
            return Err(tools::field_error(&path, "mapping IDs must be distinct"));
        }
        for id in &requirements {
            if !matches!(
                state.analysis.records.get(*id).map(|r| &r.data),
                Some(RecordData::Requirement { .. })
            ) {
                return Err(tools::field_error(
                    &path,
                    "requirement_ids must name current requirement records",
                ));
            }
        }
        for id in &relations {
            let Some(relation) = state.analysis.relations.get(*id) else {
                return Err(tools::field_error(
                    &path,
                    "inspect the current mapping relation",
                ));
            };
            if relation.kind != RelationKind::RequiresTemplate
                || !requirements.contains(&relation.from)
                || !parents.contains(&relation.to)
            {
                return Err(tools::field_error(
                    &path,
                    "relation must map a listed requirement to this template or a source-grounded parent; prose names and unrelated edges are not mappings",
                ));
            }
        }
        for (finding_index, id) in mapping.finding_ids.iter().enumerate() {
            let finding_path = format!("{path}/finding_ids/{finding_index}");
            let Some(finding) = state.review_draft.get(id) else {
                return Err(tools::field_error(
                    &finding_path,
                    "save the missing or incorrect mapping finding first",
                ));
            };
            if !judgment.finding_ids.contains(id) {
                return Err(tools::field_error(
                    &finding_path,
                    "include this saved mapping finding in the source judgment finding_ids",
                ));
            }
            if !finding.sources.iter().any(|source| {
                mapping
                    .sources
                    .iter()
                    .any(|mapped| shared_mapping_evidence(source, mapped))
            }) {
                return Err(tools::field_error(
                    &finding_path,
                    json!({
                        "finding_id":id,
                        "finding_source_example":finding.sources.first().map(|source| evidence_refs::compact(input, source).unwrap_or_else(|| json!(source))),
                        "inspect_review":{"ids":[id],"offset":0,"limit":1},
                        "instruction":"This finding has no shared mapping evidence. finding_source_example is one saved original location, not a reading receipt or proof that the finding applies to this mapping. Inspect the saved finding and compare its actual sources, including other grids or governing text, with the listed requirement/template. Independently read missing evidence before revising citations; do not reuse the active grid for a different subject. If the listed mapping is correct, use finding_ids=[] for this mapping and keep unrelated template-content findings in the source judgment finding_ids. Missing or incorrect mappings still require a saved source-backed finding; do not duplicate, withdraw or omit a valid issue just to pass."
                    }).to_string(),
                ));
            }
        }
        for id in &requirements {
            if findings.is_empty()
                && !relations
                    .iter()
                    .any(|edge| state.analysis.relations[*edge].from == **id)
            {
                return Err(tools::field_error(
                    &path,
                    "a required template mapping is absent: save a source-grounded missing-relation finding and use status=findings; do not approve merely because both records exist",
                ));
            }
        }
        for key in parents
            .iter()
            .chain(requirements.iter().copied())
            .map(|id| format!("record:{id}"))
            .chain(relations.iter().map(|id| format!("relation:{id}")))
        {
            let value = context::reference(&state.analysis, &key)?;
            if state.reviewer_coverage.candidate.get(&key) != Some(&digest(&value)?) {
                return Err(tools::field_error(
                    &path,
                    format!("independently inspect current mapping evidence: {key}"),
                ));
            }
            dependencies.references.insert(key);
        }
    }
    if judgment.status != JudgmentStatus::NeedsEvidence && !expected.is_subset(&seen) {
        return Err(tools::field_error(
            "/template_mappings",
            "explicitly judge every applicable current task template, including those with no recorded edges; use requirement_ids=[] only when the original source establishes no separate requirement mapping",
        ));
    }
    Ok(())
}

pub(super) struct Evidence {
    pub content: Value,
    pub coverage: Coverage,
}

pub(super) fn assigned_source(
    input: &FrozenInput,
    state: &Checkpoint,
    budget: usize,
) -> Result<Option<String>, String> {
    let Some(id) = state
        .source_review
        .as_ref()
        .and_then(|review| review.active_task.as_ref())
    else {
        return Ok(None);
    };
    Ok(task_inventory(input, state, budget)?
        .into_iter()
        .find(|task| &task.id == id)
        .map(|task| task.source_id))
}

/// Recover original grounds for cross-source relationship subjects that this
/// reviewer has already read. A retained candidate alone cannot replace them.
fn recalled_relationship_sources(
    input: &FrozenInput,
    state: &Checkpoint,
    task: &Task,
    refs: &BTreeSet<String>,
    budget: usize,
) -> Result<Vec<Value>, String> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for id in relationship_records(&state.analysis, refs) {
        for span in &state.analysis.records[&id].sources {
            if span.view_id.is_some()
                || tools::validate_span(input, &state.reviewer_coverage, span).is_err()
                || !seen.insert(digest(span)?)
            {
                continue;
            }
            let in_assigned_fragment = span.source_id == task.source_id
                && match &task.region {
                    Region::Text { start, end } => {
                        span.grid_cell.is_none() && span.start >= *start && span.end <= *end
                    }
                    Region::Grid {
                        form_id,
                        start,
                        end,
                    } => {
                        span.grid_cell
                            .as_ref()
                            .is_some_and(|cell| &cell.form_id == form_id)
                            && tools::validate_grid_span(input, span)
                                .is_ok_and(|offset| offset >= *start && offset < *end)
                    }
                    Region::Empty => false,
                };
            if in_assigned_fragment {
                continue;
            }
            let (name, args, expected_end) = if let Some(cell) = &span.grid_cell {
                let offset = tools::validate_grid_span(input, span)?;
                (
                    "read_form",
                    json!({"form_id":cell.form_id,"offset":offset,"limit":1}),
                    offset + 1,
                )
            } else {
                (
                    "read_source",
                    json!({"source_id":span.source_id,"start":span.start,"max_bytes":span.end-span.start}),
                    span.end,
                )
            };
            let Ok(source) = tools::invoke(
                input,
                &mut Analysis::default(),
                &mut state.reviewer_coverage.clone(),
                true,
                name,
                &args,
                budget,
            ) else {
                continue;
            };
            let end = if span.grid_cell.is_some() {
                &source["next"]
            } else {
                &source["end"]
            };
            if end.as_u64() != Some(expected_end as u64) {
                continue; // Never present a shortened citation as its complete original.
            }
            rows.push(json!({"record_id":id,"citation":evidence_refs::compact(input, span).unwrap_or_else(||json!(span)),"source":source}));
            if serde_json::to_vec(&rows).map_err(|e| e.to_string())?.len() > budget {
                rows.pop();
            }
        }
    }
    Ok(rows)
}

/// Deliver the assigned fragment and complete current candidates together.
/// This only stages reading: the normal response boundary confirms delivery.
pub(super) fn evidence(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Option<Evidence>, String> {
    let Some(review) = state
        .source_review
        .as_ref()
        .filter(|_| state.role == Role::Reviewer)
    else {
        return Ok(None);
    };
    let inventory = task_inventory(input, state, config.limits.max_tool_result_bytes)?;
    let Some(task) = inventory
        .iter()
        .find(|task| Some(&task.id) == review.active_task.as_ref())
    else {
        return Ok(None);
    };
    let budget = config.limits.max_tool_result_bytes;
    let mut coverage = state
        .pending_coverage
        .as_ref()
        .unwrap_or(state.coverage())
        .clone();
    let (name, args) = match &task.region {
        Region::Text { start, end } => (
            "read_source",
            json!({
                "source_id":task.source_id,"start":start,"max_bytes":end-start
            }),
        ),
        Region::Grid {
            form_id,
            start,
            end,
        } => (
            "read_form",
            json!({
                "form_id":form_id,"offset":start,"limit":end-start
            }),
        ),
        Region::Empty => return Ok(None),
    };
    context::check_read_scope(input, state, name, &args)?;
    // Reuse the parser-backed, transactional source tools and the task's
    // original half-budget partition; never infer source text or grid cells.
    let source = tools::invoke(
        input,
        &mut Analysis::default(),
        &mut coverage,
        true,
        name,
        &args,
        budget / 2,
    )?;
    let mut content = json!({"assigned_evidence":{
        "task_id":task.id,"source":source,"candidates":[],"boundary_evidence":[],
        "instruction":"Original frozen source and complete candidate values for the explicitly reported delivery are returned together. Check candidate_delivery: a partial packet is not the complete task inventory. Compare the included values directly; no read/index call is needed for these exact values. boundary_evidence contains bounded adjacent original text/grid ranges, not whole neighboring tasks or a semantic continuation judgment. Check exact returned ranges; read missing continuation content explicitly. This is not a comparison or approval. Unlisted candidates, cross-reference targets, metadata and original pixels still require their reading tools. Save field-level findings or clean comparisons, then judge source omissions and boundaries."
    }});
    // Admit complete candidate groups before optional neighboring context. The
    // immutable source obligation remains intact when its evidence needs more
    // than one delivery; partial delivery is explicit and grants no approval.
    evidence_candidates::append(input, state, task, &mut content, &mut coverage, budget)?;
    // Each boundary read uses an eighth of the existing packet budget,
    // leaving the original half for the assigned source and room for
    // candidates. Include the wrapper in admission before staging receipts.
    for (side, neighbor) in neighbors(input, &inventory, task) {
        let boundary_budget = budget / 8;
        let (name, mut args) = match &neighbor.region {
            Region::Text { start, end } => {
                let source = input
                    .source_units
                    .iter()
                    .find(|source| source.source_unit_revision_id == neighbor.source_id)
                    .ok_or("neighbor source missing")?;
                let mut offset = if side == "before" {
                    end.saturating_sub(boundary_budget / 2).max(*start)
                } else {
                    *start
                };
                while !source.text.is_char_boundary(offset) {
                    offset += 1;
                }
                (
                    "read_source",
                    json!({"source_id":neighbor.source_id,"start":offset,"max_bytes":(end-offset).min(boundary_budget / 2)}),
                )
            }
            Region::Grid {
                form_id,
                start,
                end,
            } => (
                "read_form",
                json!({"form_id":form_id,"offset":start,"limit":end-start}),
            ),
            Region::Empty => continue,
        };
        // This tool explicitly includes these derived boundary ranges. Keep
        // the work scope unchanged: adjacent evidence must not pull unrelated
        // candidates into the assigned task or grant arbitrary follow-up reads.
        if context::check_blocked_scope(state, std::slice::from_ref(&neighbor.source_id)).is_err() {
            continue;
        }
        let mut staged;
        let source = loop {
            staged = coverage.clone();
            let result = tools::invoke(
                input,
                &mut Analysis::default(),
                &mut staged,
                true,
                name,
                &args,
                boundary_budget,
            );
            if let Ok(source) = &result
                && let Region::Text { end, .. } = &neighbor.region
                && side == "before"
                && source["end"]
                    .as_u64()
                    .is_some_and(|actual| actual < *end as u64)
            {
                // read_source may shorten its result for annotation overhead.
                // Retry the smaller suffix so a preceding boundary includes
                // its actual end, and discard receipts from the unseen trial.
                let size = source["end"].as_u64().unwrap() - source["start"].as_u64().unwrap();
                let text = &input
                    .source_units
                    .iter()
                    .find(|source| source.source_unit_revision_id == neighbor.source_id)
                    .unwrap()
                    .text;
                let start = text.ceil_char_boundary(end.saturating_sub(size as usize));
                args["start"] = json!(start);
                args["max_bytes"] = json!(end - start);
                continue;
            }
            break result;
        };
        let Ok(source) = source else {
            continue;
        };
        content["assigned_evidence"]["boundary_evidence"]
            .as_array_mut()
            .unwrap()
            .push(json!({"side":side,"task_id":neighbor.id,"source":source}));
        if serde_json::to_vec(&content)
            .map_err(|error| error.to_string())?
            .len()
            > budget
        {
            content["assigned_evidence"]["boundary_evidence"]
                .as_array_mut()
                .unwrap()
                .pop();
        } else {
            coverage = staged;
        }
    }
    // Existing assigned originals and pending candidates keep priority. Recall
    // only complete previously delivered grounds that fit the same tool budget;
    // neither work scope, dependencies nor reading receipts are expanded.
    let subjects = recalled_relationship_sources(
        input,
        state,
        task,
        &obligations(input, state, task).comparisons,
        budget / 4,
    )?;
    if !subjects.is_empty() {
        content["assigned_evidence"]["subject_evidence"] = json!({
            "items":subjects,
            "instruction":"Previously independently delivered original grounds for listed cross-source relationship subjects, recalled with their exact citations. Compare these originals with the current candidates; cite subject and target grounds when judging a relationship. These are not new reading receipts, complete neighboring sources or approved relationships. Missing grounds still require explicit reading."
        });
        while serde_json::to_vec(&content)
            .map_err(|e| e.to_string())?
            .len()
            > budget
        {
            let items = content["assigned_evidence"]["subject_evidence"]["items"]
                .as_array_mut()
                .unwrap();
            if items.pop().is_none() {
                content["assigned_evidence"]
                    .as_object_mut()
                    .unwrap()
                    .remove("subject_evidence");
                break;
            }
        }
        if content["assigned_evidence"]["subject_evidence"]["items"]
            .as_array()
            .is_some_and(Vec::is_empty)
        {
            content["assigned_evidence"]
                .as_object_mut()
                .unwrap()
                .remove("subject_evidence");
        }
    }
    // Reopened tasks retain their prior semantic work in the journal. Make
    // it available for revision without treating it as current evidence or
    // displacing the original source and pending candidate values above.
    if let Some(prior) = review.results.get(&task.id) {
        content["assigned_evidence"]["prior_judgment"] = json!({
            "judgment":prior.judgment,
            "instruction":"Historical reviewer judgment, not current approval or a reading receipt. Recheck its omissions, boundaries, mappings and relationships against current original evidence, candidates and saved findings. Revise affected conclusions and submit the whole judgment with source_review.current.expected_version. Do not copy the old version or repeat inventories merely to reconstruct this prior result."
        });
        if serde_json::to_vec(&content)
            .map_err(|e| e.to_string())?
            .len()
            > budget
        {
            content["assigned_evidence"]
                .as_object_mut()
                .expect("assigned evidence object")
                .remove("prior_judgment");
        }
    }
    let bytes = serde_json::to_vec(&content)
        .map_err(|e| e.to_string())?
        .len();
    if bytes > budget {
        return Ok(None);
    }
    Ok(Some(Evidence { content, coverage }))
}

pub(super) fn packet(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Value, String> {
    // The reviewer cannot call put_relation, but must describe corrections in
    // the same vocabulary as the primary writer. Derive it from that schema.
    let relation_kind = tools::schemas(false)
        .into_iter()
        .find(|tool| tool["function"]["name"] == "put_relation")
        .ok_or("relationship tool schema missing")?["function"]["parameters"]["properties"]["kind"]
        .clone();
    let inventory = task_inventory(input, state, config.limits.max_tool_result_bytes)?;
    let remaining = pending_from_inventory(input, state, &inventory)?;
    let review = state
        .source_review
        .as_ref()
        .ok_or("source review state missing")?;
    let task = remaining
        .iter()
        .find(|t| Some(&t.id) == review.active_task.as_ref());
    let execution_blocked = !remaining.is_empty() && !has_executable_task(state, &remaining)?;
    let current = task.map(|task| -> Result<Value, String> {
        let adjacent = neighbors(input, &inventory, task);
        let neighbors = json!({
            "previous":adjacent.iter().find(|(side, _)| *side == "before").map(|(_, task)| task),
            "next":adjacent.iter().find(|(side, _)| *side == "after").map(|(_, task)| task),
            "instruction":"Navigation in the frozen document review order only, not evidence or a semantic continuation judgment. read_review_task includes bounded adjacent evidence when it fits; this does not expand the active work scope. Check returned ranges. Use explicit read_source/read_form or read_source_view for missing continuation content, including known frozen supporting sources outside the task scope. Supporting reads grant no other task or write authority. A null neighbor does not resolve cross-document references. Navigation grants no reading receipt, comparison or approval."
        });
        let dependencies = if Some(&task.id) == review.active_task.as_ref() { review.dependencies.clone() } else { task_dependencies(task) };
        let owned = obligations(input, state, task);
        let refs = &owned.comparisons;
        let layout_view_required = requires_layout_view(input, state, task, &dependencies);
        let mut pending_refs = Vec::new();
        let mut candidates_with_findings = Vec::new();
        for key in refs {
            if !context::has_review_outcome(state, key)? {
                pending_refs.push(key.clone());
            }
            let findings = context::candidate_findings(state, key);
            if !findings.is_empty() {
                candidates_with_findings.push(json!({"reference":key,"finding_ids":findings}));
            }
        }
        let mapped_templates: BTreeSet<_> = state.analysis.relations.values()
            .filter(|relation| relation.kind == RelationKind::RequiresTemplate)
            .map(|relation| relation.to.as_str()).collect();
        let unmapped_templates: Vec<_> = owned.subjects.iter().filter(|key| {
            let Some(id) = key.strip_prefix("record:") else { return false; };
            matches!(state.analysis.records.get(id).map(|record| &record.data),
                Some(RecordData::Template { applicability, .. })
                    if applicability.state != ApplicabilityState::NotApplicable)
                && !mapped_templates.contains(id)
        }).cloned().collect();
        let findings = required_findings(state, task, &owned.sources, refs);
        let completion = json!({
            "status_if_evidence_complete":if findings.is_empty() {"checked"} else {"findings"},
            "required_finding_ids":tools::bounded_page(&findings.into_iter().collect::<Vec<_>>(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "instruction":"Keep these saved findings, including cross-source comparison dependencies and prior assigned findings. Retrieve details with inspect_review; revise or withdraw unsupported findings only after checking their evidence. If evidence is still missing use needs_evidence. This list does not judge source completeness or approve the analysis; new citations or candidate_refs can expand the required set."
        });
        Ok(json!({"task":task,"neighbors":neighbors,"expected_version":version(state,&dependencies)?,"completion":completion,
            "layout_view":layout_view_required.then(||json!({"source_id":task.source_id,
                "instruction":"This page contributes text and tables to one or more templates. Independently read its original pixels with read_source_view, or reuse a delivered view of the same document page. Compare title/table/note/signature interleaving across all these templates, including separate text-only and grid-only records and fields already compared. Cite the visual source in put_source_review. Template instructions cannot change the emitted order. Missing or undelivered pixels require needs_evidence; a navigation entry is not visual evidence."})),
            "prior_status":review.results.get(&task.id).map(|r|&r.judgment.status),
            "prior_boundary_error":review.results.get(&task.id).and_then(|r|validate_boundaries(input,state,task,&r.judgment).err()),
            "prior_result_stale":review.results.get(&task.id).map(|r|version(state,&r.dependencies).map(|v|v!=r.version)).transpose()?,
            "evidence_requests":tools::bounded_page(&review.results.get(&task.id).map(|r|r.judgment.evidence_requests.clone()).unwrap_or_default(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "templates_without_requirement_mapping":tools::bounded_page(&unmapped_templates,0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "templates_requiring_mapping_judgment":tools::bounded_page(&templates(&state.analysis,&owned.subjects).into_iter().collect::<Vec<_>>(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "records_requiring_relationship_judgment":tools::bounded_page(&relationship_records(&state.analysis,&owned.subjects).into_iter().collect::<Vec<_>>(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "task_scope_instruction":"These required judgments belong to this original fragment or its jointly viewed page. Directly incident relation endpoints remain comparison evidence. Looking up other candidates records dependencies but does not assign their whole-source judgments here; their own source tasks remain mandatory before global acceptance.",
            "relationship_instruction":"Explicitly judge each listed rule, unresolved item and conditional/unknown/not-applicable requirement or template. Also check other records for omitted references. Independently retrieve actual selected conditions, cross-reference endpoints and continuation sources with bounded searches and inspect_analysis. Explain whether current applicability incorporates those choices. A governing condition in another record or field can require a relationship even on the same page or table; absence of a see-also phrase does not prove independence. Existing edges alone do not establish correctness. Use the relation_kind contract below when describing corrections. Use resolved with actual edges, not_required only when the source establishes no relationship to another semantic object, source_limited for documented remaining ambiguity, or findings for saved mistakes. A frozen continuation candidate must be read before calling it unavailable; do not invent edges or withdraw valid findings to finish.",
            "mapping_instruction":"Check these templates for missing source-required relationships, even when all existing candidates compare correctly. When a separate output requirement calls for a prescribed form, verify its requires_template mapping to that form or a source-grounded parent mapping; record existence and a matching prose label are not a mapping. Inspect the source, parent and response/proof entries before saving a missing-mapping finding. Standalone or non-output templates may legitimately have no requirement edge: do not invent a requirement or duplicate edge merely to empty this navigation list. Page candidate and relation details with inspect_analysis when needed.",
            "comparison_total":refs.len(),
            "candidates_with_findings":tools::bounded_page(&candidates_with_findings,0,usize::MAX,config.limits.max_tool_result_bytes/8)?,
            "pending_candidate_refs":tools::bounded_page(&pending_refs,0,usize::MAX,config.limits.max_tool_result_bytes/2)?}))
    }).transpose()?;
    Ok(
        json!({"remaining":remaining.len(),"current":current,"execution_blocked":execution_blocked,"relation_kind":relation_kind,
        "instruction":"Use pending_candidate_refs for remaining candidate comparisons; completed references are omitted from this work roster but remain available through inspect_analysis. candidates_with_findings already have recorded problem outcomes: retain their findings, do not call complete_review_check to mark them clean. An empty pending list calls for source-to-result omission checking and an explicit put_source_review judgment, not another inventory of completed comparisons. Compare the entire fragment, its headings/table notes and continuation boundaries against current candidates. Save omitted or incorrect content as findings. Reading and candidate checks do not establish source completeness. The host aggregates only after all required current judgments; no empty final submission is needed."}),
    )
}

/// Valid only while executing one complete response. Rebuilt from the prepared
/// request on replay; never serialized or carried into a later model turn.
pub(super) struct BatchVersion {
    task_id: String,
    expected_version: String,
    semantic_version: String,
}

impl BatchVersion {
    pub(super) fn capture(state: &Checkpoint, body: &Value) -> Result<Option<Self>, String> {
        let Some(review) = state
            .source_review
            .as_ref()
            .filter(|_| state.role == Role::Reviewer)
        else {
            return Ok(None);
        };
        let packet = body["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .filter(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .and_then(|content| serde_json::from_str::<Value>(content).ok());
        let Some(current) = packet
            .as_ref()
            .map(|packet| &packet["source_review"]["current"])
        else {
            return Ok(None);
        };
        let expected_version = version(state, &review.dependencies)?;
        let Some(task_id) = review.active_task.as_ref().filter(|task_id| {
            current["task"]["id"] == **task_id && current["expected_version"] == expected_version
        }) else {
            return Ok(None);
        };
        Ok(Some(Self {
            task_id: task_id.clone(),
            expected_version,
            semantic_version: dependency_version(state, &review.dependencies, false)?,
        }))
    }

    fn admits(
        &self,
        state: &Checkpoint,
        judgment: &Judgment,
        dependencies: &Dependencies,
    ) -> Result<bool, String> {
        Ok(self.task_id == judgment.task_id
            && self.expected_version == judgment.expected_version
            && self.semantic_version == dependency_version(state, dependencies, false)?)
    }
}

pub(super) fn put_in_batch(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    args: &Value,
    batch: Option<&BatchVersion>,
) -> Result<Value, String> {
    let judgment: Judgment =
        serde_json::from_value(args.clone()).map_err(|e| tools::field_error("", e))?;
    let review = state
        .source_review
        .as_ref()
        .ok_or("source review state missing")?;
    if review.active_task.as_ref() != Some(&judgment.task_id) {
        return Err(tools::field_error(
            "/task_id",
            "use the active source review task",
        ));
    }
    let task = task_inventory(input, state, config.limits.max_tool_result_bytes)?
        .into_iter()
        .find(|t| t.id == judgment.task_id)
        .ok_or("unknown source task")?;
    let mut dependencies = review.dependencies.clone();
    // Earlier finding edits in this response may change revisions. Only the
    // exact prepared version is eligible, and only while candidate content and
    // query scope are unchanged. All final findings and evidence checks below
    // still apply, and the receipt records the final dependency version.
    if judgment.expected_version != version(state, &dependencies)?
        && !batch
            .map(|batch| batch.admits(state, &judgment, &dependencies))
            .transpose()?
            .unwrap_or(false)
    {
        return Err(tools::field_error(
            "/expected_version",
            "stale dependency version; use the current task packet",
        ));
    }
    if judgment.summary.trim().is_empty() {
        return Err(tools::field_error(
            "/summary",
            "explain the source-to-result comparison",
        ));
    }
    for (index, request) in judgment.evidence_requests.iter().enumerate() {
        if request.question.trim().is_empty()
            || request.source_ids.is_empty()
            || request.source_ids.iter().any(|id| {
                !input
                    .source_units
                    .iter()
                    .any(|s| s.source_unit_revision_id == *id)
            })
        {
            return Err(tools::field_error(
                &format!("/evidence_requests/{index}"),
                "name a concrete question and valid frozen source scope",
            ));
        }
    }
    let waiting = judgment.status == JudgmentStatus::NeedsEvidence;
    if waiting == judgment.evidence_requests.is_empty() {
        return Err(tools::field_error(
            "/evidence_requests",
            "only needs_evidence requires nonempty pending questions",
        ));
    }
    validate_boundaries(input, state, &task, &judgment)?;
    // All source citations refer to frozen original content. Repeating a
    // boundary in outer sources must not require comparing every candidate on
    // that page. Task/query scope and explicit candidate/mapping/relationship
    // IDs establish mutable dependencies; original provenance stays in the
    // judgment and is bound to input_sha256 and independent reading receipts.
    for source in judgment
        .boundaries
        .before
        .sources
        .iter()
        .chain(&judgment.boundaries.after.sources)
    {
        tools::validate_span(input, &state.reviewer_coverage, source)?;
    }
    for source in &judgment.sources {
        tools::validate_span(input, &state.reviewer_coverage, source)?;
    }
    for key in &judgment.candidate_refs {
        let value = context::reference(&state.analysis, key)?;
        if state.reviewer_coverage.candidate.get(key) != Some(&digest(&value)?) {
            return Err(tools::field_error(
                "/candidate_refs",
                "independently retrieve the current candidate",
            ));
        }
        dependencies.references.insert(key.clone());
    }
    let owned = obligations(input, state, &task);
    validate_template_subjects(input, state, &judgment, &mut dependencies, &owned.subjects)?;
    validate_relationship_subjects(input, state, &judgment, &mut dependencies, &owned.subjects)?;
    validate_layout_view(input, state, &task, &dependencies, &judgment)?;
    let refs = owned.comparisons;
    let mut relevant_findings = required_findings(state, &task, &owned.sources, &refs);
    // A missing cross-source edge may have only original evidence, with no
    // affected candidate ID. The nested validators above already established
    // its relevance and independent evidence. Admit those exact findings
    // without importing unrelated candidates or findings from the cited page.
    relevant_findings.extend(
        judgment
            .template_mappings
            .iter()
            .flat_map(|mapping| &mapping.finding_ids)
            .chain(
                judgment
                    .relationship_checks
                    .iter()
                    .flat_map(|check| &check.finding_ids),
            )
            .cloned(),
    );
    let finding_ids: BTreeSet<_> = judgment.finding_ids.iter().cloned().collect();
    if finding_ids.len() != judgment.finding_ids.len() {
        return Err(tools::field_error(
            "/finding_ids",
            "use distinct saved findings affecting this task",
        ));
    }
    if !finding_ids.is_subset(&relevant_findings) {
        return Err(tools::field_error(
            "/finding_ids",
            json!({
                "unexpected_finding_ids":tools::bounded_page(
                    &finding_ids.difference(&relevant_findings).cloned().collect::<Vec<_>>(),
                    0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
                "instruction":"These IDs are not saved findings affecting this task. Correct the submitted IDs; this does not withdraw any stored finding."
            }),
        ));
    }
    if !waiting {
        for id in &finding_ids {
            validate_finding(input, state, &state.review_draft[id]).map_err(|error| {
                tools::field_error(
                    "/finding_ids",
                    format!("saved finding {id} must be revised or explicitly withdrawn: {error}"),
                )
            })?;
        }
        if let Some(gap) = tools::reading_gaps(input, &state.reviewer_coverage)
            .into_iter()
            .find(|g| g["kind"] == "unread_metadata")
        {
            return Err(format!(
                "independently read collection metadata before judging the source: {}",
                gap["collection"]
            ));
        }
        for (id, view) in &state.analysis.coverage.views {
            if dependencies.source_ids.contains(&view.source_id)
                && state.reviewer_coverage.views.get(id) != Some(view)
            {
                return Err(format!(
                    "independently inspect the original source view: {id}"
                ));
            }
        }
        if (judgment.status == JudgmentStatus::Findings) != !finding_ids.is_empty()
            || finding_ids != relevant_findings
        {
            let missing = tools::bounded_page(
                &relevant_findings
                    .difference(&finding_ids)
                    .cloned()
                    .collect::<Vec<_>>(),
                0,
                usize::MAX,
                config.limits.max_tool_result_bytes / 8,
            )?;
            let required = tools::bounded_page(
                &relevant_findings.into_iter().collect::<Vec<_>>(),
                0,
                usize::MAX,
                config.limits.max_tool_result_bytes / 8,
            )?;
            return Err(tools::field_error(
                "/finding_ids",
                json!({"expected_status":if required["total"] == 0 {"checked"} else {"findings"},
                    "missing_finding_ids":missing,
                    "required_finding_ids":required,
                    "instruction":"Add missing_finding_ids to your submitted list and retain the existing IDs, including cross-source comparison dependencies. inspect_review provides details and further IDs when paged. A findings judgment completes comparison, not approval; do not withdraw a finding merely to sign checked."}),
            ));
        }
        for key in &refs {
            if state.reviewer_coverage.candidate.get(key)
                != Some(&digest(&context::reference(&state.analysis, key)?)?)
            {
                return Err(format!("independently inspect task candidate: {key}"));
            }
            if !context::has_review_outcome(state, key)? {
                return Err(format!(
                    "record the current candidate comparison before finishing source review: {key}"
                ));
            }
        }
        match &task.region {
            Region::Text { start, end } => {
                if !tools::contains(
                    state.reviewer_coverage.text.get(&task.source_id),
                    *start,
                    *end,
                ) {
                    return Err("read the complete source fragment first".into());
                }
                if !judgment.sources.iter().any(|s| {
                    s.source_id == task.source_id
                        && (s.view_id.is_some()
                            || (s.grid_cell.is_none() && s.start < *end && s.end > *start))
                }) {
                    return Err(tools::field_error(
                        "/sources",
                        "cite the current text fragment, not unrelated evidence",
                    ));
                }
            }
            Region::Grid {
                form_id,
                start,
                end,
            } => {
                if !tools::contains(
                    state.reviewer_coverage.form_cells.get(form_id),
                    *start,
                    *end,
                ) {
                    return Err("read every cell in the source fragment first".into());
                }
                if !judgment.sources.iter().any(|s| {
                    s.source_id == task.source_id
                        && (s.view_id.is_some()
                            || s.grid_cell
                                .as_ref()
                                .is_some_and(|cell| cell.form_id == *form_id))
                }) {
                    return Err(tools::field_error(
                        "/sources",
                        "cite the original grid being compared",
                    ));
                }
            }
            Region::Empty => {
                if !state
                    .reviewer_coverage
                    .views
                    .values()
                    .any(|v| v.source_id == task.source_id)
                    && (judgment.status == JudgmentStatus::Checked
                        || !state
                            .reviewer_coverage
                            .view_failures
                            .contains_key(&task.source_id))
                {
                    return Err(
                        "inspect the original empty source or retain its verified view failure"
                            .into(),
                    );
                }
            }
        }
        if judgment.sources.is_empty() && !matches!(task.region, Region::Empty) {
            return Err(tools::field_error(
                "/sources",
                "cite the compared original fragment",
            ));
        }
    }
    let version = version(state, &dependencies)?;
    let receipt_key = digest(&json!([
        "source_review",
        task.id,
        version,
        judgment.status,
        finding_ids
    ]))?;
    let fresh = !waiting && !state.reviewer_progress.seen.contains(&receipt_key);
    let result = json!({"task_id":task.id,"status":judgment.status,"new_completion":fresh,"completion_sha256":receipt_key});
    if serde_json::to_vec(args).map_err(|e| e.to_string())?.len()
        > config.limits.max_tool_result_bytes
    {
        return Err("source judgment exceeds frozen result budget; shorten the explanation".into());
    }
    let review = state.source_review.as_mut().expect("checked source review");
    review.dependencies = dependencies.clone();
    review.results.insert(
        task.id,
        Receipt {
            judgment,
            dependencies,
            version,
        },
    );
    if fresh {
        state.reviewer_progress.seen.insert(receipt_key);
    }
    Ok(result)
}


