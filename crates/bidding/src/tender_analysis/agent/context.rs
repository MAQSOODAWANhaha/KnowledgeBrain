use super::*;
use std::collections::BTreeSet;

/// Explicit conservative estimate, not a provider tokenizer. Count all JSON
/// text bytes, replacing each image URL with the configured visual allowance.
/// Base64 is transport encoding, not text sent through the model tokenizer.
pub(super) fn estimate_input_tokens(body: &Value, limits: &Limits) -> Result<usize, AgentError> {
    crate::agent_runtime::chat::estimate_input_tokens(
        body,
        limits.image_token_reserve,
        limits.token_safety_margin,
    )
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Active,
    Complete,
    Blocked,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusAction {
    #[default]
    Locate,
    Extract,
    Link,
    Review,
    Handoff,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Focus {
    pub action: FocusAction,
    pub source_spans: Vec<Span>,
    pub references: Vec<String>,
}

/// Role-local continuation state. All referenced work remains in the analysis;
/// this is neither a second task queue nor evidence of semantic correctness.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkState {
    pub source_scope: Vec<String>,
    #[serde(default)]
    pub deferred_sources: Vec<String>,
    pub objective: String,
    #[serde(default)]
    pub focus: Focus,
    #[serde(default)]
    pub output_refs: Vec<String>,
    #[serde(default)]
    pub pending_refs: Vec<String>,
    pub status: WorkStatus,
    pub note: String,
}

pub(in crate::tender_analysis) fn reference(
    analysis: &Analysis,
    key: &str,
) -> Result<Value, String> {
    let (kind, id) = key
        .split_once(':')
        .ok_or("use record:<id>, relation:<id>, or disposition:<source_id> for a work reference, not a bare ID")?;
    match kind {
        "record" => analysis.records.get(id).map(|v| json!(v)),
        "relation" => analysis.relations.get(id).map(|v| json!(v)),
        "disposition" => analysis
            .dispositions
            .get(id)
            .map(|v| json!({"source_id":id,"disposition":v})),
        _ => None,
    }
    .ok_or_else(|| format!("unknown work reference: {key}"))
}

fn unresolved(analysis: &Analysis, key: &str) -> bool {
    let Some((kind, id)) = key.split_once(':') else {
        return false;
    };
    match kind {
        "record" => analysis
            .records
            .get(id)
            .is_some_and(|r| matches!(r.data, RecordData::Unresolved { .. })),
        "relation" => analysis
            .relations
            .get(id)
            .is_some_and(|r| r.state == RelationState::Unresolved),
        "disposition" => analysis
            .dispositions
            .get(id)
            .is_some_and(|d| d.state == DispositionState::Unresolved),
        _ => false,
    }
}

pub(in crate::tender_analysis) fn scope_references(
    analysis: &Analysis,
    scope: &[String],
) -> Vec<String> {
    let record_in_scope = |r: &Record| r.sources.iter().any(|s| scope.contains(&s.source_id));
    let mut refs: Vec<_> = analysis
        .records
        .iter()
        .filter(|(_, r)| record_in_scope(r))
        .map(|(id, _)| format!("record:{id}"))
        .collect();
    refs.extend(
        analysis
            .relations
            .iter()
            .filter(|(_, r)| {
                r.grounds.iter().any(|s| scope.contains(&s.source_id))
                    || [&r.from, &r.to]
                        .iter()
                        .any(|id| analysis.records.get(*id).is_some_and(record_in_scope))
            })
            .map(|(id, _)| format!("relation:{id}")),
    );
    refs.extend(
        scope
            .iter()
            .filter(|id| analysis.dispositions.contains_key(*id))
            .map(|id| format!("disposition:{id}")),
    );
    refs
}

/// Candidates available for explicit focus or recall, including queried support.
/// This is not the compulsory work roster for the current source task.
fn work_references(state: &Checkpoint, work: &WorkState) -> Vec<String> {
    let mut refs = scope_references(&state.analysis, &work.source_scope);
    if state.role == Role::Reviewer
        && let Some(review) = state
            .source_review
            .as_ref()
            .filter(|r| r.active_task.is_some())
    {
        for key in source_review::references(&state.analysis, &review.dependencies) {
            if !refs.contains(&key) {
                refs.push(key);
            }
        }
    }
    refs
}

fn required_work_references(
    input: &FrozenInput,
    state: &Checkpoint,
    work: &WorkState,
    max_bytes: usize,
) -> Result<Vec<String>, String> {
    Ok(
        match source_review::active_obligations(input, state, max_bytes)? {
            Some(owned) => owned.comparisons.into_iter().collect(),
            None => scope_references(&state.analysis, &work.source_scope),
        },
    )
}

/// Outcomes are host-maintained, including deferred work and unresolved cross-scope
/// dependencies. They are references only, never reading receipts or approval.
pub(in crate::tender_analysis) fn retain_outcomes(
    analysis: &Analysis,
    work: &mut WorkState,
    prior: Option<&WorkState>,
) {
    let scope: Vec<_> = work
        .source_scope
        .iter()
        .chain(&work.deferred_sources)
        .cloned()
        .collect();
    let mut refs: BTreeSet<_> = scope_references(analysis, &scope).into_iter().collect();
    refs.extend(work.output_refs.iter().chain(&work.pending_refs).cloned());
    if let Some(prior) = prior {
        refs.extend(
            prior
                .pending_refs
                .iter()
                .filter(|key| unresolved(analysis, key))
                .cloned(),
        );
    }
    work.output_refs = refs
        .iter()
        .filter(|key| reference(analysis, key).is_ok() && !unresolved(analysis, key))
        .cloned()
        .collect();
    work.pending_refs = refs
        .into_iter()
        .filter(|key| unresolved(analysis, key))
        .collect();
}

pub(super) fn synchronize_outcomes(state: &mut Checkpoint) {
    for work in [&mut state.main_work, &mut state.reviewer_work]
        .into_iter()
        .flatten()
    {
        retain_outcomes(&state.analysis, work, None);
    }
}

fn work_input(work: &WorkState) -> Result<Value, String> {
    let mut value = serde_json::to_value(work).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("work object missing")?;
    object.remove("output_refs");
    object.remove("pending_refs");
    Ok(value)
}

pub(super) fn request_work(state: &Checkpoint) -> Result<Value, String> {
    let Some(work) = state.work() else {
        return Ok(Value::Null);
    };
    let mut value = work_input(work)?;
    value["saved_outcome_count"] = json!(work.output_refs.len());
    value["pending_outcome_count"] = json!(work.pending_refs.len());
    Ok(value)
}

pub(super) fn execution_gaps(
    state: &Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    if args.as_object().is_none_or(|o| {
        o.keys()
            .any(|key| !matches!(key.as_str(), "scope" | "offset" | "limit"))
    }) {
        return Err("gap query accepts only scope, offset and limit".into());
    }
    let number = |key: &str| {
        args[key]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{key} must be a nonnegative integer"))
    };
    let rows: Vec<_> = if args["scope"] == "pending" {
        state.work().into_iter().flat_map(|w|&w.pending_refs).map(|key|json!({"reference":key,"kind":"source_uncertainty","instruction":"inspect this saved outcome when its content is needed; it is not an execution failure"})).collect()
    } else {
        state.execution().blockers.iter().map(|b| {
            let mut row = json!({"source_scope":b.scope,"dependencies_sha256":b.dependencies_sha256,"kind":"execution_blocker"});
            if super::repair_recovery::available(state, &b.scope)? {
                row["history_recovery"] = json!({"tool":"set_work_note","source_scope":b.scope,
                    "instruction":"Newly delivered original finding and prior repair history allow one bounded retry of this scope. Choose an exact correction objective and retain other deferred sources. Spent replans remain spent; this is neither evidence delivery nor approval. Re-reading this history does not renew the attempt."});
            } else if let Some(query) = super::repair_recovery::history_query(state, &b.scope)? {
                row["history_to_read"] = query;
            }
            Ok(row)
        }).collect::<Result<_, String>>()?
    };
    tools::bounded_page(&rows, number("offset")?, number("limit")?, max_bytes)
}

pub(in crate::tender_analysis) fn validate(
    input: &FrozenInput,
    state: &Checkpoint,
    next: &WorkState,
    max_bytes: usize,
) -> Result<(), String> {
    if next.source_scope.is_empty()
        || next.objective.trim().is_empty()
        || serde_json::to_vec(&work_input(next)?)
            .map_err(|e| e.to_string())?
            .len()
            > max_bytes
    {
        return Err(
            "work needs a nonempty source scope/objective and must fit the work budget".into(),
        );
    }
    for list in [
        &next.source_scope,
        &next.deferred_sources,
        &next.output_refs,
        &next.pending_refs,
        &next.focus.references,
    ] {
        if list.iter().collect::<BTreeSet<_>>().len() != list.len() {
            return Err("work references and source IDs must be distinct".into());
        }
    }
    for id in next.source_scope.iter().chain(&next.deferred_sources) {
        if !input
            .source_units
            .iter()
            .any(|s| &s.source_unit_revision_id == id)
        {
            return Err(format!("unknown work source: {id}"));
        }
    }
    if next
        .deferred_sources
        .iter()
        .any(|id| next.source_scope.contains(id))
    {
        return Err("a source cannot be both active and deferred".into());
    }
    for key in next.output_refs.iter().chain(&next.pending_refs) {
        reference(&state.analysis, key)?;
    }
    for span in &next.focus.source_spans {
        if !next.source_scope.contains(&span.source_id) {
            return Err("focus evidence must belong to the permitted source scope".into());
        }
        // A work note plans where to read; it is not a submitted citation.
        // Writes and review outcomes still validate delivered role coverage.
        if span.grid_cell.is_some() {
            tools::validate_grid_span(input, span)?;
        } else if span.view_id.is_none() {
            tools::validate_text_span(input, span)?;
        } else {
            tools::validate_span(input, state.coverage(), span)?;
        }
    }
    let allowed_focus = work_references(state, next);
    for key in &next.focus.references {
        let value = reference(&state.analysis, key)?;
        // A reviewer may focus on endpoints assigned by its current source
        // task. This plans a candidate comparison, not access to other sources.
        if !allowed_focus.contains(key) {
            return Err(format!(
                "focus reference lies outside permitted source scope: {key}"
            ));
        }
        if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > max_bytes {
            return Err(
                "focused candidate exceeds detail budget; split the candidate first".into(),
            );
        }
    }
    if next.status == WorkStatus::Active
        && next.focus.action == FocusAction::Link
        && next.focus.references.len() < 2
    {
        return Err("link focus requires the exact endpoint references to compare".into());
    }
    if next.status == WorkStatus::Active
        && next.focus.action != FocusAction::Locate
        && next.focus.action != FocusAction::Handoff
        && next.focus.references.is_empty()
        && next.focus.source_spans.is_empty()
    {
        return Err("focus needs valid planned source spans or saved candidate references".into());
    }
    if let Some(prior) = state.work() {
        let removed: Vec<_> = prior
            .source_scope
            .iter()
            .filter(|id| !next.source_scope.contains(id))
            .cloned()
            .collect();
        if prior.status == WorkStatus::Active
            && !removed.is_empty()
            && (next.status != WorkStatus::Active
                || removed.iter().any(|id| !next.deferred_sources.contains(id)))
        {
            return Err("finish the active scope before replacing it, or split it with status=active and retain every removed source in deferred_sources".into());
        }
        for id in &prior.deferred_sources {
            if !next.deferred_sources.contains(id) && !next.source_scope.contains(id) {
                return Err(format!(
                    "resume the deferred source before removing it: {id}"
                ));
            }
        }
    }
    if next.status != WorkStatus::Complete {
        return Ok(());
    }
    if let Some(gap) = completion_gaps(input, state, next, max_bytes)?.first() {
        return Err(format!(
            "{}; use check_gaps with scope=work for exact fields and references",
            gap["message"].as_str().unwrap_or("work gap")
        ));
    }
    Ok(())
}

/// The same delivered-evidence checks drive diagnostics and handoff acceptance.
/// These rows identify work; querying them does not establish reading coverage.
fn completion_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    work: &WorkState,
    max_bytes: usize,
) -> Result<Vec<Value>, String> {
    // Completion depends on confirmed evidence, not whether a redundant read
    // happened in this batch. Unseen sources and candidate versions still have
    // their own concrete gaps below.
    scoped_gaps(input, state, work, state.coverage(), max_bytes)
}

