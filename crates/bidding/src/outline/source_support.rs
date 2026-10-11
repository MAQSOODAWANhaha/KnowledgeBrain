//! Pure native structural selection. Support never acquires body ownership.
use super::evidence::{self, EvidenceRef};
use crate::analysis::{FrozenInput, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupportReason {
    MergedRow,
    Header,
    ContinuationHeader,
    ConfirmedRelation { relation_id: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SupportRange {
    pub evidence: EvidenceRef,
    pub reason: SupportReason,
}

fn bounds(cell: &Value) -> Result<(usize, usize, usize, usize), String> {
    let row = cell["row"].as_u64().ok_or("native cell row missing")? as usize;
    let col = cell["column"]
        .as_u64()
        .ok_or("native cell column missing")? as usize;
    let rs = cell
        .get("row_span")
        .map_or(Some(1), Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or("native row span missing")? as usize;
    let cs = cell
        .get("column_span")
        .or_else(|| cell.get("col_span"))
        .map_or(Some(1), Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or("native column span missing")? as usize;
    Ok((
        row,
        row.checked_add(rs).ok_or("row span overflow")?,
        col,
        col.checked_add(cs).ok_or("column span overflow")?,
    ))
}
fn cells<'a>(input: &'a FrozenInput, table: &str) -> Result<&'a Vec<Value>, String> {
    input
        .structured_forms
        .iter()
        .find(|f| f["form_definition_revision_id"] == table)
        .and_then(|f| f["definition"]["cells"].as_array())
        .ok_or_else(|| "native table missing".into())
}
fn cell_ref(table: &str, cell: &Value, digest: &str) -> Result<Option<EvidenceRef>, String> {
    let (row, _, col, _) = bounds(cell)?;
    let text = cell["text"].as_str().ok_or("native cell text missing")?;
    Ok((!text.is_empty()).then(|| EvidenceRef::GridCell {
        input_digest: digest.into(),
        table_id: table.into(),
        anchor_row: row,
        anchor_column: col,
        start_byte: 0,
        end_byte: text.len(),
    }))
}
pub(crate) fn row_body(
    input: &FrozenInput,
    table: &str,
    row: usize,
) -> Result<Vec<EvidenceRef>, String> {
    let digest = evidence::input_digest(input)?;
    let mut refs = Vec::new();
    for cell in cells(input, table)? {
        let (a, b, _, _) = bounds(cell)?;
        if a <= row
            && row < b
            && let Some(r) = cell_ref(table, cell, &digest)?
        {
            refs.push(r);
        }
    }
    Ok(refs)
}
pub(crate) fn source_for<'a>(
    input: &'a FrozenInput,
    reference: &EvidenceRef,
) -> Option<&'a Source> {
    let id = match reference {
        EvidenceRef::Text { unit_id, .. } => unit_id.as_str(),
        EvidenceRef::ImageRegion { image_id, .. } => image_id.as_str(),
        EvidenceRef::GridCell { table_id, .. } => input
            .structured_forms
            .iter()
            .find(|f| f["form_definition_revision_id"] == *table_id)?["source_unit_revision_id"]
            .as_str()?,
    };
    input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == id)
}
fn source_refs(
    input: &FrozenInput,
    source: &Source,
    digest: &str,
) -> Result<Vec<EvidenceRef>, String> {
    let mut refs = Vec::new();
    if source.locator["locator_kind"] == "image" {
        if source.locator["image_available"] != true {
            return Err("required image support unavailable".into());
        }
        refs.push(EvidenceRef::ImageRegion {
            input_digest: digest.into(),
            image_id: source.source_unit_revision_id.clone(),
            region: "original".into(),
        });
    } else if !source.text.is_empty() {
        refs.push(EvidenceRef::Text {
            input_digest: digest.into(),
            unit_id: source.source_unit_revision_id.clone(),
            start_byte: 0,
            end_byte: source.text.len(),
        });
    }
    for form in &input.structured_forms {
        if form["source_unit_revision_id"] == source.source_unit_revision_id {
            let table = form["form_definition_revision_id"]
                .as_str()
                .ok_or("table identity missing")?;
            for cell in cells(input, table)? {
                if let Some(r) = cell_ref(table, cell, digest)? {
                    refs.push(r);
                }
            }
        }
    }
    Ok(refs)
}
/// The result is derived afresh; callers retain owned references separately.
fn select_impl(
    input: &FrozenInput,
    owned: &[EvidenceRef],
    include_relations: bool,
) -> Result<Vec<SupportRange>, String> {
    if owned.is_empty() {
        return Ok(vec![]);
    }
    evidence::resolve_evidence(input, owned)?;
    let digest = evidence::input_digest(input)?;
    let mut result = Vec::<SupportRange>::new();
    let mut add = |reference: EvidenceRef, reason: SupportReason| {
        let mut coverage = owned.to_vec();
        coverage.extend(result.iter().map(|r| r.evidence.clone()));
        for reference in uncovered(&reference, &coverage) {
            result.push(SupportRange {
                evidence: reference,
                reason: reason.clone(),
            });
        }
    };
    let chains = super::chapters::attachment_chains(input);
    for reference in owned {
        if let EvidenceRef::GridCell {
            table_id,
            anchor_row,
            anchor_column,
            ..
        } = reference
        {
            let all = cells(input, table_id)?;
            let owner = all
                .iter()
                .find(|c| {
                    c["row"].as_u64() == Some(*anchor_row as u64)
                        && c["column"].as_u64() == Some(*anchor_column as u64)
                })
                .ok_or("owned cell missing")?;
            let (r0, r1, c0, c1) = bounds(owner)?;
            for cell in all {
                let (a, b, c, d) = bounds(cell)?;
                let role = cell["header_role"].as_str().unwrap_or("none");
                let merged = a < r0 && b > r0;
                let header = (role == "column_header" && a <= r0 && c < c1 && c0 < d)
                    || (role == "row_header" && c <= c0 && a < r1 && r0 < b);
                if (merged || header)
                    && let Some(r) = cell_ref(table_id, cell, &digest)?
                {
                    add(
                        r,
                        if header {
                            SupportReason::Header
                        } else {
                            SupportReason::MergedRow
                        },
                    );
                }
            }
            // A native continuation borrows the nearest prior header only
            // when its own table does not carry an applicable column header.
            let local_header = all.iter().any(|cell| {
                cell["header_role"] == "column_header"
                    && bounds(cell).is_ok_and(|(a, _, c, d)| a <= r0 && c < c1 && c0 < d)
            });
            if !local_header
                && let Some(chain) = chains.iter().find(|chain| chain.contains(table_id))
            {
                let index = chain.iter().position(|id| id == table_id).unwrap();
                for previous in chain[..index].iter().rev() {
                    let mut found = false;
                    for cell in cells(input, previous)? {
                        let (_, _, c, d) = bounds(cell)?;
                        if cell["header_role"] == "column_header"
                            && c < c1
                            && c0 < d
                            && let Some(r) = cell_ref(previous, cell, &digest)?
                        {
                            add(r, SupportReason::ContinuationHeader);
                            found = true;
                        }
                    }
                    if found {
                        break;
                    }
                }
            }
        }
    }
    if !include_relations {
        return Ok(result);
    }
    use crate::analysis::source_manifest::{DocumentRelationKind, DocumentRelationStatus};
    for relation in &input.document_relations {
        if relation.kind != DocumentRelationKind::ExplicitReference
            || relation.status != DocumentRelationStatus::Confirmed
            || !relation.required
        {
            continue;
        }
        let Some(to) = &relation.to else {
            continue;
        };
        let (Some(from_unit), Some(to_unit)) = (&relation.from.unit_id, &to.unit_id) else {
            continue;
        };
        for (from, from_unit, target, target_unit) in [(&relation.from, from_unit, to, to_unit)] {
            if !owned
                .iter()
                .filter_map(|r| source_for(input, r))
                .any(|s| s.document_id == from.document_id && s.locator["unit_id"] == *from_unit)
            {
                continue;
            }
            let source = input
                .source_units
                .iter()
                .find(|s| {
                    s.document_id == target.document_id && s.locator["unit_id"] == *target_unit
                })
                .ok_or("required relation endpoint missing")?;
            for reference in source_refs(input, source, &digest)? {
                add(
                    reference,
                    SupportReason::ConfirmedRelation {
                        relation_id: relation.id.clone(),
                    },
                );
            }
        }
    }
    Ok(result)
}

