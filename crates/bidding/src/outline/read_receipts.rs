//! Read credit belongs to exact tool bytes included in a completed request.
//! Queueing a tool result, advancing a turn, or possessing cached data is not delivery.
use super::{evidence::EvidenceRef, tools::Draft};
use crate::analysis::agent::Checkpoint;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub call_id: String,
    pub revision: u64,
    pub epoch: u64,
    pub check: bool,
    pub sources: Vec<EvidenceRef>,
    pub structures: Vec<String>,
    pub empty_packs: Vec<String>,
    pub slots: Vec<(String, usize, usize)>,
    pub metadata: Vec<(String, Vec<super::metadata_fragments::Field>)>,
    pub sealed_sha256: Option<String>,
}
/// Phase handoffs retire old history, but cannot discard an unread current
/// result. This changes visibility only; exact-wire confirmation remains the
/// sole authority for credit, scoped to its revision and Check epoch.
pub(crate) fn handoff_history(state: &mut Checkpoint) {
    let revision = state
        .outline_run
        .reading_packs
        .as_ref()
        .map(|w| w.revision)
        .unwrap_or(0);
    retain_current_undelivered(
        &mut state.transcript,
        &state.outline_run.tool_draft.read_frames,
        revision,
        state.outline_run.tool_draft.read_epoch,
    );
}
fn retain_current_undelivered(
    transcript: &mut Vec<Value>,
    frames: &[Frame],
    revision: u64,
    epoch: u64,
) {
    let Some(latest) = transcript.iter().rposition(|m| m["role"] == "assistant") else {
        transcript.clear();
        return;
    };
    let pending = transcript[latest..]
        .iter()
        .filter_map(|m| m["tool_call_id"].as_str())
        .any(|id| {
            frames.iter().any(|f| {
                f.call_id == id && f.revision == revision && (!f.check || f.epoch == epoch)
            })
        });
    if pending {
        transcript.drain(..latest);
    } else {
        transcript.clear();
    }
}