fn scoped_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    work: &WorkState,
    coverage: &Coverage,
    max_bytes: usize,
) -> Result<Vec<Value>, String> {
    let mut gaps = Vec::new();
    for mut gap in tools::reading_gaps(input, coverage) {
        let source_id = gap["source_id"].as_str().or_else(|| {
            input
                .structured_forms
                .iter()
                .find(|f| f["form_definition_revision_id"] == gap["form_id"])
                .and_then(|f| f["source_unit_revision_id"].as_str())
        });
        if source_id.is_some_and(|id| work.source_scope.iter().any(|s| s == id)) {
            gap["source_id"] = json!(source_id);
            gap["field"] = json!("source_scope");
            gap["message"] = json!("work scope still has undelivered source ranges or grid cells");
            gaps.push(gap);
        }
    }
    for id in &work.source_scope {
        if !state.analysis.dispositions.contains_key(id) {
            gaps.push(
                json!({"kind":"missing_disposition","field":"source_scope","source_id":id,
                "message":"work scope needs source dispositions"}),
            );
        }
    }
    if state.role == Role::Reviewer {
        for (id, view) in &state.analysis.coverage.views {
            if work.source_scope.contains(&view.source_id) && coverage.views.get(id) != Some(view) {
                gaps.push(json!({"kind":"unreviewed_view","field":"source_scope",
                    "source_id":view.source_id,"view_id":id,
                    "message":"independently inspect the scope's original views before completing it"}));
            }
        }
    }
    // Retention is global; completion is local. Unrelated pending outcomes stay
    // in WorkState and the final review gate, but must not pull deferred work
    // into every local comparison. Related cross-scope relations remain here.
    for key in required_work_references(input, state, work, max_bytes)? {
        if state.role == Role::Reviewer
            && coverage.candidate.get(&key) != Some(&digest(&reference(&state.analysis, &key)?)?)
        {
            gaps.push(
                json!({"kind":"unreviewed_scope_outcome","field":"source_scope","reference":key,
                "message":format!("independently inspect current scope outcome: {key}")}),
            );
        }
    }
    gaps.extend(super::repair::recovery_completion_gaps(
        state,
        &work.source_scope,
    )?);
    Ok(gaps)
}

pub(super) fn work_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    if args.as_object().is_none_or(|args| {
        args.keys()
            .any(|key| !matches!(key.as_str(), "scope" | "offset" | "limit"))
    }) {
        return Err("work gap query accepts only scope, offset and limit".into());
    }
    let number = |name: &str| {
        args[name]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{name} must be a nonnegative integer"))
    };
    let work = state
        .work()
        .ok_or("declare a source scope with set_work_note before querying work gaps")?;
    let rows = fragment_gaps(
        input,
        state,
        state.coverage(),
        completion_gaps(input, state, work, max_bytes)?,
        max_bytes,
    )?;
    tools::bounded_page(&rows, number("offset")?, number("limit")?, max_bytes)
}

