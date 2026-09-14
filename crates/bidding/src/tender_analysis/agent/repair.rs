//! Main-role repair dispositions. These cannot approve an independent review.
use super::*;
use std::collections::BTreeSet;

pub(in crate::tender_analysis) mod tasks;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub feedback_sha256: Option<String>,
    pub baseline: BTreeMap<String, String>,
    pub results: BTreeMap<String, Receipt>,
    #[serde(default)]
    pub tasks: tasks::State,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub conclusion: Conclusion,
    pub summary: String,
    pub sources: Vec<Span>,
    pub candidate_versions: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conclusion {
    Revised,
    Disputed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    finding_sha256: String,
    conclusion: Conclusion,
    summary: String,
    sources: Vec<Span>,
    candidate_refs: Vec<String>,
}

fn feedback_version(state: &Checkpoint) -> Result<String, String> {
    digest(&json!([state.review_rounds, state.findings_for_repair()]))
}

// Capture once at a completed main response, before its tools can edit any
// candidate. A new independent report starts a new repair baseline.
pub(super) fn begin(state: &mut Checkpoint) -> Result<(), String> {
    if state.role != Role::Main || state.findings_for_repair().is_empty() {
        return Ok(());
    }
    let key = feedback_version(state)?;
    if state.repair.feedback_sha256.as_ref() == Some(&key) {
        return Ok(());
    }
    let baseline = state
        .analysis
        .records
        .keys()
        .map(|id| format!("record:{id}"))
        .chain(
            state
                .analysis
                .relations
                .keys()
                .map(|id| format!("relation:{id}")),
        )
        .map(|key| {
            Ok((
                key.clone(),
                digest(&context::reference(&state.analysis, &key)?)?,
            ))
        })
        .collect::<Result<_, String>>()?;
    state.repair = State {
        feedback_sha256: Some(key),
        baseline,
        results: BTreeMap::new(),
        tasks: std::mem::take(&mut state.repair.tasks),
    };
    Ok(())
}

fn candidate_version(state: &Checkpoint, key: &str) -> Result<Option<String>, String> {
    if !key.starts_with("record:") && !key.starts_with("relation:") {
        return Err("repair candidate_refs must name records or relations".into());
    }
    if context::reference(&state.analysis, key).is_ok() {
        // Main repairs depend on their explicit local graph. Independent
        // review keeps its broader global-rule and finding-version contract.
        digest(&json!([
            "main-repair-local-v2",
            state.input_sha256,
            source_review::candidate_reference_values(state, key)?
        ]))
        .map(Some)
    } else if state.repair.baseline.contains_key(key) {
        Ok(None)
    } else {
        Err(format!("unknown repair candidate: {key}"))
    }
}

// Old checkpoints retain their exact stored hashes. Only a currently matching
// legacy digest is accepted; a stale legacy receipt is never silently upgraded.
fn saved_version_matches(state: &Checkpoint, key: &str, saved: &Option<String>) -> bool {
    let Ok(current) = candidate_version(state, key) else {
        return false;
    };
    current == *saved
        || (current.is_some()
            && saved.as_ref().is_some_and(|prior| {
                source_review::candidate_version(state, key).is_ok_and(|legacy| &legacy == prior)
            }))
}

// A changed unrelated record must not satisfy an unchanged reported defect.
// Findings without affected IDs can describe missing candidates; those additions
// must overlap the finding's cited original span. Semantics remain the reviewer's job.
fn relevant_change(state: &Checkpoint, finding: &Finding, key: &str) -> Result<bool, String> {
    let value = context::reference(&state.analysis, key).ok();
    if let Some(value) = &value
        && state.repair.baseline.get(key) == Some(&digest(value)?)
    {
        return Ok(false);
    }
    let Some((kind, id)) = key.split_once(':') else {
        return Ok(false);
    };
    if finding.affected.iter().any(|affected| affected.id == id) {
        return Ok(value.is_some() || state.repair.baseline.contains_key(key));
    }
    if kind == "relation"
        && let Some(relation) = state.analysis.relations.get(id)
        && finding
            .affected
            .iter()
            .any(|affected| affected.id == relation.from || affected.id == relation.to)
    {
        return Ok(true);
    }
    if !finding.affected.is_empty() {
        return Ok(false);
    }
    let sources = match kind {
        "record" => state.analysis.records.get(id).map(|r| &r.sources),
        "relation" => state.analysis.relations.get(id).map(|r| &r.grounds),
        _ => None,
    };
    Ok(sources.is_some_and(|sources| {
        sources.iter().any(|source| {
            finding
                .sources
                .iter()
                .any(|original| source_review::shared_mapping_evidence(source, original))
        })
    }))
}