fn delta<T: Clone + PartialEq>(after: &[T], before: &[T]) -> Vec<T> {
    after
        .iter()
        .filter(|v| !before.contains(v))
        .cloned()
        .collect()
}
pub(crate) fn queue(state: &mut Checkpoint, before: &Draft, call_id: &str, check: bool) {
    let draft = &mut state.outline_run.tool_draft;
    let sources = if check {
        delta(
            &draft.check_reads.pending_evidence,
            &before.check_reads.pending_evidence,
        )
    } else {
        delta(&draft.pending_evidence, &before.pending_evidence)
    }
    .into_iter()
    .map(|(_, r)| r)
    .collect::<Vec<_>>();
    let structures = delta(
        &draft.check_reads.pending_structure_keys,
        &before.check_reads.pending_structure_keys,
    )
    .into_iter()
    .map(|(_, k)| k)
    .collect::<Vec<_>>();
    let slots = delta(
        &draft.check_reads.pending_slot_ranges,
        &before.check_reads.pending_slot_ranges,
    )
    .into_iter()
    .map(|(_, id, a, b)| (id, a, b))
    .collect::<Vec<_>>();
    let empty_packs = delta(
        &draft.pending_empty_pack_ids,
        &before.pending_empty_pack_ids,
    )
    .into_iter()
    .map(|(_, id)| id)
    .collect::<Vec<_>>();
    let metadata = delta(
        &draft.pending_metadata_parts,
        &before.pending_metadata_parts,
    )
    .into_iter()
    .map(|(_, key, parts)| (key, parts))
    .collect::<Vec<_>>();
    if sources.is_empty()
        && structures.is_empty()
        && slots.is_empty()
        && empty_packs.is_empty()
        && metadata.is_empty()
    {
        return;
    }
    draft.read_frames.push(Frame {
        call_id: call_id.into(),
        revision: state
            .outline_run
            .reading_packs
            .as_ref()
            .map(|w| w.revision)
            .unwrap_or(0),
        epoch: draft.read_epoch,
        check,
        sources,
        structures,
        empty_packs,
        slots,
        metadata,
        sealed_sha256: None,
    });
}
pub(crate) fn wire_hash(body: &Value, call_id: &str) -> Result<Option<String>, String> {
    let found = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "tool" && m["tool_call_id"] == call_id)
        .collect::<Vec<_>>();
    if found.len() > 1 {
        return Err("duplicate tool result identity in request".into());
    }
    found.first().map(super::canonical_sha256).transpose()
}
pub(crate) fn seal(state: &mut Checkpoint, body: &Value) -> Result<(), String> {
    for frame in &mut state.outline_run.tool_draft.read_frames {
        frame.sealed_sha256 = wire_hash(body, &frame.call_id)?;
    }
    Ok(())
}
pub(crate) fn confirm(state: &mut Checkpoint, body: &Value) -> Result<(), String> {
    let revision = state
        .outline_run
        .reading_packs
        .as_ref()
        .map(|w| w.revision)
        .unwrap_or(0);
    let mut delivered = Vec::new();
    let mut pending = Vec::new();
    for frame in &state.outline_run.tool_draft.read_frames {
        if frame.revision != revision
            || (frame.check && frame.epoch != state.outline_run.tool_draft.read_epoch)
        {
            continue;
        }
        if frame.sealed_sha256.is_some() && wire_hash(body, &frame.call_id)? == frame.sealed_sha256
        {
            delivered.push(frame.clone())
        } else {
            pending.push(frame.clone())
        }
    }
    let draft = &mut state.outline_run.tool_draft;
    draft.read_frames = pending;
    for frame in delivered {
        for (key, parts) in &frame.metadata {
            let record = draft
                .metadata_records
                .get_mut(key)
                .ok_or("delivered metadata record missing")?;
            if record.receive(parts)? && frame.check {
                let restored = record.restored()?;
                if let Some(structure) = restored["structural_receipt_id"].as_str() {
                    draft.check_reads.structure_keys.insert(structure.into());
                }
                if restored["empty_carrier"] == true
                    && let Some(id) = restored["pack_id"].as_str()
                {
                    draft.check_reads.empty_pack_ids.insert(id.into());
                }
            }
        }
        draft
            .pending_metadata_parts
            .retain(|(_, key, parts)| !frame.metadata.contains(&(key.clone(), parts.clone())));

        for reference in &frame.sources {
            if !draft.delivered_evidence.contains(reference) {
                draft.delivered_evidence.push(reference.clone())
            }
            if frame.check && !draft.check_reads.evidence.contains(reference) {
                draft.check_reads.evidence.push(reference.clone())
            }
        }
        draft
            .pending_evidence
            .retain(|(_, r)| !frame.sources.contains(r));
        draft
            .check_reads
            .pending_evidence
            .retain(|(_, r)| !frame.sources.contains(r));
        draft
            .delivered_empty_pack_ids
            .extend(frame.empty_packs.iter().cloned());
        draft
            .pending_empty_pack_ids
            .retain(|(_, id)| !frame.empty_packs.contains(id));
        if frame.check {
            draft
                .check_reads
                .empty_pack_ids
                .extend(frame.empty_packs.iter().cloned());
            draft
                .check_reads
                .structure_keys
                .extend(frame.structures.iter().cloned());
            for (id, a, b) in &frame.slots {
                draft
                    .check_reads
                    .slot_ranges
                    .entry(id.clone())
                    .or_default()
                    .push((*a, *b));
            }
            draft
                .check_reads
                .pending_structure_keys
                .retain(|(_, k)| !frame.structures.contains(k));
            draft
                .check_reads
                .pending_slot_ranges
                .retain(|(_, id, a, b)| !frame.slots.contains(&(id.clone(), *a, *b)));
            draft
                .check_reads
                .pending_empty_pack_ids
                .retain(|(_, id)| !frame.empty_packs.contains(id));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn handoff_retains_only_current_undelivered_group_and_never_grants_credit() {
        let frame = Frame {
            call_id: "current".into(),
            revision: 7,
            epoch: 3,
            check: true,
            sources: vec![],
            structures: vec!["s".into()],
            empty_packs: vec![],
            slots: vec![],
            metadata: vec![],
            sealed_sha256: None,
        };
        let history = vec![
            json!({"role":"assistant"}),
            json!({"role":"tool","tool_call_id":"old"}),
            json!({"role":"assistant"}),
            json!({"role":"tool","tool_call_id":"current"}),
        ];
        let mut current = history.clone();
        retain_current_undelivered(&mut current, std::slice::from_ref(&frame), 7, 3);
        assert_eq!(current, history[2..]);
        assert!(frame.sealed_sha256.is_none());
        for (revision, epoch) in [(8, 3), (7, 4)] {
            let mut stale = history.clone();
            retain_current_undelivered(&mut stale, std::slice::from_ref(&frame), revision, epoch);
            assert!(stale.is_empty());
        }
        let mut delivered = history.clone();
        retain_current_undelivered(&mut delivered, &[], 7, 3);
        assert!(delivered.is_empty());
        let mut earlier = frame;
        earlier.call_id = "old".into();
        let mut old = history.clone();
        retain_current_undelivered(&mut old, &[earlier], 7, 3);
        assert!(old.is_empty());
    }
    #[test]
    #[ignore = "offline private actual-request receipt boundaries; requires KB_PRIVATE_PROJECTION_DIR"]
    fn private_receipts_require_exact_completed_wire_and_survive_replay() {
        let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap())
            .join("production-claim-cycle-v2");
        let mut state: Checkpoint =
            serde_json::from_slice(&std::fs::read(root.join("seeded-checkpoint.json")).unwrap())
                .unwrap();
        let input: crate::analysis::FrozenInput =
            serde_json::from_slice(&std::fs::read(root.join("frozen-scoped-input.json")).unwrap())
                .unwrap();
        let reference = state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records()["pack-0:0"]
            .evidence[0]
            .clone();
        let before = state.outline_run.tool_draft.clone();
        state
            .outline_run
            .tool_draft
            .check_reads
            .pending_evidence
            .push((state.turn, reference.clone()));
        queue(&mut state, &before, "read-1", true);
        let body = json!({"messages":[{"role":"tool","tool_call_id":"read-1","content":json!({"ok":true,"result":{"excerpts":super::super::evidence::resolve_evidence(&input,std::slice::from_ref(&reference)).unwrap()}}).to_string()}]});
        confirm(&mut state, &body).unwrap();
        assert!(
            state.outline_run.tool_draft.check_reads.evidence.is_empty(),
            "unsealed result is not delivery"
        );
        state.turn += 1;
        assert!(
            state.outline_run.tool_draft.check_reads.evidence.is_empty(),
            "turn advance grants nothing"
        );
        seal(&mut state, &body).unwrap();
        let sealed = state.clone();
        confirm(&mut state, &json!({"messages":[]})).unwrap();
        assert!(
            state.outline_run.tool_draft.check_reads.evidence.is_empty(),
            "evicted result grants nothing"
        );
        let mut tampered = body.clone();
        tampered["messages"][0]["content"] = json!("{\"ok\":false}");
        confirm(&mut state, &tampered).unwrap();
        assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
        let mut changed = sealed.clone();
        changed.outline_run.reading_packs.as_mut().unwrap().revision += 1;
        confirm(&mut changed, &body).unwrap();
        assert!(
            changed
                .outline_run
                .tool_draft
                .check_reads
                .evidence
                .is_empty(),
            "old revision is invalid"
        );
        let mut rolled_back = sealed.clone();
        rolled_back.outline_run.tool_draft = before;
        confirm(&mut rolled_back, &body).unwrap();
        assert!(
            rolled_back
                .outline_run
                .tool_draft
                .check_reads
                .evidence
                .is_empty(),
            "discarded staged read grants nothing"
        );
        let mut replay: Checkpoint = serde_json::from_value(json!(sealed)).unwrap();
        confirm(&mut replay, &body).unwrap();
        assert_eq!(
            replay.outline_run.tool_draft.check_reads.evidence,
            vec![reference]
        );
        let once = json!(replay.outline_run.tool_draft);
        confirm(&mut replay, &body).unwrap();
        assert_eq!(
            once,
            json!(replay.outline_run.tool_draft),
            "repeat delivery is idempotent"
        );
    }
}