fn fragment_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    coverage: &Coverage,
    mut rows: Vec<Value>,
    max_bytes: usize,
) -> Result<Vec<Value>, String> {
    if state.role == Role::Reviewer
        && let Some(gaps) = source_review::reading_gaps(input, state, coverage, max_bytes)?
    {
        rows.retain(|gap| gap["kind"] != "unread_source" && gap["kind"] != "unread_grid");
        rows.extend(gaps.into_iter().map(|mut gap| {
            gap["field"] = json!("source_review.current.task");
            gap["message"] =
                json!("independently read the assigned fragment and collection metadata");
            gap
        }));
    }
    Ok(rows)
}

pub(super) fn check_delete(state: &Checkpoint, name: &str, args: &Value) -> Result<(), String> {
    let kind = match name {
        "delete_record" => "record",
        "delete_relation" => "relation",
        _ => return Ok(()),
    };
    let key = format!(
        "{kind}:{}",
        args["id"].as_str().ok_or("delete needs an ID")?
    );
    // A prior reviewer finding can identify the pending item itself as wrong.
    // Retirement still requires independent rereview; neither the finding nor
    // the source obligation is cleared by deleting its candidate record.
    let reviewed_issue = state.role == Role::Main
        && state
            .findings_for_repair()
            .iter()
            .any(|finding| finding.affected.iter().any(|field| args["id"] == field.id));
    if unresolved(&state.analysis, &key)
        && !reviewed_issue
        && [&state.main_work, &state.reviewer_work]
            .iter()
            .filter_map(|w| w.as_ref())
            .any(|w| w.pending_refs.contains(&key))
    {
        return Err("resolve the pending outcome before deleting it; deletion cannot erase a handoff obligation".into());
    }
    Ok(())
}

pub(in crate::tender_analysis) fn check_read_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    let source_id = match name {
        "read_source" | "read_source_view" => args["source_id"].as_str(),
        "read_form" => input
            .structured_forms
            .iter()
            .find(|f| f["form_definition_revision_id"] == args["form_id"])
            .and_then(|f| f["source_unit_revision_id"].as_str()),
        _ => return Ok(()),
    }
    .ok_or("reading requires a known source or form identity")?;
    let work = state
        .work()
        .ok_or("declare an active source scope with set_work_note before reading")?;
    if work.status != WorkStatus::Active || !work.source_scope.iter().any(|id| id == source_id) {
        return Err("read lies outside active work; expand the scope for a cross-reference or complete its handoff first".into());
    }
    Ok(())
}

/// A bounded, derived work checklist accompanies every request. It is not an
/// evidence receipt. Pending reads are projected here only because that exact
/// last protocol group is being delivered in this request; the durable ledger
/// is still promoted only after a complete model response.
pub(in crate::tender_analysis) fn request_work_state(
    input: &FrozenInput,
    state: &Checkpoint,
    max_bytes: usize,
) -> Result<Value, String> {
    let Some(work) = state.work() else {
        return Ok(Value::Null);
    };
    if state.role == Role::Main && work.status == WorkStatus::Complete {
        let gaps = tools::gaps(input, &state.analysis);
        let blocked = !state.main_progress.blockers.is_empty()
            || !state.reviewer_progress.blockers.is_empty();
        if work.deferred_sources.is_empty() && !blocked && gaps.is_empty() {
            let findings = state.findings_for_repair();
            if !findings.is_empty() {
                // A changed analysis digest proves only that something was
                // edited. It cannot establish that the other findings were
                // handled, especially omissions and missing relationships.
                return Ok(json!({"next_action":"verify_review_findings",
                    "finding_count":findings.len(),
                    "instruction":"Reconcile all previous model findings against original evidence before requesting another review. A retained draft is repair feedback, not proof of a completed independent review. Use already delivered findings; inspect_review only for missing pages. Correct all grounded issues, including missing objects and relationships, in coherent source scopes. A single edit or completed scope does not resolve the other findings. Use review_findings.repair.next_finding and put_repair_result to record each actual correction or source-backed dispute before requesting independent verification. Unchanged subjects may have been repaired through related records or edges; do not invent edits to clear this navigation. The main Agent cannot withdraw reviewer findings, and this navigation does not attest repairs; the per-finding disposition gate still applies."}));
            }
            let changed = state
                .review
                .as_ref()
                .map(|review| digest(&state.analysis).map(|sha| sha != review.analysis_sha256))
                .transpose()?
                .unwrap_or(true);
            return Ok(if changed {
                json!({"next_action":"request_review",
                    "instruction":"The declared scope is complete and the full collection has no structural gaps. Call request_review to independently check the current analysis. Previous review_findings remain the previous report until the reviewer rechecks; the main Agent does not clear them. This navigation is not semantic approval."})
            } else {
                json!({"next_action":"verify_review_findings",
                    "instruction":"The analysis is unchanged since the previous review. Recheck the findings against original sources before deciding a correction or requesting another review; unchanged data is not a completed repair."})
            });
        }
        let deferred = work.deferred_sources.first();
        let rows = if blocked || deferred.is_some() {
            &[][..]
        } else {
            gaps.as_slice()
        };
        let source_id = deferred.map(String::as_str).or_else(|| {
            let gap = rows.first()?;
            gap["source_id"].as_str().or_else(|| {
                input
                    .structured_forms
                    .iter()
                    .find(|f| f["form_definition_revision_id"] == gap["form_id"])
                    .and_then(|f| f["source_unit_revision_id"].as_str())
            })
        });
        let source = input
            .source_units
            .iter()
            .find(|s| Some(s.source_unit_revision_id.as_str()) == source_id);
        let mut packet = json!({
            "next_action":if blocked {"resolve_execution_blockers"} else if deferred.is_some() {"resume_deferred_scope"} else {"select_next_scope"},
            "next_source":source.map(|s| json!({"source_id":s.source_unit_revision_id,
                "document_id":s.document_id,"ordinal":s.ordinal,"page_ordinal":s.locator["page_ordinal"],"bytes":s.text.len()})),
            "instruction":if blocked {
                "Resolve the existing execution blockers shown in execution before requesting review; a completed local scope does not clear them."
            } else if deferred.is_some() {
                "Resume a deferred source with set_work_note, keeping all other deferred work. Opening it will show its current local gaps. Navigation is not evidence or semantic completion."
            } else {
                "One pending global gap is shown. Choose a coherent next scope with set_work_note, or follow another needed cross-reference; this is not a prescribed reading order. Read an indicated metadata collection directly with collection_index. More global gaps are available with check_gaps scope=analysis at blockers.next. These hints grant no reading receipt or semantic approval."
            },
            "blockers":null
        });
        let overhead = serde_json::to_vec(&packet)
            .map_err(|e| e.to_string())?
            .len()
            - serde_json::to_vec(&Value::Null)
                .map_err(|e| e.to_string())?
                .len();
        let page_budget = max_bytes
            .checked_sub(overhead)
            .ok_or("next work hint exceeds input budget")?;
        packet["blockers"] = tools::bounded_page(rows, 0, 1, page_budget)?;
        return Ok(packet);
    }
    if work.status != WorkStatus::Active {
        return Ok(Value::Null);
    }
    // Independent scopes remain executable; global blockers are still checked
    // when requesting or accepting the whole-analysis review.
    let blocked = scope_is_blocked(state, &work.source_scope)?;
    let coverage = state
        .pending_coverage
        .as_ref()
        .unwrap_or_else(|| state.coverage());
    let rows = fragment_gaps(
        input,
        state,
        coverage,
        scoped_gaps(input, state, work, coverage, max_bytes)?,
        max_bytes,
    )?;
    let mut counts = BTreeMap::<String, usize>::new();
    for gap in &rows {
        *counts
            .entry(
                gap["kind"]
                    .as_str()
                    .ok_or("work gap kind missing")?
                    .to_owned(),
            )
            .or_default() += 1;
    }
    let mut packet = json!({
        "gap_counts":counts,
        "next_action":if blocked {
            "resolve_execution_blockers"
        } else if !rows.is_empty() {
            "resolve_work_gaps"
        } else if state.role == Role::Reviewer {
            "compare_then_judge_source"
        } else {
            "complete_scope"
        },
        "instruction":if state.role == Role::Reviewer {
            "Work gaps track evidence delivery, not semantic approval. Compare the focused original evidence and candidate now. Save a grounded mismatch with put_review_finding; if the comparison is correct, record it with complete_review_check instead of inventing a finding. Judge source omissions and boundaries with put_source_review for the assigned task. The host advances completed tasks and aggregates when all required judgments exist. Re-read only a specific missing comparison field, not the entire inventory. Execution blockers still prevent submission."
        } else {
            "Resolve the listed gaps, save the grounded results, then use set_work_note status=complete for the SAME source_scope. An empty gap list does not call for another inventory read."
        },
        "receipt_basis":"Committed evidence plus reads delivered in this request; confirmation requires your complete tool response. This checklist does not itself establish evidence.",
        "blockers":null
    });
    if state.role == Role::Reviewer {
        let refs = required_work_references(input, state, work, max_bytes)?;
        let mut remaining = Vec::new();
        for key in &refs {
            if !has_review_outcome(state, key)? {
                remaining.push(key);
            }
        }
        let mut focus_remaining = 0;
        let mut next_focused = None;
        for key in &work.focus.references {
            if !has_review_outcome(state, key)? {
                focus_remaining += 1;
                next_focused.get_or_insert(key);
            }
        }
        packet["comparison_progress"] = json!({"total":refs.len(),
            "with_recorded_outcome":refs.len()-remaining.len(),"remaining":remaining.len(),
            "focus_total":work.focus.references.len(),"focus_remaining":focus_remaining,
            "next_reference":next_focused.or_else(|| remaining.first().copied()),
            "instruction":"Save a finding or complete_review_check after comparing a candidate. This is local progress, not automatic approval; source-to-result omissions still need independent checking."});
        if review_focus_complete(state)? && !blocked {
            if !remaining.is_empty() {
                packet["next_action"] = json!("select_next_review_focus");
                packet["instruction"] = json!(
                    "The current review focus already has recorded outcomes. Use set_work_note to select unfinished exact references, starting with comparison_progress.next_reference; then retrieve only the missing evidence or candidate fields for that comparison. Do not repeat completed checks or re-read the completed focus. Preserve deferred sources and check source-to-result omissions before completing the scope."
                );
            } else if rows.is_empty() {
                packet["next_action"] = json!("complete_source_review");
                packet["instruction"] = json!(
                    "All scope candidates have recorded comparison outcomes. Independently check whether original source obligations were omitted from the candidate set; save any grounded findings, then call put_source_review with the current task version and explicit boundaries. Recorded candidate checks alone do not prove source completeness or approve the analysis."
                );
            }
        }
    }
    let overhead = serde_json::to_vec(&packet)
        .map_err(|e| e.to_string())?
        .len()
        - serde_json::to_vec(&Value::Null)
            .map_err(|e| e.to_string())?
            .len();
    let page_budget = max_bytes
        .checked_sub(overhead)
        .ok_or("work checklist exceeds input budget")?;
    packet["blockers"] = tools::bounded_page(&rows, 0, rows.len().max(1), page_budget)?;
    Ok(packet)
}

