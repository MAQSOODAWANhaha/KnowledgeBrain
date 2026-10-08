//! Derived Main source packages. Only a received request installs their scope
//! and reading receipts; package preparation never advances extraction.
use super::*;

fn unread(ranges: Option<&Vec<(usize, usize)>>, total: usize) -> Option<usize> {
    let mut offset = 0;
    for &(start, end) in ranges.into_iter().flatten() {
        if start > offset {
            break;
        }
        offset = offset.max(end);
    }
    (offset < total).then_some(offset)
}

fn size(value: &Value) -> Result<usize, String> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| error.to_string())
}

/// Read through the existing tools into a temporary ledger, admitting the
/// exact returned payload and its receipts together or neither.
fn append(
    input: &FrozenInput,
    content: &mut Value,
    coverage: &mut Coverage,
    name: &str,
    mut args: Value,
    budget: usize,
) -> Result<bool, String> {
    let remaining = budget.saturating_sub(size(content)?);
    if remaining == 0 {
        return Ok(false);
    }
    loop {
        let mut staged = coverage.clone();
        let result = tools::invoke(
            input,
            &mut Analysis::default(),
            &mut staged,
            true,
            name,
            &args,
            remaining,
        );
        if let Ok(source) = result {
            let mut next = content.clone();
            next["assigned_evidence"]["boundary_evidence"]
                .as_array_mut()
                .expect("package evidence array")
                .push(json!({"tool":name,"arguments":args,"source":source}));
            if content["assigned_evidence"].get("scan_ranges").is_some() {
                let source = &next["assigned_evidence"]["boundary_evidence"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["source"];
                let entry = match name {
                    "read_source" => Some((
                        "text",
                        args["source_id"].as_str(),
                        source["start"].as_u64(),
                        source["end"].as_u64(),
                    )),
                    "read_form" => Some((
                        "forms",
                        args["form_id"].as_str(),
                        source["offset"].as_u64(),
                        source["next"].as_u64(),
                    )),
                    "collection_index" => Some((
                        "metadata",
                        args["kind"].as_str(),
                        source["offset"].as_u64(),
                        source["next"].as_u64(),
                    )),
                    _ => None,
                };
                if let Some((kind, Some(id), Some(start), Some(end))) = entry
                    && start < end
                {
                    let ranges = &mut next["assigned_evidence"]["scan_ranges"][kind];
                    if ranges.get(id).is_none() {
                        ranges[id] = json!([]);
                    }
                    ranges[id].as_array_mut().unwrap().push(json!([start, end]));
                }
            }
            if size(&next)? <= budget {
                *content = next;
                *coverage = staged;
                return Ok(true);
            }
        }
        let field = if matches!(name, "read_source" | "read_form_cell") {
            "max_bytes"
        } else {
            "limit"
        };
        let count = args[field].as_u64().unwrap_or(0);
        if count <= 1 {
            return Ok(false);
        }
        args[field] = json!(count / 2);
    }
}

pub(super) fn evidence(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
    package_budget: Option<usize>,
) -> Result<Option<source_review::Evidence>, String> {
    if matches!(
        state.draft_stage,
        draft::DraftStage::None | draft::DraftStage::Outline
    ) {
        return Ok(None);
    }
    if state.role != Role::Main || state.pending_coverage.is_some() {
        return Ok(None);
    }
    let Some(work) = state
        .main_work
        .clone()
        .filter(|work| work.status == WorkStatus::Active)
    else {
        return Ok(None);
    };
    // Input and output are different budgets. Four input bytes per reserved
    // output token is a packing heuristic, not a promise about extraction size.
    // Leave context space for instructions, state, tools and retained history;
    // request() still checks the complete serialized request and token estimate.
    let budget = config
        .limits
        .max_tool_result_bytes
        .min(config.provider.max_tokens as usize);
    let budget = package_budget.map_or(budget, |cap| cap.min(budget));
    let mut coverage = state.coverage().clone();
    let mut content = json!({"main_work":work,"assigned_evidence":{
        "source":null,"candidates":[],"boundary_evidence":[],"navigation":[],
        "workload_bytes_limit":budget,
        "instruction":"This is the current bounded Main source package. Its main_work scope is installed when this response is received; no preliminary set_work_note or read call is needed for the included exact evidence. Save grounded records and known-endpoint relations, then set_disposition for each fully processed source. Keep missing targets or source uncertainty explicit. Newly allocated IDs may require another relation-writing response before the final dispositions. Partial text/grid ranges are not complete sources; read or receive the remaining ranges before final disposition. Adjacent sources are navigation, not a semantic relation. The host advances only after a successful complete batch and checks independent review separately."
    }});
    let selection = coverage.clone();
    for source in &input.source_units {
        let id = &source.source_unit_revision_id;
        if !work.source_scope.contains(id) {
            continue;
        }
        let before = content.clone();
        let before_coverage = coverage.clone();
        content["assigned_evidence"]["navigation"].as_array_mut().unwrap().push(json!({
            "source_id":id,"document_id":source.document_id,"ordinal":source.ordinal,
            "locator":source.locator,"total_bytes":source.text.len(),
            "forms":input.structured_forms.iter().filter(|form| form["source_unit_revision_id"] == *id)
                .map(|form| &form["form_definition_revision_id"]).collect::<Vec<_>>()
        }));
        let mut delivered = content["assigned_evidence"]["candidates"]
            != before["assigned_evidence"]["candidates"]
            || content["assigned_evidence"]["candidate_delivery"]["next_inspection"].is_object();
        let start = unread(selection.text.get(id), source.text.len());
        if let Some(start) = start {
            let max_bytes = (source.text.len() - start).min(budget).max(1);
            delivered |= append(
                input,
                &mut content,
                &mut coverage,
                "read_source",
                json!({"source_id":id,"start":start,"max_bytes":max_bytes}),
                budget,
            )?;
        } else if source.text.is_empty() && !state.analysis.dispositions.contains_key(id) {
            delivered |= append(
                input,
                &mut content,
                &mut coverage,
                "read_source",
                json!({"source_id":id,"start":0,"max_bytes":1}),
                budget,
            )?;
        }
        for form in input
            .structured_forms
            .iter()
            .filter(|form| form["source_unit_revision_id"] == *id)
        {
            let form_id = form["form_definition_revision_id"]
                .as_str()
                .ok_or("form identity missing")?;
            let total = super::super::relations::form_total(&form["definition"])
                .ok_or("form grid unavailable")?;
            if let Some(offset) = unread(selection.form_cells.get(form_id), total) {
                let (rows, columns) =
                    super::super::relations::form_dimensions(&form["definition"]).unwrap_or((1, 1));
                let header = if rows > 1 { columns } else { 0 };
                if header > 0 && offset >= header {
                    let _ = append(
                        input,
                        &mut content,
                        &mut coverage,
                        "read_form",
                        json!({"form_id":form_id,"offset":0,"limit":header}),
                        budget,
                    )?;
                }
                let limit = (total - offset).max(1);
                let grid_delivered = append(
                    input,
                    &mut content,
                    &mut coverage,
                    "read_form",
                    json!({"form_id":form_id,"offset":offset,"limit":limit}),
                    budget,
                )?;
                delivered |= grid_delivered;
                if !grid_delivered
                    && let Some(text) = super::super::relations::sparse_cell(
                        &form["definition"],
                        offset / columns,
                        offset % columns,
                    )
                    .and_then(|cell| cell["text"].as_str())
                {
                    let key = format!("form-cell:{form_id}:{offset}");
                    let start = unread(coverage.metadata.get(&key), text.len()).unwrap_or(0);
                    delivered |= append(
                        input,
                        &mut content,
                        &mut coverage,
                        "read_form_cell",
                        json!({"form_id":form_id,"offset":offset,"start":start,"max_bytes":budget}),
                        budget,
                    )?;
                }
            }
        }
        if !delivered || size(&content)? > budget {
            content = before;
            coverage = before_coverage;
        }
    }
    if work.source_scope.is_empty() {
        // Collection metadata has no source permission boundary. Continue it
        // even when every source is already disposed, without reopening or
        // replacing a completed scope merely to receive collection pages.
        content.as_object_mut().unwrap().remove("main_work");
        content["assigned_evidence"]["instruction"] = json!(
            "These are the remaining frozen collection metadata pages. Their exact kind and offset are in arguments. They do not replace the active/completed source scope or declare semantic approval. Inspect decisions and document relationships; save any grounded consequences before requesting independent review. Unsent metadata retains its reading gaps."
        );
    }
    if state.analysis.outline.phase == super::super::outline_flow::Phase::Discover {
        // Frozen metadata is evidence too. Admit bounded real collection pages;
        // an omitted page retains its existing global reading gap.
        for (kind, values) in [
            ("documents", &input.documents),
            ("document_relations", &input.document_relations),
            ("decisions", &input.decisions),
        ] {
            if let Some(offset) = unread(selection.metadata.get(kind), values.len()) {
                append(
                    input,
                    &mut content,
                    &mut coverage,
                    "collection_index",
                    json!({"kind":kind,"offset":offset,"limit":values.len()-offset}),
                    budget,
                )?;
            }
        }
    }
    if content["assigned_evidence"]["boundary_evidence"]
        .as_array()
        .unwrap()
        .is_empty()
        && content["assigned_evidence"]["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
        && !content["assigned_evidence"]["candidate_delivery"]["next_inspection"].is_object()
    {
        return Ok(None);
    }
    if size(&content)? > budget {
        return Ok(None);
    }
    Ok(Some(source_review::Evidence { content, coverage }))
}

pub(super) fn confirm_work(
    _input: &FrozenInput,
    _config: &Config,
    _state: &mut Checkpoint,
    _sent: &Value,
) -> Result<(), String> {
    Ok(())
}
