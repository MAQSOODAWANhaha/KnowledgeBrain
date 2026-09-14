//! A single bounded retry after newly delivered prior repair history.
//! Eligibility is informational until an explicit scope handoff consumes it.
use super::*;
use crate::agent_runtime::progress::ProgressWatch;
use std::collections::BTreeSet;

const POLICY: &str = "main-repair-history-recovery-v1";

fn key(state: &Checkpoint, scope: &[String]) -> Result<String, String> {
    let mut exact = scope.to_vec();
    exact.sort();
    exact.dedup();
    digest(&json!([
        POLICY,
        state.role,
        exact,
        context::raw_scope_dependencies(state, scope)?
    ]))
}

fn marker(kind: &str, key: &str) -> String {
    format!("{POLICY}:{kind}:{key}")
}

fn consumed(state: &Checkpoint, scope: &[String]) -> Result<bool, String> {
    Ok(state
        .main_progress
        .seen
        .contains(&marker("consumed", &key(state, scope)?)))
}

pub(in crate::tender_analysis) fn available(
    state: &Checkpoint,
    scope: &[String],
) -> Result<bool, String> {
    let key = key(state, scope)?;
    Ok(state.role == Role::Main
        && state.main_progress.seen.contains(&marker("eligible", &key))
        && !consumed(state, scope)?)
}

fn unknown_history_sources(
    state: &Checkpoint,
    finding: &Finding,
    id: &str,
) -> Result<BTreeMap<String, String>, String> {
    let Some(receipt) = state.repair.results.get(id) else {
        return Ok(BTreeMap::new());
    };
    let sources: BTreeSet<_> = finding
        .sources
        .iter()
        .chain(&receipt.sources)
        .map(|span| span.source_id.clone())
        .collect();
    let mut unknown = BTreeMap::new();
    for source in sources {
        let known = marker("known", &digest(&json!([state.role, id, source]))?);
        if !state.main_progress.seen.contains(&known) {
            unknown.insert(source, known);
        }
    }
    Ok(unknown)
}

/// A query suggestion only. The full original finding and history must still
/// be delivered through the existing completed-response path to earn a retry.
pub(super) fn history_query(state: &Checkpoint, scope: &[String]) -> Result<Option<Value>, String> {
    if state.role != Role::Main || available(state, scope)? || consumed(state, scope)? {
        return Ok(None);
    }
    for (offset, finding) in state.findings_for_repair().into_iter().enumerate() {
        let id = digest(finding)?;
        if unknown_history_sources(state, finding, &id)?
            .keys()
            .any(|source| scope.contains(source))
        {
            return Ok(Some(
                json!({"finding_sha256":id,"inspect_review":{"offset":offset,"limit":1},
                "instruction":"Saved repair history for this blocked scope has not been delivered. Retrieve this complete original finding and history if needed to reconsider the blockage. Only actual complete delivery can earn the existing one-time history retry; this query grants no reading, scope access, renewed budget or approval."}),
            ));
        }
    }
    Ok(None)
}

pub(in crate::tender_analysis) fn dependencies(
    state: &Checkpoint,
    scope: &[String],
    raw: String,
) -> Result<String, String> {
    let key = key(state, scope)?;
    if state.execution().seen.contains(&marker("consumed", &key)) {
        digest(&json!([raw, marker("consumed", &key)]))
    } else {
        Ok(raw)
    }
}

/// Called only after the persisted request has a complete model response.
/// Full current Finding and full saved history must be present in its tool result.
pub(in crate::tender_analysis) fn delivered(
    state: &Checkpoint,
    messages: &[Value],
) -> Result<BTreeSet<String>, String> {
    let mut received = BTreeSet::new();
    if state.role != Role::Main {
        return Ok(received);
    }
    let queries: BTreeSet<_> = messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter(|c| c["function"]["name"] == "inspect_review")
        .filter_map(|c| c["id"].as_str())
        .collect();
    for message in messages {
        if message["role"] != "tool"
            || !message["tool_call_id"]
                .as_str()
                .is_some_and(|id| queries.contains(id))
        {
            continue;
        }
        let Some(content) = message["content"]
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
        else {
            continue;
        };
        if content["ok"] != true {
            continue;
        }
        for finding in state.findings_for_repair() {
            let id = digest(finding)?;
            if !state.repair.results.contains_key(&id) {
                continue;
            }
            if !content["result"]["items"]
                .as_array()
                .is_some_and(|items| items.contains(&json!(finding)))
                || content["result"]["repair_history"][&id] != repair::main_history(state, finding)?
            {
                continue;
            }
            // Remember actual delivery even before a source ever blocks. An old
            // retained result must not become new information after stalling.
            // The same original finding's history is known for this source even
            // if its disposition is rewritten later. Source IDs cover future
            // combined scopes; business changes retain their own retry path.
            let newly_delivered = unknown_history_sources(state, finding, &id)?;
            received.extend(newly_delivered.values().cloned());
            for blocker in &state.main_progress.blockers {
                if blocker
                    .scope
                    .iter()
                    .any(|source| newly_delivered.contains_key(source))
                {
                    received.insert(marker("eligible", &key(state, &blocker.scope)?));
                }
            }
        }
    }
    Ok(received)
}

// Active attempts keep their spent watch in the existing blocker. Batch
// observation updates it; failed attempts also replace the dependency hash.
pub(in crate::tender_analysis) fn enter(
    state: &mut Checkpoint,
    scope: &[String],
) -> Result<bool, String> {
    let blocked = state.execution().watch.recovery == Recovery::Blocked;
    let targets: Vec<_> = state
        .execution()
        .blockers
        .iter()
        .filter(|b| b.scope.iter().any(|id| scope.contains(id)))
        .cloned()
        .collect();
    if targets.is_empty() {
        if blocked {
            let progress = if state.role == Role::Main {
                &mut state.main_progress
            } else {
                &mut state.reviewer_progress
            };
            progress.resume(None);
        }
        return Ok(blocked);
    }
    let mut prior = ProgressWatch::default();
    let mut consumed = Vec::new();
    for target in &targets {
        prior.replans = prior.replans.max(target.watch.replans);
        if target.watch.recovery != Recovery::Blocked {
            prior.no_progress_turns = prior.no_progress_turns.max(target.watch.no_progress_turns);
            prior.focus_turns = prior.focus_turns.max(target.watch.focus_turns);
            if target.watch.recovery == Recovery::Replan {
                prior.recovery = Recovery::Replan;
            }
        }
        if available(state, &target.scope)? {
            consumed.push(marker("consumed", &key(state, &target.scope)?));
        }
    }
    let progress = if state.role == Role::Main {
        &mut state.main_progress
    } else {
        &mut state.reviewer_progress
    };
    // Continuing within an active attempt also keeps the current batch's spent watch.
    if targets
        .iter()
        .any(|t| t.watch.recovery != Recovery::Blocked)
        && !blocked
    {
        prior.no_progress_turns = prior
            .no_progress_turns
            .max(progress.watch.no_progress_turns);
        prior.focus_turns = prior.focus_turns.max(progress.watch.focus_turns);
        prior.replans = prior.replans.max(progress.watch.replans);
    }
    progress.seen.extend(consumed);
    progress.watch = prior;
    for blocker in &mut progress.blockers {
        if targets.iter().any(|target| target.scope == blocker.scope) {
            blocker.watch = progress.watch.clone();
        }
    }
    Ok(blocked
        || targets
            .iter()
            .any(|target| target.watch.recovery == Recovery::Blocked))
}