fn valid(state: &Checkpoint, finding: &Finding, id: &str) -> Result<bool, String> {
    let Some(receipt) = state.repair.results.get(id) else {
        return Ok(false);
    };
    if receipt.summary.trim().is_empty()
        || receipt.sources.is_empty()
        || finding.affected.iter().any(|affected| {
            !receipt
                .candidate_versions
                .keys()
                .any(|key| key.split_once(':').is_some_and(|(_, id)| id == affected.id))
        })
    {
        return Ok(false);
    }
    let mut changed = false;
    for (key, version) in &receipt.candidate_versions {
        changed |= relevant_change(state, finding, key)?;
        if !saved_version_matches(state, key, version) {
            return Ok(false);
        }
    }
    Ok(receipt.conclusion != Conclusion::Revised || changed)
}

// Original finding citations define the correction's reading scope. Aggregated
// candidate provenance may include unrelated sources and is only a fallback
// when the finding and saved correction have no direct source locator.
fn finding_scope(state: &Checkpoint, finding: &Finding, id: &str) -> Vec<String> {
    let mut sources: BTreeSet<_> = finding
        .sources
        .iter()
        .map(|s| s.source_id.clone())
        .collect();
    if !sources.is_empty() {
        return sources.into_iter().collect();
    }
    if let Some(receipt) = state.repair.results.get(id) {
        sources.extend(receipt.sources.iter().map(|s| s.source_id.clone()));
    }
    if !sources.is_empty() {
        return sources.into_iter().collect();
    }
    for affected in &finding.affected {
        if let Some(record) = state.analysis.records.get(&affected.id) {
            sources.extend(record.sources.iter().map(|s| s.source_id.clone()));
        }
        if let Some(relation) = state.analysis.relations.get(&affected.id) {
            sources.extend(relation.grounds.iter().map(|s| s.source_id.clone()));
            for endpoint in [&relation.from, &relation.to] {
                if let Some(record) = state.analysis.records.get(endpoint) {
                    sources.extend(record.sources.iter().map(|s| s.source_id.clone()));
                }
            }
        }
    }
    sources.into_iter().collect()
}

/// Completing overlapping work must not refund an unfinished recovery target.
/// Ordinary local work and independent review keep their own completion rules.
pub(super) fn recovery_completion_gaps(
    state: &Checkpoint,
    scope: &[String],
) -> Result<Vec<Value>, String> {
    if state.role != Role::Main {
        return Ok(Vec::new());
    }
    let targets: BTreeSet<_> = state
        .main_progress
        .blockers
        .iter()
        .filter(|blocker| blocker.scope.iter().any(|id| scope.contains(id)))
        .flat_map(|blocker| &blocker.scope)
        .collect();
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let current = state.repair.feedback_sha256.as_ref() == Some(&feedback_version(state)?);
    let mut seen = BTreeSet::new();
    let mut gaps = Vec::new();
    for (offset, finding) in state.findings_for_repair().into_iter().enumerate() {
        let id = digest(finding)?;
        if !seen.insert(id.clone()) || (current && valid(state, finding, &id)?) {
            continue;
        }
        let finding_sources = finding_scope(state, finding, &id);
        if finding_sources
            .iter()
            .any(|source| targets.contains(source))
        {
            gaps.push(json!({
                "kind":"pending_recovery_repair","field":"source_scope",
                "finding_sha256":id,"source_scope":finding_sources,
                "inspect_review":{"offset":offset,"limit":1},
                "message":format!("previously blocked scope still has an unhandled review finding: {id}; record its grounded repair or dispute before completing overlapping work, or keep the unfinished target deferred with an active scope split")
            }));
        }
    }
    Ok(gaps)
}

