//! Bounded delivery groups, derived from the unchanged source obligations.
use super::*;

pub(in crate::tender_analysis) fn group(
    state: &Checkpoint,
    reference: &str,
) -> Result<BTreeSet<String>, String> {
    let mut refs = BTreeSet::from([reference.to_owned()]);
    if let Some(id) = reference.strip_prefix("relation:") {
        let edge = state.analysis.relations.get(id).ok_or("relation missing")?;
        refs.insert(format!("record:{}", edge.from));
        refs.insert(format!("record:{}", edge.to));
    }
    if let Some(id) = reference.strip_prefix("record:")
        && let Some(Record {
            data:
                RecordData::Template {
                    parent: Some(parent),
                    ..
                },
            ..
        }) = state.analysis.records.get(id)
    {
        refs.insert(format!("record:{parent}"));
    }
    Ok(refs)
}

fn fits(content: &Value, budget: usize) -> Result<bool, String> {
    Ok(serde_json::to_vec(content)
        .map_err(|e| e.to_string())?
        .len()
        <= budget)
}

pub(super) fn append(
    input: &FrozenInput,
    state: &Checkpoint,
    task: &Task,
    content: &mut Value,
    coverage: &mut Coverage,
    budget: usize,
) -> Result<(), String> {
    let refs = obligations(input, state, task).comparisons;
    let mut pending = Vec::new();
    let mut compared = Vec::new();
    for reference in refs {
        if context::has_review_outcome(state, &reference)? {
            compared.push(reference);
        } else {
            pending.push(reference);
        }
    }
    // A comparison receipt does not make its current values unnecessary for
    // source omissions, template mappings or another relation's endpoints.
    if let Some(work) = state.work() {
        pending.sort_by_key(|reference| !work.focus.references.contains(reference));
    }
    let pending_count = pending.len();
    pending.extend(compared);
    content["assigned_evidence"]["candidate_delivery"] = json!({
        "group_total":pending.len(), "group_delivered":0,
        "pending_comparisons":pending_count,
        "complete":false, "next_group":null, "capacity_blocked_group":null,
        "instruction":"Only complete candidate groups are delivered. Partial packets do not finish the source task: compare included pending items, then continue. A capacity_blocked_group needs narrower evidence or an explicit capacity failure, never truncation."
    });
    let empty = content.clone();
    let mut delivered = BTreeSet::new();
    let mut completed = 0;
    let mut next: Option<String> = None;
    let mut blocked: Option<String> = None;
    for reference in &pending {
        let raw: Vec<_> = group(state, reference)?
            .into_iter()
            .map(|key| {
                let value = context::reference(&state.analysis, &key)?;
                Ok(json!({"reference":key.clone(),"sha256":digest(&value)?,"value":value}))
            })
            .collect::<Result<_, String>>()?;
        let attached: Vec<_> = raw
            .iter()
            .map(|row| {
                crate::tender_analysis::semantic_compare::attach(
                    input,
                    &state.analysis,
                    row["reference"].as_str().unwrap(),
                    row.clone(),
                )
            })
            .collect();
        let admit = |packet: &mut Value, rows: &[Value]| {
            let items = packet["assigned_evidence"]["candidates"]
                .as_array_mut()
                .unwrap();
            items.extend(
                rows.iter()
                    .filter(|value| !delivered.contains(value["reference"].as_str().unwrap()))
                    .cloned(),
            );
            packet["assigned_evidence"]["candidate_delivery"]["group_delivered"] =
                json!(completed + 1);
            packet["assigned_evidence"]["candidate_delivery"]["next_group"] =
                json!(next.as_ref().unwrap_or(reference));
            packet["assigned_evidence"]["candidate_delivery"]["capacity_blocked_group"] =
                json!(blocked.as_ref().unwrap_or(reference));
        };
        let mut proposed = content.clone();
        admit(&mut proposed, &attached);
        if !fits(&proposed, budget)? {
            proposed = content.clone();
            admit(&mut proposed, &raw);
            proposed["assigned_evidence"]["comparison_projection"] = json!({
                "complete":false,
                "omitted_for":reference,
                "instruction":"Host field/blank projections omitted so the complete candidate group still fits. Compare value against original evidence; do not treat omission as approval."
            });
        }
        if fits(&proposed, budget)? {
            *content = proposed;
            completed += 1;
            for value in &raw {
                let key = value["reference"].as_str().unwrap().to_owned();
                coverage
                    .candidate
                    .insert(key.clone(), value["sha256"].as_str().unwrap().into());
                delivered.insert(key);
            }
        } else {
            next.get_or_insert_with(|| reference.clone());
            let mut alone = empty.clone();
            alone["assigned_evidence"]["candidates"] = json!(raw);
            alone["assigned_evidence"]["candidate_delivery"]["next_group"] = json!(reference);
            alone["assigned_evidence"]["candidate_delivery"]["capacity_blocked_group"] =
                json!(reference);
            if !fits(&alone, budget)? {
                blocked.get_or_insert_with(|| reference.clone());
            }
        }
    }
    let delivery = &mut content["assigned_evidence"]["candidate_delivery"];
    delivery["complete"] = json!(completed == pending.len());
    delivery["next_group"] = json!(next);
    delivery["capacity_blocked_group"] = json!(blocked);
    if !fits(content, budget)? {
        return Err(
            "source and explicit candidate delivery status exceed the evidence budget".into(),
        );
    }
    Ok(())
}
