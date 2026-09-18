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

fn append_candidates(
    input: &FrozenInput,
    state: &Checkpoint,
    scope: &[String],
    content: &mut Value,
    coverage: &mut Coverage,
    budget: usize,
) -> Result<(), String> {
    let references = context::scope_references(&state.analysis, scope);
    let mut delivered: std::collections::BTreeSet<String> =
        content["assigned_evidence"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["reference"].as_str().map(str::to_owned))
            .collect();
    let mut complete = 0;
    let mut next = None;
    for reference in &references {
        let group = source_review::evidence_candidates::group(state, reference)?;
        if group.iter().all(|key| delivered.contains(key)) {
            complete += 1;
            continue;
        }
        let values: Vec<_> = group
            .iter()
            .filter(|key| !delivered.contains(*key))
            .map(|key| {
                let value = context::reference(&state.analysis, key)?;
                Ok(json!({"reference":key,"sha256":digest(&value)?,"value":value}))
            })
            .collect::<Result<_, String>>()?;
        let ids: Vec<_> = group
            .iter()
            .map(|key| key.split_once(':').expect("typed candidate reference").1)
            .collect();
        let inspection =
            json!({"kind":"all","view":"detail","ids":ids,"offset":0,"limit":ids.len()});
        let mut proposed = content.clone();
        proposed["assigned_evidence"]["candidate_delivery"] = json!({
            "group_total":references.len(),"group_delivered":complete + 1,
            "complete":false,"next_inspection":inspection,
            "instruction":"Only complete groups are included. A relation is delivered with both complete endpoints; templates retain their parent. When complete is false, use the exact next_inspection IDs and continue its returned pagination. Missing groups are not received or judged. Narrow the comparison if the group and original evidence cannot fit together."
        });
        proposed["assigned_evidence"]["candidates"]
            .as_array_mut()
            .unwrap()
            .extend(values.clone());
        // Source evidence needs room in the same packet. This split is a
        // conservative admission allowance, not a semantic output estimate.
        if size(&proposed)? <= budget / 2 {
            *content = proposed;
            complete += 1;
            for value in values {
                let key = value["reference"].as_str().unwrap().to_owned();
                coverage
                    .candidate
                    .insert(key.clone(), value["sha256"].as_str().unwrap().to_owned());
                delivered.insert(key);
            }
        } else if next.is_none() {
            next = Some(inspection);
        }
    }
    content["assigned_evidence"]["candidate_delivery"] = json!({
        "group_total":references.len(),"group_delivered":complete,
        "complete":complete == references.len(),"next_inspection":next,
        "instruction":"Missing groups are not received or judged. Continue with the exact next_inspection IDs; inspect returned pagination and narrow the comparison if the whole group and original evidence cannot fit."
    });
    crate::tender_analysis::semantic_compare::annotate_candidates(
        input,
        &state.analysis,
        coverage,
        scope,
        content,
        budget / 2,
    )?;
    Ok(())
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
    if config.limits.draft_path
        && matches!(
            state.draft_stage,
            draft::DraftStage::None | draft::DraftStage::Outline
        )
        && state.analysis.outline.phase != super::super::outline_flow::Phase::Discover
    {
        return Ok(None);
    }
    if state.role != Role::Main || state.pending_coverage.is_some() {
        return Ok(None);
    }
    let (mut work, check_replace) = if config.limits.draft_path {
        match state
            .main_work
            .clone()
            .filter(|work| work.status == WorkStatus::Active)
        {
            Some(work) => (work, false),
            None => return Ok(None),
        }
    } else {
        if state.source_review.is_some() && state.dispatch.active.is_none() {
            return Ok(None);
        }
        let Some(assigned) = main_dispatch::projection(input, config, state)? else {
            return Ok(None);
        };
        if matches!(assigned.owner, main_dispatch::Active::Repair(_)) {
            return Ok(None);
        }
        let work = main_dispatch::work(input, &config.limits, state, &assigned);
        if !work.deferred_sources.is_empty()
            || context::scope_is_blocked(state, &work.source_scope)?
        {
            return Ok(None);
        }
        (work, true)
    };
    let discovering = config.limits.draft_path
        && matches!(
            state.draft_stage,
            draft::DraftStage::None | draft::DraftStage::Outline
        )
        && state.analysis.outline.phase == super::super::outline_flow::Phase::Discover;
    // Input and output are different budgets. Four input bytes per reserved
    // output token is a packing heuristic, not a promise about extraction size.
    // Leave context space for instructions, state, tools and retained history;
    // request() still checks the complete serialized request and token estimate.
    let budget = if discovering {
        config
            .limits
            .max_tool_result_bytes
            .min(config.limits.max_context_bytes / 4)
            .min((config.provider.max_tokens as usize).saturating_mul(4))
    } else {
        config
            .limits
            .max_tool_result_bytes
            .min(config.provider.max_tokens as usize)
    };
    let budget = package_budget.map_or(budget, |cap| cap.min(budget));
    if discovering {
        work.source_scope = draft::outline_reading_scope(input, state, budget);
    }
    let mut coverage = state.coverage().clone();
    let mut content = json!({"main_work":work,"assigned_evidence":{
        "source":null,"candidates":[],"boundary_evidence":[],"navigation":[],
        "workload_bytes_limit":budget,
        "instruction":"This is the current bounded Main source package. Its main_work scope is installed when this response is received; no preliminary set_work_note or read call is needed for the included exact evidence. Save grounded records and known-endpoint relations, then set_disposition for each fully processed source. Keep missing targets or source uncertainty explicit. Newly allocated IDs may require another relation-writing response before the final dispositions. Partial text/grid ranges are not complete sources; read or receive the remaining ranges before final disposition. Adjacent sources are navigation, not a semantic relation. The host advances only after a successful complete batch and checks independent review separately."
    }});
    if discovering {
        content["assigned_evidence"]["instruction"] = json!(
            "Inspect all exact delivered text, grid and metadata ranges together. Submit requirements, references, review fragments and inspected ranges in one submit_outline_scan batch. Navigation is not evidence. A reading package may cross multiple accounting chunks; never confirm unsent ranges."
        );
    }
    // Selection skips completed scans and evidence still visible, not lifetime
    // delivery receipts. Evicted, unsubmitted ranges must be offered again.
    let selection = if discovering {
        let mut selected = state.analysis.outline.scanned.clone();
        for (key, ranges) in context::visible_work_evidence(state, &state.transcript) {
            let Some((kind, id)) = key.split_once(':') else {
                continue;
            };
            let map = match kind {
                "text" => &mut selected.text,
                "form" => &mut selected.form_cells,
                "metadata" => &mut selected.metadata,
                _ => continue,
            };
            for (start, end) in ranges {
                tools::cover(map.entry(id.into()).or_default(), start, end);
            }
        }
        selected
    } else {
        coverage.clone()
    };
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
        if !config.limits.draft_path {
            append_candidates(
                input,
                state,
                &work.source_scope,
                &mut content,
                &mut coverage,
                budget,
            )?;
        }
        let mut delivered = content["assigned_evidence"]["candidates"]
            != before["assigned_evidence"]["candidates"]
            || content["assigned_evidence"]["candidate_delivery"]["next_inspection"].is_object();
        let start = if config.limits.draft_path {
            unread(selection.text.get(id), source.text.len())
        } else {
            unread(selection.text.get(id), source.text.len()).or_else(|| {
                (state.dispatch.active.is_none() && !source.text.is_empty()).then_some(0)
            })
        };
        if let Some(start) = start {
            let max_bytes = if discovering {
                (source.text.len() - start).min(budget)
            } else if config.limits.draft_path {
                (source.text.len() - start).min(crate::tender_analysis::draft::DRAFT_WINDOW_BYTES)
            } else {
                source.text.len() - start
            };
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
                if config.limits.draft_path && header > 0 && offset >= header {
                    let _ = append(
                        input,
                        &mut content,
                        &mut coverage,
                        "read_form",
                        json!({"form_id":form_id,"offset":0,"limit":header}),
                        budget,
                    )?;
                }
                let limit = if discovering {
                    total - offset
                } else if config.limits.draft_path {
                    columns
                        .saturating_mul(crate::tender_analysis::draft::OUTLINE_FORM_BODY_ROWS)
                        .max(1)
                        .min(total - offset)
                        .max(1)
                } else {
                    total - offset
                };
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
    if !config.limits.draft_path
        || state.analysis.outline.phase == super::super::outline_flow::Phase::Discover
    {
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
    if check_replace && !work.source_scope.is_empty() {
        context::validate(input, state, &work, config.limits.max_tool_result_bytes)?;
    }
    Ok(Some(source_review::Evidence { content, coverage }))
}

pub(super) fn confirm_work(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    sent: &Value,
) -> Result<(), String> {
    if config.limits.draft_path {
        return Ok(());
    }
    let Some(value) = sent.get("main_work") else {
        return Ok(());
    };
    if state.role != Role::Main {
        return Err("Main package cannot replace Reviewer work".into());
    }
    let work: WorkState =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    main_dispatch::check_scope(input, state, &config.limits, &work.source_scope)?;
    context::validate(input, state, &work, config.limits.max_tool_result_bytes)?;
    state.main_work = Some(work);
    Ok(())
}
