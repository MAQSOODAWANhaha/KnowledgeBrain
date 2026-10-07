//! A single bounded retry after newly delivered prior repair history.
//! Eligibility is informational until an explicit scope handoff consumes it.
use super::*;
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

pub(in crate::analysis) fn available(state: &Checkpoint, scope: &[String]) -> Result<bool, String> {
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

pub(in crate::analysis) fn dependencies(
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
pub(in crate::analysis) fn delivered(
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