/// Subtract only same-carrier confirmed intervals; never bridge unread gaps.
pub(crate) fn uncovered(reference: &EvidenceRef, coverage: &[EvidenceRef]) -> Vec<EvidenceRef> {
    let Some((start, end)) = reference.range() else {
        return if evidence::covered_by_union(reference, coverage) {
            vec![]
        } else {
            vec![reference.clone()]
        };
    };
    let mut intervals: Vec<_> = coverage
        .iter()
        .filter(|r| reference.same_carrier(r))
        .filter_map(EvidenceRef::range)
        .collect();
    intervals.sort_unstable();
    let mut cursor = start;
    let mut ranges = Vec::new();
    for (a, b) in intervals {
        if b <= cursor || a >= end {
            continue;
        }
        if a > cursor {
            ranges.push((cursor, a.min(end)));
        }
        cursor = cursor.max(b).min(end);
    }
    if cursor < end {
        ranges.push((cursor, end));
    }
    ranges
        .into_iter()
        .map(|(a, b)| {
            let mut r = reference.clone();
            match &mut r {
                EvidenceRef::Text {
                    start_byte,
                    end_byte,
                    ..
                }
                | EvidenceRef::GridCell {
                    start_byte,
                    end_byte,
                    ..
                } => {
                    *start_byte = a;
                    *end_byte = b
                }
                _ => {}
            }
            r
        })
        .collect()
}