pub(super) fn packet(state: &Checkpoint, limits: &Limits) -> Result<Value, String> {
    let tasks = tasks::current(state, limits)?;
    let active_task = state.repair.tasks.active.as_deref();
    let active_work = state
        .main_work
        .as_ref()
        .filter(|work| work.status == WorkStatus::Active);
    let mut outstanding = 0;
    let mut blocked = Vec::new();
    let mut total = BTreeSet::new();
    let mut next = None;
    let current = state.repair.feedback_sha256.as_ref() == Some(&feedback_version(state)?);
    for (offset, finding) in state.findings_for_repair().into_iter().enumerate() {
        let id = digest(finding)?;
        if !total.insert(id.clone()) || (current && valid(state, finding, &id)?) {
            continue;
        }
        outstanding += 1;
        let task = tasks.iter().find(|task| task.finding_sha256 == id);
        let status = if state.repair.results.contains_key(&id) {
            "stale"
        } else {
            "never_handled"
        };
        let scope = finding_scope(state, finding, &id);
        let mut item = json!({"finding_sha256":id,"status":status,"source_scope":scope,
            "inspect_review":{"offset":offset,"limit":1}});
        if let Some(task) = task {
            item["task_id"] = json!(task.id);
            item["committed_turns"] = json!(task.committed_turns);
        }
        if task.is_some_and(|task| task.exhausted)
            || (task.is_some() && repair_task_host::check_scope(state, &scope).is_err())
            || scope.is_empty()
            || context::check_blocked_scope(state, &scope).is_err()
        {
            blocked.push(item);
        } else if task.is_some_and(|task| Some(task.id.as_str()) == active_task) {
            if !active_work.is_some_and(|work| {
                scope
                    .iter()
                    .all(|source| work.source_scope.contains(source))
            }) {
                item["scope_handoff"] = json!({"tool":"set_work_note","source_scope":scope,
                    "instruction":"Declare the evidence scope for this assigned repair task before reading or editing. Retain unfinished sources in deferred_sources. Changing source scope or focus does not select another task or renew its allowance."});
            }
            next = Some(item);
        }
    }
    Ok(
        json!({"total":total.len(),"handled":total.len()-outstanding,"pending":outstanding,
        "blocked_pending":blocked.len(),"next_blocked_finding":blocked.first(),"next_finding":next,
        "instruction":if next.is_some() {
            "next_finding is the host-assigned repair task. Compare its original finding, current candidate details and source evidence, then put_repair_result for that finding; retrieve only missing or evicted details. Follow scope_handoff before reading or editing outside active work. Candidate dependency changes require rechecking, not necessarily another edit or edge. disputed requires a source-backed explanation and independent reconsideration. never_handled means no saved disposition, not that the finding has never been attempted. Switching sources, focus or wording does not renew the task allowance."
        } else if outstanding > 0 && blocked.len() == outstanding {
            "All pending repairs are blocked or exhausted. next_blocked_finding is diagnostic feedback, not permission to retry, clear blockers or claim completion. Pending repairs prevent independent review handoff."
        } else if outstanding > 0 {
            "No unfinished repair task is currently assigned. The host selects the next eligible task at the next real boundary; this projection must not choose one from source priority. Do not switch tasks or claim completion from this packet."
        } else {
            "All repair dispositions are current. This does not grant independent approval or clear legacy execution blockers. Finish the required source work and request independent review."
        }}),
    )
}

// Review tasks depend on relevant main repair statements as well as candidate
// bytes. A new dispute must not be finalized from the old source judgment.
// This is excluded from candidate versions to avoid a receipt depending on itself.
pub(in crate::tender_analysis) fn review_dependencies<'a>(
    repair: &'a State,
    dependencies: &source_review::Dependencies,
    references: &BTreeMap<String, Value>,
) -> BTreeMap<String, &'a Receipt> {
    repair
        .results
        .iter()
        .filter(|(_, receipt)| {
            dependencies.global
                || receipt
                    .sources
                    .iter()
                    .any(|source| dependencies.source_ids.contains(&source.source_id))
                || receipt
                    .candidate_versions
                    .keys()
                    .any(|key| references.contains_key(key))
        })
        .map(|(id, receipt)| (id.clone(), receipt))
        .collect()
}

