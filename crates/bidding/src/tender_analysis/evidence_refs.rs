//! Tool-only addresses within the frozen input. Expansion grants no reading
//! receipt; canonical Span validation still checks geometry and role coverage.
use super::*;
use serde_json::json;

pub fn expand(input: &FrozenInput, args: &Value) -> Result<Value, String> {
    fn visit(input: &FrozenInput, value: &mut Value, path: &str) -> Result<(), String> {
        match value {
            Value::Object(object) if object.len() == 1 && object.contains_key("ref") => {
                let reference = object["ref"]
                    .as_str()
                    .ok_or_else(|| tools::field_error(path, "evidence ref must be a string"))?;
                *value = json!(resolve(input, reference).map_err(|e| tools::field_error(path, e))?);
            }
            Value::Object(object) => {
                for (key, value) in object {
                    let key = key.replace('~', "~0").replace('/', "~1");
                    visit(input, value, &format!("{path}/{key}"))?;
                }
            }
            Value::Array(items) => {
                for (index, value) in items.iter_mut().enumerate() {
                    visit(input, value, &format!("{path}/{index}"))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut expanded = args.clone();
    visit(input, &mut expanded, "")?;
    Ok(expanded)
}

fn resolve(input: &FrozenInput, reference: &str) -> Result<Span, String> {
    let parts: Vec<_> = reference.split(':').collect();
    if parts.len() != 4 || !matches!(parts[0], "t" | "g") {
        return Err(
            "use a returned t:source_index:start:end or g:form_index:row:column ref".into(),
        );
    }
    let number = |index: usize| -> Result<usize, String> {
        let part = parts[index];
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err("evidence ref positions must be nonnegative integers".into());
        }
        part.parse()
            .map_err(|_| "evidence ref position overflow".into())
    };
    let (index, first, last) = (number(1)?, number(2)?, number(3)?);
    if parts[0] == "t" {
        let source = input
            .source_units
            .get(index)
            .ok_or("unknown text evidence ref")?;
        Ok(Span {
            source_id: source.source_unit_revision_id.clone(),
            start: first,
            end: last,
            view_id: None,
            grid_cell: None,
        })
    } else {
        let form = input
            .structured_forms
            .get(index)
            .ok_or("unknown grid evidence ref")?;
        Ok(Span {
            source_id: form["source_unit_revision_id"]
                .as_str()
                .ok_or("grid source missing")?
                .into(),
            start: 0,
            end: 0,
            view_id: None,
            grid_cell: Some(GridCitation {
                form_id: form["form_definition_revision_id"]
                    .as_str()
                    .ok_or("grid identity missing")?
                    .into(),
                row: first,
                column: last,
            }),
        })
    }
}

pub fn compact(input: &FrozenInput, span: &Span) -> Option<Value> {
    if span.view_id.is_some() {
        return None;
    }
    let reference = if let Some(cell) = &span.grid_cell {
        let index = input.structured_forms.iter().position(|f| {
            f["form_definition_revision_id"] == cell.form_id
                && f["source_unit_revision_id"] == span.source_id
        })?;
        if span.start != 0 || span.end != 0 {
            return None;
        }
        format!("g:{index}:{}:{}", cell.row, cell.column)
    } else {
        let index = input
            .source_units
            .iter()
            .position(|s| s.source_unit_revision_id == span.source_id)?;
        format!("t:{index}:{}:{}", span.start, span.end)
    };
    Some(json!({"ref":reference}))
}

pub(super) fn decorate(input: &FrozenInput, name: &str, out: &mut Value) -> Result<(), String> {
    if name == "read_form" {
        let refs = out["citations"]
            .as_array()
            .ok_or("grid citations missing")?
            .iter()
            .map(|v| {
                if v.is_null() {
                    return Ok(Value::Null);
                }
                let span: Span = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
                compact(input, &span).ok_or_else(|| "grid citation identity missing".into())
            })
            .collect::<Result<Vec<_>, String>>()?;
        out["citation_refs"] = json!(refs);
    } else if name == "read_source" {
        let source_id = out["source_id"]
            .as_str()
            .ok_or("text source missing")?
            .to_owned();
        let reference = |value: &Value| -> Result<Value, String> {
            let span: Span = serde_json::from_value(
                json!({"source_id":source_id,"start":value["start"],"end":value["end"]}),
            )
            .map_err(|e| e.to_string())?;
            if span.start == span.end {
                return Ok(Value::Null);
            }
            compact(input, &span).ok_or_else(|| "text citation identity missing".into())
        };
        out["citation_ref"] = reference(out)?;
        for line in out["line_spans"]
            .as_array_mut()
            .ok_or("text line spans missing")?
        {
            line["citation_ref"] = reference(line)?;
        }
    }
    Ok(())
}