pub(crate) fn select(
    input: &FrozenInput,
    owned: &[EvidenceRef],
) -> Result<Vec<SupportRange>, String> {
    select_impl(input, owned, true)
}
pub(crate) fn select_structure(
    input: &FrozenInput,
    owned: &[EvidenceRef],
) -> Result<Vec<SupportRange>, String> {
    select_impl(input, owned, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn required_relations_are_exact_directed_and_not_semantic_candidates() {
        let mut input = crate::outline::tests::input();
        input.source_units[0].locator["unit_id"] = json!("a");
        let mut target = input.source_units[0].clone();
        target.source_unit_revision_id = "target".into();
        target.document_id = "other".into();
        target.locator["unit_id"] = json!("b");
        target.text = "Support".into();
        input.source_units.push(target);
        input.document_relations=vec![serde_json::from_value(json!({"id":"r","from":{"document_id":"doc","unit_id":"a"},"to":{"document_id":"other","unit_id":"b"},"kind":"explicit_reference","status":"confirmed","required":true,"basis":"reference","locator":{}})).unwrap()];
        let owned = crate::outline::tests::evidence(&input);
        let support = select(&input, &[owned]).unwrap();
        assert_eq!(support.len(), 1);
        assert!(
            select(&input, &[support[0].evidence.clone()])
                .unwrap()
                .is_empty(),
            "target does not require reference source"
        );
        for mode in 0..3 {
            let mut candidate = input.clone();
            match mode {
                0 => candidate.document_relations[0].required = false,
                1 => candidate.document_relations[0].from.unit_id = None,
                _ => {
                    candidate.document_relations[0].status =
                        crate::analysis::source_manifest::DocumentRelationStatus::Unconfirmed
                }
            };
            assert!(
                select(&candidate, &[crate::outline::tests::evidence(&candidate)])
                    .unwrap()
                    .is_empty()
            );
        }
    }
    #[test]
    fn native_continuation_uses_previous_header_without_prose_guessing() {
        let mut input = crate::outline::tests::input();
        input.source_units[0].text.clear();
        input.source_units[0].locator =
            json!({"heading_path":"格式","locator_kind":"page_table","completeness":"complete"});
        let mut tail = input.source_units[0].clone();
        tail.source_unit_revision_id = "tail".into();
        tail.ordinal = 1;
        tail.locator["heading_path"] = json!("");
        input.source_units.push(tail);
        input.structured_forms = vec![
            json!({"form_definition_revision_id":"head","source_unit_revision_id":"source","definition":{"row_count":2,"column_count":2,"cells":[{"row":0,"column":0,"text":"名称","header_role":"column_header"},{"row":0,"column":1,"text":"内容","header_role":"column_header"},{"row":1,"column":0,"text":"","header_role":"none"},{"row":1,"column":1,"text":"","header_role":"none"}]}}),
            json!({"form_definition_revision_id":"tail","source_unit_revision_id":"tail","definition":{"row_count":2,"column_count":2,"cells":[{"row":0,"column":0,"text":"","header_role":"none"},{"row":0,"column":1,"text":"","header_role":"none"},{"row":1,"column":0,"text":"Submit","header_role":"none"},{"row":1,"column":1,"text":"","header_role":"none"}]}}),
        ];
        let reference = EvidenceRef::GridCell {
            input_digest: evidence::input_digest(&input).unwrap(),
            table_id: "tail".into(),
            anchor_row: 1,
            anchor_column: 0,
            start_byte: 0,
            end_byte: 6,
        };
        assert_eq!(
            super::super::chapters::attachment_chains(&input),
            vec![vec!["head".to_string(), "tail".to_string()]]
        );
        let support = select(&input, &[reference]).unwrap();
        assert_eq!(support.len(), 1);
        assert_eq!(support[0].reason, SupportReason::ContinuationHeader);
    }
    #[test]
    fn minimal_headers_row_span_and_optional_prose() {
        let mut input = crate::outline::tests::input();
        input.structured_forms = vec![
            json!({"form_definition_revision_id":"t","source_unit_revision_id":"source","definition":{"row_count":3,"column_count":3,"cells":[
                {"row":0,"column":0,"row_span":3,"column_span":1,"header_role":"row_header","text":"row"},
                {"row":0,"column":1,"column_span":1,"header_role":"column_header","text":"yes"},
                {"row":0,"column":2,"header_role":"column_header","text":"other"},
                {"row":1,"column":1,"text":"body"},{"row":1,"column":2,"text":"peer"},
                {"row":2,"column":1,"text":"tail"},{"row":2,"column":2,"text":"peer2"}
            ]}}),
        ];
        let digest = evidence::input_digest(&input).unwrap();
        let owned = EvidenceRef::GridCell {
            input_digest: digest,
            table_id: "t".into(),
            anchor_row: 1,
            anchor_column: 1,
            start_byte: 0,
            end_byte: 4,
        };
        let support = select(&input, std::slice::from_ref(&owned)).unwrap();
        assert_eq!(support.len(), 2);
        assert!(support.iter().all(|r| matches!(
            &r.evidence,
            EvidenceRef::GridCell {
                anchor_row: 0,
                anchor_column: 0 | 1,
                ..
            }
        )));
        assert_eq!(row_body(&input, "t", 1).unwrap().len(), 3);
        assert!(
            select(&input, &[crate::outline::tests::evidence(&input)])
                .unwrap()
                .is_empty()
        );
        let sliced_header = EvidenceRef::GridCell {
            input_digest: evidence::input_digest(&input).unwrap(),
            table_id: "t".into(),
            anchor_row: 0,
            anchor_column: 1,
            start_byte: 0,
            end_byte: 1,
        };
        let selected = select(&input, &[sliced_header]).unwrap();
        assert!(selected.iter().any(|s| matches!(
            &s.evidence,
            EvidenceRef::GridCell {
                anchor_row: 0,
                anchor_column: 1,
                start_byte: 1,
                end_byte: 3,
                ..
            }
        )));
        let partial = match owned.clone() {
            EvidenceRef::GridCell {
                input_digest,
                table_id,
                anchor_row,
                anchor_column,
                ..
            } => EvidenceRef::GridCell {
                input_digest,
                table_id,
                anchor_row,
                anchor_column,
                start_byte: 0,
                end_byte: 2,
            },
            _ => unreachable!(),
        };
        assert_eq!(uncovered(&owned, &[partial])[0].range(), Some((2, 4)));
    }
}
