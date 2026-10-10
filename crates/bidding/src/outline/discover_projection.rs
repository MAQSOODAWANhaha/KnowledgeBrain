//! Lossless source identity with a compact, request-local table dictionary.
//! Canonical PackCarrier and FrozenInput are never mutated here.
use super::*;

fn rectangles(points: &BTreeSet<(usize, usize)>) -> Vec<Value> {
    let mut remaining = points.clone();
    let mut ranges = Vec::new();
    while let Some(&(row, column)) = remaining.first() {
        let mut end_column = column + 1;
        while remaining.contains(&(row, end_column)) {
            end_column += 1;
        }
        let mut end_row = row + 1;
        while (column..end_column).all(|c| remaining.contains(&(end_row, c))) {
            end_row += 1;
        }
        for r in row..end_row {
            for c in column..end_column {
                remaining.remove(&(r, c));
            }
        }
        ranges.push(json!({"start_row":row,"end_row_exclusive":end_row,"row_count":end_row-row,
            "start_column":column,"end_column_exclusive":end_column,"column_count":end_column-column}));
    }
    ranges
}

pub(super) fn render_grid(
    rendered: &mut Value,
    input: &FrozenInput,
    table_id: &str,
    cells: &[GridFragment],
    header_context: &[GridFragment],
    _digest: &str,
) -> Result<(), String> {
    let def = super::super::evidence::table_definition(input, table_id)?;
    // Until controls have typed occupancy, conservatively preserve empty cells
    // in any document carrying images/forms or spreadsheet metadata.
    let form = input
        .structured_forms
        .iter()
        .find(|f| f["form_definition_revision_id"] == table_id)
        .ok_or("table missing")?;
    let owner = input.source_units.iter().find(|s| {
        s.source_unit_revision_id == form["source_unit_revision_id"].as_str().unwrap_or("")
    });
    let protected = owner.is_some_and(|owner| {
        input.source_units.iter().any(|s| {
            s.document_id == owner.document_id
                && ["image", "form", "spreadsheet"].iter().any(|k| {
                    s.locator["kind"].as_str() == Some(k)
                        || s.locator["locator_kind"].as_str() == Some(k)
                })
        })
    });
    let mut structure = json!({"table_id":table_id,"row_count":def["row_count"],"column_count":def["column_count"],
        "anchors":{},"blank_source_semantics":"Ranges are original empty 1x1 body-cell positions (text exactly empty), available for filling. Row/column counts describe source layout only, not mandatory response counts or limits."});
    if let Some(title) = def.get("title") {
        structure["title"] = title.clone();
    }
    let mut body = Vec::new();
    let mut context_refs = Vec::new();
    let mut blanks = BTreeSet::new();
    let mut evidence_index = 0usize;
    for (cell, context_only) in cells
        .iter()
        .map(|c| (c, false))
        .chain(header_context.iter().map(|c| (c, true)))
    {
        let source = super::super::evidence::grid_cell(def, cell.anchor_row, cell.anchor_column)?;
        let full = source["text"].as_str().ok_or("cell text missing")?;
        let text = full
            .get(cell.start_byte..cell.end_byte)
            .ok_or("invalid grid fragment")?;
        let simple = source.as_object().is_some_and(|m| {
            m.keys().all(|k| {
                matches!(
                    k.as_str(),
                    "row"
                        | "column"
                        | "row_span"
                        | "column_span"
                        | "col_span"
                        | "text"
                        | "header_role"
                )
            })
        });
        if !protected
            && simple
            && !context_only
            && cell.anchor_row > 0
            && cell.header_role == "none"
            && cell.row_span == 1
            && cell.column_span == 1
            && full.is_empty()
        {
            blanks.insert((cell.anchor_row, cell.anchor_column));
            continue;
        }
        let key = format!("{},{}", cell.anchor_row, cell.anchor_column);
        let header = cell.anchor_row == 0 || cell.header_role != "none";
        let mut metadata = source.clone();
        let object = metadata.as_object_mut().ok_or("cell object missing")?;
        object.remove("row");
        object.remove("column");
        object.remove("text");
        object.remove("col_span");
        object.insert("row_span".into(), json!(cell.row_span));
        object.insert("column_span".into(), json!(cell.column_span));
        object.insert("header_role".into(), json!(cell.header_role));
        // Keep only the admitted UTF-8 fragment, so an oversized header
        // remains splittable under the exact request budget.
        if header {
            metadata["header_fragments"] =
                json!({format!("{},{}",cell.start_byte,cell.end_byte):text});
        }
        structure["anchors"][&key] = metadata;
        let mut reference = json!({"anchor_row":cell.anchor_row,"anchor_column":cell.anchor_column,
            "start_byte":cell.start_byte,"end_byte":cell.end_byte});
        if !header {
            reference["text"] = json!(text);
        }
        if cell.start_byte < cell.end_byte {
            reference["evidence_index"] = json!(evidence_index);
            evidence_index += 1;
        }
        if context_only {
            context_refs.push(reference);
        } else {
            body.push(reference);
        }
    }
    rendered["carrier"] = json!({"kind":"grid","table_id":table_id});
    rendered["cells"] = json!(body);
    if !context_refs.is_empty() {
        rendered["header_context"] = json!(context_refs);
    }
    if !blanks.is_empty() {
        structure["blank_source_ranges"] = json!({});
        let mut refs = Vec::new();
        for range in rectangles(&blanks) {
            let key = format!(
                "{}:{},{}:{}",
                range["start_row"],
                range["end_row_exclusive"],
                range["start_column"],
                range["end_column_exclusive"]
            );
            structure["blank_source_ranges"][&key] = range;
            refs.push(key);
        }
        rendered["blank_range_refs"] = json!(refs);
    }
    rendered["table_structure"] = structure;
    Ok(())
}

