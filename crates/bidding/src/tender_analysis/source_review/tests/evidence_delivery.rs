use super::*;
use crate::tender_analysis::agent::evidence_delivery::{confirm, select};

fn last_packet(body: &Value) -> Value {
    serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn prepared_evidence_has_no_receipt_and_received_replay_delivers_exact_values() {
    let (input, config, mut state) = fixture();
    state.reviewer_coverage = Coverage::default();
    let coverage = json!(state.reviewer_coverage);
    let main_coverage = json!(state.analysis.coverage);
    let bytes = request(&input, &config, &mut state).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let sent = last_packet(&body)["preloaded_evidence"].clone();
    assert!(sent["assigned_evidence"].is_object());
    assert_eq!(json!(state.reviewer_coverage), coverage);
    assert_eq!(state.read_bytes, 0);
    assert!(state.pending_coverage.is_none());

    let mut restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    confirm(&input, &config, &mut restored, &body).unwrap();
    assert_eq!(
        restored.read_bytes,
        serde_json::to_vec(&sent).unwrap().len()
    );
    assert_eq!(json!(restored.analysis.coverage), main_coverage);
    assert!(tools::reading_gaps(&input, &restored.reviewer_coverage).is_empty());
    assert!(
        restored
            .reviewer_coverage
            .candidate
            .contains_key("disposition:source")
    );
    assert!(restored.reviewer_coverage.views.is_empty());
    assert_eq!(
        restored.source_review.as_ref().unwrap().results.len(),
        0,
        "delivery must not manufacture a source judgment"
    );

    // Replay from the same received state yields the same effect, not an
    // additional provider call or a receipt granted to the other role.
    confirm(&input, &config, &mut state, &body).unwrap();
    assert_eq!(digest(&state).unwrap(), digest(&restored).unwrap());
}

#[tokio::test]
async fn changed_or_wrong_role_evidence_is_rejected_before_mutation() {
    let (input, config, mut state) = fixture();
    state.reviewer_coverage = Coverage::default();
    let body: Value =
        serde_json::from_slice(&request(&input, &config, &mut state).await.unwrap()).unwrap();
    let mut changed = body.clone();
    let mut packet = last_packet(&changed);
    packet["preloaded_evidence"]["assigned_evidence"]["source"]["text"] = json!("forged source");
    changed["messages"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["content"] = json!(packet.to_string());
    let before = digest(&state).unwrap();
    assert!(confirm(&input, &config, &mut state, &changed).is_err());
    assert_eq!(digest(&state).unwrap(), before);

    state.role = Role::Main;
    let before = digest(&state).unwrap();
    assert!(confirm(&input, &config, &mut state, &body).is_err());
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
async fn omitted_evidence_and_pending_tool_reads_never_gain_preload_credit() {
    let (input, config, mut state) = fixture();
    state.reviewer_coverage = Coverage::default();
    state.pending_coverage = Some(Coverage::default());
    assert!(select(&input, &config, &state).unwrap().is_none());
    let body: Value =
        serde_json::from_slice(&request(&input, &config, &mut state).await.unwrap()).unwrap();
    assert!(last_packet(&body)["preloaded_evidence"].is_null());
    let before = digest(&state).unwrap();
    confirm(&input, &config, &mut state, &body).unwrap();
    assert_eq!(digest(&state).unwrap(), before);

    state.pending_coverage = None;
    state.read_bytes = config.limits.max_read_bytes;
    assert!(select(&input, &config, &state).unwrap().is_none());
}

#[tokio::test]
async fn retained_preload_survives_next_tool_delivery_and_history_compaction_without_recharging() {
    let (input, mut config, mut state) = fixture();
    state.reviewer_coverage = Coverage::default();
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"old","type":"function","function":{"name":"source_index","arguments":"{\"offset\":0,\"limit\":1}"}}]}),
        json!({"role":"tool","tool_call_id":"old","content":json!({"ok":true,"result":{"items":[],"total":0,"next":0}}).to_string()}),
    ];
    let body: Value =
        serde_json::from_slice(&request(&input, &config, &mut state).await.unwrap()).unwrap();
    confirm(&input, &config, &mut state, &body).unwrap();
    let read_bytes = state.read_bytes;
    assert!(
        select(&input, &config, &state).unwrap().is_none(),
        "already visible evidence is not read again"
    );
    state.transcript.extend([
        json!({"role":"assistant","tool_calls":[{"id":"next","type":"function","function":{"name":"source_index","arguments":"{\"offset\":0,\"limit\":1}"}}]}),
        json!({"role":"tool","tool_call_id":"next","content":json!({"ok":true,"result":{"items":[],"total":0,"next":0}}).to_string()}),
    ]);
    state.pending_coverage = Some(state.reviewer_coverage.clone());
    config.limits.max_history_bytes = 1;
    let next: Value =
        serde_json::from_slice(&request(&input, &config, &mut state).await.unwrap()).unwrap();
    let visible = context::visible_work_evidence(&state, next["messages"].as_array().unwrap());
    assert!(
        visible
            .values()
            .any(|ranges| ranges.contains(&(0, input.source_units[0].text.len()))),
        "delivered original must survive with its comparison group"
    );
    assert!(
        !state
            .transcript
            .iter()
            .any(|message| message["tool_call_id"] == "old")
    );
    assert!(
        state
            .transcript
            .iter()
            .any(|message| message["tool_call_id"] == "next")
    );
    assert!(last_packet(&next)["preloaded_evidence"].is_null());
    assert_eq!(state.read_bytes, read_bytes);
}