fn receipt_detail(receipt: &Receipt) -> Value {
    json!({"conclusion":receipt.conclusion,"summary":receipt.summary,
        "sources":receipt.sources,"candidate_refs":receipt.candidate_versions.keys().collect::<Vec<_>>(),
        "independent_approval":false})
}

fn detail(id: &str, finding: &Finding, receipt: Option<&Receipt>) -> Value {
    let mut item = json!({"id":id,"finding":finding});
    if let Some(receipt) = receipt {
        item["main_repair"] = receipt_detail(receipt);
    }
    item
}

fn dependency_navigation(key: &str, reason: &str, available: bool) -> Value {
    let query = key
        .split_once(':')
        .filter(|_| available)
        .map(|(kind, id)| json!({"kind":kind,"view":"detail","ids":[id],"offset":0,"limit":1}));
    json!({"reference":key,"reason":reason,"inspect_analysis":query})
}

fn history(
    receipt: Option<&Receipt>,
    status: &str,
    reason: Option<&str>,
    refs: Vec<Value>,
) -> Value {
    json!({"status":status,"reason":reason,"previous":receipt.map(receipt_detail),
        "invalidated_refs":refs,"current_candidate_read_receipt":false,"independent_approval":false})
}

pub(super) fn main_history(state: &Checkpoint, finding: &Finding) -> Result<Value, String> {
    let id = digest(finding)?;
    let Some(receipt) = state.repair.results.get(&id) else {
        return Ok(json!({"status":"never_handled"}));
    };
    let current_generation =
        state.repair.feedback_sha256.as_ref() == Some(&feedback_version(state)?);
    let mut invalidated = Vec::new();
    for (key, prior) in &receipt.candidate_versions {
        let current = candidate_version(state, key).ok();
        if saved_version_matches(state, key, prior) {
            continue;
        }
        let reason = match (&current, prior) {
            (Some(None), _) => "deleted",
            (None, _) => "unavailable",
            (Some(Some(_)), None) => "restored",
            _ => "dependency_changed",
        };
        invalidated.push(dependency_navigation(
            key,
            reason,
            context::reference(&state.analysis, key).is_ok(),
        ));
    }
    let reason = if !current_generation {
        Some("feedback_generation_changed")
    } else if !invalidated.is_empty() {
        Some("dependency_changed")
    } else if !valid(state, finding, &id)? {
        Some("disposition_no_longer_valid")
    } else {
        None
    };
    Ok(history(
        Some(receipt),
        if reason.is_some() { "stale" } else { "current" },
        reason,
        invalidated,
    ))
}

pub(super) fn check_main_budget(
    finding: &Finding,
    receipt: Option<&Receipt>,
    max_items: usize,
    max_bytes: usize,
) -> Result<(), String> {
    // Reserve complete history even if every tracked dependency later changes.
    // This projection is never displayed or persisted as an actual judgment.
    let reserved = match receipt {
        Some(receipt) => history(
            Some(receipt),
            "stale",
            Some("feedback_generation_changed"),
            receipt
                .candidate_versions
                .keys()
                .map(|key| dependency_navigation(key, "dependency_changed", true))
                .collect(),
        ),
        None => json!({"status":"never_handled"}),
    };
    let envelope = json!({"total":max_items,"next":max_items,"items":[finding],
        "repair_history":{digest(finding)?:reserved}});
    if serde_json::to_vec(&envelope)
        .map_err(|e| e.to_string())?
        .len()
        > max_bytes
    {
        return Err("repair disposition and original finding exceed the main history budget; shorten the explanation or supporting references".into());
    }
    Ok(())
}

pub(super) fn reviewer_item(
    state: &Checkpoint,
    id: &str,
    finding: &Finding,
) -> Result<Value, String> {
    Ok(detail(
        id,
        finding,
        state.repair.results.get(&digest(finding)?),
    ))
}

