//! Final-file receipts become usable only after their exact tool bodies reach
//! the reviewer. Keep the same prepared/received/committed Journal boundaries.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingDelivery {
    pub tender_coverage: Coverage,
    pub output_coverage: OutputCoverage,
    pub messages: BTreeMap<String, String>,
    pub view_refs: Vec<visual::ViewRef>,
}

pub(super) fn accept_pending(
    state: &mut Checkpoint,
    source_views: &BTreeMap<String, crate::tender_analysis::views::SourceView>,
) -> Result<(), String> {
    let Some(pending) = &state.pending_delivery else {
        return Ok(());
    };
    if state
        .journal
        .pending
        .as_ref()
        .is_none_or(|turn| turn.role != "reviewer" || turn.response.is_none())
    {
        return Err("export-review evidence needs a received reviewer request".into());
    }
    let body: Value = serde_json::from_slice(state.journal.body().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let messages = body["messages"]
        .as_array()
        .ok_or("frozen messages missing")?;
    if pending.messages.is_empty()
        || pending.messages.iter().any(|(id, expected)| {
            !messages.iter().any(|message| {
                message["role"] == "tool"
                    && message["tool_call_id"] == *id
                    && message["content"]
                        .as_str()
                        .is_some_and(|content| digest(&content).ok().as_ref() == Some(expected))
            })
        })
    {
        return Err("pending export evidence is absent from the frozen request".into());
    }
    for view in &pending.view_refs {
        if !visual::was_delivered(state, source_views, view, messages)? {
            return Err(
                "pending export/original pixels were not delivered in the frozen request".into(),
            );
        }
    }
    let pending = state.pending_delivery.take().expect("validated delivery");
    state.tender_coverage = pending.tender_coverage;
    state.output_coverage = pending.output_coverage;
    Ok(())
}

/// Evict complete oldest protocol groups, preserving the latest batch and any
/// undelivered tool bodies. Previously delivered coverage stays in the journal.
pub(super) fn evict_history(state: &mut Checkpoint) -> bool {
    let Some(end) = state
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "assistant")
        .nth(1)
        .map(|(index, _)| index)
    else {
        return false;
    };
    if state.pending_delivery.as_ref().is_some_and(|pending| {
        state.transcript[..end].iter().any(|message| {
            message["tool_call_id"]
                .as_str()
                .is_some_and(|id| pending.messages.contains_key(id))
        })
    }) {
        return false;
    }
    state.transcript.drain(..end);
    true
}
