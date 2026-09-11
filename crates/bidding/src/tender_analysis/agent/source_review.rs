//! Independent source-to-result judgments. Tasks are derived from immutable
//! source geometry; neither reading receipts nor candidate checks finish them.
use super::*;
use std::collections::BTreeSet;

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
    /// Unscoped searches include negative results. Any semantic edit can
    /// change their answer, even if no old candidate ID was returned.
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
    })
}

pub(super) fn references(analysis: &Analysis, dependencies: &Dependencies) -> BTreeSet<String> {
    dependency_references(analysis, dependencies)
        .into_iter()
        .filter(|key| context::reference(analysis, key).is_ok())
        .collect()
}

// Deleted IDs remain version dependencies, but cannot be inspected or compared
// as current candidates. Their findings still require explicit reviewer action.
fn dependency_references(analysis: &Analysis, dependencies: &Dependencies) -> BTreeSet<String> {
    let scope: Vec<_> = dependencies.source_ids.iter().cloned().collect();
    let mut refs: BTreeSet<_> = context::scope_references(analysis, &scope)
        .into_iter()
        .collect();
    refs.extend(dependencies.references.iter().cloned());
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
    digest(
        &json!({"input":state.input_sha256,"sources":dependencies.source_ids,"references":refs,
        "rules":rules,"finding_revisions":revisions,
        "global":dependencies.global.then(||semantic_analysis(&state.analysis))}),
    )
}

pub(in crate::tender_analysis) fn candidate_version(
    state: &Checkpoint,
    key: &str,
) -> Result<String, String> {
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
    let revisions: BTreeMap<_, _> = state
        .source_review
        .as_ref()
        .into_iter()
        .flat_map(|review| &review.candidate_revisions)
        .filter(|(reference, _)| {
            dependencies.references.contains(*reference)
                || reference
                    .strip_prefix("record:")
                    .and_then(|id| state.analysis.records.get(id))
                    .is_some_and(|record| matches!(record.data, RecordData::Rule { .. }))
        })
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
    let task = tasks(input, max_bytes)?
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
        {
            return Err(tools::field_error(
                &format!("{path}/state"),
                "unresolved cannot be checked: keep the unresolved boundary and use status=needs_evidence with a concrete question and frozen source_ids while searching. If the frozen collection cannot supply the missing evidence, save that source limitation with put_review_finding, then use status=findings with its finding_id after completing the comparison. Do not relabel the boundary complete or continuation without evidence",
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
                "continuation requires a read citation beyond the current fragment on this side or in a distinct source. If no target has been established, use boundary state=unresolved and status=needs_evidence with a concrete question and frozen source_ids; if the frozen collection lacks the evidence, save a source-grounded put_review_finding and submit status=findings after completing the comparison. The current fragment alone is not a continuation target",
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
    Ok(receipt.judgment.task_id == task.id
        && receipt.dependencies.source_ids.contains(&task.source_id)
        && receipt.judgment.status != JudgmentStatus::NeedsEvidence
        && validate_boundaries(input, state, task, &receipt.judgment).is_ok()
        && validate_layout_view(input, state, task, &dependencies, &receipt.judgment).is_ok()
        && validate_relationship_checks(input, state, &receipt.judgment, &mut dependencies).is_ok()
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
    let tasks = tasks(input, config.limits.max_tool_result_bytes)?;
    let review = state
        .source_review
        .as_ref()
        .ok_or("independent source review state missing")?;
    if review.schema_version != 1 || review.manifest_sha256 != digest(&tasks)? {
        return Err("independent source task manifest changed".into());
    }
    tasks
        .into_iter()
        .filter_map(|task| match complete(input, state, &task) {
            Ok(true) => None,
            Ok(false) => Some(Ok(task)),
            Err(e) => Some(Err(e)),
        })
        .collect()
}

/// Track the actual scope of successful queries, including zero-result ones.
/// This state is committed with the same tool batch; it is not reading coverage.
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
        "read_source" | "read_source_view" => {
            if let Some(id) = args["source_id"].as_str() {
                dependencies.source_ids.insert(id.into());
                if name == "read_source_view" {
                    dependencies.source_ids.extend(
                        input
                            .source_units
                            .iter()
                            .filter(|source| same_page(input, id, &source.source_unit_revision_id))
                            .map(|source| source.source_unit_revision_id.clone()),
                    );
                }
            }
        }
        "read_form" => {
            if let Some(id) = input
                .structured_forms
                .iter()
                .find(|f| f["form_definition_revision_id"] == args["form_id"])
                .and_then(|f| f["source_unit_revision_id"].as_str())
            {
                dependencies.source_ids.insert(id.into());
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
            } else if let Some(scope) = scope {
                dependencies.source_ids.extend(scope);
            } else {
                dependencies.global = true;
            }
        }
        // Source searches do not select candidate IDs; conservatively include
        // the full semantic set so a previously absent target cannot stay clean.
        "search_sources" => dependencies.global = true,
        _ => {}
    }
    expand_view_dependencies(input, &state.reviewer_coverage, dependencies);
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
    let executable: Vec<_> = pending
        .iter()
        .filter(|task| {
            context::check_blocked_scope(state, std::slice::from_ref(&task.source_id)).is_ok()
        })
        .collect();
    let review = state
        .source_review
        .as_ref()
        .ok_or("source review state missing")?;
    if review
        .active_task
        .as_ref()
        .is_some_and(|id| executable.iter().any(|task| &task.id == id))
    {
        return Ok(());
    }
    let next = executable.first().copied();
    let mut dependencies = next
        .map(|task| {
            review
                .results
                .get(&task.id)
                .map(|r| r.dependencies.clone())
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
        let source_scope = vec![task.source_id.clone()];
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
    dependencies: &Dependencies,
    refs: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut findings: BTreeSet<_> = state
        .review_draft
        .iter()
        .filter(|(_, f)| {
            f.sources
                .iter()
                .any(|s| dependencies.source_ids.contains(&s.source_id))
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
    templates(&state.analysis, &references(&state.analysis, dependencies))
        .iter()
        .any(|key| {
            let RecordData::Template { regions, .. } = &state.analysis.records[key].data else {
                return false;
            };
            let on_page = |region: &&TemplateRegion| {
                same_page(input, &task.source_id, &region.source.source_id)
            };
            regions.iter().filter(on_page).any(|r| r.form_id.is_some())
                && regions.iter().filter(on_page).any(|r| r.form_id.is_none())
        })
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
                "Compare the mixed text/table template's emitted region order with original page pixels and cite the delivered visual citation. Use read_source_view with source_id={} or reuse an independently delivered view of the same document page. Parsed source order cannot establish interleaving. If the original view is unavailable, keep status=needs_evidence with the concrete layout question; do not sign a completed layout comparison.",
                task.source_id
            ),
        ));
    }
    Ok(())
}

fn shared_mapping_evidence(a: &Span, b: &Span) -> bool {
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
                Some(
                    RecordData::Requirement { applicability, .. }
                    | RecordData::Template { applicability, .. },
                ) => applicability.state != ApplicabilityState::Applicable,
                _ => false,
            },
        )
        .map(str::to_owned)
        .collect()
}