fn merge(catalog: &mut BTreeMap<String, Value>, value: Value) -> Result<(), String> {
    let id = value["table_id"]
        .as_str()
        .ok_or("table dictionary identity missing")?
        .to_owned();
    if let Some(existing) = catalog.get_mut(&id) {
        for (key, item) in value.as_object().ok_or("table dictionary missing")? {
            if key == "blank_source_ranges" {
                if existing.get(key).is_none() {
                    existing[key] = json!({});
                }
                for (range, details) in item.as_object().ok_or("blank ranges missing")? {
                    if existing[key].get(range).is_some_and(|old| old != details) {
                        return Err("conflicting blank range".into());
                    }
                    existing[key][range] = details.clone();
                }
            } else if key == "anchors" {
                for (coordinate, anchor) in item.as_object().ok_or("table anchors missing")? {
                    if existing["anchors"].get(coordinate).is_none() {
                        existing["anchors"][coordinate] = json!({});
                    }
                    for (field, value) in anchor.as_object().ok_or("anchor metadata missing")? {
                        if field == "header_fragments" {
                            if existing["anchors"][coordinate].get(field).is_none() {
                                existing["anchors"][coordinate][field] = json!({});
                            }
                            for (range, text) in
                                value.as_object().ok_or("header fragments missing")?
                            {
                                if existing["anchors"][coordinate][field]
                                    .get(range)
                                    .is_some_and(|old| old != text)
                                {
                                    return Err("conflicting header text".into());
                                }
                                existing["anchors"][coordinate][field][range] = text.clone();
                            }
                            continue;
                        }
                        if existing["anchors"][coordinate]
                            .get(field)
                            .is_some_and(|old| old != value)
                        {
                            return Err("conflicting table anchor projection".into());
                        }
                        existing["anchors"][coordinate][field] = value.clone();
                    }
                }
            } else if existing.get(key).is_some_and(|old| old != item) {
                return Err("conflicting table structure projection".into());
            } else {
                existing[key] = item.clone();
            }
        }
    } else {
        catalog.insert(id, value);
    }
    Ok(())
}
fn collect(value: &mut Value, catalog: &mut BTreeMap<String, Value>) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            // Navigation cards also carry table headers. Move them out of host
            // summaries/tool history, then resolve duplicate text to anchors.
            if object.get("header").is_some_and(Value::is_array) && object.contains_key("form_id") {
                let id = object["form_id"]
                    .as_str()
                    .ok_or("form identity missing")?
                    .to_owned();
                let mut table = json!({"table_id":id,"anchors":{},"navigation_headers":object.remove("header").unwrap()});
                if let Some(title) = object.remove("title") {
                    table["title"] = title;
                }
                merge(catalog, table)?;
            }
            // Grid evidence locators share spans/provenance with the dictionary.
            if object.contains_key("table_id")
                && object.contains_key("anchor_row")
                && object.contains_key("anchor_column")
                && (object.contains_key("row_span") || object.contains_key("column_span"))
            {
                let id = object["table_id"]
                    .as_str()
                    .ok_or("table identity missing")?
                    .to_owned();
                let coordinate = format!("{},{}", object["anchor_row"], object["anchor_column"]);
                let mut table = json!({"table_id":id,"anchors":{}});
                let mut anchor = json!({});
                for key in ["row_span", "column_span"] {
                    if let Some(item) = object.remove(key) {
                        anchor[key] = item;
                    }
                }
                table["anchors"][coordinate] = anchor;
                if let Some(locator) = object.remove("physical_locator") {
                    table["physical_locator"] = locator;
                }
                merge(catalog, table)?;
            }
            // Check tool rows keep their exact receipt ID and fragment bounds;
            // structural metadata is shared by the same request dictionary.
            if object.contains_key("structural_receipt_id")
                && let Some(id) = object
                    .get("table_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            {
                let mut table = json!({"table_id":id,"anchors":{}});
                for key in ["row_count", "column_count"] {
                    if let Some(item) = object.remove(key).filter(|v| !v.is_null()) {
                        table[key] = item;
                    }
                }
                if let Some(cell) = object.get_mut("cell").and_then(Value::as_object_mut) {
                    let key = format!("{},{}", cell["anchor_row"], cell["anchor_column"]);
                    let mut anchor = json!({});
                    for field in ["row_span", "column_span", "header_role"] {
                        if let Some(item) = cell.remove(field) {
                            anchor[field] = item;
                        }
                    }
                    table["anchors"][key] = anchor;
                }
                merge(catalog, table)?;
            }
            if let Some(table) = object.remove("table_structure") {
                merge(catalog, table)?;
            }
            if let Some(tables) = object.remove("table_structures") {
                for table in tables.as_array().ok_or("table dictionary array missing")? {
                    merge(catalog, table.clone())?;
                }
            }
            for child in object.values_mut() {
                collect(child, catalog)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                collect(child, catalog)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Used both by full-envelope planning and by the exact bytes sent to transport.
/// Repeated tables across sessions/history share one request-local dictionary.
pub(crate) fn compact_request(body: &mut Value) -> Result<(), String> {
    let messages = body["messages"]
        .as_array_mut()
        .ok_or("request messages missing")?;
    let mut catalog = BTreeMap::new();
    let mut payloads = Vec::new();
    for (index, message) in messages.iter_mut().enumerate() {
        if let Some(text) = message["content"].as_str()
            && let Ok(mut payload) = serde_json::from_str::<Value>(text)
        {
            collect(&mut payload, &mut catalog)?;
            payloads.push((index, payload));
        }
    }
    for table in catalog.values_mut() {
        if let Some(headers) = table
            .as_object_mut()
            .and_then(|m| m.remove("navigation_headers"))
        {
            let references = headers
                .as_array()
                .ok_or("navigation headers missing")?
                .iter()
                .enumerate()
                .map(|(column, text)| {
                    if !text.is_string() {
                        return text.clone();
                    } // already compact: idempotent
                    let found = table["anchors"]
                        .as_object()
                        .into_iter()
                        .flat_map(|m| m.iter())
                        .find(|(coordinate, anchor)| {
                            coordinate
                                .split(',')
                                .nth(1)
                                .and_then(|c| c.parse::<usize>().ok())
                                == Some(column)
                                && anchor["header_fragments"]
                                    .as_object()
                                    .into_iter()
                                    .flat_map(|m| m.values())
                                    .any(|fragment| {
                                        fragment.as_str().map(str::trim)
                                            == text.as_str().map(str::trim)
                                    })
                        });
                    match found {
                        Some((coordinate, _)) => json!({"anchor":coordinate}),
                        None => json!({"column":column,"text":text}),
                    }
                })
                .collect::<Vec<_>>();
            table["navigation_headers"] = json!(references);
        }
    }
    if !catalog.is_empty() {
        let (_, brief) = payloads
            .iter_mut()
            .find(|(index, _)| *index == 1)
            .ok_or("request brief missing")?;
        brief["table_structures"] = json!(catalog.into_values().collect::<Vec<_>>());
    }
    for (index, payload) in payloads {
        messages[index]["content"] = json!(payload.to_string());
    }
    Ok(())
}
