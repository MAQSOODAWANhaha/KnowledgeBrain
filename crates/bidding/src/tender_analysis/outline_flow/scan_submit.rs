//! Scan retries reuse the failed tool arguments already in the durable transcript.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repair {
    call_id: String,
    arguments_sha256: String,
    changes: Vec<Change>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: String,
    value: Value,
}

fn patched(base: &Value, repair: &Repair, tool: &str) -> Result<Value, String> {
    if digest(base)? != repair.arguments_sha256
        || repair.changes.is_empty()
        || repair.changes.len() > 64
    {
        return Err("scan repair digest differs or changes must contain 1..64 entries".into());
    }
    let mut value = base.clone();
    for change in &repair.changes {
        let root = change.path.split('/').nth(1).unwrap_or_default();
        if !(if tool == "put_outline_items" {
            matches!(root, "items" | "remove_ids")
        } else {
            matches!(
                root,
                "text"
                    | "forms"
                    | "metadata"
                    | "empty_sources"
                    | "requirements"
                    | "references"
                    | "issues"
                    | "review_fragments"
                    | "requirement_replacements"
            )
        }) {
            return Err(format!(
                "repair path is outside the scan contract: {}",
                change.path
            ));
        }
        let (parent, key) = change
            .path
            .rsplit_once('/')
            .ok_or("repair needs a JSON pointer")?;
        let key = key.replace("~1", "/").replace("~0", "~");
        let container = value
            .pointer_mut(parent)
            .ok_or("repair parent does not exist")?;
        match container {
            Value::Object(map) => {
                map.insert(key, change.value.clone());
            }
            Value::Array(items) => {
                let index: usize = key.parse().map_err(|_| "repair array index invalid")?;
                *items
                    .get_mut(index)
                    .ok_or("repair array index outside batch")? = change.value.clone();
            }
            _ => return Err("repair parent must be an object or array".into()),
        }
    }
    Ok(value)
}

/// Last failed scan and all tool calls needed to reconstruct it. A successful
/// scan consumes it. Other tool results do not manufacture a repair base.
fn pending(state: &Checkpoint, tool: &str) -> Option<(String, Value, BTreeSet<String>)> {
    let mut calls = BTreeMap::new();
    let mut last: Option<(String, Value, BTreeSet<String>)> = None;
    for message in &state.transcript {
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            if call["function"]["name"] == tool
                && let (Some(id), Some(args)) =
                    (call["id"].as_str(), call["function"]["arguments"].as_str())
                && let Ok(args) = serde_json::from_str::<Value>(args)
            {
                calls.insert(id.to_owned(), args);
            }
        }
        let Some(id) = message["tool_call_id"].as_str() else {
            continue;
        };
        let Some(args) = calls.get(id) else { continue };
        let Some(output) = message["content"]
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
        else {
            continue;
        };
        if output["ok"] == true {
            last = None;
            continue;
        }
        if output["ok"] != false {
            continue;
        }
        if let Some(repair) = args.get("repair") {
            let Some((prior_id, prior, dependencies)) = &last else {
                continue;
            };
            let Ok(repair) = serde_json::from_value::<Repair>(repair.clone()) else {
                continue;
            };
            if repair.call_id != *prior_id {
                continue;
            }
            if let Ok(base) = patched(prior, &repair, tool) {
                let mut dependencies = dependencies.clone();
                dependencies.insert(id.to_owned());
                last = Some((id.to_owned(), base, dependencies));
            }
        } else {
            last = Some((id.to_owned(), args.clone(), BTreeSet::from([id.to_owned()])));
        }
    }
    last
}

pub(super) fn projection(state: &Checkpoint) -> Value {
    pending(state, "submit_outline_scan").map(|(id, base, _)| json!({"call_id":id,"arguments_sha256":digest(&base).ok(),
        "instruction":"Repair the last failed scan with field changes; do not resend the full batch. No failed ranges have been committed."})).unwrap_or(Value::Null)
}