/// Inventory of source payloads actually retained in a request transcript.
/// It is used only for context selection, never to acknowledge reading.
pub(in crate::tender_analysis) fn visible_work_evidence(
    state: &Checkpoint,
    messages: &[Value],
) -> BTreeMap<String, Vec<(usize, usize)>> {
    let scope = state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
        .map(|work| work.source_scope.as_slice())
        .unwrap_or_default();
    let mut ranges = BTreeMap::<String, Vec<(usize, usize)>>::new();
    for message in messages {
        if let Some(ids) = message["source_view_refs"].as_array() {
            for id in ids.iter().filter_map(Value::as_str) {
                if state
                    .source_views
                    .get(id)
                    .is_some_and(|view| scope.contains(&view.identity.source_id))
                {
                    ranges.insert(format!("view:{id}"), vec![(0, 1)]);
                }
            }
        }
        let Some(output) = message["content"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
        else {
            continue;
        };
        let assigned = if message["role"] == "tool" && output["ok"] == true {
            output["result"].get("assigned_evidence")
        } else if message["role"] == "user" {
            output["preloaded_evidence"].get("assigned_evidence")
        } else {
            None
        };
        if let Some(assigned) = assigned {
            // Inspect actual packet values just like tool results. This
            // inventory controls context admission, never reading credit.
            let mut items = Vec::new();
            for item in assigned["candidates"].as_array().into_iter().flatten() {
                let Some(key) = item["reference"].as_str() else {
                    continue;
                };
                if let Ok(current) = reference(&state.analysis, key)
                    && current == item["value"]
                    && digest(&current).ok().as_deref() == item["sha256"].as_str()
                {
                    items.push(current);
                }
            }
            let mut projected = vec![
                json!({"role":"tool","content":json!({"ok":true,"result":assigned["source"]}).to_string()}),
                json!({"role":"tool","content":json!({"ok":true,"result":{"view":"detail","items":items}}).to_string()}),
            ];
            for boundary in assigned["boundary_evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(
                    assigned["subject_evidence"]["items"]
                        .as_array()
                        .into_iter()
                        .flatten(),
                )
            {
                projected.push(json!({"role":"tool","content":json!({"ok":true,"result":boundary["source"]}).to_string()}));
            }
            for (key, spans) in visible_work_evidence(state, &projected) {
                for (start, end) in spans {
                    tools::cover(ranges.entry(key.clone()).or_default(), start, end);
                }
            }
        }
        if message["role"] == "user" {
            for item in output["retained_candidate_details"]["items"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(key) = item["reference"].as_str() else {
                    continue;
                };
                let Ok(current) = reference(&state.analysis, key) else {
                    continue;
                };
                let Ok(version) = digest(&current) else {
                    continue;
                };
                if state
                    .coverage()
                    .candidate
                    .get(key)
                    .is_some_and(|saved| saved == &version)
                    && current == item["value"]
                    && item
                        .get("sha256")
                        .is_none_or(|sha| sha.as_str() == Some(version.as_str()))
                {
                    ranges.insert(format!("candidate:{key}:{version}"), vec![(0, 1)]);
                }
            }
        }
        if message["role"] != "tool" {
            continue;
        }
        let result = &output["result"];
        if output["ok"] == true && result["view"] == "detail" {
            for item in result["items"].as_array().into_iter().flatten() {
                let key = if item["data"].is_object() {
                    item["id"].as_str().map(|id| format!("record:{id}"))
                } else if item["from"].is_string() && item["to"].is_string() {
                    item["id"].as_str().map(|id| format!("relation:{id}"))
                } else if item["disposition"].is_object() {
                    item["source_id"]
                        .as_str()
                        .map(|id| format!("disposition:{id}"))
                } else {
                    None
                };
                if let Some(key) = key
                    && scope_references(&state.analysis, scope).contains(&key)
                    && let Ok(current) = reference(&state.analysis, &key)
                    && current == *item
                    && let Ok(version) = digest(item)
                {
                    ranges.insert(format!("candidate:{key}:{version}"), vec![(0, 1)]);
                }
            }
        }
        let Some(source_id) = result["source_id"].as_str() else {
            continue;
        };
        if output["ok"] != true || !scope.iter().any(|id| id == source_id) {
            continue;
        }
        let (key, start, end) = if let Some(form_id) = result["form_id"].as_str() {
            (
                format!("form:{form_id}"),
                result["offset"].as_u64(),
                result["next"].as_u64(),
            )
        } else if result["text"].is_string() {
            (
                format!("text:{source_id}"),
                result["start"].as_u64(),
                result["end"].as_u64(),
            )
        } else {
            continue;
        };
        if let (Some(start), Some(end)) = (
            start.and_then(|n| usize::try_from(n).ok()),
            end.and_then(|n| usize::try_from(n).ok()),
        ) {
            tools::cover(ranges.entry(key).or_default(), start, end);
        }
    }
    ranges
}

pub(super) fn focused_work_evidence(
    state: &Checkpoint,
    messages: &[Value],
) -> BTreeMap<String, Vec<(usize, usize)>> {
    let mut evidence = visible_work_evidence(state, messages);
    let refs = state.work().map(|w| &w.focus.references);
    let views = focused_view_ids(state);
    evidence.retain(|key, _| {
        if let Some(id) = key.strip_prefix("view:") {
            return views.contains(id);
        }
        !key.starts_with("candidate:")
            || refs.is_some_and(|refs| {
                refs.iter()
                    .any(|r| key.starts_with(&format!("candidate:{r}:")))
            })
    });
    evidence
}

pub(super) fn focused_view_ids(state: &Checkpoint) -> BTreeSet<String> {
    state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
        .into_iter()
        .flat_map(|work| &work.focus.source_spans)
        .filter_map(|span| span.view_id.clone())
        .collect()
}

/// Recall current focused/assigned versions already delivered to this role. This
/// working message is separate from evictable tool history, but participates
/// in the same total request budget. It creates no receipt or semantic result.
pub(super) fn retained_candidate_message(
    state: &Checkpoint,
    budget: usize,
) -> Result<Value, String> {
    let visible = visible_work_evidence(state, &state.transcript);
    candidate_recall_message(state, budget, &visible)
}

pub(super) fn trim_optional_candidate_recall(
    state: &Checkpoint,
    message: &mut Value,
    excluded: &mut BTreeSet<String>,
    excess: usize,
) -> Result<bool, String> {
    let Some(raw) = message["content"].as_str() else {
        return Ok(false);
    };
    let mut content: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let items = content["retained_candidate_details"]["items"]
        .as_array_mut()
        .ok_or("candidate recall items missing")?;
    let mut removed_bytes = 0;
    for item in items.iter().rev() {
        if removed_bytes >= excess {
            break;
        }
        let key = item["reference"]
            .as_str()
            .ok_or("candidate recall reference missing")?;
        if !excluded.contains(key)
            && !state
                .work()
                .is_some_and(|work| work.focus.references.iter().any(|r| r == key))
        {
            excluded.insert(key.to_owned());
            removed_bytes += serde_json::to_vec(item).map_err(|e| e.to_string())?.len();
        }
    }
    items.retain(|item| {
        !item["reference"]
            .as_str()
            .is_some_and(|key| excluded.contains(key))
    });
    let count = items.len();
    if count == 0 {
        *message = Value::Null;
    } else {
        content["retained_candidate_details"]["next"] = json!(count);
        message["content"] = json!(content.to_string());
    }
    Ok(removed_bytes > 0)
}

fn candidate_recall_message(
    state: &Checkpoint,
    budget: usize,
    visible: &BTreeMap<String, Vec<(usize, usize)>>,
) -> Result<Value, String> {
    let Some(work) = state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
    else {
        return Ok(Value::Null);
    };
    let mut keys = work.focus.references.clone();
    if state.role == Role::Reviewer
        && state
            .source_review
            .as_ref()
            .is_some_and(|r| r.active_task.is_some())
    {
        for key in work_references(state, work) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    let mut candidates = Vec::new();
    for key in &keys {
        let Ok(value) = reference(&state.analysis, key) else {
            continue;
        };
        let sha = digest(&value)?;
        if state.coverage().candidate.get(key) == Some(&sha)
            && !visible.contains_key(&format!("candidate:{key}:{sha}"))
        {
            candidates.push(json!({"reference":key,"value":value}));
        }
    }
    if candidates.is_empty() {
        return Ok(Value::Null);
    }
    let mut content = json!({"retained_candidate_details":null,
        "note":"Current focused or assigned source-task candidate versions previously delivered to this role, recalled after tool-history eviction. This is not a new read or a comparison result. Unlisted candidates remain retrievable by exact ID."});
    let overhead = serde_json::to_vec(&content)
        .map_err(|e| e.to_string())?
        .len();
    let Ok(page) = tools::bounded_page(&candidates, 0, usize::MAX, budget.saturating_sub(overhead))
    else {
        // Recall is optional if a wrapped candidate cannot fit its allowance.
        // Exact reading and ordinary focus backpressure remain available.
        return Ok(Value::Null);
    };
    if page["items"].as_array().is_none_or(Vec::is_empty) {
        return Ok(Value::Null);
    }
    content["retained_candidate_details"] = page;
    Ok(json!({"role":"user","content":content.to_string()}))
}

/// Supply the same deterministic locations for already-delivered source reads
/// when resuming an older compatible checkpoint. Never rewrite a reserved body
/// or the latest pending group, or count annotations as newly read evidence.
pub(super) fn annotate_delivered_source_lines(
    state: &mut Checkpoint,
    max_bytes: usize,
) -> Result<(), String> {
    let Some(latest) = state
        .transcript
        .iter()
        .rposition(|m| m["role"] == "assistant")
    else {
        return Ok(());
    };
    let reads: BTreeSet<_> = state.transcript[..latest]
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter(|call| call["function"]["name"] == "read_source")
        .filter_map(|call| call["id"].as_str().map(str::to_owned))
        .collect();
    for message in &mut state.transcript[..latest] {
        if !message["tool_call_id"]
            .as_str()
            .is_some_and(|id| reads.contains(id))
        {
            continue;
        }
        let Some(mut output) = message["content"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
        else {
            continue;
        };
        if output["ok"] != true || output["result"].get("line_spans").is_some() {
            continue;
        }
        let result = &mut output["result"];
        let (Some(text), Some(start)) = (
            result["text"].as_str(),
            result["start"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok()),
        ) else {
            continue;
        };
        result["line_spans"] = json!(tools::source_line_spans(text, start));
        if serde_json::to_vec(result).map_err(|e| e.to_string())?.len() <= max_bytes {
            message["content"] = json!(output.to_string());
        }
    }
    Ok(())
}

/// Replace one old navigation payload before evicting its mixed protocol group.
/// Navigation is not evidence. The latest group is still awaiting delivery and
/// must remain verbatim, as must all source and complete candidate results.
pub(super) fn compact_delivered_navigation(transcript: &mut [Value]) -> bool {
    let Some(latest) = transcript.iter().rposition(|m| m["role"] == "assistant") else {
        return false;
    };
    let names: BTreeMap<_, _> = transcript[..latest]
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter_map(|call| {
            Some((
                call["id"].as_str()?.to_owned(),
                call["function"]["name"].as_str()?.to_owned(),
            ))
        })
        .collect();
    for message in &mut transcript[..latest] {
        let Some(name) = message["tool_call_id"]
            .as_str()
            .and_then(|id| names.get(id))
        else {
            continue;
        };
        if !matches!(
            name.as_str(),
            "source_index" | "search_sources" | "inspect_analysis"
        ) {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        let Ok(output) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if output["ok"] != true
            || output["result"]["history_omitted"] == true
            || (name == "inspect_analysis" && output["result"]["view"] != "index")
        {
            continue;
        }
        let compact = json!({"ok":true,"result":{"history_omitted":true,
            "note":"Previously delivered navigation omitted from history to retain source evidence; query this tool again only if needed."}}).to_string();
        if compact.len() < content.len() {
            message["content"] = json!(compact);
            return true;
        }
    }
    false
}

/// Evict a complete delivered protocol group, preserving unique active source
/// evidence when another group can be removed instead.
pub(super) fn evict_delivered_group(
    state: &mut Checkpoint,
    history_budget: usize,
    allow_unique: bool,
) -> bool {
    let starts: Vec<_> = state
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "assistant")
        .map(|(index, _)| {
            if index > 0
                && super::evidence_delivery::is_retained_message(&state.transcript[index - 1])
            {
                index - 1
            } else {
                index
            }
        })
        .collect();
    if starts.len() < 2 {
        return false;
    }
    let groups: Vec<_> = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            (
                if i == 0 { 0 } else { start },
                starts.get(i + 1).copied().unwrap_or(state.transcript.len()),
            )
        })
        .collect();
    let evidence: Vec<_> = groups
        .iter()
        .map(|&(start, end)| visible_work_evidence(state, &state.transcript[start..end]))
        .collect();
    let redundant = (0..groups.len() - 1).find(|&candidate| {
        let mut other = BTreeMap::<String, Vec<(usize, usize)>>::new();
        for (_, items) in evidence
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != candidate)
        {
            for (key, ranges) in items {
                for &(start, end) in ranges {
                    tools::cover(other.entry(key.clone()).or_default(), start, end);
                }
            }
        }
        evidence[candidate].iter().all(|(key, ranges)| {
            ranges.iter().all(|&(start, end)| {
                other
                    .get(key)
                    .is_some_and(|other| other.iter().any(|&(a, b)| a <= start && b >= end))
            })
        })
    });
    // Release oversized delivered images, or smaller ones before the fallback
    // sacrifices unique evidence. A mixed batch can still contain the exact
    // candidates and grids needed for the active comparison. Request assembly
    // separately retains the actual pixels of currently required views.
    // Use a lower bound from the actual cached payloads, not token estimates.
    let releasable_images = (0..groups.len() - 1).find(|&candidate| {
        let (start, end) = groups[candidate];
        let image_bytes = state.transcript[start..end]
            .iter()
            .filter_map(|message| message["source_view_refs"].as_array())
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|id| state.source_views.get(id))
            .fold(0usize, |bytes, view| {
                bytes.saturating_add(view.jpeg_base64.len())
            });
        image_bytes > history_budget || (allow_unique && image_bytes > 0)
    });
    if redundant.is_none()
        && let Some(candidate) = releasable_images
    {
        let (start, end) = groups[candidate];
        for message in &mut state.transcript[start..end] {
            if let Some(ids) = message.get("source_view_refs") {
                *message = json!({"role":"user","content":json!({
                    "history_omitted_source_views":ids,
                    "note":"Previously delivered image pixels omitted from history. The image receipt is not a new reading or semantic judgment. Use read_source_view for a specific visual comparison when needed."
                }).to_string()});
            }
        }
        return true;
    }
    if redundant.is_none() && !allow_unique {
        return false;
    }
    // Preserve the latest pending group and enforce the frozen ceiling even
    // when all remaining groups contain unique evidence.
    let unfocused = (0..groups.len() - 1).find(|&candidate| {
        let (start, end) = groups[candidate];
        focused_work_evidence(state, &state.transcript[start..end]).is_empty()
    });
    if redundant.or(unfocused).is_none()
        && compact_delivered_candidate_details(state, *starts.last().unwrap(), &[])
    {
        return true;
    }
    let (start, end) = groups[redundant.or(unfocused).unwrap_or(0)];
    state.transcript.drain(start..end);
    true
}