pub(super) fn put(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    if state.role != Role::Main {
        return Err("only the main Agent can record a repair disposition".into());
    }
    let args = evidence_refs::expand(input, args)?;
    if serde_json::to_vec(&args).map_err(|e| e.to_string())?.len()
        > config.limits.max_tool_result_bytes
    {
        return Err("repair disposition exceeds the frozen tool budget".into());
    }
    let args: Arguments = serde_json::from_value(args).map_err(|e| e.to_string())?;
    let finding = state
        .findings_for_repair()
        .into_iter()
        .find(|finding| digest(finding).ok().as_ref() == Some(&args.finding_sha256))
        .ok_or("use the current finding_sha256 from review_findings.repair.next_finding")?;
    if !state
        .main_progress
        .seen
        .contains(&repair_finding_receipt(finding)?)
    {
        return Err("receive the complete current finding before recording its disposition".into());
    }
    if state.repair.feedback_sha256.as_ref() != Some(&feedback_version(state)?) {
        return Err("repair baseline is not initialized at a completed response boundary".into());
    }
    if args.summary.trim().is_empty() || args.sources.is_empty() {
        return Err("explain the actual correction or disagreement using original evidence".into());
    }
    for source in &args.sources {
        tools::validate_span(input, &state.analysis.coverage, source)?;
    }
    for source in &finding.sources {
        tools::validate_span(input, &state.analysis.coverage, source)?;
    }
    let keys: BTreeSet<_> = args.candidate_refs.iter().cloned().collect();
    if keys.len() != args.candidate_refs.len() {
        return Err("repair candidate_refs must be distinct".into());
    }
    for affected in &finding.affected {
        if !keys
            .iter()
            .any(|key| key.split_once(':').is_some_and(|(_, id)| id == affected.id))
        {
            return Err(format!(
                "include the affected candidate in candidate_refs, even when deleted: {}",
                affected.id
            ));
        }
    }
    let mut versions = BTreeMap::new();
    let mut changed = false;
    for key in &keys {
        let version = candidate_version(state, key)?;
        if version.is_some() {
            let value = context::reference(&state.analysis, key)?;
            let current = digest(&value)?;
            if state.analysis.coverage.candidate.get(key) != Some(&current) {
                return Err(format!(
                    "inspect the current repaired candidate detail before recording its disposition: {key}"
                ));
            }
        }
        changed |= relevant_change(state, finding, key)?;
        versions.insert(key.clone(), version);
    }
    if args.conclusion == Conclusion::Revised && !changed {
        return Err("revised requires an actual relevant candidate addition, edit or deletion since this repair baseline; use disputed only with a source-backed explanation of why the finding needs no further edit".into());
    }
    // Do not borrow an unrelated page to justify either kind of disposition.
    let evidence_sources: BTreeSet<_> = finding
        .sources
        .iter()
        .map(|s| s.source_id.as_str())
        .chain(
            finding
                .affected
                .iter()
                .filter_map(|a| state.analysis.records.get(&a.id))
                .flat_map(|r| r.sources.iter().map(|s| s.source_id.as_str())),
        )
        .collect();
    if !evidence_sources.is_empty()
        && !args
            .sources
            .iter()
            .any(|s| evidence_sources.contains(s.source_id.as_str()))
    {
        return Err(
            "cite original evidence belonging to this finding or its affected candidate".into(),
        );
    }
    let receipt = Receipt {
        conclusion: args.conclusion,
        summary: args.summary,
        sources: args.sources,
        candidate_versions: versions,
    };
    check_main_budget(
        finding,
        Some(&receipt),
        config.limits.max_tool_calls,
        config.limits.max_tool_result_bytes,
    )?;
    // Keep the correction and original finding retrievable together. A valid
    // write cannot strand the reviewer behind an oversized detail envelope.
    for (id, original) in &state.review_draft {
        if digest(original)? == args.finding_sha256 {
            let envelope = json!({"total":state.review_draft.len(),"next":state.review_draft.len(),
                "items":[detail(id,original,Some(&receipt))]});
            if serde_json::to_vec(&envelope)
                .map_err(|e| e.to_string())?
                .len()
                > config.limits.max_tool_result_bytes
            {
                return Err("repair disposition and original finding exceed the detail budget; shorten the explanation or supporting references".into());
            }
        }
    }
    state
        .repair
        .results
        .insert(args.finding_sha256.clone(), receipt);
    Ok(
        json!({"finding_sha256":args.finding_sha256,"disposition_recorded":true,"independent_approval":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_statements_invalidate_only_related_source_judgments() {
        let receipt = |source: &str| Receipt {
            conclusion: Conclusion::Disputed,
            summary: "Source-backed comparison request".into(),
            sources: vec![Span {
                source_id: source.into(),
                start: 0,
                end: 1,
                view_id: None,
                grid_cell: None,
            }],
            candidate_versions: BTreeMap::from([("record:subject".into(), Some("version".into()))]),
        };
        let repair = State {
            results: BTreeMap::from([("issue".into(), receipt("source-a"))]),
            ..Default::default()
        };
        let mut dependencies = source_review::Dependencies::default();
        dependencies.source_ids.insert("source-b".into());
        assert!(review_dependencies(&repair, &dependencies, &BTreeMap::new()).is_empty());
        dependencies.source_ids.insert("source-a".into());
        assert_eq!(
            review_dependencies(&repair, &dependencies, &BTreeMap::new()).len(),
            1
        );
        dependencies.source_ids.clear();
        assert_eq!(
            review_dependencies(
                &repair,
                &dependencies,
                &BTreeMap::from([("record:subject".into(), Value::Null)])
            )
            .len(),
            1
        );
        dependencies.global = true;
        assert_eq!(
            review_dependencies(&repair, &dependencies, &BTreeMap::new()).len(),
            1
        );
    }

    #[test]
    #[ignore = "requires KB_REPAIR_HANDOFF_RUN and KB_REPAIR_HANDOFF_REPORT; offline guard projection only"]
    fn archived_all_read_handoff_still_requires_dispositions() {
        use std::path::PathBuf;
        let root = PathBuf::from(std::env::var("KB_REPAIR_HANDOFF_RUN").unwrap());
        let path = root.join("unhandled-feedback-checkpoint.json");
        let original = std::fs::read(&path).unwrap();
        let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
        let input: FrozenInput =
            serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
                .unwrap();
        let runtime: Config =
            serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
                .unwrap();
        let config = Config::with_provider(runtime.provider, runtime.limits).unwrap();
        let analysis = digest(&state.analysis).unwrap();
        let independent = digest(&state.reviewer_coverage).unwrap();
        let counts = (
            state.turn,
            state.tool_calls,
            state.read_bytes,
            state.review_rounds,
        );
        // Project only the handoff guard onto archived claims. This is neither
        // restoration of the old pending provider request nor a model repair.
        state.role = Role::Main;
        let feedback = super::super::repair_feedback_packet(&state, &[], &config.limits).unwrap();
        assert_eq!(
            feedback["unread"], 0,
            "archive must prove complete feedback delivery"
        );
        begin(&mut state).unwrap();
        let pending = packet(&state, &config.limits).unwrap();
        assert!(pending["pending"].as_u64().unwrap() > 0);
        let rejection =
            super::super::apply(&input, &config, &mut state, "request_review", &json!({}))
                .unwrap_err();
        assert!(
            rejection.starts_with("repair dispositions remain:"),
            "{rejection}"
        );
        assert_eq!(digest(&state.analysis).unwrap(), analysis);
        assert_eq!(digest(&state.reviewer_coverage).unwrap(), independent);
        assert_eq!(
            (
                state.turn,
                state.tool_calls,
                state.read_bytes,
                state.review_rounds
            ),
            counts
        );
        assert_eq!(std::fs::read(path).unwrap(), original);
        let report = PathBuf::from(std::env::var("KB_REPAIR_HANDOFF_REPORT").unwrap());
        assert!(!report.exists(), "preserve prior diagnostic evidence");
        std::fs::write(report,serde_json::to_vec_pretty(&json!({
            "scope":"Offline guard projection onto archived all-read model claims; no provider calls, model repairs, pending-request replay or independent acceptance",
            "feedback":feedback,"dispositions":pending,"rejection":rejection,
            "candidate_analysis_unchanged":true,"reviewer_receipts_unchanged":true,
            "historical_counters_unchanged":true,"original_checkpoint_unchanged":true,
            "full_acceptance":false
        })).unwrap()).unwrap();
    }
}