pub(super) fn protects(state: &Checkpoint, messages: &[Value]) -> bool {
    protects_tool(state, messages, "submit_outline_scan")
        || protects_tool(state, messages, "put_outline_items")
}
fn protects_tool(state: &Checkpoint, messages: &[Value], tool: &str) -> bool {
    let Some((_, _, ids)) = pending(state, tool) else {
        return false;
    };
    messages.iter().any(|message| {
        message["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|call| call["id"].as_str().is_some_and(|id| ids.contains(id)))
    })
}

fn error(path: String, id: &Value, code: &str, message: impl ToString) -> Value {
    fn bounded(value: &str) -> &str {
        let mut end = value.len().min(160);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        &value[..end]
    }
    json!({"path":bounded(&path),"id":id.as_str().map(bounded),"code":code,"message":bounded(&message.to_string())})
}

/// Report independent field/identity/evidence failures together. Final apply
/// remains authoritative for interdependent state transitions and range maps.
fn range_errors(input: &FrozenInput, state: &Checkpoint, args: &Value) -> Vec<Value> {
    let mut errors = Vec::new();
    for category in ["text", "forms", "metadata"] {
        let Some(map) = args[category].as_object() else {
            errors.push(error(
                format!("/{category}"),
                &Value::Null,
                "MAP_REQUIRED",
                "expected a map of IDs to ranges",
            ));
            continue;
        };
        for (id, raw) in map {
            let path = format!("/{category}/{}", id.replace('~', "~0").replace('/', "~1"));
            let identity = json!(id);
            if category == "metadata"
                && !matches!(
                    id.as_str(),
                    "documents" | "document_relations" | "decisions"
                )
            {
                errors.push(error(path, &identity, "UNKNOWN_METADATA_KIND", "use documents, document_relations or decisions; table cells belong in forms, not metadata"));
                continue;
            }
            let ranges: Vec<(usize, usize)> = match serde_json::from_value(raw.clone()) {
                Ok(ranges) => ranges,
                Err(_) => {
                    errors.push(error(
                        path,
                        &identity,
                        "INVALID_RANGE_SHAPE",
                        "expected [[start,end],...] with nonnegative integer bounds",
                    ));
                    continue;
                }
            };
            for (index, (start, end)) in ranges.into_iter().enumerate() {
                let path = format!("{path}/{index}");
                let result = if start >= end {
                    Err("range requires start < end; grid-only sources use forms; empty sources require empty_sources disposition, never [0,0)".into())
                } else if category == "text" {
                    tools::validate_span(
                        input,
                        state.coverage(),
                        &Span {
                            source_id: id.clone(),
                            start,
                            end,
                            view_id: None,
                            grid_cell: None,
                        },
                    )
                } else {
                    let (total, delivered) = if category == "forms" {
                        (
                            input
                                .structured_forms
                                .iter()
                                .find(|f| f["form_definition_revision_id"] == *id)
                                .and_then(|f| {
                                    super::super::relations::form_total(&f["definition"])
                                }),
                            state.coverage().form_cells.get(id),
                        )
                    } else {
                        (
                            Some(match id.as_str() {
                                "documents" => input.documents.len(),
                                "decisions" => input.decisions.len(),
                                _ => input.document_relations.len(),
                            }),
                            state.coverage().metadata.get(id),
                        )
                    };
                    match total {
                        None => Err("unknown form or invalid grid definition".into()),
                        Some(total) if end > total => Err(format!("range exceeds valid bounds [0,{total}); use the delivered scan_ranges")),
                        Some(_) if !tools::contains(delivered, start, end) => Err("range not confirmed delivered; inspect the exact range before submitting".into()),
                        _ => Ok(()),
                    }
                };
                if let Err(message) = result {
                    errors.push(error(path, &identity, "INVALID_SCAN_RANGE", message));
                }
            }
        }
    }
    if let Some(ids) = args["empty_sources"].as_array() {
        for (index, id) in ids.iter().enumerate() {
            let source = input
                .source_units
                .iter()
                .find(|s| id.as_str() == Some(s.source_unit_revision_id.as_str()));
            let reason = match source {
                None => Some("unknown source ID"),
                Some(source) if !source.text.is_empty() => {
                    Some("nonempty source needs text ranges")
                }
                Some(source)
                    if !input.structured_forms.iter().any(|f| {
                        f["source_unit_revision_id"] == source.source_unit_revision_id
                    }) && !state
                        .coverage()
                        .views
                        .values()
                        .any(|v| v.source_id == source.source_unit_revision_id) =>
                {
                    Some(
                        "empty text does not prove a blank page; inspect original view before disposition",
                    )
                }
                _ => None,
            };
            if let Some(reason) = reason {
                errors.push(error(
                    format!("/empty_sources/{index}"),
                    id,
                    "INVALID_EMPTY_SOURCE",
                    reason,
                ));
            }
        }
    } else {
        errors.push(error(
            "/empty_sources".into(),
            &Value::Null,
            "ARRAY_REQUIRED",
            "empty_sources must be an array",
        ));
    }
    errors
}

fn preflight(input: &FrozenInput, state: &Checkpoint, args: &Value) -> Vec<Value> {
    let mut errors = range_errors(input, state, args);
    let flow = &state.analysis.outline;
    let mut requirement_ids: BTreeSet<String> = flow.requirements.keys().cloned().collect();
    let mut reference_ids: BTreeSet<String> = flow.references.keys().cloned().collect();
    for (category, ids) in [
        ("requirements", &mut requirement_ids),
        ("references", &mut reference_ids),
    ] {
        ids.extend(
            args[category]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["id"].as_str())
                .filter(|id| id.starts_with("tmp-") && id.len() > 4)
                .map(str::to_owned),
        );
    }
    for category in ["requirements", "references", "issues", "review_fragments"] {
        let Some(items) = args[category].as_array() else {
            errors.push(error(
                format!("/{category}"),
                &Value::Null,
                "ARRAY_REQUIRED",
                "array required",
            ));
            continue;
        };
        let mut seen = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            let path = format!("/{category}/{index}");
            let id = &item["id"];
            let known = id.as_str().is_some_and(|id| match category {
                "requirements" => flow.requirements.contains_key(id),
                "references" => flow.references.contains_key(id),
                "issues" => flow.issues.contains_key(id),
                _ => flow.review_fragments.contains_key(id),
            });
            match id.as_str() {
                Some("") => {}
                Some(id) if known || id.starts_with("tmp-") && id.len() > 4 => {
                    if !seen.insert(id) {
                        errors.push(error(
                            format!("{path}/id"),
                            &item["id"],
                            "DUPLICATE_ID",
                            "ID repeated in this category",
                        ));
                    }
                }
                _ => errors.push(error(
                    format!("{path}/id"),
                    id,
                    "UNKNOWN_ID",
                    "use empty ID or tmp- alias for new entries; stable IDs must exist",
                )),
            }
            let shape = match category {
                "requirements" => serde_json::from_value::<ScanNeed>(item.clone()).map(|_| ()),
                "references" => serde_json::from_value::<ScanReference>(item.clone()).map(|_| ()),
                "issues" => serde_json::from_value::<ScanIssue>(item.clone()).map(|_| ()),
                _ => serde_json::from_value::<ScanFragment>(item.clone()).map(|_| ()),
            };
            if let Err(reason) = shape {
                errors.push(error(path.clone(), id, "INVALID_SHAPE", reason));
                continue;
            }
            for (field, identities) in [
                ("requirement_ids", &requirement_ids),
                ("reference_ids", &reference_ids),
            ] {
                for (offset, raw) in item[field].as_array().into_iter().flatten().enumerate() {
                    if raw.as_str().is_none_or(|id| !identities.contains(id)) {
                        errors.push(error(format!("{path}/{field}/{offset}"), id, "UNKNOWN_ASSOCIATION", "reference targets must be frozen source IDs and requirement IDs must exist; associated IDs must be existing or same-batch aliases"));
                    }
                }
            }
            for (offset, source) in item["target_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if !input
                    .source_units
                    .iter()
                    .any(|entry| source == &entry.source_unit_revision_id)
                {
                    errors.push(error(
                        format!("{path}/target_ids/{offset}"),
                        id,
                        "UNKNOWN_SOURCE",
                        "reference target must be a frozen source ID",
                    ));
                }
            }
            if category == "requirements" {
                if matches!(
                    item["kind"].as_str(),
                    Some("structure_constraint" | "non_document")
                ) && item["format_required"] == true
                {
                    errors.push(error(format!("{path}/format_required"), id, "FORMAT_KIND_CONFLICT", "prescribed material format must belong to a submission or content constraint"));
                }
                if item["applicability"] != "required"
                    && item["condition"]
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .is_empty()
                {
                    errors.push(error(
                        format!("{path}/condition"),
                        id,
                        "CONDITION_REQUIRED",
                        "conditional/not-applicable requirement needs its source condition",
                    ));
                }
                let reclassified = id
                    .as_str()
                    .and_then(|id| flow.requirements.get(id))
                    .is_some_and(|old| json!(old.kind) != item["kind"]);
                if (item["kind"] == "non_document" || reclassified)
                    && item["classification_reason"]
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .is_empty()
                {
                    errors.push(error(
                        format!("{path}/classification_reason"),
                        id,
                        "CLASSIFICATION_REASON_REQUIRED",
                        "non-document or reclassified requirement needs classification_reason",
                    ));
                }
                if (item["kind"] == "submission"
                    && item["submission_name"]
                        .as_str()
                        .is_none_or(|v| v.trim().is_empty()))
                    || (item["kind"] != "submission" && !item["submission_name"].is_null())
                {
                    errors.push(error(format!("{path}/submission_name"), id, "MATERIAL_NAME_INVALID", "submission_name must name one actual material for submission and be null otherwise"));
                }
                if item["description"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                    || item["grounds"].as_array().is_none_or(Vec::is_empty)
                {
                    errors.push(error(
                        path.clone(),
                        id,
                        "BASIS_REQUIRED",
                        "submission requirement needs description and exact grounds",
                    ));
                }
            }
            if matches!(category, "references" | "issues")
                && item["status"] == "resolved"
                && item["resolution_grounds"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
            {
                errors.push(error(
                    format!("{path}/resolution_grounds"),
                    id,
                    "RESOLUTION_REQUIRED",
                    "resolved conclusion needs resolution grounds",
                ));
            }
            for field in ["grounds", "format_grounds", "resolution_grounds"] {
                for (offset, raw) in item[field].as_array().into_iter().flatten().enumerate() {
                    let result = serde_json::from_value::<Span>(raw.clone())
                        .map_err(|e| e.to_string())
                        .and_then(|span| tools::validate_span(input, state.coverage(), &span));
                    if let Err(reason) = result {
                        errors.push(error(
                            format!("{path}/{field}/{offset}"),
                            id,
                            "INVALID_EVIDENCE",
                            reason,
                        ));
                    }
                }
            }
            if category == "review_fragments" {
                let result = serde_json::from_value::<Span>(item["span"].clone())
                    .map_err(|e| e.to_string())
                    .and_then(|span| tools::validate_span(input, state.coverage(), &span));
                if let Err(reason) = result {
                    errors.push(error(
                        format!("{path}/span"),
                        id,
                        "INVALID_EVIDENCE",
                        reason,
                    ));
                }
            }
        }
    }
    errors
}

pub(super) fn apply(
    input: &FrozenInput,
    state: &mut Checkpoint,
    args: &Value,
    budget: usize,
) -> Result<Value, String> {
    let resolved;
    let args = if let Some(raw) = args.get("repair") {
        if args.as_object().is_none_or(|map| map.len() != 1) {
            return Err("repair and full scan are mutually exclusive".into());
        }
        let repair: Repair = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
        let (id, base, _) =
            pending(state, "submit_outline_scan").ok_or("no failed scan available for repair")?;
        if id != repair.call_id {
            return Err("repair must target the latest failed scan".into());
        }
        resolved = patched(&base, &repair, "submit_outline_scan")?;
        &resolved
    } else {
        args
    };
    let arguments_sha256 = digest(args)?;
    let expanded = super::super::evidence_refs::expand(input, args)?;
    let args = &expanded;
    let mut errors = preflight(input, state, args);
    if errors.is_empty() {
        let previous = state.analysis.outline.checks.clone();
        match super::apply_validated(input, state, "submit_outline_scan", args, budget) {
            Ok(value) => {
                super::retain_unchanged_checks(input, state, &previous);
                return Ok(value);
            }
            Err(reason) => errors.push(error("/".into(), &Value::Null, "BATCH_INVALID", reason)),
        }
    }
    let mut result = json!({"committed":false,"arguments_sha256":arguments_sha256,"errors":[],"truncated":false,
        "repair_hint":"Use submit_outline_scan with repair.call_id from this tool result, arguments_sha256 and changes [{path,value}]. All changes are revalidated; no scan ranges were committed."});
    let count = errors.len();
    result["errors"] = json!(errors);
    result["error_count"] = json!(count);
    while serde_json::to_vec(&result)
        .map_err(|e| e.to_string())?
        .len()
        > budget
    {
        let items = result["errors"].as_array_mut().unwrap();
        if items.len() <= 1 {
            break;
        }
        items.pop();
        result["truncated"] = json!(true);
    }
    Err(result.to_string())
}

pub(super) fn chapter_projection(state: &Checkpoint) -> Value {
    pending(state, "put_outline_items").map(|(id, base, _)| json!({
        "call_id":id,"arguments_sha256":digest(&base).ok(),
        "instruction":"Repair failed chapter fields with put_outline_items.repair; do not rewrite the batch. No chapters were committed."
    })).unwrap_or(Value::Null)
}

pub(super) fn apply_chapters(
    input: &FrozenInput,
    config: &super::super::agent::Config,
    state: &mut Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    let resolved;
    let args = if let Some(raw) = args.get("repair") {
        if args.as_object().is_none_or(|map| map.len() != 1) {
            return Err("repair and full chapter batch are mutually exclusive".into());
        }
        let repair: Repair = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
        let (id, base, _) = pending(state, "put_outline_items")
            .ok_or("no failed chapter batch available for repair")?;
        if id != repair.call_id {
            return Err("repair must target the latest failed chapter batch".into());
        }
        resolved = patched(&base, &repair, "put_outline_items")?;
        &resolved
    } else {
        args
    };
    let mut errors = Vec::new();
    let items = args["items"].as_array().ok_or("items required")?;
    let mut seen = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        let path = format!("/items/{index}");
        let id = &item["id"];
        let mut report = |field: &str, code: &str, message: String| {
            errors.push(error(format!("{path}/{field}"), id, code, message));
        };
        match id.as_str() {
            Some(id)
                if state.analysis.draft_plan.iter().any(|n| n.id == id)
                    || id.starts_with("tmp-") && id.len() > 4 =>
            {
                if !seen.insert(id) {
                    report("id", "DUPLICATE_ID", "chapter ID repeated in batch".into());
                }
            }
            _ => report(
                "id",
                "UNKNOWN_ID",
                "new chapters need tmp- aliases; updates need saved chapter IDs".into(),
            ),
        }
        if item.get("grounds").is_some() || item.get("format_refs").is_some() {
            report(
                "requirement_ids",
                "HOST_DERIVED_BASIS",
                "supply requirement_ids only; grounds and format_refs are host-derived".into(),
            );
        }
        let purpose = item["purpose"].as_str();
        if !matches!(purpose, Some("response" | "group")) {
            report(
                "purpose",
                "INVALID_PURPOSE",
                "purpose must be response or group".into(),
            );
        }
        let Some(ids) = item["requirement_ids"].as_array() else {
            report(
                "requirement_ids",
                "ARRAY_REQUIRED",
                "requirement_ids must be an array of saved IDs".into(),
            );
            continue;
        };
        if purpose == Some("response") && ids.is_empty() {
            report("requirement_ids", "RESPONSE_BASIS_REQUIRED", "response material needs saved obligations; use group only for organizational headings".into());
        }
        for (position, value) in ids.iter().enumerate() {
            let field = format!("requirement_ids/{position}");
            let Some(need) = value
                .as_str()
                .and_then(|id| state.analysis.outline.requirements.get(id))
            else {
                report(
                    &field,
                    "UNKNOWN_REQUIREMENT",
                    format!("unknown requirement {value}; read saved requirement IDs"),
                );
                continue;
            };
            let valid = match purpose {
                Some("response") => need.needs_chapter(),
                Some("group") => {
                    need.kind == NeedKind::StructureConstraint
                        && need.applicability != Applicability::NotApplicable
                }
                _ => true,
            };
            if !valid {
                let action = if need.applicability == Applicability::NotApplicable
                    || need.kind == NeedKind::NonDocument
                {
                    "excluded/non-document: retain exclusion for review; do not attach to a chapter; correct classification only with source evidence"
                } else if need.kind == NeedKind::StructureConstraint {
                    "structure constraint: attach to an organizational group"
                } else {
                    "material/content constraint: attach to a response node; split mixed materials using submit_outline_scan before mapping"
                };
                report(
                    &field,
                    "REQUIREMENT_DESTINATION",
                    format!("{}: {action}", value.as_str().unwrap_or_default()),
                );
            }
        }
    }
    if errors.is_empty() {
        let previous = state.analysis.outline.checks.clone();
        match super::super::draft::apply_validated(input, config, state, "put_outline_items", args)
        {
            Ok(value) => {
                super::retain_unchanged_checks(input, state, &previous);
                return Ok(value);
            }
            Err(reason) => errors.push(error("/".into(), &Value::Null, "BATCH_INVALID", reason)),
        }
    }
    let count = errors.len();
    let mut result = json!({"committed":false,"arguments_sha256":digest(args)?,"error_count":count,
        "errors":errors,"truncated":false,
        "repair_hint":"Use put_outline_items.repair with call_id, arguments_sha256 and changes [{path,value}]. Revalidate the entire final tree; no partial commit."});
    while result.to_string().len() > config.limits.max_tool_result_bytes {
        let items = result["errors"].as_array_mut().unwrap();
        if items.len() <= 1 {
            break;
        }
        items.pop();
        result["truncated"] = json!(true);
    }
    Err(result.to_string())
}