/// A delivered batch can mix current source evidence and focused candidates
/// with details no longer in focus. Release only the latter before sacrificing
/// the complete group. Latest outputs and all reading receipts stay untouched.
pub(super) fn compact_recallable_candidate_details(state: &mut Checkpoint, budget: usize) -> bool {
    let Some(latest) = state
        .transcript
        .iter()
        .rposition(|m| m["role"] == "assistant")
    else {
        return false;
    };
    // Reserve room for these versions even if every candidate history payload
    // disappears. Never release a focused value that cannot fit this recall.
    let Ok(message) = candidate_recall_message(state, budget, &BTreeMap::new()) else {
        return false;
    };
    let Some(content) = message["content"]
        .as_str()
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
    else {
        return false;
    };
    let recallable: Vec<_> = content["retained_candidate_details"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| item["value"].clone())
        .collect();
    compact_delivered_candidate_details(state, latest, &recallable)
}

fn compact_delivered_candidate_details(
    state: &mut Checkpoint,
    latest: usize,
    recallable: &[Value],
) -> bool {
    let protected: Vec<_> = state
        .work()
        .into_iter()
        .flat_map(|work| &work.focus.references)
        .filter_map(|key| reference(&state.analysis, key).ok())
        .filter(|value| !recallable.contains(value))
        .collect();
    let inspections: BTreeSet<_> = state.transcript[..latest]
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter(|call| call["function"]["name"] == "inspect_analysis")
        .filter_map(|call| call["id"].as_str().map(str::to_owned))
        .collect();
    for message in &mut state.transcript[..latest] {
        if message["role"] != "tool"
            || !message["tool_call_id"]
                .as_str()
                .is_some_and(|id| inspections.contains(id))
        {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        let Ok(mut output) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if output["ok"] != true || output["result"]["view"] != "detail" {
            continue;
        }
        let Some(items) = output["result"]["items"].as_array_mut() else {
            continue;
        };
        let before = items.len();
        items.retain(|item| protected.contains(item));
        let removed = before - items.len();
        if removed == 0 {
            continue;
        }
        output["result"]["history_omitted_items"] = json!(
            output["result"]["history_omitted_items"]
                .as_u64()
                .unwrap_or(0)
                + removed as u64
        );
        output["result"]["history_note"] = json!(
            "Previously delivered candidate details omitted when outside focus or retained in the bounded working message; original pagination counters are historical. Retrieve exact IDs when needed. This establishes no new reading or comparison."
        );
        let compact = output.to_string();
        if compact.len() < content.len() {
            message["content"] = json!(compact);
            return true;
        }
    }
    false
}

pub(super) fn raw_scope_dependencies(
    state: &Checkpoint,
    scope: &[String],
) -> Result<String, String> {
    let values: Vec<_> = scope_references(&state.analysis, scope)
        .iter()
        .map(|key| reference(&state.analysis, key))
        .collect::<Result<_, _>>()?;
    digest(&values)
}

fn scope_dependencies(state: &Checkpoint, scope: &[String]) -> Result<String, String> {
    super::repair_recovery::dependencies(state, scope, raw_scope_dependencies(state, scope)?)
}

pub(in crate::tender_analysis) fn check_blocked_scope(
    state: &Checkpoint,
    scope: &[String],
) -> Result<(), String> {
    if scope_is_blocked(state, scope)? {
        return Err(
            "blocked source dependencies are unchanged; continue an independent scope instead"
                .into(),
        );
    }
    Ok(())
}

fn scope_is_blocked(state: &Checkpoint, scope: &[String]) -> Result<bool, String> {
    for blocker in &state.execution().blockers {
        if blocker.scope.iter().any(|id| scope.contains(id))
            && scope_dependencies(state, &blocker.scope)? == blocker.dependencies_sha256
            && !super::repair_recovery::available(state, &blocker.scope)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Only used before preparing a fresh boundary. Saved responses still execute,
/// and changed dependencies or an independent source retain the handoff path.
pub(super) fn check_independent_work(
    input: &FrozenInput,
    state: &Checkpoint,
) -> Result<(), AgentError> {
    if state.execution().watch.recovery != Recovery::Blocked {
        return Ok(());
    }
    let mut blocked = BTreeSet::new();
    for blocker in &state.execution().blockers {
        if scope_dependencies(state, &blocker.scope).map_err(invalid)?
            == blocker.dependencies_sha256
            && !super::repair_recovery::available(state, &blocker.scope).map_err(invalid)?
        {
            blocked.extend(&blocker.scope);
        }
    }
    if input
        .source_units
        .iter()
        .all(|source| blocked.contains(&source.source_unit_revision_id))
    {
        return Err(error(
            "AGENT_TURN_BUDGET_EXCEEDED",
            "no independent source scope remains; execution blockers and checkpoint retained",
        ));
    }
    Ok(())
}

pub(in crate::tender_analysis) fn observe_progress(
    state: &mut Checkpoint,
    role: &Role,
    local_completion: Option<String>,
    limits: &Limits,
) -> Result<(), String> {
    let work = if *role == Role::Main {
        &state.main_work
    } else {
        &state.reviewer_work
    };
    let mut scope = work
        .as_ref()
        .map(|w| w.source_scope.clone())
        .unwrap_or_default();
    scope.sort();
    let dependencies = scope_dependencies(state, &scope)?;
    let coverage = if *role == Role::Main {
        &state.analysis.coverage
    } else {
        &state.reviewer_coverage
    };
    let mut versions = Vec::new();
    // Receipts are already confirmed at this complete response boundary.
    // Pending reads are excluded; evicting and re-reading cannot create novelty.
    for (kind, value) in serde_json::to_value(coverage)
        .map_err(|e| e.to_string())?
        .as_object()
        .ok_or("coverage object missing")?
    {
        if kind == "view_failures" {
            continue;
        }
        if let Some(entries) = value.as_object() {
            for (id, value) in entries {
                versions.push(digest(&json!([kind, id, value]))?);
            }
        } else {
            versions.push(digest(&json!([kind, value]))?);
        }
    }
    for key in scope_references(&state.analysis, &scope) {
        versions.push(digest(&reference(&state.analysis, &key)?)?);
    }
    for finding in state.review_draft.values() {
        versions.push(digest(finding)?);
    }
    if *role == Role::Main {
        for (id, receipt) in &state.repair.results {
            // Rephrasing the explanation is not another unit of progress.
            versions.push(digest(&json!([
                "repair_disposition",
                id,
                receipt.candidate_versions
            ]))?);
        }
    }
    if *role == Role::Main && repair::tasks::active(state).is_some() {
        // Task completion is judged after the entire batch. Local writes, scope
        // notes and role handoffs cannot refund this task or edit legacy blockers.
        state
            .main_progress
            .observe(versions, None, &limits.progress());
        return Ok(());
    }
    let completed = work
        .as_ref()
        .is_some_and(|w| w.status == WorkStatus::Complete)
        || state.role != *role
        || state.done;
    let completion = completed
        .then(|| digest(&json!([scope, dependencies, state.role, state.done])))
        .transpose()?;
    let active_progress = if *role == Role::Main {
        &state.main_progress
    } else {
        &state.reviewer_progress
    };
    let active_dependencies: Vec<_> = active_progress
        .blockers
        .iter()
        .filter(|blocker| {
            blocker.watch.recovery != Recovery::Blocked
                && blocker.scope.iter().any(|id| scope.contains(id))
        })
        .map(|blocker| {
            Ok((
                blocker.scope.clone(),
                scope_dependencies(state, &blocker.scope)?,
            ))
        })
        .collect::<Result<_, String>>()?;
    let progress = if *role == Role::Main {
        &mut state.main_progress
    } else {
        &mut state.reviewer_progress
    };
    progress.observe(
        versions,
        completion.or(local_completion),
        &limits.progress(),
    );
    if completed {
        progress
            .blockers
            .retain(|b| !b.scope.iter().all(|id| scope.contains(id)));
    }
    for (active_scope, active_hash) in active_dependencies {
        if let Some(blocker) = progress
            .blockers
            .iter_mut()
            .find(|b| b.scope == active_scope)
        {
            blocker.watch = progress.watch.clone();
            if progress.watch.recovery == Recovery::Blocked {
                // Expanded or narrowed work must also close the original retry.
                blocker.dependencies_sha256 = active_hash;
            }
        }
    }
    progress.block(scope, dependencies);
    if progress.watch.recovery == Recovery::Blocked {
        let work = if *role == Role::Main {
            &mut state.main_work
        } else {
            &mut state.reviewer_work
        };
        if let Some(work) = work {
            work.status = WorkStatus::Blocked;
        }
    }
    Ok(())
}

pub(in crate::tender_analysis) fn execution_packet(
    state: &Checkpoint,
    max_bytes: usize,
) -> Result<Value, String> {
    let progress = state.execution();
    let completed_focus = review_focus_complete(state)?
        && !scope_is_blocked(
            state,
            &state
                .work()
                .expect("completed review focus has work")
                .source_scope,
        )?;
    let history_available = progress.blockers.iter().try_fold(false, |found, blocker| {
        super::repair_recovery::available(state, &blocker.scope).map(|available| found || available)
    })?;
    let mut packet = json!({"watch":progress.watch,"blocker_count":progress.blockers.len(),
    "next_action":if history_available {
        "A blocker below has newly delivered prior repair history. Its history_recovery navigation permits one explicit set_work_note retry while retaining spent replans and deferred work. This does not approve or complete the scope."
    } else if completed_focus {
        "The current review focus already has recorded outcomes. Follow work_state.next_action and comparison_progress.next_reference to select unfinished comparisons. Do not repeat the completed focus. If none remain, complete the assigned source-to-result judgment with put_source_review; the host advances and aggregates. This guidance neither grants approval nor resets recovery budgets."
    } else if state.role == Role::Reviewer && progress.watch.needs_replan_context() {
        "Replan the stalled comparison now. Select a single unfinished candidate or one exact source uncertainty and narrow focus with set_work_note; retain the assigned source task and permitted cross-reference scope. Compare that candidate with its original evidence and save put_review_finding or complete_review_check before loading another inventory page. If a particular field is missing, retrieve only that evidence. Do not wait to read every candidate before saving the first comparison. After the local comparisons, judge source omissions and boundaries with put_source_review; the host advances tasks and aggregates complete judgments. Execution blockers prevent submission. This neither approves the analysis nor renews recovery allowances."
    } else { match (&state.role, &progress.watch.recovery) {
        (Role::Reviewer, Recovery::Running) => "Compare the focused source and candidate now. Save actionable mismatches with put_review_finding; record a correct comparison with complete_review_check, without inventing a finding. Judge the assigned source fragment with put_source_review, including omissions and continuation boundaries. The host advances tasks and aggregates complete judgments. Repeated reads/notes are not progress; inspect only a specific missing field. Execution blockers prevent submission.",
        (_, Recovery::Running) => "Save the current grounded local result before expanding comparisons.",
        (_, Recovery::Replan) => "Repeated reads/notes are not progress. Narrow focus to an exact clause or endpoint pair and write its result; inspect only a specific missing field.",
        (_, Recovery::Blocked) => "Select an independent source scope. This execution failure prevents final acceptance and cannot be converted to source uncertainty."
    }}});
    if state.role == Role::Main
        && let Some((id, entry)) = repair::tasks::active(state)
    {
        packet["repair_task"] = json!({"id":id,"finding_sha256":entry.finding_sha256,"committed_turns":entry.committed_turns});
        packet["next_action"] = json!(
            "Work on the host-assigned repair task. Changing source scope or focus does not change the task or renew its allowance. Save a grounded revised or disputed result; the host checks the full batch before selecting another task. Existing source blockers remain binding."
        );
    }
    packet["blockers"] = execution_gaps(
        state,
        &json!({"scope":"execution","offset":0,"limit":progress.blockers.len().max(1)}),
        max_bytes.saturating_sub(
            serde_json::to_vec(&packet)
                .map_err(|e| e.to_string())?
                .len()
                + 16,
        ),
    )?;
    Ok(packet)
}

/// A clean comparison is a local outcome too. Its receipt is keyed by the
/// candidate version, not wording, so repeated assertions cannot renew budgets.
fn review_check_key(reference: &str, version: &str) -> Result<String, String> {
    digest(&json!(["review_check", reference, version]))
}

pub(in crate::tender_analysis) fn candidate_findings(
    state: &Checkpoint,
    reference: &str,
) -> Vec<String> {
    let Some((kind, id)) = reference.split_once(':') else {
        return vec![];
    };
    let current = self::reference(&state.analysis, reference)
        .ok()
        .and_then(|value| digest(&value).ok());
    if current.is_none() || current.as_ref() != state.reviewer_coverage.candidate.get(reference) {
        return vec![];
    }
    state
        .review_draft
        .iter()
        .filter(|(_, finding)| {
            if kind == "disposition" {
                finding.sources.iter().any(|source| source.source_id == id)
            } else {
                finding.affected.iter().any(|affected| affected.id == id)
            }
        })
        .map(|(id, _)| id.clone())
        .collect()
}

pub(in crate::tender_analysis) fn has_review_outcome(
    state: &Checkpoint,
    key: &str,
) -> Result<bool, String> {
    let version = source_review::candidate_version(state, key)?;
    Ok(state
        .reviewer_progress
        .seen
        .contains(&review_check_key(key, &version)?)
        || !candidate_findings(state, key).is_empty())
}

fn review_focus_complete(state: &Checkpoint) -> Result<bool, String> {
    let Some(work) = state.work().filter(|work| {
        state.role == Role::Reviewer
            && work.status == WorkStatus::Active
            && work.focus.action == FocusAction::Review
            && !work.focus.references.is_empty()
    }) else {
        return Ok(false);
    };
    for key in &work.focus.references {
        if !has_review_outcome(state, key)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::tender_analysis) fn complete_review_check(
    input: &FrozenInput,
    state: &mut Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Check {
        reference: String,
        summary: String,
        sources: Vec<Span>,
    }
    let check: Check = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    let work = state.work().ok_or("declare a review focus first")?;
    let assigned = state
        .source_review
        .as_ref()
        .is_some_and(|r| r.active_task.is_some())
        && work_references(state, work).contains(&check.reference);
    if work.status != WorkStatus::Active
        || (!assigned
            && (work.focus.action != FocusAction::Review
                || !work.focus.references.contains(&check.reference)))
    {
        return Err(
            "clean review check must name an exact candidate in the active review focus".into(),
        );
    }
    if check.summary.trim().is_empty() || check.sources.is_empty() {
        return Err(
            "clean review check needs a comparison summary and original source citations".into(),
        );
    }
    let version = digest(&reference(&state.analysis, &check.reference)?)?;
    if state.reviewer_coverage.candidate.get(&check.reference) != Some(&version) {
        return Err(
            "independently inspect the current candidate before recording a clean comparison"
                .into(),
        );
    }
    let findings = candidate_findings(state, &check.reference);
    if !findings.is_empty() {
        return Err(tools::field_error(
            "/reference",
            json!({"reference":check.reference,"recorded_outcome":"findings",
                "finding_ids":tools::bounded_page(&findings,0,usize::MAX,max_bytes/4)?,
                "instruction":"This candidate already has a recorded finding outcome. Do not submit a clean comparison for it or repeat an inventory just to record completion. Retain these findings and compare the remaining pending_candidate_refs, then judge source omissions, relationships and boundaries with put_source_review using status=findings. Use inspect_review only when finding details are needed for a correction or an evidence-backed withdrawal. This rejection grants no clean comparison or source approval."}),
        ));
    }
    for source in &check.sources {
        // The exact candidate was authorized by its task/focus above. Original
        // evidence can already have been delivered by a boundary bundle or a
        // previous cross-reference read. Citing it does not expand the current
        // read scope or authorize comparisons of other candidates.
        tools::validate_span(input, &state.reviewer_coverage, source)?;
    }
    let cited: Vec<_> = check.sources.iter().map(|s| s.source_id.clone()).collect();
    if !scope_references(&state.analysis, &cited).contains(&check.reference) {
        return Err("comparison citations must concern the referenced candidate".into());
    }
    let receipt = review_check_key(
        &check.reference,
        &source_review::candidate_version(state, &check.reference)?,
    )?;
    let fresh = !state.reviewer_progress.seen.contains(&receipt);
    let output = json!({"reference":check.reference,"candidate_sha256":version,
        "assessment":"no_issue_in_comparison","summary":check.summary,"sources":check.sources,
        "new_completion":fresh,"completion_sha256":receipt});
    if serde_json::to_vec(&output)
        .map_err(|e| e.to_string())?
        .len()
        > max_bytes
    {
        return Err("clean comparison exceeds result budget; shorten the summary".into());
    }
    // Every check in a multi-tool batch is retained. The last novel local
    // completion resets the turn watch once, at the normal commit boundary.
    state.reviewer_progress.seen.insert(receipt);
    Ok(output)
}

/// A validated focused write closes a local action without pretending the whole
/// source scope is complete. Rewriting an identical version cannot reset it.
pub(super) fn focused_completion(
    state: &Checkpoint,
    name: &str,
    output: &Value,
) -> Result<Option<String>, String> {
    if name == "put_source_review" {
        return Ok((output["new_completion"] == true)
            .then(|| output["completion_sha256"].as_str().map(str::to_owned))
            .flatten());
    }
    let Some(work) = state.work().filter(|w| w.status == WorkStatus::Active) else {
        return Ok(None);
    };
    if name == "complete_review_check" {
        return Ok((output["new_completion"] == true)
            .then(|| output["completion_sha256"].as_str().map(str::to_owned))
            .flatten());
    }
    let Some(id) = output["id"].as_str() else {
        return Ok(None);
    };
    match (&work.focus.action, name) {
        (FocusAction::Link, "put_relation") => {
            let relation = state
                .analysis
                .relations
                .get(id)
                .ok_or("saved relation missing")?;
            if [&relation.from, &relation.to]
                .iter()
                .all(|id| work.focus.references.contains(&format!("record:{id}")))
            {
                return digest(relation).map(Some);
            }
        }
        (FocusAction::Extract, "put_record") => {
            let record = state
                .analysis
                .records
                .get(id)
                .ok_or("saved record missing")?;
            if record.sources.iter().any(|source| {
                work.focus.source_spans.iter().any(|focus| {
                    focus.source_id == source.source_id
                        && focus.grid_cell == source.grid_cell
                        && focus.view_id == source.view_id
                        && focus.start <= source.start
                        && focus.end >= source.end
                })
            }) {
                return digest(record).map(Some);
            }
        }
        (FocusAction::Review, "put_review_finding") => {
            if let Some(finding) = state.review_draft.get(id) {
                return digest(finding).map(Some);
            }
        }
        _ => {}
    }
    Ok(None)
}
