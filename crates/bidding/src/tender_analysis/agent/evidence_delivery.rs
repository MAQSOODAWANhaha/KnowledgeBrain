//! Deliver already assigned evidence through the reserved request itself.
//! Preparing a request grants no receipt and changes no business state. The
//! received boundary verifies the exact saved payload before promoting reads.
use super::*;

fn message(content: &Value) -> Value {
    json!({"role":"user","content":json!({"preloaded_evidence":content}).to_string()})
}

pub(super) fn is_retained_message(message: &Value) -> bool {
    message["role"] == "user"
        && message["content"]
            .as_str()
            .and_then(|content| serde_json::from_str::<Value>(content).ok())
            .is_some_and(|content| content["preloaded_evidence"]["assigned_evidence"].is_object())
}

pub(in crate::tender_analysis) fn select(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<Option<source_review::Evidence>, AgentError> {
    if state.pending_coverage.is_some()
        || (state.execution().watch.recovery == Recovery::Blocked && !config.limits.draft_path)
        || (state.role == Role::Reviewer
            && state
                .work()
                .is_none_or(|work| work.status != WorkStatus::Active))
    {
        return Ok(None);
    }
    let evidence = if state.role == Role::Main {
        main_work::evidence(input, config, state).map_err(invalid)?
    } else {
        let assigned_source =
            source_review::assigned_source(input, state, config.limits.max_tool_result_bytes)
                .map_err(invalid)?;
        if !assigned_source.is_some_and(|source| {
            state
                .work()
                .is_some_and(|work| work.source_scope.contains(&source))
        }) {
            // A legal cross-reference scope may temporarily defer the assigned
            // source. Optional preloading must not prevent its next tool request.
            return Ok(None);
        }
        source_review::evidence(input, config, state).map_err(invalid)?
    };
    let Some(evidence) = evidence else {
        return Ok(None);
    };
    let expected = context::visible_work_evidence(state, &[message(&evidence.content)]);
    let retained = context::visible_work_evidence(state, &state.transcript);
    // Visible source/candidate projections deliberately do not interpret
    // collection metadata. Retained candidates must not suppress a new page
    // of documents, decisions or document relationships in the same packet.
    let new_metadata = evidence.coverage.metadata.iter().any(|(kind, ranges)| {
        ranges
            .iter()
            .any(|&(start, end)| !tools::contains(state.coverage().metadata.get(kind), start, end))
    });
    let draft_fill = config.limits.draft_path
        && state.draft_stage == crate::tender_analysis::draft::DraftStage::Fill;
    if !draft_fill
        && !new_metadata
        && !expected.is_empty()
        && expected.iter().all(|(key, spans)| {
            spans.iter().all(|(start, end)| {
                retained
                    .get(key)
                    .is_some_and(|ranges| ranges.iter().any(|(a, b)| a <= start && b >= end))
            })
        })
    {
        return Ok(None);
    }
    let bytes = serde_json::to_vec(&evidence.content)
        .map_err(invalid)?
        .len();
    if state
        .read_bytes
        .checked_add(bytes)
        .is_none_or(|total| total > config.limits.max_read_bytes)
    {
        return Ok(None);
    }
    Ok(Some(evidence))
}

pub(in crate::tender_analysis) fn confirm(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    body: &Value,
) -> Result<(), AgentError> {
    let packet = body["messages"]
        .as_array()
        .and_then(|messages| messages.last())
        .and_then(|message| message["content"].as_str())
        .and_then(|content| serde_json::from_str::<Value>(content).ok());
    let Some(sent) = packet
        .as_ref()
        .and_then(|packet| packet.get("preloaded_evidence"))
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    let evidence = select(input, config, state)?
        .filter(|evidence| &evidence.content == sent)
        .ok_or_else(|| invalid("reserved evidence differs from the assigned frozen evidence"))?;
    let bytes = serde_json::to_vec(sent).map_err(invalid)?.len();
    main_work::confirm_work(input, config, state, sent).map_err(invalid)?;
    // Selection checked the remaining budget. No mutation precedes validation;
    // received replay starts from the same state and recomputes the same reads.
    state.read_bytes = state
        .read_bytes
        .checked_add(bytes)
        .ok_or_else(|| invalid("read budget overflow"))?;
    state.replace_coverage(evidence.coverage);
    // This is the actual delivered request evidence, not a fabricated tool
    // result. Retain it so subsequent reads and sizing preserve the grounds
    // for comparison even when the next request has pending tool receipts.
    state.transcript.push(message(sent));
    Ok(())
}