/// This validates explicit evidence, not the model's semantic conclusion. A
/// source-grounded decision may legitimately need no edge or retain ambiguity.
fn validate_relationship_checks(
    input: &FrozenInput,
    state: &Checkpoint,
    judgment: &Judgment,
    dependencies: &mut Dependencies,
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
        if check.reason.trim().is_empty() || check.sources.is_empty() {
            return Err(fail(
                "explain the actual reference targets, selected conditions and continuation interpretation using independently read sources",
            ));
        }
        for source in &check.sources {
            tools::validate_span(input, &state.reviewer_coverage, source)
                .map_err(|error| tools::field_error(&path, error))?;
            dependencies.source_ids.insert(source.source_id.clone());
        }
        if !subject.sources.iter().any(|source| {
            check
                .sources
                .iter()
                .any(|span| shared_mapping_evidence(source, span))
        }) {
            return Err(fail(
                "cite the subject record's original evidence, not just an unrelated target",
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
            dependencies
                .source_ids
                .extend(finding.sources.iter().map(|span| span.source_id.clone()));
        }
        if (check.status == RelationshipStatus::Findings) != !check.finding_ids.is_empty() {
            return Err(fail(
                "use findings with saved relationship problems; otherwise leave this check's finding_ids empty",
            ));
        }
        match check.status {
            RelationshipStatus::Resolved => {
                if matches!(subject.data, RecordData::Unresolved { .. })
                    || check.relation_ids.is_empty()
                    || !check.unresolved_record_ids.is_empty()
                    || check
                        .relation_ids
                        .iter()
                        .any(|id| state.analysis.relations[id].state == RelationState::Unresolved)
                    || check.related_record_ids.iter().any(|id| {
                        !check.relation_ids.iter().any(|edge| {
                            let edge = &state.analysis.relations[edge];
                            edge.from == *id || edge.to == *id
                        })
                    })
                {
                    return Err(fail(
                        "resolved requires actual resolved edges to every listed target and consistent applicability/continuation interpretation; report a stale unresolved record as a finding",
                    ));
                }
            }
            RelationshipStatus::NotRequired => {
                if matches!(subject.data, RecordData::Unresolved { .. })
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
                        "not_required needs a source-grounded absence of external relationships; existing edges or unresolved records must be examined, not dismissed",
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
        && let Some(id) =
            relationship_records(&state.analysis, &references(&state.analysis, dependencies))
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

/// The reviewer decides which obligations use each template. The host checks
/// that this explicit decision is supported by current edges or saved findings.
/// An empty obligation list is permitted with a source-grounded explanation;
/// neither names nor co-location imply a semantic relationship.
fn validate_template_mappings(
    input: &FrozenInput,
    state: &Checkpoint,
    judgment: &Judgment,
    dependencies: &mut Dependencies,
) -> Result<(), String> {
    let expected = templates(&state.analysis, &references(&state.analysis, dependencies));
    let mut seen = BTreeSet::new();
    for (index, mapping) in judgment.template_mappings.iter().enumerate() {
        let path = format!("/template_mappings/{index}");
        if !expected.contains(&mapping.template_id) || !seen.insert(mapping.template_id.clone()) {
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
            dependencies.source_ids.insert(source.source_id.clone());
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
                    "this finding has no shared mapping evidence. If the listed mapping is correct, use finding_ids=[] for this mapping and keep unrelated template-content findings in the source judgment finding_ids. Do not invent a mapping problem; missing or incorrect mappings still require a saved source-backed finding",
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
    if judgment.status != JudgmentStatus::NeedsEvidence && seen != expected {
        return Err(tools::field_error(
            "/template_mappings",
            "explicitly judge every applicable current task template, including those with no recorded edges; use requirement_ids=[] only when the original source establishes no separate requirement mapping",
        ));
    }
    Ok(())
}

pub(super) fn packet(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Value, String> {
    let remaining = pending(input, config, state)?;
    let review = state
        .source_review
        .as_ref()
        .ok_or("source review state missing")?;
    let task = remaining
        .iter()
        .find(|t| Some(&t.id) == review.active_task.as_ref())
        .or(remaining.first());
    let current = task.map(|task| -> Result<Value, String> {
        let dependencies = if Some(&task.id) == review.active_task.as_ref() { review.dependencies.clone() } else { task_dependencies(task) };
        let refs = references(&state.analysis, &dependencies);
        let layout_view_required = requires_layout_view(input, state, task, &dependencies);
        let mut pending_refs = Vec::new();
        for key in &refs {
            if !context::has_review_outcome(state, key)? {
                pending_refs.push(key.clone());
            }
        }
        let mapped_templates: BTreeSet<_> = state.analysis.relations.values()
            .filter(|relation| relation.kind == RelationKind::RequiresTemplate)
            .map(|relation| relation.to.as_str()).collect();
        let unmapped_templates: Vec<_> = refs.iter().filter(|key| {
            let Some(id) = key.strip_prefix("record:") else { return false; };
            matches!(state.analysis.records.get(id).map(|record| &record.data),
                Some(RecordData::Template { applicability, .. })
                    if applicability.state != ApplicabilityState::NotApplicable)
                && !mapped_templates.contains(id)
        }).cloned().collect();
        let findings = required_findings(state, task, &dependencies, &refs);
        let completion = json!({
            "status_if_evidence_complete":if findings.is_empty() {"checked"} else {"findings"},
            "required_finding_ids":tools::bounded_page(&findings.into_iter().collect::<Vec<_>>(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "instruction":"Keep these saved findings, including cross-source comparison dependencies and prior assigned findings. Retrieve details with inspect_review; revise or withdraw unsupported findings only after checking their evidence. If evidence is still missing use needs_evidence. This list does not judge source completeness or approve the analysis; new citations or candidate_refs can expand the required set."
        });
        Ok(json!({"task":task,"expected_version":version(state,&dependencies)?,"completion":completion,
            "layout_view":layout_view_required.then(||json!({"source_id":task.source_id,
                "instruction":"This page contributes both text and a table to a template. Independently read its original pixels with read_source_view, or reuse a delivered view of the same document page. Compare the actual regions emission sequence with title/table/note/signature interleaving, including templates whose individual fields already have clean comparisons. Cite the visual source in put_source_review. Template instructions cannot change the emitted order. Missing or undelivered pixels require needs_evidence; a navigation entry is not visual evidence."})),
            "prior_status":review.results.get(&task.id).map(|r|&r.judgment.status),
            "prior_boundary_error":review.results.get(&task.id).and_then(|r|validate_boundaries(input,state,task,&r.judgment).err()),
            "prior_result_stale":review.results.get(&task.id).map(|r|version(state,&r.dependencies).map(|v|v!=r.version)).transpose()?,
            "evidence_requests":tools::bounded_page(&review.results.get(&task.id).map(|r|r.judgment.evidence_requests.clone()).unwrap_or_default(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "templates_without_requirement_mapping":tools::bounded_page(&unmapped_templates,0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "templates_requiring_mapping_judgment":tools::bounded_page(&templates(&state.analysis,&refs).into_iter().collect::<Vec<_>>(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "records_requiring_relationship_judgment":tools::bounded_page(&relationship_records(&state.analysis,&refs).into_iter().collect::<Vec<_>>(),0,usize::MAX,config.limits.max_tool_result_bytes/4)?,
            "relationship_instruction":"Explicitly judge each listed rule, unresolved item and conditional/unknown/not-applicable requirement or template. Also check other records for omitted references. Independently retrieve actual selected conditions, cross-reference endpoints and continuation sources with bounded searches and inspect_analysis. Explain whether current applicability incorporates those choices. Existing edges alone do not establish correctness. Use resolved with actual edges, not_required only when the source requires no external relationship, source_limited for documented remaining ambiguity, or findings for saved mistakes. A frozen continuation candidate must be read before calling it unavailable; do not invent edges or withdraw valid findings to finish.",
            "mapping_instruction":"Check these templates for missing source-required relationships, even when all existing candidates compare correctly. When a separate output requirement calls for a prescribed form, verify its requires_template mapping to that form or a source-grounded parent mapping; record existence and a matching prose label are not a mapping. Inspect the source, parent and response/proof entries before saving a missing-mapping finding. Standalone or non-output templates may legitimately have no requirement edge: do not invent a requirement or duplicate edge merely to empty this navigation list. Page candidate and relation details with inspect_analysis when needed.",
            "comparison_total":refs.len(),
            "pending_candidate_refs":tools::bounded_page(&pending_refs,0,usize::MAX,config.limits.max_tool_result_bytes/2)?}))
    }).transpose()?;
    Ok(json!({"remaining":remaining.len(),"current":current,
        "instruction":"Use pending_candidate_refs for remaining candidate comparisons; completed references are omitted from this work roster but remain available through inspect_analysis. An empty pending list calls for source-to-result omission checking and an explicit put_source_review judgment, not another inventory of completed comparisons. Compare the entire fragment, its headings/table notes and continuation boundaries against current candidates. Save omitted or incorrect content as findings. Reading and candidate checks do not establish source completeness. The host aggregates only after all required current judgments; no empty final submission is needed."}))
}

pub(super) fn put(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    args: &Value,
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
    let task = tasks(input, config.limits.max_tool_result_bytes)?
        .into_iter()
        .find(|t| t.id == judgment.task_id)
        .ok_or("unknown source task")?;
    let mut dependencies = review.dependencies.clone();
    if judgment.expected_version != version(state, &dependencies)? {
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
    for source in judgment
        .sources
        .iter()
        .chain(&judgment.boundaries.before.sources)
        .chain(&judgment.boundaries.after.sources)
    {
        tools::validate_span(input, &state.reviewer_coverage, source)?;
        dependencies.source_ids.insert(source.source_id.clone());
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
    validate_template_mappings(input, state, &judgment, &mut dependencies)?;
    validate_relationship_checks(input, state, &judgment, &mut dependencies)?;
    validate_layout_view(input, state, &task, &dependencies, &judgment)?;
    let refs = references(&state.analysis, &dependencies);
    let relevant_findings = required_findings(state, &task, &dependencies, &refs);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (FrozenInput, Config, Checkpoint) {
        let input = FrozenInput {
            schema_version: 1,
            project_id: "test".into(),
            document_set_id: "test".into(),
            documents: vec![],
            document_relations: vec![],
            decisions: vec![],
            structured_forms: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "document".into(),
                text: "项目背景介绍。".into(),
                locator: json!({}),
                ordinal: 0,
            }],
        };
        let config = crate::tender_analysis::tests::config();
        let mut analysis = Analysis::default();
        tools::cover(
            analysis.coverage.text.entry("source".into()).or_default(),
            0,
            input.source_units[0].text.len(),
        );
        analysis.dispositions.insert(
            "source".into(),
            Disposition {
                state: DispositionState::NonRequirement,
                reason: "背景介绍".into(),
            },
        );
        let mut coverage = analysis.coverage.clone();
        let value = context::reference(&analysis, "disposition:source").unwrap();
        coverage
            .candidate
            .insert("disposition:source".into(), digest(&value).unwrap());
        let mut state = Checkpoint {
            journal: Default::default(),
            input_sha256: digest(&input).unwrap(),
            config_sha256: digest(&config).unwrap(),
            turn: 0,
            tool_calls: 0,
            read_bytes: 0,
            review_rounds: 0,
            role: Role::Reviewer,
            analysis,
            review: None,
            review_draft: BTreeMap::new(),
            source_review: Some(initialize(&input, &config).unwrap()),
            reviewer_coverage: coverage,
            pending_coverage: None,
            transcript: vec![],
            main_progress: Default::default(),
            reviewer_progress: Default::default(),
            main_work: None,
            reviewer_work: None,
            done: false,
            source_views: BTreeMap::new(),
        };
        select_next(&input, &config, &mut state).unwrap();
        compare(&input, &config, &mut state);
        (input, config, state)
    }

    fn citation(input: &FrozenInput) -> Span {
        Span {
            source_id: "source".into(),
            start: 0,
            end: input.source_units[0].text.len(),
            view_id: None,
            grid_cell: None,
        }
    }

    fn compare(input: &FrozenInput, config: &Config, state: &mut Checkpoint) {
        context::complete_review_check(
            input,
            state,
            &json!({"reference":"disposition:source",
            "summary":"原文为项目背景，没有独立投标输出义务。","sources":[citation(input)]}),
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
    }

    fn judgment(input: &FrozenInput, config: &Config, state: &Checkpoint) -> Value {
        let packet = packet(input, config, state).unwrap();
        let boundary = json!({"state":"complete","reason":"完整段落，与相邻内容无未解决的续接。","sources":[citation(input)]});
        json!({"task_id":packet["current"]["task"]["id"],"expected_version":packet["current"]["expected_version"],
            "status":"checked","summary":"逐段比较后未见遗漏的投标要求；来源处置与背景内容一致。",
            "sources":[citation(input)],"candidate_refs":["disposition:source"],
            "template_mappings":[],"boundaries":{"before":boundary,"after":boundary},"finding_ids":[],"evidence_requests":[]})
    }

    fn relationship_fixture() -> (FrozenInput, Config, Checkpoint) {
        let (input, config, mut state) = fixture();
        state.analysis.records.insert(
            "rule".into(),
            Record {
                id: "rule".into(),
                sources: vec![citation(&input)],
                data: RecordData::Rule {
                    text: "项目背景约定".into(),
                    scope: "项目".into(),
                    applicability: Applicability {
                        state: ApplicabilityState::Applicable,
                        condition: String::new(),
                        scope: "项目".into(),
                        grounds: vec![citation(&input)],
                    },
                },
            },
        );
        let key = "record:rule";
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        context::complete_review_check(&input, &mut state,
            &json!({"reference":key,"summary":"Synthetic rule text comparison.","sources":[citation(&input)]}),
            config.limits.max_tool_result_bytes).unwrap();
        compare(&input, &config, &mut state);
        (input, config, state)
    }

    fn relationship_judgment(input: &FrozenInput, config: &Config, state: &Checkpoint) -> Judgment {
        let mut value = judgment(input, config, state);
        value["relationship_checks"] = json!([{"record_id":"rule","status":"not_required",
            "related_record_ids":[],"relation_ids":[],"unresolved_record_ids":[],"finding_ids":[],
            "reason":"Synthetic background rule is standalone; no external condition or continuation.","sources":[citation(input)]}]);
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn source_finding_feedback_identifies_the_exact_missing_and_unrelated_ids() {
        let (input, config, mut state) = fixture();
        for index in 0..8 {
            state.review_draft.insert(
                format!("issue-{index}"),
                Finding {
                    code: "source_issue".into(),
                    message: "Synthetic saved source issue.".into(),
                    correction: "Compare and repair against the original source.".into(),
                    affected: vec![],
                    sources: vec![citation(&input)],
                },
            );
        }
        let mut args = judgment(&input, &config, &state);
        args["status"] = json!("findings");
        let kept: Vec<_> = state
            .review_draft
            .keys()
            .filter(|id| id.as_str() != "issue-4")
            .cloned()
            .collect();
        args["finding_ids"] = json!(kept);
        let before = digest(&state).unwrap();
        let detail = |error: String| -> Value {
            serde_json::from_str(error.strip_prefix("INVALID_FIELD /finding_ids: ").unwrap())
                .unwrap()
        };
        let error = detail(put(&input, &config, &mut state, &args).unwrap_err());
        assert_eq!(error["missing_finding_ids"]["items"], json!(["issue-4"]));
        assert_eq!(digest(&state).unwrap(), before);
        args["finding_ids"]
            .as_array_mut()
            .unwrap()
            .push(json!("not-in-scope"));
        let error = detail(put(&input, &config, &mut state, &args).unwrap_err());
        assert_eq!(
            error["unexpected_finding_ids"]["items"],
            json!(["not-in-scope"])
        );
        assert_eq!(digest(&state).unwrap(), before);
        args["finding_ids"] = json!(state.review_draft.keys().collect::<Vec<_>>());
        put(&input, &config, &mut state, &args).unwrap();
        assert_eq!(state.review_draft.len(), 8);
    }

    #[test]
    #[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
    fn archived_relationship_receipt_versions_are_reported() {
        let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
        let state: Checkpoint = serde_json::from_slice(
            &std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        )
        .unwrap();
        let mut receipts = Vec::new();
        for (task, receipt) in &state.source_review.as_ref().unwrap().results {
            if !receipt.judgment.relationship_checks.is_empty() {
                receipts.push(json!({"task_id":task,"current":receipt.version == version(&state,&receipt.dependencies).unwrap(),"stored_version":receipt.version,"current_version":version(&state,&receipt.dependencies).unwrap(),"record_ids":receipt.judgment.relationship_checks.iter().map(|c|&c.record_id).collect::<Vec<_>>(),"dependencies":receipt.dependencies}));
            }
        }
        assert!(
            !receipts.is_empty(),
            "the archive must contain saved relationship decisions"
        );
        std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({"turn":state.turn,"receipts":receipts,"source_checkpoint_unchanged":true,"semantic_acceptance":false})).unwrap()).unwrap();
    }

    #[test]
    fn clean_candidate_cannot_replace_relationship_judgment() {
        let (input, config, mut state) = relationship_fixture();
        let args = judgment(&input, &config, &state);
        let before = digest(&state).unwrap();
        let error = put(&input, &config, &mut state, &args).unwrap_err();
        assert!(error.contains("/relationship_checks"), "{error}");
        assert!(
            error.contains("rule"),
            "missing record ID must be actionable: {error}"
        );
        assert_eq!(before, digest(&state).unwrap());
    }

    #[test]
    fn relationship_receipt_reopens_after_edge_or_selected_target_changes() {
        let (input, config, mut state) = relationship_fixture();
        let check = relationship_judgment(&input, &config, &state);
        put(&input, &config, &mut state, &json!(check)).unwrap();
        assert!(pending(&input, &config, &state).unwrap().is_empty());
        // Restoring a clean legacy receipt cannot bypass the new explicit check.
        let mut legacy = state.clone();
        legacy
            .source_review
            .as_mut()
            .unwrap()
            .results
            .values_mut()
            .next()
            .unwrap()
            .judgment
            .relationship_checks
            .clear();
        assert_eq!(pending(&input, &config, &legacy).unwrap().len(), 1);
        state.analysis.records.insert(
            "target".into(),
            Record {
                id: "target".into(),
                sources: vec![],
                data: RecordData::Fact {
                    name: "choice".into(),
                    value: "selected".into(),
                    scope: "project".into(),
                },
            },
        );
        let dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
        let before = version(&state, &dependencies).unwrap();
        state.analysis.relations.insert(
            "edge".into(),
            Relation {
                id: "edge".into(),
                from: "rule".into(),
                to: "target".into(),
                from_target: RelationTarget::Record,
                to_target: RelationTarget::Record,
                from_record_sha256: digest(&state.analysis.records["rule"]).unwrap(),
                to_record_sha256: digest(&state.analysis.records["target"]).unwrap(),
                kind: RelationKind::References,
                state: RelationState::Explicit,
                scope: "project".into(),
                explanation: "selected target".into(),
                grounds: vec![],
            },
        );
        let added = version(&state, &dependencies).unwrap();
        assert_ne!(before, added);
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        if let RecordData::Fact { value, .. } =
            &mut state.analysis.records.get_mut("target").unwrap().data
        {
            *value = "changed selection".into();
        }
        assert_ne!(added, version(&state, &dependencies).unwrap());
        state.analysis.relations.remove("edge");
        assert_eq!(before, version(&state, &dependencies).unwrap());
    }

    #[test]
    fn relationship_resolution_requires_current_incident_edges_and_read_endpoints() {
        let (input, config, mut state) = relationship_fixture();
        let mut check = relationship_judgment(&input, &config, &state);
        let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
        check.relationship_checks[0].status = RelationshipStatus::Resolved;
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("resolved requires")
        );
        state.analysis.records.insert(
            "target".into(),
            Record {
                id: "target".into(),
                sources: vec![citation(&input)],
                data: RecordData::Fact {
                    name: "choice".into(),
                    value: "selected".into(),
                    scope: "project".into(),
                },
            },
        );
        state.analysis.relations.insert(
            "edge".into(),
            Relation {
                id: "edge".into(),
                from: "rule".into(),
                to: "target".into(),
                from_target: RelationTarget::Record,
                to_target: RelationTarget::Record,
                from_record_sha256: digest(&state.analysis.records["rule"]).unwrap(),
                to_record_sha256: digest(&state.analysis.records["target"]).unwrap(),
                kind: RelationKind::References,
                state: RelationState::Explicit,
                scope: "project".into(),
                explanation: "selected target".into(),
                grounds: vec![citation(&input)],
            },
        );
        check.relationship_checks[0].relation_ids = vec!["edge".into()];
        check.relationship_checks[0].related_record_ids = vec!["target".into()];
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("independently inspect")
        );
        for key in ["record:target", "relation:edge"] {
            state.reviewer_coverage.candidate.insert(
                key.into(),
                digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
            );
        }
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
        state.analysis.relations.get_mut("edge").unwrap().state = RelationState::Unresolved;
        state.reviewer_coverage.candidate.insert(
            "relation:edge".into(),
            digest(&context::reference(&state.analysis, "relation:edge").unwrap()).unwrap(),
        );
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("resolved requires")
        );
        check.relationship_checks[0].status = RelationshipStatus::NotRequired;
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("not_required")
        );
    }

    #[test]
    fn source_limited_requires_examining_available_continuation_without_forcing_resolution() {
        let (mut input, config, mut state) = relationship_fixture();
        let mut check = relationship_judgment(&input, &config, &state);
        input.source_units.push(Source {
            source_unit_revision_id: "continuation".into(),
            document_id: "document".into(),
            text: "Ambiguous continuation.".into(),
            locator: json!({}),
            ordinal: 1,
        });
        state.analysis.records.get_mut("rule").unwrap().data = RecordData::Unresolved {
            problem: "Unclear continuation target".into(),
            affected: vec![],
            candidates: vec!["continuation".into()],
        };
        state.reviewer_coverage.candidate.insert(
            "record:rule".into(),
            digest(&context::reference(&state.analysis, "record:rule").unwrap()).unwrap(),
        );
        check.relationship_checks = vec![RelationshipCheck {
            record_id: "rule".into(),
            status: RelationshipStatus::SourceLimited,
            related_record_ids: vec![],
            relation_ids: vec![],
            unresolved_record_ids: vec!["rule".into()],
            finding_ids: vec![],
            reason: "The referenced continuation still has two possible interpretations.".into(),
            sources: vec![citation(&input)],
        }];
        let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("frozen candidate source continuation")
        );
        check.relationship_checks[0].sources.push(Span {
            source_id: "continuation".into(),
            start: 0,
            end: input.source_units[1].text.len(),
            view_id: None,
            grid_cell: None,
        });
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps).is_err(),
            "citation without independent delivery must fail"
        );
        tools::cover(
            state
                .reviewer_coverage
                .text
                .entry("continuation".into())
                .or_default(),
            0,
            input.source_units[1].text.len(),
        );
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
        assert!(deps.source_ids.contains("continuation"));
        check.relationship_checks[0].status = RelationshipStatus::NotRequired;
        assert!(validate_relationship_checks(&input, &state, &check, &mut deps).is_err());
        check.relationship_checks.clear();
        check.status = JudgmentStatus::NeedsEvidence;
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    }

    #[test]
    fn relationship_review_can_cite_unresolved_evidence_without_an_affected_id() {
        let (input, config, mut state) = relationship_fixture();
        let unresolved = Record {
            id: "uncertain".into(),
            sources: vec![citation(&input)],
            data: RecordData::Unresolved {
                problem: "The source leaves this rule ambiguous.".into(),
                affected: vec![],
                candidates: vec![],
            },
        };
        state
            .analysis
            .records
            .insert(unresolved.id.clone(), unresolved);
        let key = "record:uncertain";
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        let mut check = relationship_judgment(&input, &config, &state);
        check.relationship_checks[0].status = RelationshipStatus::SourceLimited;
        check.relationship_checks[0].unresolved_record_ids = vec!["uncertain".into()];
        let mut uncertainty = check.relationship_checks[0].clone();
        uncertainty.record_id = "uncertain".into();
        check.relationship_checks.push(uncertainty);
        let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
        // Shared evidence is required; unrelated unresolved items cannot be borrowed.
        state.analysis.records.get_mut("uncertain").unwrap().sources[0].source_id =
            "unrelated".into();
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        let error = validate_relationship_checks(&input, &state, &check, &mut deps).unwrap_err();
        assert!(error.contains("/relationship_checks/0"));
    }

    #[test]
    fn relationship_finding_can_use_cross_source_grounds_for_an_explicit_affected_field() {
        let (mut input, config, mut state) = relationship_fixture();
        let mut check = relationship_judgment(&input, &config, &state);
        input.source_units.push(Source {
            source_unit_revision_id: "selection".into(),
            document_id: "document".into(),
            text: "The selected condition governs the earlier rule.".into(),
            locator: json!({}),
            ordinal: 1,
        });
        let evidence = Span {
            source_id: "selection".into(),
            start: 0,
            end: input.source_units[1].text.len(),
            view_id: None,
            grid_cell: None,
        };
        tools::cover(
            state
                .reviewer_coverage
                .text
                .entry("selection".into())
                .or_default(),
            0,
            evidence.end,
        );
        state.review_draft.insert(
            "issue".into(),
            Finding {
                code: "wrong_selection".into(),
                message: "The rule does not incorporate the source selection.".into(),
                correction: "Apply the source selection.".into(),
                affected: vec![ReviewedField {
                    id: "rule".into(),
                    path: "/data/applicability".into(),
                }],
                sources: vec![evidence],
            },
        );
        check.status = JudgmentStatus::Findings;
        check.finding_ids = vec!["issue".into()];
        check.relationship_checks[0].status = RelationshipStatus::Findings;
        check.relationship_checks[0].finding_ids = vec!["issue".into()];
        let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
        assert!(deps.source_ids.contains("selection"));
        state
            .review_draft
            .get_mut("issue")
            .unwrap()
            .affected
            .clear();
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps).is_err(),
            "unrelated evidence without an affected subject is not a relationship judgment"
        );
    }

    #[test]
    fn relationship_finding_feedback_identifies_the_wrong_id_and_a_saved_subject_match() {
        let (mut input, config, mut state) = relationship_fixture();
        let mut check = relationship_judgment(&input, &config, &state);
        input.source_units.push(Source {
            source_unit_revision_id: "other".into(),
            document_id: "document".into(),
            text: "Unrelated source issue.".into(),
            locator: json!({}),
            ordinal: 1,
        });
        let other = Span {
            source_id: "other".into(),
            start: 0,
            end: input.source_units[1].text.len(),
            view_id: None,
            grid_cell: None,
        };
        tools::cover(
            state
                .reviewer_coverage
                .text
                .entry("other".into())
                .or_default(),
            0,
            other.end,
        );
        let correct = Finding {
            code: "source_issue".into(),
            message: "The rule is misinterpreted.".into(),
            correction: "Compare the source condition.".into(),
            affected: vec![ReviewedField {
                id: "rule".into(),
                path: "/data/applicability".into(),
            }],
            sources: vec![citation(&input)],
        };
        state.review_draft.insert("correct".into(), correct.clone());
        state.review_draft.insert(
            "wrong".into(),
            Finding {
                affected: vec![],
                sources: vec![other],
                ..correct
            },
        );
        check.status = JudgmentStatus::Findings;
        check.finding_ids = vec!["wrong".into(), "correct".into()];
        check.relationship_checks[0].status = RelationshipStatus::Findings;
        check.relationship_checks[0].finding_ids = vec!["wrong".into()];
        let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
        let before = digest(&state).unwrap();
        let error = validate_relationship_checks(&input, &state, &check, &mut deps).unwrap_err();
        let detail: Value = serde_json::from_str(
            error
                .strip_prefix("INVALID_FIELD /relationship_checks/0/finding_ids/0: ")
                .expect(&error),
        )
        .unwrap();
        assert_eq!(detail["record_id"], "rule");
        assert_eq!(detail["unrelated_finding_id"], "wrong");
        assert_eq!(detail["candidate_finding_id"], "correct");
        assert_eq!(before, digest(&state).unwrap());
        check.relationship_checks[0].finding_ids = vec!["correct".into()];
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    }

    #[test]
    fn relationship_findings_require_saved_subject_evidence_and_outer_retention() {
        let (input, config, mut state) = relationship_fixture();
        let mut check = relationship_judgment(&input, &config, &state);
        check.status = JudgmentStatus::Findings;
        check.relationship_checks[0].status = RelationshipStatus::Findings;
        check.relationship_checks[0].finding_ids = vec!["issue".into()];
        let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("save the relationship")
        );
        state.review_draft.insert(
            "issue".into(),
            Finding {
                code: "missing_reference".into(),
                message: "Missing source-required edge.".into(),
                correction: "Inspect the selected target and preserve the reference.".into(),
                affected: vec![ReviewedField {
                    id: "rule".into(),
                    path: String::new(),
                }],
                sources: vec![citation(&input)],
            },
        );
        assert!(
            validate_relationship_checks(&input, &state, &check, &mut deps)
                .unwrap_err()
                .contains("outer finding_ids")
        );
        check.finding_ids = vec!["issue".into()];
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
        state.review_draft.get_mut("issue").unwrap().sources[0].source_id = "unrelated".into();
        assert!(validate_relationship_checks(&input, &state, &check, &mut deps).is_err());
        // A content finding does not force a fabricated relationship problem.
        check.relationship_checks[0].status = RelationshipStatus::NotRequired;
        check.relationship_checks[0].finding_ids.clear();
        validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    }

    #[test]
    #[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
    fn archived_clean_source_receipts_require_explicit_relationship_judgments() {
        let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
        let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
        let state: Checkpoint = serde_json::from_slice(&original).unwrap();
        let input: FrozenInput =
            serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
                .unwrap();
        let config: Config =
            serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
                .unwrap();
        let mut reopened = Vec::new();
        for task in tasks(&input, config.limits.max_tool_result_bytes).unwrap() {
            let receipt = &state.source_review.as_ref().unwrap().results[&task.id];
            let mut dependencies = receipt.dependencies.clone();
            let required =
                relationship_records(&state.analysis, &references(&state.analysis, &dependencies));
            if required.is_empty() {
                continue;
            }
            let error =
                validate_relationship_checks(&input, &state, &receipt.judgment, &mut dependencies)
                    .unwrap_err();
            assert!(error.contains("/relationship_checks"));
            assert!(!complete(&input, &state, &task).unwrap());
            reopened.push(json!({"task_id":task.id,"source_id":task.source_id,"required_record_ids":required,"error":error}));
        }
        assert!(!reopened.is_empty());
        assert_eq!(
            original,
            std::fs::read(root.join("extraction/checkpoint.json")).unwrap()
        );
        std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({
            "mode":"offline receipt gate projection; not a cross-contract checkpoint resume",
            "turn":state.turn,"records":state.analysis.records.len(),"relations":state.analysis.relations.len(),
            "reopened":reopened,"checkpoint_unchanged":true,"semantic_acceptance":false})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "requires an archived source judgment replay directory"]
    fn checkpoint_mapping_judgment_preserves_nested_source_evidence() {
        let directory = std::path::PathBuf::from(
            std::env::var("KB_TENDER_MAPPING_REPLAY_DIR").expect("explicit replay directory"),
        );
        let read = |name| std::fs::read(directory.join(name)).unwrap();
        let input: FrozenInput = serde_json::from_slice(&read("frozen-input.json")).unwrap();
        let state: Checkpoint = serde_json::from_slice(&read("checkpoint.json")).unwrap();
        let judgment: Judgment = serde_json::from_slice(&read("judgment.json")).unwrap();
        assert!(
            judgment.template_mappings.iter().any(|mapping| {
                mapping.finding_ids.iter().any(|id| {
                    let finding = &state.review_draft[id];
                    !finding
                        .sources
                        .iter()
                        .any(|source| mapping.sources.contains(source))
                        && finding.sources.iter().any(|source| {
                            mapping
                                .sources
                                .iter()
                                .any(|mapped| shared_mapping_evidence(source, mapped))
                        })
                })
            }),
            "the archived judgment must reproduce the exact-span false rejection"
        );
        let before = digest(&state).unwrap();
        let mut dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
        validate_template_mappings(&input, &state, &judgment, &mut dependencies).unwrap();
        assert_eq!(digest(&state).unwrap(), before);
    }

    #[test]
    fn mapping_evidence_preserves_source_and_modality_boundaries() {
        let text = Span {
            start: 3,
            end: 9,
            ..citation(&fixture().0)
        };
        for other in [
            Span {
                source_id: "other".into(),
                ..text.clone()
            },
            Span {
                start: 0,
                end: 6,
                ..text.clone()
            },
            Span {
                start: 9,
                end: 12,
                ..text.clone()
            },
            Span {
                start: 3,
                end: 3,
                ..text.clone()
            },
        ] {
            assert!(!shared_mapping_evidence(&text, &other));
        }
        for evidence in [
            json!({"source_id":"source","start":0,"end":0,"grid_cell":{"form_id":"form","row":0,"column":0}}),
            json!({"source_id":"source","start":0,"end":0,"view_id":"view"}),
        ] {
            let span: Span = serde_json::from_value(evidence.clone()).unwrap();
            assert!(shared_mapping_evidence(&span, &span));
            assert!(!shared_mapping_evidence(&span, &text));
            let mut changed = evidence.clone();
            changed["source_id"] = json!("other");
            assert!(!shared_mapping_evidence(
                &span,
                &serde_json::from_value(changed).unwrap()
            ));
            let mut changed = evidence;
            if span.grid_cell.is_some() {
                changed["grid_cell"]["column"] = json!(1);
            } else {
                changed["view_id"] = json!("other-view");
            }
            assert!(!shared_mapping_evidence(
                &span,
                &serde_json::from_value(changed).unwrap()
            ));
        }
    }

    #[test]
    #[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
    fn archived_mixed_layout_receipts_require_original_pixels() {
        let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
        let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
        let state: Checkpoint = serde_json::from_slice(&original).unwrap();
        let input: FrozenInput =
            serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
                .unwrap();
        let config: Config =
            serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
                .unwrap();
        let review = state.source_review.as_ref().unwrap();
        let mut reopened = Vec::new();
        for task in tasks(&input, config.limits.max_tool_result_bytes).unwrap() {
            let Some(receipt) = review.results.get(&task.id) else {
                continue;
            };
            if requires_layout_view(&input, &state, &task, &receipt.dependencies) {
                let error = validate_layout_view(
                    &input,
                    &state,
                    &task,
                    &receipt.dependencies,
                    &receipt.judgment,
                )
                .unwrap_err();
                assert!(!complete(&input, &state, &task).unwrap());
                reopened.push(json!({"source_id":task.source_id,"prior_status":receipt.judgment.status,"error":error}));
            }
        }
        assert!(
            !reopened.is_empty(),
            "archive must reproduce missing mixed-layout evidence"
        );
        assert_eq!(
            std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
            original
        );
        std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({
            "mode":"offline gate projection only; not a checkpoint resumed under a new tool contract",
            "turn":state.turn,"reopened":reopened,"checkpoint_unchanged":true,"semantic_acceptance":false
        })).unwrap()).unwrap();
    }

    #[test]
    fn whole_page_view_keeps_sibling_candidates_in_review_scope_after_restore() {
        let (mut input, config, mut state) = fixture();
        input.source_units[0].locator = json!({"page_ordinal":3});
        for (id, document, page) in [
            ("note", "document", 3),
            ("other-page", "document", 4),
            ("other-document", "different", 3),
        ] {
            input.source_units.push(Source {
                source_unit_revision_id: id.into(),
                document_id: document.into(),
                text: "表下注释".into(),
                locator: json!({"page_ordinal":page}),
                ordinal: input.source_units.len(),
            });
            state.analysis.records.insert(
                id.into(),
                Record {
                    id: id.into(),
                    sources: vec![Span {
                        source_id: id.into(),
                        start: 0,
                        end: "表下注释".len(),
                        view_id: None,
                        grid_cell: None,
                    }],
                    data: RecordData::Fact {
                        name: "说明".into(),
                        value: "原文已提取".into(),
                        scope: "表格".into(),
                    },
                },
            );
        }
        state.source_review = Some(initialize(&input, &config).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        compare(&input, &config, &mut state);
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        let old_task = tasks(&input, config.limits.max_tool_result_bytes)
            .unwrap()
            .remove(0);
        assert!(complete(&input, &state, &old_task).unwrap());
        let mut queried = state.clone();
        record_query(
            &input,
            &mut queried,
            "read_source_view",
            &json!({"source_id":"source"}),
        );
        assert!(
            queried
                .source_review
                .as_ref()
                .unwrap()
                .dependencies
                .source_ids
                .contains("note")
        );
        assert!(
            queried.reviewer_coverage.views.is_empty(),
            "query scope is not a delivered image receipt"
        );
        state.reviewer_coverage.views.insert(
            "page".into(),
            views::ViewIdentity {
                source_id: "source".into(),
                original_sha256: "a".repeat(64),
                image_sha256: "b".repeat(64),
                page_ordinal: 3,
                width: 1,
                height: 1,
                renderer: "docreader-source-view-v1/test".into(),
            },
        );
        // Simulate a restored old checkpoint whose view receipt is present but
        // dependencies still only name the table source, not the page note.
        state = serde_json::from_value(json!(state)).unwrap();
        let coverage = digest(&state.reviewer_coverage).unwrap();
        assert!(!complete(&input, &state, &old_task).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        let current = packet(&input, &config, &state).unwrap();
        let pending = current["current"]["pending_candidate_refs"]["items"]
            .as_array()
            .unwrap();
        assert!(pending.contains(&json!("record:note")));
        assert!(!pending.contains(&json!("record:other-page")));
        assert!(!pending.contains(&json!("record:other-document")));
        assert_eq!(digest(&state.reviewer_coverage).unwrap(), coverage);
        assert!(!state.reviewer_coverage.text.contains_key("note"));
        assert!(
            !state
                .reviewer_coverage
                .candidate
                .contains_key("record:note")
        );
    }

    #[test]
    fn mixed_layout_cannot_be_checked_without_independent_page_pixels() {
        let (mut input, config, mut state) = fixture();
        input.source_units[0].locator = json!({"page_ordinal":7});
        state.source_review = Some(initialize(&input, &config).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        let record: Record = serde_json::from_value(json!({
            "id":"layout","sources":[citation(&input)],"data":{
                "kind":"template","label":"form","title":"Mixed layout","parent":null,
                "order":null,"purpose":"submission","applicability":{
                    "state":"applicable","condition":"submission","scope":"bid","grounds":[citation(&input)]},
                "regions":[
                    {"source":citation(&input),"role":"fixed_text","form_id":null,"cells":[],"instruction":"title"},
                    {"source":citation(&input),"role":"fixed_text","form_id":"grid","cells":[],"instruction":"table"}
                ]}})).unwrap();
        state
            .reviewer_coverage
            .candidate
            .insert("record:layout".into(), digest(&record).unwrap());
        state.analysis.records.insert(record.id.clone(), record);
        context::complete_review_check(&input,&mut state,&json!({
            "reference":"record:layout","summary":"Compared parsed candidate","sources":[citation(&input)]
        }),config.limits.max_tool_result_bytes).unwrap();
        compare(&input, &config, &mut state);
        let mut args = judgment(&input, &config, &state);
        args["template_mappings"] = json!([{"template_id":"layout","requirement_ids":[],"relation_ids":[],
            "finding_ids":[],"reason":"Standalone original form","sources":[citation(&input)]}]);
        let before = digest(&state).unwrap();
        let error = put(&input, &config, &mut state, &args).unwrap_err();
        assert!(error.contains("original page pixels"), "{error}");
        assert_eq!(digest(&state).unwrap(), before);
        assert_eq!(
            packet(&input, &config, &state).unwrap()["current"]["layout_view"]["source_id"],
            "source"
        );
        let mut waiting = state.clone();
        let mut waiting_args = args.clone();
        waiting_args["status"] = json!("needs_evidence");
        waiting_args["evidence_requests"] =
            json!([{"question":"Original layout pixels unavailable","source_ids":["source"]}]);
        put(&input, &config, &mut waiting, &waiting_args).unwrap();
        assert_eq!(pending(&input, &config, &waiting).unwrap().len(), 1);

        // A primary receipt or an image merely cached by a read cannot satisfy
        // the independent review ledger, even when its geometry is correct.
        let view = views::ViewIdentity {
            source_id: "source".into(),
            original_sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            page_ordinal: 7,
            width: 1,
            height: 1,
            renderer: "docreader-source-view-v1/test".into(),
        };
        state
            .analysis
            .coverage
            .views
            .insert("page".into(), view.clone());
        state.source_views.insert(
            "page".into(),
            views::SourceView {
                identity: view.clone(),
                jpeg_base64: String::new(),
            },
        );
        args["sources"]
            .as_array_mut()
            .unwrap()
            .push(json!({"source_id":"source","start":0,"end":0,"view_id":"page"}));
        assert!(put(&input, &config, &mut state, &args).is_err());
        state.reviewer_coverage.views.insert("page".into(), view);
        put(&input, &config, &mut state, &args).unwrap();
        assert!(pending(&input, &config, &state).unwrap().is_empty());
        let mut alternate_input = input.clone();
        let mut neighbor = input.source_units[0].clone();
        neighbor.source_unit_revision_id = "neighbor".into();
        alternate_input.source_units.push(neighbor);
        let mut alternate_state = state.clone();
        alternate_state
            .reviewer_coverage
            .views
            .get_mut("page")
            .unwrap()
            .source_id = "neighbor".into();
        let mut alternate_args = args.clone();
        alternate_args["sources"][1]["source_id"] = json!("neighbor");
        let alternate_judgment: Judgment = serde_json::from_value(alternate_args).unwrap();
        let task = tasks(&input, config.limits.max_tool_result_bytes)
            .unwrap()
            .remove(0);
        let dependencies = &state.source_review.as_ref().unwrap().dependencies;
        assert!(
            validate_layout_view(
                &alternate_input,
                &alternate_state,
                &task,
                dependencies,
                &alternate_judgment
            )
            .is_ok(),
            "the same original page may be read using a different parsed source identity"
        );
        alternate_input.source_units[1].document_id = "other-document".into();
        assert!(
            validate_layout_view(
                &alternate_input,
                &alternate_state,
                &task,
                dependencies,
                &alternate_judgment
            )
            .is_err()
        );
        alternate_input.source_units[1].document_id = input.source_units[0].document_id.clone();
        alternate_input.source_units[1].locator["page_ordinal"] = json!(8);
        assert!(
            validate_layout_view(
                &alternate_input,
                &alternate_state,
                &task,
                dependencies,
                &alternate_judgment
            )
            .is_err()
        );
        state.reviewer_coverage.views.clear();
        let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
        assert_eq!(
            pending(&input, &config, &restored).unwrap().len(),
            1,
            "restored source receipts cannot bypass the required independent visual evidence"
        );
    }

    #[test]
    fn source_packet_highlights_templates_without_a_recorded_requirement_mapping() {
        let (input, config, mut state) = fixture();
        let applicability = json!({"state":"applicable","condition":"submission","scope":"bid","grounds":[citation(&input)]});
        for value in [
            json!({"id":"requirement","sources":[citation(&input)],"data":{
                "kind":"requirement","text":"Use the prescribed form","categories":["format"],
                "strength":"mandatory","compliance":[],"applicability":applicability,
                "response":[{"channel":"structured_form","description":"Prescribed form","condition":"submission","grounds":[citation(&input)]}],
                "scoring_rule":null,"proofs":[],"criteria":[]}}),
            json!({"id":"template","sources":[citation(&input)],"data":{
                "kind":"template","label":"Form","title":"Prescribed form","parent":null,"order":null,
                "purpose":"submission","applicability":applicability,
                "regions":[{"source":citation(&input),"role":"fixed_text","form_id":null,"cells":[],"instruction":"Preserve wording"}]}}),
        ] {
            let record: Record = serde_json::from_value(value).unwrap();
            state.analysis.records.insert(record.id.clone(), record);
        }
        let before = digest(&state).unwrap();
        let work = packet(&input, &config, &state).unwrap();
        assert_eq!(
            work["current"]["templates_without_requirement_mapping"]["items"],
            json!(["record:template"])
        );
        assert_eq!(
            before,
            digest(&state).unwrap(),
            "navigation is not evidence or a finding"
        );
        let relation: Relation = serde_json::from_value(json!({
            "id":"mapping","from":"requirement","to":"template",
            "from_target":{"kind":"response","index":0},"to_target":{"kind":"record"},
            "from_record_sha256":digest(&state.analysis.records["requirement"]).unwrap(),
            "to_record_sha256":digest(&state.analysis.records["template"]).unwrap(),
            "kind":"requires_template","state":"explicit","scope":"submission",
            "explanation":"Use the prescribed form","grounds":[citation(&input)]
        }))
        .unwrap();
        state
            .analysis
            .relations
            .insert(relation.id.clone(), relation.clone());
        let mut queried = state.clone();
        record_query(
            &input,
            &mut queried,
            "inspect_analysis",
            &json!({
                "kind":"all","ids":["requirement","mapping","source"]
            }),
        );
        let refs = &queried
            .source_review
            .as_ref()
            .unwrap()
            .dependencies
            .references;
        assert!(refs.contains("record:requirement"));
        assert!(refs.contains("relation:mapping"));
        assert!(refs.contains("disposition:source"));
        assert!(!refs.contains("record:mapping") && !refs.contains("record:source"));
        assert_eq!(
            packet(&input, &config, &state).unwrap()["current"]["templates_without_requirement_mapping"]
                ["total"],
            0
        );
        state.analysis.relations.clear();
        assert_eq!(
            packet(&input, &config, &state).unwrap()["current"]["templates_without_requirement_mapping"]
                ["total"],
            1
        );
        assert!(!state.done && state.review_draft.is_empty());

        // Reading and comparing both records must not approve a missing edge
        // without an explicit source-grounded mapping judgment.
        for key in [
            "record:requirement",
            "record:template",
            "disposition:source",
        ] {
            state.reviewer_coverage.candidate.insert(
                key.into(),
                digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
            );
            context::complete_review_check(
                &input,
                &mut state,
                &json!({
                    "reference":key,"summary":"The existing record matches the source.",
                    "sources":[citation(&input)]
                }),
                config.limits.max_tool_result_bytes,
            )
            .unwrap();
        }
        let args = judgment(&input, &config, &state);
        assert!(
            put(&input, &config, &mut state, &args)
                .unwrap_err()
                .contains("/template_mappings")
        );
        let mut judgment: Judgment = serde_json::from_value(args).unwrap();
        judgment.template_mappings.push(TemplateMapping {
            template_id: "template".into(),
            requirement_ids: vec!["requirement".into()],
            relation_ids: vec![],
            finding_ids: vec![],
            reason: "The original requires this output in the prescribed form.".into(),
            sources: vec![citation(&input)],
        });
        let dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
        let validate = |state: &Checkpoint, judgment: &Judgment| {
            validate_template_mappings(&input, state, judgment, &mut dependencies.clone())
        };
        assert!(
            validate(&state, &judgment)
                .unwrap_err()
                .contains("mapping is absent")
        );

        // A missing edge can be reported, but cannot silently become checked.
        state.review_draft.insert(
            "missing".into(),
            Finding {
                code: "MISSING_MAPPING".into(),
                message: "Required mapping absent".into(),
                correction: "Map the source requirement to its prescribed template".into(),
                affected: vec![],
                sources: vec![citation(&input)],
            },
        );
        judgment.finding_ids.push("missing".into());
        judgment.template_mappings[0]
            .finding_ids
            .push("missing".into());
        validate(&state, &judgment).unwrap();
        // A finding may quote the exact clause while the mapping includes its
        // surrounding paragraph. Both cite the same independently read text.
        state.review_draft.get_mut("missing").unwrap().sources[0].end = 6;
        validate(&state, &judgment).unwrap();
        judgment.template_mappings[0].sources[0].start = 6;
        assert!(
            validate(&state, &judgment).is_err(),
            "adjacent clauses are not shared evidence"
        );
        judgment.template_mappings[0].sources[0] = citation(&input);
        state.review_draft.get_mut("missing").unwrap().sources[0] = citation(&input);
        judgment.template_mappings[0].sources[0].end = 6;
        validate(&state, &judgment).unwrap();
        judgment.template_mappings[0].sources[0] = citation(&input);
        judgment.finding_ids.clear();
        assert!(validate(&state, &judgment).is_err());
        judgment.template_mappings[0].finding_ids.clear();
        state.review_draft.clear();

        state
            .analysis
            .relations
            .insert(relation.id.clone(), relation.clone());
        judgment.template_mappings[0]
            .relation_ids
            .push("mapping".into());
        assert!(
            validate(&state, &judgment)
                .unwrap_err()
                .contains("independently inspect")
        );
        state.reviewer_coverage.candidate.insert(
            "relation:mapping".into(),
            digest(&context::reference(&state.analysis, "relation:mapping").unwrap()).unwrap(),
        );
        validate(&state, &judgment).unwrap();

        // A correct mapping can coexist with an unrelated template-content
        // finding. Misfiling that issue must not ask for an invented mapping
        // problem when removing it from this nested list is sufficient.
        let mut content_source = citation(&input);
        content_source.start = 6;
        state.review_draft.insert(
            "content".into(),
            Finding {
                code: "TEMPLATE_CONTENT".into(),
                message: "The template removes fixed source wording".into(),
                correction: "Preserve the original fixed wording".into(),
                affected: vec![],
                sources: vec![content_source],
            },
        );
        judgment.finding_ids.push("content".into());
        judgment.template_mappings[0].sources[0].end = 6;
        validate(&state, &judgment).unwrap();
        judgment.template_mappings[0]
            .finding_ids
            .push("content".into());
        let error = validate(&state, &judgment).unwrap_err();
        assert!(error.contains("/template_mappings/0/finding_ids/0"));
        assert!(error.contains("finding_ids=[]"));
        judgment.template_mappings[0].finding_ids.clear();
        validate(&state, &judgment).unwrap();
        assert_eq!(judgment.finding_ids, ["content"]);
        assert!(state.review_draft.contains_key("content"));
        judgment.finding_ids.clear();
        state.review_draft.clear();
        judgment.template_mappings[0].sources[0] = citation(&input);

        state.analysis.relations.get_mut("mapping").unwrap().kind = RelationKind::References;
        assert!(
            validate(&state, &judgment)
                .unwrap_err()
                .contains("relation must map")
        );

        // Standalone material remains legal; no edge is manufactured to satisfy
        // a fixed count. This semantic assertion still needs read evidence.
        judgment.template_mappings[0].requirement_ids.clear();
        judgment.template_mappings[0].relation_ids.clear();
        judgment.template_mappings[0].reason =
            "Source material has no separate output obligation.".into();
        validate(&state, &judgment).unwrap();
        let mut unread = state.clone();
        unread.reviewer_coverage.text.clear();
        assert!(validate(&unread, &judgment).is_err());
        judgment
            .template_mappings
            .push(judgment.template_mappings[0].clone());
        assert!(validate(&state, &judgment).unwrap_err().contains("once"));
    }

    #[test]
    fn candidate_comparisons_do_not_replace_source_judgment_and_host_finishes_without_submission() {
        let (input, config, mut state) = fixture();
        let task = packet(&input, &config, &state).unwrap();
        assert_eq!(task["current"]["comparison_total"], 1);
        assert_eq!(task["current"]["pending_candidate_refs"]["total"], 0);
        assert!(
            task["current"].get("candidate_refs").is_none(),
            "do not present completed comparisons as a new work roster"
        );
        assert!(tools::review_gaps(&input, &state.analysis, &state.reviewer_coverage).is_empty());
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(!state.done);
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        assert!(
            !state.done,
            "individual tools must not finalize before the batch ends"
        );
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(state.done);
        assert_eq!(state.review_rounds, 1);
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert_eq!(
            state.review_rounds, 1,
            "replay cannot count the completed review twice"
        );
    }

    #[test]
    fn late_finding_and_withdrawal_invalidate_old_receipts_without_resurrecting_clean_checks() {
        let (input, config, mut state) = fixture();
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        let finding = Finding {
            code: "MISSED_CONDITION".into(),
            message: "发现尚未处理的限定条件".into(),
            correction: "补全对应条件并复核".into(),
            affected: vec![],
            sources: vec![citation(&input)],
        };
        finding_changed(&mut state, None, Some(&finding)).unwrap();
        state.review_draft.insert("finding".into(), finding.clone());
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(
            !state.done,
            "a later tool in the same batch invalidates earlier completion"
        );
        finding_changed(&mut state, Some(&finding), None).unwrap();
        state.review_draft.clear();
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        assert!(!context::has_review_outcome(&state, "disposition:source").unwrap());
        assert!(
            put(&input, &config, &mut state, &args)
                .unwrap_err()
                .contains("stale")
        );
        compare(&input, &config, &mut state);
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(state.done);
    }

    #[test]
    #[ignore = "requires archived before/after analyses; offline dependency projection only"]
    fn archived_repair_invalidates_only_dependent_candidate_versions() {
        let directory =
            std::path::PathBuf::from(std::env::var("KB_TENDER_DEPENDENCY_REPLAY_DIR").unwrap());
        let read = |name| std::fs::read(directory.join(name)).unwrap();
        let original = read("checkpoint.json");
        let mut before: Checkpoint = serde_json::from_slice(&original).unwrap();
        let mut after = before.clone();
        before.analysis = serde_json::from_slice(&read("before-analysis.json")).unwrap();
        after.analysis = serde_json::from_slice(&read("after-analysis.json")).unwrap();
        // Compare the same new contract on both analyses. This is not a
        // migration or replay of archived comparison receipts.
        let refs = context::scope_references(
            &before.analysis,
            &before
                .analysis
                .dispositions
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
        );
        let mut retained = Vec::new();
        let mut invalidated = Vec::new();
        for key in refs {
            if candidate_version(&before, &key).unwrap() == candidate_version(&after, &key).unwrap()
            {
                retained.push(key);
            } else {
                invalidated.push(key);
            }
        }
        let expected: Vec<String> =
            serde_json::from_slice(&read("expected-invalidated.json")).unwrap();
        std::fs::write(directory.join("projection.json"), serde_json::to_vec_pretty(&json!({
            "mode":"offline new-contract dependency projection; no old receipts reused or model calls",
            "retained":retained,"invalidated":invalidated,
            "before_analysis_sha256":digest(&before.analysis).unwrap(),
            "after_analysis_sha256":digest(&after.analysis).unwrap(),
        })).unwrap()).unwrap();
        assert_eq!(
            invalidated.into_iter().collect::<BTreeSet<_>>(),
            expected.into_iter().collect()
        );
        assert!(
            !retained.is_empty(),
            "unrelated comparisons must survive a local repair"
        );
        assert_eq!(read("checkpoint.json"), original);
    }

    #[test]
    fn unrelated_same_source_edit_preserves_candidate_check_but_reopens_source_judgment() {
        let (input, config, mut state) = fixture();
        for id in ["changed", "unrelated"] {
            state.analysis.records.insert(
                id.into(),
                Record {
                    id: id.into(),
                    sources: vec![citation(&input)],
                    data: RecordData::Fact {
                        name: id.into(),
                        value: "背景信息".into(),
                        scope: "项目".into(),
                    },
                },
            );
        }
        for key in ["record:changed", "record:unrelated", "disposition:source"] {
            state.reviewer_coverage.candidate.insert(
                key.into(),
                digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
            );
            context::complete_review_check(&input, &mut state,
                &json!({"reference":key,"summary":"已与原文逐项比较。","sources":[citation(&input)]}),
                config.limits.max_tool_result_bytes).unwrap();
        }
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        assert!(pending(&input, &config, &state).unwrap().is_empty());
        let RecordData::Fact { value, .. } =
            &mut state.analysis.records.get_mut("changed").unwrap().data
        else {
            panic!("fact");
        };
        *value = "修订后的背景信息".into();
        assert!(
            context::has_review_outcome(&state, "record:unrelated").unwrap(),
            "sharing a source does not make unrelated candidate checks stale"
        );
        assert!(!context::has_review_outcome(&state, "record:changed").unwrap());
        assert!(!context::has_review_outcome(&state, "disposition:source").unwrap());
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    }

    #[test]
    fn candidate_checks_track_relation_membership_endpoints_and_finding_withdrawal() {
        let (input, _, mut state) = fixture();
        for id in ["local", "endpoint", "other"] {
            state.analysis.records.insert(
                id.into(),
                Record {
                    id: id.into(),
                    sources: vec![Span {
                        source_id: id.into(),
                        ..citation(&input)
                    }],
                    data: RecordData::Fact {
                        name: id.into(),
                        value: "背景".into(),
                        scope: "项目".into(),
                    },
                },
            );
        }
        let without_link = candidate_version(&state, "record:local").unwrap();
        let link = Relation {
            id: "link".into(),
            from: "local".into(),
            to: "endpoint".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["local"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["endpoint"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "项目".into(),
            explanation: "直接引用".into(),
            grounds: vec![citation(&input)],
        };
        state
            .analysis
            .relations
            .insert(link.id.clone(), link.clone());
        let linked = candidate_version(&state, "record:local").unwrap();
        let relation = candidate_version(&state, "relation:link").unwrap();
        assert_ne!(
            without_link, linked,
            "new incident relation invalidates the check"
        );
        let original_endpoint = state.analysis.records["endpoint"].clone();
        state.analysis.records.get_mut("endpoint").unwrap().sources[0].start = 1;
        assert_ne!(linked, candidate_version(&state, "record:local").unwrap());
        assert_ne!(
            relation,
            candidate_version(&state, "relation:link").unwrap()
        );
        state
            .analysis
            .records
            .insert("endpoint".into(), original_endpoint);
        state.analysis.relations.get_mut("link").unwrap().to = "other".into();
        assert_ne!(linked, candidate_version(&state, "record:local").unwrap());
        state.analysis.relations.remove("link");
        assert_ne!(linked, candidate_version(&state, "record:local").unwrap());
        state.analysis.relations.insert("link".into(), link);
        let finding = Finding {
            code: "ENDPOINT_MISMATCH".into(),
            message: "端点需复核".into(),
            correction: "对照原文".into(),
            affected: vec![
                serde_json::from_value(json!({"id":"endpoint","path":"/sources"})).unwrap(),
            ],
            sources: vec![citation(&input)],
        };
        finding_changed(&mut state, None, Some(&finding)).unwrap();
        finding_changed(&mut state, Some(&finding), None).unwrap();
        assert_ne!(
            linked,
            candidate_version(&state, "record:local").unwrap(),
            "withdrawing an endpoint finding must not resurrect a dependent clean check"
        );
    }

    #[test]
    fn candidate_checks_track_explicit_template_parent_and_global_rules() {
        let (input, _, mut state) = fixture();
        let applicability = Applicability {
            state: ApplicabilityState::Applicable,
            condition: "".into(),
            scope: "项目".into(),
            grounds: vec![citation(&input)],
        };
        for (id, parent) in [("parent", None), ("child", Some("parent"))] {
            state.analysis.records.insert(
                id.into(),
                Record {
                    id: id.into(),
                    sources: vec![Span {
                        source_id: id.into(),
                        ..citation(&input)
                    }],
                    data: RecordData::Template {
                        label: id.into(),
                        title: "模板".into(),
                        parent: parent.map(str::to_owned),
                        order: None,
                        purpose: "提交".into(),
                        applicability: applicability.clone(),
                        regions: vec![],
                    },
                },
            );
        }
        let original = candidate_version(&state, "record:child").unwrap();
        state.analysis.records.get_mut("parent").unwrap().sources[0].start = 1;
        let changed_parent = candidate_version(&state, "record:child").unwrap();
        assert_ne!(original, changed_parent);
        state.analysis.records.remove("parent");
        assert_ne!(
            changed_parent,
            candidate_version(&state, "record:child").unwrap()
        );
        let no_rule = candidate_version(&state, "record:child").unwrap();
        state.analysis.records.insert(
            "rule".into(),
            Record {
                id: "rule".into(),
                sources: vec![citation(&input)],
                data: RecordData::Rule {
                    text: "统一约定".into(),
                    scope: "项目".into(),
                    applicability,
                },
            },
        );
        let with_rule = candidate_version(&state, "record:child").unwrap();
        assert_ne!(no_rule, with_rule);
        state.analysis.records.get_mut("rule").unwrap().sources[0].start = 1;
        assert_ne!(
            with_rule,
            candidate_version(&state, "record:child").unwrap()
        );
        state.analysis.records.remove("rule");
        assert_eq!(no_rule, candidate_version(&state, "record:child").unwrap());
    }

    #[test]
    fn navigation_keeps_cross_source_endpoints_required_by_the_assigned_task_pending() {
        let (mut input, config, mut state) = fixture();
        input.source_units.push(Source {
            source_unit_revision_id: "other".into(),
            document_id: "document".into(),
            text: "另一段项目背景。".into(),
            locator: json!({}),
            ordinal: 1,
        });
        for (id, source_id) in [("local", "source"), ("endpoint", "other")] {
            let record = Record {
                id: id.into(),
                sources: vec![Span {
                    source_id: source_id.into(),
                    ..citation(&input)
                }],
                data: RecordData::Fact {
                    name: "背景".into(),
                    value: "项目背景".into(),
                    scope: "项目".into(),
                },
            };
            state.analysis.records.insert(id.into(), record);
        }
        state.analysis.relations.insert(
            "link".into(),
            Relation {
                id: "link".into(),
                from: "local".into(),
                to: "endpoint".into(),
                from_target: RelationTarget::Record,
                to_target: RelationTarget::Record,
                from_record_sha256: digest(&state.analysis.records["local"]).unwrap(),
                to_record_sha256: digest(&state.analysis.records["endpoint"]).unwrap(),
                kind: RelationKind::References,
                state: RelationState::Explicit,
                scope: "项目".into(),
                explanation: "本段引用另一段背景。".into(),
                grounds: vec![citation(&input)],
            },
        );
        state.input_sha256 = digest(&input).unwrap();
        state.source_review = Some(initialize(&input, &config).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        for key in ["record:local", "relation:link", "disposition:source"] {
            state.reviewer_coverage.candidate.insert(
                key.into(),
                digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
            );
            context::complete_review_check(
                &input,
                &mut state,
                &json!({"reference":key,"summary":"与本段证据相符。","sources":[citation(&input)]}),
                config.limits.max_tool_result_bytes,
            )
            .unwrap();
        }
        let source = packet(&input, &config, &state).unwrap();
        assert!(
            source["current"]["pending_candidate_refs"]["items"]
                .as_array()
                .unwrap()
                .contains(&json!("record:endpoint"))
        );
        let work = context::request_work_state(&input, &state, config.limits.max_tool_result_bytes)
            .unwrap();
        assert_eq!(work["comparison_progress"]["remaining"], 1);
        assert_eq!(
            work["comparison_progress"]["next_reference"],
            "record:endpoint"
        );
        assert_ne!(work["next_action"], "complete_source_review");

        let mut focus = state.reviewer_work.clone().unwrap();
        focus.focus.action = context::FocusAction::Review;
        focus.focus.references = vec!["record:endpoint".into()];
        context::validate(&input, &state, &focus, config.limits.max_tool_result_bytes).unwrap();
        assert!(
            context::check_read_scope(&input, &state, "read_source", &json!({"source_id":"other"}))
                .is_err(),
            "a comparison focus does not grant cross-source reading permission"
        );
        let mut unrelated = state.clone();
        let mut record = unrelated.analysis.records["endpoint"].clone();
        record.id = "unrelated".into();
        unrelated.analysis.records.insert(record.id.clone(), record);
        focus.focus.references = vec!["record:unrelated".into()];
        assert!(
            context::validate(
                &input,
                &unrelated,
                &focus,
                config.limits.max_tool_result_bytes
            )
            .is_err()
        );

        // The other endpoint is a genuine comparison obligation. After it is
        // checked, changing its meaning must reopen the original source too.
        state
            .reviewer_work
            .as_mut()
            .unwrap()
            .source_scope
            .push("other".into());
        state.reviewer_coverage.candidate.insert(
            "record:endpoint".into(),
            digest(&state.analysis.records["endpoint"]).unwrap(),
        );
        let endpoint_span = Span {
            source_id: "other".into(),
            ..citation(&input)
        };
        tools::cover(
            state
                .reviewer_coverage
                .text
                .entry("other".into())
                .or_default(),
            endpoint_span.start,
            endpoint_span.end,
        );
        context::complete_review_check(
            &input,
            &mut state,
            &json!({"reference":"record:endpoint","summary":"已对照另一段背景。","sources":[endpoint_span]}),
            config.limits.max_tool_result_bytes,
        ).unwrap();
        assert_eq!(
            packet(&input, &config, &state).unwrap()["current"]["pending_candidate_refs"]["total"],
            0
        );
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        let completed_task = args["task_id"].as_str().unwrap();
        assert!(
            !pending(&input, &config, &state)
                .unwrap()
                .iter()
                .any(|t| t.id == completed_task)
        );
        let RecordData::Fact { value, .. } =
            &mut state.analysis.records.get_mut("endpoint").unwrap().data
        else {
            panic!("fact endpoint");
        };
        *value = "修订后的项目背景".into();
        let reopened = packet(&input, &config, &state).unwrap();
        assert!(
            reopened["current"]["pending_candidate_refs"]["items"]
                .as_array()
                .unwrap()
                .contains(&json!("record:endpoint")),
            "a changed endpoint must reappear in the unfinished roster"
        );
        assert!(
            pending(&input, &config, &state)
                .unwrap()
                .iter()
                .any(|t| t.id == completed_task)
        );
        assert!(!context::has_review_outcome(&state, "record:local").unwrap());
    }

    #[test]
    fn adding_deleting_and_moving_candidates_reopens_source_results() {
        let (input, config, mut state) = fixture();
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        let record = Record {
            id: "new".into(),
            sources: vec![citation(&input)],
            data: RecordData::Fact {
                name: "项目".into(),
                value: "背景".into(),
                scope: "本项目".into(),
            },
        };
        state
            .analysis
            .records
            .insert(record.id.clone(), record.clone());
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        let changed = version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap();
        state.analysis.records.remove("new");
        assert_ne!(
            changed,
            version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap()
        );
        state.analysis.records.insert(record.id.clone(), record);
        state
            .analysis
            .records
            .get_mut("new")
            .unwrap()
            .sources
            .clear();
        assert_ne!(
            changed,
            version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap()
        );
    }

    #[test]
    fn local_dependencies_survive_unrelated_edits_but_global_negative_queries_do_not() {
        let (input, config, mut state) = fixture();
        let local = version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap();
        record_query(
            &input,
            &mut state,
            "search_sources",
            &json!({"query":"尚不存在的对应成果"}),
        );
        let global = version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap();
        state.analysis.records.insert(
            "elsewhere".into(),
            Record {
                id: "elsewhere".into(),
                sources: vec![Span {
                    source_id: "other".into(),
                    ..citation(&input)
                }],
                data: RecordData::Fact {
                    name: "别处".into(),
                    value: "新增内容".into(),
                    scope: "其他范围".into(),
                },
            },
        );
        assert_eq!(
            local,
            version(
                &state,
                &task_dependencies(&tasks(&input, config.limits.max_tool_result_bytes).unwrap()[0])
            )
            .unwrap()
        );
        assert_ne!(
            global,
            version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap()
        );
    }

    #[test]
    fn unresolved_boundary_feedback_preserves_evidence_work_without_approving() {
        let (input, config, mut state) = fixture();
        let mut args = judgment(&input, &config, &state);
        args["boundaries"]["after"]["state"] = json!("unresolved");
        args["boundaries"]["after"]["reason"] =
            json!("The continuation is not in the available fragment.");
        let error = put(&input, &config, &mut state, &args).unwrap_err();
        assert!(error.contains("/boundaries/after/state"));
        assert!(error.contains("needs_evidence") && error.contains("put_review_finding"));
        assert!(state.source_review.as_ref().unwrap().results.is_empty());

        args["status"] = json!("needs_evidence");
        args["evidence_requests"] = json!([{
            "question":"Locate the continuation or establish that the frozen collection lacks it.",
            "source_ids":["source"]
        }]);
        put(&input, &config, &mut state, &args).unwrap();
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(!state.done);
        assert_eq!(state.review_rounds, 0);
    }

    #[test]
    fn continuation_requires_evidence_beyond_the_current_fragment_even_after_restore() {
        let (input, config, mut state) = fixture();
        let mut args = judgment(&input, &config, &state);
        args["boundaries"]["after"]["state"] = json!("continuation");
        assert!(
            put(&input, &config, &mut state, &args)
                .unwrap_err()
                .contains("/boundaries/after")
        );
        assert!(state.source_review.as_ref().unwrap().results.is_empty());

        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        // A receipt written by an older validator cannot bypass the same
        // boundary requirement when restored and considered for finalization.
        state
            .source_review
            .as_mut()
            .unwrap()
            .results
            .values_mut()
            .next()
            .unwrap()
            .judgment
            .boundaries
            .after
            .state = BoundaryStatus::Continuation;
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        assert!(
            packet(&input, &config, &state).unwrap()["current"]["prior_boundary_error"]
                .as_str()
                .unwrap()
                .contains("/boundaries/after")
        );
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(!state.done);
    }

    #[test]
    fn continuation_accepts_a_read_distinct_source_and_tracks_its_dependency() {
        let (mut input, config, mut state) = fixture();
        let mut next = input.source_units[0].clone();
        next.source_unit_revision_id = "next".into();
        next.ordinal = 1;
        input.source_units.push(next);
        state.input_sha256 = digest(&input).unwrap();
        state.source_review = Some(initialize(&input, &config).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        compare(&input, &config, &mut state);
        let mut args = judgment(&input, &config, &state);
        let target = Span {
            source_id: "next".into(),
            ..citation(&input)
        };
        args["boundaries"]["after"]["state"] = json!("continuation");
        args["boundaries"]["after"]["sources"] = json!([target]);
        assert!(
            put(&input, &config, &mut state, &args)
                .unwrap_err()
                .contains("read the cited source")
        );
        tools::cover(
            state
                .reviewer_coverage
                .text
                .entry("next".into())
                .or_default(),
            target.start,
            target.end,
        );
        put(&input, &config, &mut state, &args).unwrap();
        let receipt =
            &state.source_review.as_ref().unwrap().results[args["task_id"].as_str().unwrap()];
        assert!(receipt.dependencies.source_ids.contains("next"));
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    }

    #[test]
    fn same_source_continuation_requires_the_correct_side_and_exact_geometry() {
        let (mut input, config, state) = fixture();
        let ordinary: Judgment = serde_json::from_value(judgment(&input, &config, &state)).unwrap();
        input.structured_forms.push(json!({"form_definition_revision_id":"grid",
            "source_unit_revision_id":"source","definition":{"kind":"grid","row_count":4,"column_count":2}}));
        let task = Task {
            id: "fragment".into(),
            source_id: "source".into(),
            region: Region::Text { start: 6, end: 12 },
        };
        let before = Span {
            start: 0,
            end: 6,
            ..citation(&input)
        };
        let after = Span {
            start: 12,
            end: 18,
            ..citation(&input)
        };
        assert!(continuation_target(&input, &task, &before, true));
        assert!(!continuation_target(&input, &task, &before, false));
        assert!(continuation_target(&input, &task, &after, false));
        assert!(!continuation_target(&input, &task, &after, true));
        assert!(!continuation_target(
            &input,
            &task,
            &citation(&input),
            false
        ));
        let view = Span {
            start: 0,
            end: 0,
            view_id: Some("whole-page".into()),
            ..citation(&input)
        };
        assert!(!continuation_target(&input, &task, &view, false));
        let grid = Task {
            region: Region::Grid {
                form_id: "grid".into(),
                start: 2,
                end: 6,
            },
            ..task
        };
        let cell = |row| Span {
            start: 0,
            end: 0,
            grid_cell: Some(GridCitation {
                form_id: "grid".into(),
                row,
                column: 0,
            }),
            ..citation(&input)
        };
        assert!(continuation_target(&input, &grid, &cell(0), true));
        assert!(!continuation_target(&input, &grid, &cell(0), false));
        assert!(!continuation_target(&input, &grid, &cell(2), false));
        assert!(continuation_target(&input, &grid, &cell(3), false));
        assert!(!continuation_target(&input, &grid, &view, false));
        // An ordinary complete boundary remains valid without a target.
        validate_boundaries(&input, &state, &grid, &ordinary).unwrap();
    }

    #[test]
    fn completed_main_repair_navigates_to_independent_review_without_self_approval() {
        let (input, config, mut state) = fixture();
        state.role = Role::Main;
        state.main_work = state.reviewer_work.take();
        state.main_work.as_mut().unwrap().status = WorkStatus::Complete;
        state.review = Some(Review {
            analysis_sha256: digest(&state.analysis).unwrap(),
            coverage: state.reviewer_coverage.clone(),
            findings: vec![],
        });
        state
            .analysis
            .dispositions
            .get_mut("source")
            .unwrap()
            .reason = "经原文核对的背景说明".into();
        let packet =
            context::request_work_state(&input, &state, config.limits.max_tool_result_bytes)
                .unwrap();
        assert_eq!(packet["next_action"], "request_review");
        assert!(!state.done);
        assert_eq!(state.role, Role::Main);
        state.review.as_mut().unwrap().analysis_sha256 = digest(&state.analysis).unwrap();
        let packet =
            context::request_work_state(&input, &state, config.limits.max_tool_result_bytes)
                .unwrap();
        assert_ne!(
            packet["next_action"], "request_review",
            "unchanged analysis must not be presented as repaired"
        );
        state
            .analysis
            .dispositions
            .get_mut("source")
            .unwrap()
            .reason = "再次修订的背景说明".into();
        state.analysis.coverage.text.clear();
        let packet =
            context::request_work_state(&input, &state, config.limits.max_tool_result_bytes)
                .unwrap();
        assert_ne!(
            packet["next_action"], "request_review",
            "scope completion cannot hide global unread sources"
        );
    }

    #[test]
    fn source_judgment_reports_the_required_saved_findings_for_correction() {
        let (input, config, mut state) = fixture();
        let finding = Finding {
            code: "OMISSION".into(),
            message: "原文限定条件待补全".into(),
            correction: "补全原文限定条件".into(),
            affected: vec![],
            sources: vec![citation(&input)],
        };
        finding_changed(&mut state, None, Some(&finding)).unwrap();
        state.review_draft.insert("saved-omission".into(), finding);
        let before_packet = json!(state);
        let guidance = packet(&input, &config, &state).unwrap();
        assert_eq!(
            guidance["current"]["completion"]["required_finding_ids"]["items"],
            json!(["saved-omission"])
        );
        assert_eq!(
            guidance["current"]["completion"]["status_if_evidence_complete"],
            "findings"
        );
        assert_eq!(
            json!(state),
            before_packet,
            "navigation must not sign a judgment"
        );
        let mut args = judgment(&input, &config, &state);
        let error = put(&input, &config, &mut state, &args).unwrap_err();
        assert!(error.contains("saved-omission"));
        assert!(error.contains("expected_status"));
        assert!(state.source_review.as_ref().unwrap().results.is_empty());
        args["status"] = json!("findings");
        args["finding_ids"] = json!(["saved-omission"]);
        put(&input, &config, &mut state, &args).unwrap();
        assert!(pending(&input, &config, &state).unwrap().is_empty());
    }

    #[test]
    fn evidence_requests_and_unresolved_boundaries_never_sign_a_clean_result() {
        let (input, config, mut state) = fixture();
        let mut args = judgment(&input, &config, &state);
        args["boundaries"]["after"]["state"] = json!("unresolved");
        assert!(put(&input, &config, &mut state, &args).is_err());
        args["status"] = json!("needs_evidence");
        args["evidence_requests"] =
            json!([{"question":"该段是否还有后续限定？","source_ids":["source"]}]);
        let result = put(&input, &config, &mut state, &args).unwrap();
        assert_eq!(result["new_completion"], false);
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(!state.done);
    }

    #[test]
    fn source_fragments_partition_utf8_and_preserve_all_grid_slots_without_fixed_pages() {
        let (mut input, mut config, _) = fixture();
        config.limits.max_tool_result_bytes = 2048;
        input.source_units[0].text = "中文😀条款\n".repeat(400);
        input.structured_forms.push(
            json!({"form_definition_revision_id":"grid","source_unit_revision_id":"source",
            "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],
                "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"固定标题"},
                    {"row":1,"column":0,"row_span":1,"col_span":1,"text":"名称："},
                    {"row":1,"column":1,"row_span":1,"col_span":1,"text":""}]}}),
        );
        let tasks = tasks(&input, config.limits.max_tool_result_bytes).unwrap();
        let (mut text_end, mut cell_end) = (0, 0);
        for task in tasks {
            match task.region {
                Region::Text { start, end } => {
                    assert_eq!(start, text_end);
                    assert!(input.source_units[0].text.is_char_boundary(end));
                    text_end = end;
                }
                Region::Grid { start, end, .. } => {
                    assert_eq!(start, cell_end);
                    cell_end = end;
                }
                Region::Empty => panic!("nonempty source"),
            }
        }
        assert_eq!(text_end, input.source_units[0].text.len());
        assert_eq!(cell_end, 4);
    }

    #[test]
    fn current_fragment_navigation_does_not_require_rereading_the_whole_long_source() {
        let (mut input, mut config, mut state) = fixture();
        config.limits.max_tool_result_bytes = 2048;
        input.source_units[0].text = "完整条款及条件。\n".repeat(400);
        state.input_sha256 = digest(&input).unwrap();
        state.source_review = Some(initialize(&input, &config).unwrap());
        state.reviewer_coverage.text.clear();
        select_next(&input, &config, &mut state).unwrap();
        let tasks = tasks(&input, config.limits.max_tool_result_bytes).unwrap();
        assert!(tasks.len() > 1);
        let Region::Text { start, end } = tasks[0].region else {
            panic!("text task")
        };
        let gaps = reading_gaps(
            &input,
            &state,
            &state.reviewer_coverage,
            config.limits.max_tool_result_bytes,
        )
        .unwrap()
        .unwrap();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0]["start"], start);
        assert_eq!(gaps[0]["end"], end);
        tools::cover(
            state
                .reviewer_coverage
                .text
                .entry("source".into())
                .or_default(),
            start,
            end,
        );
        assert!(
            reading_gaps(
                &input,
                &state,
                &state.reviewer_coverage,
                config.limits.max_tool_result_bytes
            )
            .unwrap()
            .unwrap()
            .is_empty()
        );
        assert!(!tools::reading_gaps(&input, &state.reviewer_coverage).is_empty());
        assert_eq!(
            pending(&input, &config, &state).unwrap().len(),
            tasks.len(),
            "reading the assigned fragment grants no semantic completion"
        );
    }

    #[test]
    #[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
    fn archived_deleted_candidate_keeps_findings_and_builds_review_navigation() {
        let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
        let path = root.join("extraction/checkpoint.json");
        let original = std::fs::read(&path).unwrap();
        let state: Checkpoint = serde_json::from_slice(&original).unwrap();
        let input: FrozenInput =
            serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
                .unwrap();
        let config: Config =
            serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
                .unwrap();
        let dependencies = &state.source_review.as_ref().unwrap().dependencies;
        let missing: Vec<_> = dependency_references(&state.analysis, dependencies)
            .into_iter()
            .filter(|key| context::reference(&state.analysis, key).is_err())
            .collect();
        assert!(
            !missing.is_empty(),
            "archive must contain a deleted dependency"
        );
        let before = digest(&state).unwrap();
        let navigation = packet(&input, &config, &state).unwrap();
        let required = &navigation["current"]["completion"]["required_finding_ids"];
        assert!(required["total"].as_u64().unwrap() > 0);
        assert_eq!(
            navigation["current"]["completion"]["status_if_evidence_complete"],
            "findings"
        );
        assert_eq!(digest(&state).unwrap(), before);
        assert_eq!(std::fs::read(path).unwrap(), original);
        std::fs::write(
            std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
            serde_json::to_vec_pretty(&json!({"mode":"offline archived navigation; no provider or journal writes",
                "turn":state.turn,"deleted_dependencies":missing,"required_findings":required,
                "comparison_total":navigation["current"]["comparison_total"],"checkpoint_unchanged":true})).unwrap(),
        ).unwrap();
    }

    #[test]
    #[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
    fn archived_reviewed_uncertainty_can_be_retired_without_erasing_review() {
        let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
        let path = root.join("extraction/checkpoint.json");
        let original = std::fs::read(&path).unwrap();
        let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
        let input: FrozenInput =
            serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
                .unwrap();
        let config: Config =
            serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
                .unwrap();
        assert!(state.review_rounds > 0);
        let review = json!(state.review);
        let draft = json!(state.review_draft);
        let pending: Vec<_> = state
            .analysis
            .records
            .values()
            .filter(|r| matches!(r.data, RecordData::Unresolved { .. }))
            .map(|r| r.id.clone())
            .collect();
        // Project the actual archived candidate into primary repair in memory.
        // This is neither journal recovery nor a model-authored repair.
        state.role = Role::Main;
        let mut retired = vec![];
        let mut protected = vec![];
        for id in pending {
            let result = apply(
                &input,
                &config,
                &mut state,
                "delete_record",
                &json!({"id":id}),
            );
            if let Err(error) = result {
                assert!(error.contains("resolve the pending outcome"));
                assert!(state.analysis.records.contains_key(&id));
                protected.push(id);
            } else {
                assert!(!state.analysis.records.contains_key(&id));
                for work in [&state.main_work, &state.reviewer_work]
                    .into_iter()
                    .flatten()
                {
                    assert!(!work.pending_refs.contains(&format!("record:{id}")));
                }
                retired.push(id);
            }
        }
        assert!(!retired.is_empty());
        assert!(!protected.is_empty());
        assert_eq!(json!(state.review), review);
        assert_eq!(json!(state.review_draft), draft);
        assert!(!state.done);
        assert_eq!(std::fs::read(path).unwrap(), original);
        std::fs::write(
            std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
            serde_json::to_vec_pretty(&json!({
                "mode":"offline primary-role projection; no provider, journal writes or live acceptance",
                "retired":retired,"protected":protected,"review_preserved":true,
                "draft_preserved":true,"original_checkpoint_unchanged":true,"done":state.done
            })).unwrap(),
        ).unwrap();
    }

    #[test]
    fn unread_grid_focus_is_a_plan_not_delivered_evidence() {
        for role in [Role::Main, Role::Reviewer] {
            for action in ["locate", "extract", "review"] {
                let (mut input, config, mut state) = fixture();
                input.structured_forms.push(json!({
                    "form_definition_revision_id":"grid","source_unit_revision_id":"source",
                    "definition":{"kind":"grid","row_count":2,"column_count":2,
                        "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"条件"},
                            {"row":1,"column":0,"row_span":1,"col_span":1,"text":"响应"},
                            {"row":1,"column":1,"row_span":1,"col_span":1,"text":"证明"}]}
                }));
                state.role = role.clone();
                let span = json!({"source_id":"source","start":0,"end":0,
                    "grid_cell":{"form_id":"grid","row":0,"column":0}});
                let work = json!({"source_scope":["source"],"objective":"核对计划中的网格",
                    "focus":{"action":action,"source_spans":[span],"references":[]},
                    "status":"active","note":"计划不是阅读回执"});
                let before = json!(state.coverage());
                apply(&input, &config, &mut state, "set_work_note", &work).unwrap();
                context::check_read_scope(&input, &state, "read_form", &json!({"form_id":"grid"}))
                    .unwrap();
                assert_eq!(json!(state.coverage()), before);
                assert!(
                    tools::validate_span(
                        &input,
                        state.coverage(),
                        &serde_json::from_value(span.clone()).unwrap()
                    )
                    .unwrap_err()
                    .contains("read the cited grid cell"),
                    "a plan cannot authorize a primary citation or independent review"
                );
                let (tool, args) = if role == Role::Main {
                    (
                        "put_record",
                        json!({"id":null,"sources":[span],
                        "data":{"kind":"fact","name":"计划中的条件","value":"未读取","scope":"来源"}}),
                    )
                } else {
                    (
                        "put_review_finding",
                        json!({"id":null,"finding":{
                        "code":"unread","message":"未读取的条件","correction":"核对原文",
                        "sources":[span],"affected":[]}}),
                    )
                };
                assert!(
                    apply(&input, &config, &mut state, tool, &args)
                        .unwrap_err()
                        .contains("read the cited grid cell")
                );
                let saved = json!(state.work());
                for (field, value) in [
                    ("/focus/source_spans/0/grid_cell/column", json!(1)),
                    ("/focus/source_spans/0/grid_cell/row", json!(2)),
                    ("/focus/source_spans/0/grid_cell/form_id", json!("unknown")),
                    ("/focus/source_spans/0/source_id", json!("foreign")),
                    ("/focus/source_spans/0/start", json!(1)),
                ] {
                    let mut invalid = work.clone();
                    *invalid.pointer_mut(field).unwrap() = value;
                    assert!(apply(&input, &config, &mut state, "set_work_note", &invalid).is_err());
                    assert_eq!(json!(state.work()), saved);
                }
                let mut invalid = work.clone();
                invalid["focus"]["source_spans"][0]["view_id"] = json!("undelivered");
                assert!(apply(&input, &config, &mut state, "set_work_note", &invalid).is_err());
                assert_eq!(json!(state.coverage()), before);
            }
        }
    }

    #[test]
    fn independent_finding_allows_retiring_a_wrong_unresolved_item_without_clearing_review() {
        let (input, config, mut state) = fixture();
        state.role = Role::Main;
        state.main_work = state.reviewer_work.clone();
        state.main_work.as_mut().unwrap().pending_refs = vec!["record:uncertain".into()];
        state.analysis.records.insert(
            "uncertain".into(),
            Record {
                id: "uncertain".into(),
                sources: vec![citation(&input)],
                data: RecordData::Unresolved {
                    problem: "Old uncertainty.".into(),
                    affected: vec![],
                    candidates: vec![],
                },
            },
        );
        let args = json!({"id":"uncertain"});
        assert!(
            apply(&input, &config, &mut state, "delete_record", &args)
                .unwrap_err()
                .contains("resolve the pending outcome")
        );
        let issue = Finding {
            code: "stale_uncertainty".into(),
            message: "Original evidence resolves this item.".into(),
            correction:
                "Retire the stale unresolved item after preserving the actual source requirements."
                    .into(),
            affected: vec![ReviewedField {
                id: "uncertain".into(),
                path: "/data/problem".into(),
            }],
            sources: vec![citation(&input)],
        };
        state.review_rounds = 1;
        state.review = Some(Review {
            analysis_sha256: digest(&state.analysis).unwrap(),
            coverage: state.reviewer_coverage.clone(),
            findings: vec![issue.clone()],
        });
        state.review.as_mut().unwrap().findings[0].affected.clear();
        assert!(
            apply(&input, &config, &mut state, "delete_record", &args).is_err(),
            "a generic source finding must not authorize deleting any pending item"
        );
        state.review.as_mut().unwrap().findings[0] = issue.clone();
        state.review_draft.insert("issue".into(), issue);
        let review_before = json!(state.review);
        apply(&input, &config, &mut state, "delete_record", &args).unwrap();
        assert!(!state.analysis.records.contains_key("uncertain"));
        assert_eq!(json!(state.review), review_before);
        assert!(state.review_draft.contains_key("issue"));
        assert!(
            !state.done,
            "retirement is a primary repair, never independent acceptance"
        );
    }

    #[test]
    fn deleting_an_affected_candidate_does_not_silently_drop_its_source_finding() {
        let (input, config, mut state) = fixture();
        let record = Record {
            id: "fact".into(),
            sources: vec![citation(&input)],
            data: RecordData::Fact {
                name: "背景".into(),
                value: "错误解释".into(),
                scope: "项目".into(),
            },
        };
        state
            .reviewer_coverage
            .candidate
            .insert("record:fact".into(), digest(&record).unwrap());
        state.analysis.records.insert(record.id.clone(), record);
        record_query(
            &input,
            &mut state,
            "inspect_analysis",
            &json!({"kind":"record","ids":["fact"]}),
        );
        let finding = Finding {
            code: "WRONG_VALUE".into(),
            message: "解释不对应背景".into(),
            correction: "按背景原文修正".into(),
            sources: vec![],
            affected: vec![ReviewedField {
                id: "fact".into(),
                path: "/data/value".into(),
            }],
        };
        finding_changed(&mut state, None, Some(&finding)).unwrap();
        state.review_draft.insert("finding".into(), finding.clone());
        compare(&input, &config, &mut state);
        let mut args = judgment(&input, &config, &state);
        args["status"] = json!("findings");
        args["finding_ids"] = json!(["finding"]);
        put(&input, &config, &mut state, &args).unwrap();
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert_eq!(state.role, Role::Main);
        state.analysis.records.remove("fact");
        state.role = Role::Reviewer;
        // Real repairs can merge and delete a candidate inspected in an earlier
        // round. Restore the archived dependencies exactly, including that ID.
        state = serde_json::from_value(json!(state)).unwrap();
        select_next(&input, &config, &mut state).unwrap();
        let dependencies = &state.source_review.as_ref().unwrap().dependencies;
        assert!(dependencies.references.contains("record:fact"));
        let deleted_version = version(&state, dependencies).unwrap();
        let mut without_tombstone = dependencies.clone();
        without_tombstone.references.remove("record:fact");
        assert_ne!(
            deleted_version,
            version(&state, &without_tombstone).unwrap()
        );
        assert!(!references(&state.analysis, dependencies).contains("record:fact"));
        compare(&input, &config, &mut state);
        assert_eq!(
            packet(&input, &config, &state).unwrap()["current"]["completion"]["required_finding_ids"]
                ["items"],
            json!(["finding"])
        );
        let mut args = judgment(&input, &config, &state);
        let error = put(&input, &config, &mut state, &args).unwrap_err();
        let detail: Value =
            serde_json::from_str(error.strip_prefix("INVALID_FIELD /finding_ids: ").unwrap())
                .unwrap();
        assert_eq!(detail["required_finding_ids"]["items"], json!(["finding"]));
        assert_eq!(detail["expected_status"], "findings");
        args["status"] = json!("findings");
        args["finding_ids"] = json!(["finding"]);
        assert!(
            put(&input, &config, &mut state, &args)
                .unwrap_err()
                .contains("revised or explicitly withdrawn")
        );
        assert!(!state.done);
        assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
        finding_changed(&mut state, Some(&finding), None).unwrap();
        state.review_draft.clear();
        let current = packet(&input, &config, &state).unwrap();
        assert_eq!(
            current["current"]["completion"]["required_finding_ids"]["total"],
            0
        );
        assert_eq!(
            current["current"]["completion"]["status_if_evidence_complete"],
            "checked"
        );
        let args = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &args).unwrap();
        finish_review_batch(&input, &config, &mut state).unwrap();
        assert!(state.done);
    }
}
