//! Precise, revision-bound relation endpoints for appendix and output mapping.
use super::*;

pub fn form_dimensions(definition: &Value) -> Option<(usize, usize)> {
    let rows = definition["row_count"].as_u64()? as usize;
    let columns = definition["column_count"].as_u64()? as usize;
    (rows > 0 && columns > 0).then_some((rows, columns))
}

pub fn form_total(definition: &Value) -> Option<usize> {
    let (rows, columns) = form_dimensions(definition)?;
    rows.checked_mul(columns)
}

pub fn dense_index(row: usize, column: usize, column_count: usize) -> Option<usize> {
    row.checked_mul(column_count)?.checked_add(column)
}

pub fn sparse_cell(definition: &Value, row: usize, column: usize) -> Option<&Value> {
    definition["cells"].as_array()?.iter().find(|cell| {
        cell["row"].as_u64() == Some(row as u64) && cell["column"].as_u64() == Some(column as u64)
    })
}

/// Schema 3 stores only anchors. Covered merge slots are omitted.
pub fn grid_cell_is_anchor(definition: &Value, row: usize, column: usize) -> bool {
    let Some((rows, columns)) = form_dimensions(definition) else {
        return false;
    };
    if row >= rows || column >= columns {
        return false;
    }
    let Some(cells) = definition["cells"].as_array() else {
        return false;
    };
    if !cells.iter().any(|c| {
        c["row"].as_u64() == Some(row as u64) && c["column"].as_u64() == Some(column as u64)
    }) {
        return false;
    }
    !cells.iter().any(|c| {
        let (Some(r), Some(col), Some(rs), Some(cs)) = (
            c["row"].as_u64(),
            c["column"].as_u64(),
            c["row_span"].as_u64(),
            c["col_span"].as_u64(),
        ) else {
            return false;
        };
        (row as u64, column as u64) != (r, col)
            && row as u64 >= r
            && column as u64 >= col
            && (row as u64 - r) < rs
            && (column as u64 - col) < cs
    })
}

pub fn validate_target(
    input: &FrozenInput,
    record: &Record,
    target: &RelationTarget,
) -> Result<(), String> {
    let valid = match (&record.data, target) {
        (_, RelationTarget::Record) => true,
        (RecordData::Template { regions, .. }, RelationTarget::TemplateRegion { index }) => {
            regions.get(*index).is_some()
        }
        (
            RecordData::Template { regions, .. },
            RelationTarget::TemplateCell {
                form_id,
                row,
                column,
            },
        ) => {
            // Checking the global grid alone would permit linking a cell owned
            // by another template (even on the same page).
            regions.iter().any(|region| {
                region.form_id.as_ref() == Some(form_id)
                    && region
                        .cells
                        .iter()
                        .any(|cell| cell.row == *row && cell.column == *column)
            }) && input.structured_forms.iter().any(|form| {
                form["form_definition_revision_id"] == *form_id
                    && grid_cell_is_anchor(&form["definition"], *row, *column)
            })
        }
        (RecordData::Requirement { response, .. }, RelationTarget::Response { index }) => {
            response.get(*index).is_some()
        }
        (RecordData::Requirement { proofs, .. }, RelationTarget::Proof { index }) => {
            proofs.get(*index).is_some()
        }
        (RecordData::Requirement { criteria, .. }, RelationTarget::Criterion { index }) => {
            criteria.get(*index).is_some()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("relation target is not a location in this record".into())
    }
}

pub fn validate_endpoints(
    input: &FrozenInput,
    analysis: &Analysis,
    relation: &Relation,
) -> Result<(), String> {
    for (id, target, expected, field) in [
        (
            &relation.from,
            &relation.from_target,
            &relation.from_record_sha256,
            "from",
        ),
        (
            &relation.to,
            &relation.to_target,
            &relation.to_record_sha256,
            "to",
        ),
    ] {
        let record = analysis.records.get(id).ok_or_else(|| {
            tools::field_error(&format!("/{field}"), "relation endpoint record missing")
        })?;
        if digest(record)? != *expected {
            return Err(tools::field_error(
                &format!("/{field}"),
                "relation endpoint changed; inspect and re-establish the relation",
            ));
        }
        let target_path = format!("/{field}_target");
        validate_target(input, record, target)
            .map_err(|error| tools::field_error(&target_path, error))?;
        if matches!(
            relation.kind,
            RelationKind::SameValue | RelationKind::Aggregates
        ) && relation.state != RelationState::Unresolved
            && let (RecordData::Template { regions, .. }, RelationTarget::TemplateRegion { index }) =
                (&record.data, target)
            && regions[*index].form_id.is_some()
            && regions[*index].cells.len() != 1
        {
            return Err(tools::field_error(
                &target_path,
                "value relation cannot target a multi-cell region; select the actual field",
            ));
        }
        if matches!(
            relation.kind,
            RelationKind::SameValue | RelationKind::Aggregates
        ) && relation.state != RelationState::Unresolved
            && (matches!(target, RelationTarget::Record)
                && !matches!(record.data, RecordData::Fact { .. })
                || matches!(
                    target,
                    RelationTarget::Response { .. } | RelationTarget::Proof { .. }
                ))
        {
            return Err(tools::field_error(
                &target_path,
                "value relation needs specific value fields; unresolved targets must stay unresolved",
            ));
        }
    }
    let from = canonical_target(&analysis.records[&relation.from], &relation.from_target);
    let to = canonical_target(&analysis.records[&relation.to], &relation.to_target);
    if from == to
        && (relation.from == relation.to || matches!(from, RelationTarget::TemplateCell { .. }))
    {
        return Err(tools::field_error(
            "/to_target",
            "relation endpoints must identify distinct locations",
        ));
    }
    if matches!(
        relation.kind,
        RelationKind::Contains | RelationKind::RequiresTemplate
    ) && relation.to_target != RelationTarget::Record
    {
        return Err(tools::field_error(
            "/to_target",
            "template containment or selection must target the whole template",
        ));
    }
    if relation.kind == RelationKind::Contains && relation.from_target != RelationTarget::Record {
        return Err(tools::field_error(
            "/from_target",
            "template containment must start at the parent template",
        ));
    }
    Ok(())
}

fn canonical_target(record: &Record, target: &RelationTarget) -> RelationTarget {
    if let (RecordData::Template { regions, .. }, RelationTarget::TemplateRegion { index }) =
        (&record.data, target)
    {
        let region = &regions[*index];
        if let (Some(form_id), [cell]) = (&region.form_id, region.cells.as_slice()) {
            return RelationTarget::TemplateCell {
                form_id: form_id.clone(),
                row: cell.row,
                column: cell.column,
            };
        }
    }
    target.clone()
}
