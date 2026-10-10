use super::*;
use std::collections::BTreeSet;

/// Explicit model-bound BPE profile accounts for the complete JSON envelope,
/// with separate image allowance and request framing.
/// Base64 is transport encoding, not text sent through the model tokenizer.
pub(super) fn estimate_input_tokens(body: &Value, limits: &Limits) -> Result<usize, AgentError> {
    crate::agent_runtime::chat::estimate_input_tokens(
        body,
        &limits.tokenizer,
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

pub(in crate::analysis) fn reference(analysis: &Analysis, key: &str) -> Result<Value, String> {
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

pub(in crate::analysis) fn scope_references(analysis: &Analysis, scope: &[String]) -> Vec<String> {
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

/// Outcomes are host-maintained, including deferred work and unresolved cross-scope
/// dependencies. They are references only, never reading receipts or approval.
pub(in crate::analysis) fn retain_outcomes(
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
    for work in [&mut state.main_work].into_iter().flatten() {
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

pub(in crate::analysis) fn check_read_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    let source_id = match name {
        "read_source" | "read_source_view" => args["source_id"].as_str(),
        "read_form" | "read_form_cell" => input
            .structured_forms
            .iter()
            .find(|f| f["form_definition_revision_id"] == args["form_id"])
            .and_then(|f| f["source_unit_revision_id"].as_str()),
        _ => return Ok(()),
    }
    .ok_or("reading requires a known source or form identity")?;
    if !input
        .source_units
        .iter()
        .any(|source| source.source_unit_revision_id == source_id)
    {
        return Err("reading requires a known source or form identity".into());
    }
    if state.draft_stage == super::super::draft::DraftStage::Outline {
        return match state.analysis.outline.phase {
            super::super::outline_flow::Phase::Discover
            | super::super::outline_flow::Phase::Outline => Ok(()),
            super::super::outline_flow::Phase::Check if name == "read_source_view" => Ok(()),
            _ => Err("check phase reads must use the active outline packet".into()),
        };
    }
    let work = state
        .work()
        .ok_or("host must assign an active source pack before reading")?;
    // Independent comparison can require another frozen original. Reading it
    // changes neither the assigned task nor candidate/write authorization.
    if work.status != WorkStatus::Active
        || work.source_scope.is_empty()
        || !work.source_scope.iter().any(|id| id == source_id)
    {
        return Err("read lies outside active work; expand the scope for a cross-reference or complete its handoff first".into());
    }
    Ok(())
}

/// Inventory of source payloads actually retained in a request transcript.
/// It is used only for context selection, never to acknowledge reading.
pub(in crate::analysis) fn visible_work_evidence(
    state: &Checkpoint,
    messages: &[Value],
) -> BTreeMap<String, Vec<(usize, usize)>> {
    let scope = state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
        .map(|work| work.source_scope.as_slice())
        .unwrap_or_default();
    let supporting_read = matches!(
        state.analysis.outline.phase,
        super::super::outline_flow::Phase::Discover | super::super::outline_flow::Phase::Outline
    ) && matches!(
        state.draft_stage,
        super::super::draft::DraftStage::Outline
    ) && state
        .work()
        .is_some_and(|work| work.status == WorkStatus::Active);
    let outline_reads: BTreeMap<_, _> = messages
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter_map(|call| {
            let name = call["function"]["name"].as_str()?;
            matches!(
                name,
                "read_requirements" | "read_outline" | "read_evidence" | "read_claim_evidence"
            )
            .then(|| {
                (
                    call["id"].as_str().unwrap_or("").to_owned(),
                    name.to_owned(),
                )
            })
        })
        .collect();
    let mut ranges = BTreeMap::<String, Vec<(usize, usize)>>::new();
    for message in messages {
        if let Some(ids) = message["source_view_refs"].as_array() {
            for id in ids.iter().filter_map(Value::as_str) {
                if state
                    .source_views
                    .get(id)
                    .is_some_and(|view| supporting_read || scope.contains(&view.identity.source_id))
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
        // Outline pages carry the working requirement, target, form or source
        // details. An empty legacy evidence inventory does not make them
        // redundant. Exact payload hashes allow duplicate pages to be released
        // without granting read credit or asserting semantic equivalence.
        if message["role"] == "tool"
            && output["ok"] == true
            && let Some(name) = message["tool_call_id"]
                .as_str()
                .and_then(|id| outline_reads.get(id))
            && let Ok(hash) = digest(&output["result"])
        {
            ranges.insert(format!("outline-page:{name}:{hash}"), vec![(0, 1)]);
        }
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
        if output["ok"] == true
            && result["items"].is_array()
            && let Some(kind) = result["kind"].as_str()
            && ["documents", "document_relations", "decisions"].contains(&kind)
            && let (Some(start), Some(end)) = (result["offset"].as_u64(), result["next"].as_u64())
        {
            tools::cover(
                ranges.entry(format!("metadata:{kind}")).or_default(),
                start as usize,
                end as usize,
            );
        }
        let Some(source_id) = result["source_id"].as_str() else {
            continue;
        };
        if output["ok"] != true || (!supporting_read && !scope.iter().any(|id| id == source_id)) {
            continue;
        }
        let (key, start, end) = if let (Some(form_id), Some(offset)) =
            (result["form_id"].as_str(), result["cell_offset"].as_u64())
        {
            (
                format!("metadata:form-cell:{form_id}:{offset}"),
                result["start"].as_u64(),
                result["end"].as_u64(),
            )
        } else if let Some(form_id) = result["form_id"].as_str() {
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
    let keys = work.focus.references.clone();
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

/// Evict one delivered discovery turn represented by durable pack receipts.
///
/// Failed submissions require an exact saved receipt and retained feedback.
/// Unprocessed, unacknowledged, and identity-invalid groups stay.
pub(in crate::analysis) fn evict_completed_discovery_history(
    state: &mut Checkpoint,
    _history_budget: usize,
) -> bool {
    if state.outline_run.reading_packs.is_some() {
        return evict_committed_pack_turns(state);
    }
    false
}

fn evict_committed_pack_turns(state: &mut Checkpoint) -> bool {
    let Some(work) = state.outline_run.reading_packs.clone() else {
        return false;
    };
    let starts = assistant_group_starts(state);
    for pair in starts.windows(2) {
        let start = if pair[0] == starts[0] { 0 } else { pair[0] };
        let end = pair[1];
        let ids = pack_ids_in_messages(&state.transcript[start..end]);
        if work.turn_only_committed(&ids)
            || retained_pack_submissions(
                &work,
                &state.outline_run.tool_draft.model_wire,
                &state.transcript[start..end],
            )
        {
            state.transcript.drain(start..end);
            return true;
        }
    }
    // A sole/latest successful discovery acknowledgement is fully represented
    // by durable requirements and pack receipts. Keeping its verbose submitted
    // arguments can otherwise deadlock the next context-sized immutable pack.
    // Never drop a pending response, unretained failure, or mixed read/write batch.
    if let Some(&start) = starts.last() {
        let group = &state.transcript[start..];
        let ids = pack_ids_in_messages(group);
        let calls = group
            .iter()
            .filter_map(|message| message["tool_calls"].as_array())
            .flatten()
            .collect::<Vec<_>>();
        let safely_acknowledged = !calls.is_empty()
            && calls.iter().all(|call| {
                call["function"]["name"] == "submit_pack"
                    && call["id"].as_str().is_some_and(|id| {
                        group.iter().any(|message| {
                            message["role"] == "tool"
                                && message["tool_call_id"] == id
                                && message["content"]
                                    .as_str()
                                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                                    .is_some_and(|result| result["ok"] == true)
                        })
                    })
            });
        if (work.turn_only_committed(&ids) && safely_acknowledged)
            || retained_pack_submissions(&work, &state.outline_run.tool_draft.model_wire, group)
        {
            state.transcript.drain(start..);
            return true;
        }
    }
    false
}

/// A failed business validation retains its exact receipt and complete repair
/// feedback in DiscoverWork. Its duplicate transcript can be reconstituted;
/// malformed envelopes and mixed read/write groups must remain intact.
fn retained_pack_submissions(
    work: &crate::outline::discover::DiscoverWork,
    registry: &crate::outline::model_wire::Registry,
    group: &[Value],
) -> bool {
    let calls = group
        .iter()
        .filter_map(|message| message["tool_calls"].as_array())
        .flatten()
        .collect::<Vec<_>>();
    !calls.is_empty()
        && calls.iter().all(|call| {
            call["function"]["name"] == "submit_pack"
                && call["function"]["arguments"]
                    .as_str()
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .and_then(|args| {
                        if args.get("wire_scope").is_some() {
                            registry.decode(args, true).ok()
                        } else {
                            Some(args)
                        }
                    })
                    .is_some_and(|args| work.retains_submission(&args))
                && call["id"].as_str().is_some_and(|id| {
                    group.iter().any(|message| {
                        message["role"] == "tool"
                            && message["tool_call_id"] == id
                            && message["content"]
                                .as_str()
                                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                                .is_some_and(|result| {
                                    result["ok"] == true
                                        || (result["ok"] == false
                                            && result["result"]["ok"] == false)
                                })
                    })
                })
        })
}

fn assistant_group_starts(state: &Checkpoint) -> Vec<usize> {
    state
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "assistant")
        .map(|(index, _)| index)
        .collect()
}

fn pack_ids_in_messages(messages: &[Value]) -> Vec<String> {
    let mut ids = Vec::new();
    for message in messages {
        let Some(calls) = message["tool_calls"].as_array() else {
            continue;
        };
        for call in calls {
            if call["function"]["name"] != "submit_pack" {
                continue;
            }
            let Some(id) = argument_pack_id(&call["function"]["arguments"]) else {
                continue;
            };
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
    }
    ids
}

fn argument_pack_id(value: &Value) -> Option<String> {
    let parsed = if let Some(text) = value.as_str() {
        serde_json::from_str::<Value>(text).ok()?
    } else {
        value.clone()
    };
    parsed
        .get("pack_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub(in crate::analysis) fn evict_delivered_group(
    state: &mut Checkpoint,
    history_budget: usize,
    allow_unique: bool,
) -> bool {
    let starts: Vec<_> = state
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "assistant")
        .map(|(index, _)| index)
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
    let protected: Vec<_> = groups
        .iter()
        .map(|&(start, end)| {
            super::super::outline_flow::protects_scan_repair(state, &state.transcript[start..end])
        })
        .collect();
    let evidence: Vec<_> = groups
        .iter()
        .map(|&(start, end)| visible_work_evidence(state, &state.transcript[start..end]))
        .collect();
    let redundant = (0..groups.len() - 1).find(|&candidate| {
        if protected[candidate] {
            return false;
        }
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
        if protected[candidate] {
            return false;
        }
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
        if protected[candidate] {
            return false;
        }
        let (start, end) = groups[candidate];
        focused_work_evidence(state, &state.transcript[start..end]).is_empty()
    });
    if redundant.or(unfocused).is_none()
        && compact_delivered_candidate_details(state, *starts.last().unwrap(), &[])
    {
        return true;
    }
    let Some(candidate) = redundant
        .or(unfocused)
        .or_else(|| (0..groups.len() - 1).find(|&index| !protected[index]))
    else {
        return false;
    };
    let (start, end) = groups[candidate];
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

/// One-shot progress. Discover reads pack counts and the requirement total.
/// Later phases read `tool_draft`. `scanned` is not discovery progress.
fn one_shot_progress_marker(state: &Checkpoint) -> Result<Option<String>, String> {
    let Some(work) = state.outline_run.reading_packs.as_ref() else {
        return Ok(None);
    };
    if state.analysis.outline.phase == super::super::outline_flow::Phase::Discover {
        let counts = work.pack_counts();
        return Ok(Some(digest(&json!([
            counts.total,
            counts.committed,
            work.requirement_count(),
        ]))?));
    }
    let draft = &state.outline_run.tool_draft;
    Ok(Some(digest(&json!({
        "chapters":draft.chapters,"bindings":draft.bindings,"slots":draft.slots,
        "slots_submitted":draft.slots_submitted,"fulfillments":draft.fulfillments,
        "reviewed_requirements":draft.reviewed_requirement_ids,
        "reviewed_packs":draft.reviewed_pack_ids,"reviewed_pack_evidence":draft.reviewed_pack_evidence,
        "review_issues":draft.review_issues,"claim_comparisons":draft.claim_comparisons,"finished":draft.finished
    }))?))
}

pub(in crate::analysis) fn observe_progress(
    state: &mut Checkpoint,
    role: &Role,
    local_completion: Option<String>,
    limits: &Limits,
) -> Result<(), String> {
    if *role == Role::Main
        && state.draft_stage == super::super::draft::DraftStage::Outline
        && let Some(marker) = one_shot_progress_marker(state)?
    {
        let mut versions = vec![marker.clone()];
        // Delivered reading advances work, never semantic completion. Discovery
        // workers already account for their assigned packets at pack commit.
        if state.analysis.outline.phase != super::super::outline_flow::Phase::Discover {
            versions.push(delivered_read_progress_marker(state)?);
        }
        state
            .main_progress
            .observe(versions, Some(marker), &limits.progress());
        return Ok(());
    }
    if *role == Role::Main
        && state.draft_stage == super::super::draft::DraftStage::Outline
        && state.analysis.outline.phase == super::super::outline_flow::Phase::Discover
    {
        let flow = &state.analysis.outline;
        let evidence = digest(&state.analysis.coverage)?;
        // Only new inspected ranges complete discovery work. Requirement edits
        // cannot reset the scan watchdog while the cursor remains unaccounted.
        let scanned = digest(&flow.scanned)?;
        state.main_progress.observe(
            [evidence, scanned.clone()],
            Some(scanned),
            &limits.progress(),
        );
        return Ok(());
    }
    if *role == Role::Main && state.draft_stage == super::super::draft::DraftStage::Outline {
        let flow = &state.analysis.outline;
        // Completion measures covered obligations, not node identity or array
        // order. Cosmetic edits still count as work, but cannot reset focus.
        let organized: BTreeSet<_> = state
            .analysis
            .draft_plan
            .iter()
            .filter(|node| node.status != super::super::draft::DraftStatus::Omitted)
            .flat_map(|node| node.requirement_ids.iter().cloned())
            .collect();
        let formats: BTreeSet<_> = state
            .analysis
            .draft_plan
            .iter()
            .filter(|node| node.status != super::super::draft::DraftStatus::Omitted)
            .flat_map(|node| {
                node.requirement_ids.iter().flat_map(|id| {
                    node.format_refs
                        .iter()
                        .map(move |span| (id.clone(), serde_json::to_string(span).unwrap()))
                })
            })
            .collect();
        let resolved: BTreeSet<_> = flow
            .references
            .iter()
            .filter(|(_, reference)| {
                reference.status == super::super::outline_flow::ReferenceStatus::Resolved
            })
            .map(|(id, reference)| {
                (
                    id.clone(),
                    reference
                        .resolution_grounds
                        .iter()
                        .map(|span| serde_json::to_string(span).unwrap())
                        .collect::<BTreeSet<_>>(),
                )
            })
            .collect();
        let checks: BTreeSet<_> = flow
            .checks
            .iter()
            .filter(|(_, packet)| packet.status == "pass")
            .map(|(id, packet)| (id.clone(), packet.snapshot_sha256.clone()))
            .collect();
        let completed = digest(&json!([organized, formats, resolved, checks]))?;
        state.main_progress.observe(
            [
                digest(&state.analysis.coverage)?,
                digest(&state.analysis.draft_plan)?,
            ],
            Some(completed),
            &limits.progress(),
        );
        return Ok(());
    }
    let _ = local_completion;
    Ok(())
}

fn merged_read_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.retain(|(start, end)| start < end);
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

/// Exact-wire receipt confirmation supplies these sets; pending/expired frames,
/// cursor movement and wire identities confer no credit. Canonical unions keep
/// repeated, reordered and overlapping reads from inventing work.
fn delivered_read_progress_marker(state: &Checkpoint) -> Result<String, String> {
    let draft = &state.outline_run.tool_draft;
    let mut carriers = std::collections::BTreeMap::<String, Vec<(usize, usize)>>::new();
    for (scope, reference) in draft
        .delivered_evidence
        .iter()
        .map(|r| ("outline", r))
        .chain(draft.check_reads.evidence.iter().map(|r| ("check", r)))
    {
        let mut carrier = serde_json::to_value(reference).map_err(|e| e.to_string())?;
        let object = carrier
            .as_object_mut()
            .ok_or("evidence carrier must be an object")?;
        object.remove("start_byte");
        object.remove("end_byte");
        let ranges = carriers.entry(format!("{scope}:{carrier}")).or_default();
        if let Some(range) = reference.range() {
            ranges.push(range);
        }
    }
    for ranges in carriers.values_mut() {
        *ranges = merged_read_ranges(std::mem::take(ranges));
    }
    let slots: std::collections::BTreeMap<_, _> = draft
        .check_reads
        .slot_ranges
        .iter()
        .map(|(id, ranges)| (id, merged_read_ranges(ranges.clone())))
        .collect();
    digest(&json!({"delivered_reading":carriers,"slots":slots,
        "structures":draft.check_reads.structure_keys,
        "empty_packs":draft.delivered_empty_pack_ids,
        "check_empty_packs":draft.check_reads.empty_pack_ids}))
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

        _ => {}
    }
    Ok(None)
}

/// Only suppress ranges present in the request the model actually answered.
/// Historical coverage and outputs from this response are not visibility.
pub(in crate::analysis) fn visible_read_receipt(
    state: &Checkpoint,
    delivered: &BTreeMap<String, Vec<(usize, usize)>>,
    name: &str,
    result: Value,
) -> Value {
    if !matches!(name, "read_source" | "read_form" | "collection_index") {
        return result;
    }
    let message = json!({"role":"tool","content":json!({"ok":true,"result":result}).to_string()});
    let requested = visible_work_evidence(state, &[message]);
    if requested.is_empty()
        || !requested.iter().all(|(key, ranges)| {
            ranges
                .iter()
                .all(|&(start, end)| end > start && tools::contains(delivered.get(key), start, end))
        })
    {
        return result;
    }
    json!({"already_visible":true,"ranges":requested,
        "instruction":"Original evidence is already in the delivered request. Reading receipts are not new conclusions. If evidence is evicted later, reread the required range."})
}
