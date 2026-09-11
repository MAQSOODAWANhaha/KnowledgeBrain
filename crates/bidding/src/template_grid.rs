//! Deterministic source-grid rendering shared by DOCX template compilers.
//! Geometry and cell policies come from validated frozen inputs.

use crate::content_block::{BlockContent, Inline, RichNode, SignatureKind, TableCell};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Reviewed bidder-input bytes within one frozen grid anchor's original text.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CellTextRange {
    pub row: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}

/// Remove only explicitly selected UTF-8 ranges; preserve all other source bytes.
pub fn blank_cell_text(
    source: &str,
    row: usize,
    column: usize,
    ranges: &[CellTextRange],
) -> Result<String, String> {
    let mut selected: Vec<_> = ranges
        .iter()
        .filter(|r| r.row == row && r.column == column)
        .collect();
    selected.sort_by_key(|r| r.start);
    let mut cursor = 0;
    let mut output = String::new();
    for r in selected {
        if r.start >= r.end || r.start < cursor || source.get(r.start..r.end).is_none() {
            return Err("cell blank ranges must be nonoverlapping valid UTF-8 byte ranges".into());
        }
        output.push_str(&source[cursor..r.start]);
        cursor = r.end;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}

fn rich_paragraph(text: &str) -> Vec<RichNode> {
    vec![RichNode::Paragraph {
        content: vec![Inline::Text {
            text: text.to_owned(),
            marks: Vec::new(),
        }],
    }]
}

fn integer(value: &Value, name: &str) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| format!("DOCX_TEMPLATE_GRID_INVALID: {name} is invalid"))
}

/// Compile a frozen grid using only objective geometry and the Agent-authored
/// closed column policy. Covered cells of merged ranges are omitted and the
/// anchor cell receives the exact rowspan/colspan.
pub fn table_block_from_grid(
    definition: &Value,
    column_policies: &[Value],
    header_row_count: usize,
) -> Result<BlockContent, String> {
    if definition.get("kind").and_then(Value::as_str) != Some("grid") {
        return Err("DOCX_TEMPLATE_GRID_INVALID: form is not a grid".into());
    }
    let rows = integer(
        definition
            .get("row_count")
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: row_count missing".to_string())?,
        "row_count",
    )?;
    let columns = integer(
        definition
            .get("column_count")
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: column_count missing".to_string())?,
        "column_count",
    )?;
    if rows == 0 || columns == 0 || header_row_count > rows {
        return Err("DOCX_TEMPLATE_GRID_INVALID: grid dimensions are invalid".into());
    }

    let widths = definition
        .get("widths_mm")
        .and_then(Value::as_array)
        .filter(|items| items.len() == columns)
        .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: objective widths are missing".to_string())?
        .iter()
        .map(|value| {
            value
                .as_f64()
                .filter(|width| width.is_finite() && *width > 0.0)
                .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: objective width is invalid".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !widths.iter().copied().sum::<f64>().is_finite() {
        return Err("DOCX_TEMPLATE_GRID_INVALID: objective widths are not finite".into());
    }
    let mut edges = vec![0.0];
    for width in &widths {
        edges.push(edges.last().copied().unwrap_or(0.0) + width);
    }
    if edges.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err("DOCX_TEMPLATE_GRID_INVALID: objective column edges are not increasing".into());
    }

    let mut roles = BTreeMap::new();
    for policy in column_policies {
        let column = policy
            .get("column")
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: column policy is invalid".to_string())
            .and_then(|value| integer(value, "column policy"))?;
        let role = policy
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: column role is missing".to_string())?;
        if column >= columns
            || !matches!(role, "copy_verbatim" | "blank_bidder_response")
            || roles.insert(column, role).is_some()
        {
            return Err("DOCX_TEMPLATE_GRID_INVALID: column policy is invalid".into());
        }
    }
    if roles.len() != columns {
        return Err("DOCX_TEMPLATE_GRID_INVALID: column policies are incomplete".into());
    }

    let raw_cells = definition
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: grid cells missing".to_string())?;
    let mut texts = BTreeMap::new();
    let mut spans = BTreeMap::new();
    let mut covered = BTreeSet::new();
    let mut occupied = BTreeSet::new();
    for cell in raw_cells {
        let row = cell
            .get("row")
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: cell row missing".to_string())
            .and_then(|value| integer(value, "cell row"))?;
        let column = cell
            .get("column")
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: cell column missing".to_string())
            .and_then(|value| integer(value, "cell column"))?;
        let text = cell
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: cell text is invalid".to_string())?;
        let row_span = integer(
            cell.get("row_span")
                .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: cell row_span missing".to_string())?,
            "cell row_span",
        )?;
        let col_span = integer(
            cell.get("col_span")
                .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: cell col_span missing".to_string())?,
            "cell col_span",
        )?;
        if row >= rows
            || column >= columns
            || row_span == 0
            || col_span == 0
            || row.saturating_add(row_span) > rows
            || column.saturating_add(col_span) > columns
            || texts.insert((row, column), text).is_some()
        {
            return Err("DOCX_TEMPLATE_GRID_INVALID: grid cell is invalid".into());
        }
        spans.insert((row, column), (row_span, col_span));
        if row_span > 1 || col_span > 1 {
            let end_row = row + row_span - 1;
            let end_column = column + col_span - 1;
            if end_row >= header_row_count {
                let first_role = roles.get(&column).copied();
                if (column..=end_column).any(|col| roles.get(&col).copied() != first_role) {
                    return Err(
                        "DOCX_TEMPLATE_GRID_INVALID: merged body range crosses column policies"
                            .into(),
                    );
                }
            }
        }
        for occupied_row in row..row + row_span {
            for occupied_column in column..column + col_span {
                if !occupied.insert((occupied_row, occupied_column)) {
                    return Err("DOCX_TEMPLATE_GRID_INVALID: grid spans overlap".into());
                }
                if (occupied_row, occupied_column) != (row, column) {
                    covered.insert((occupied_row, occupied_column));
                }
            }
        }
    }
    if occupied.len() != rows.saturating_mul(columns) {
        return Err("DOCX_TEMPLATE_GRID_INVALID: grid cells do not tile".into());
    }

    let mut cells = Vec::with_capacity(texts.len());
    for row in 0..rows {
        for column in 0..columns {
            if covered.contains(&(row, column)) {
                continue;
            }
            let source = texts
                .get(&(row, column))
                .ok_or_else(|| "DOCX_TEMPLATE_GRID_INVALID: grid cell missing".to_string())?;
            let text = if row >= header_row_count
                && roles.get(&column).copied() == Some("blank_bidder_response")
            {
                ""
            } else {
                source
            };
            let (rowspan, colspan) = spans.get(&(row, column)).copied().unwrap_or((1, 1));
            cells.push(TableCell {
                row,
                column,
                rowspan,
                colspan,
                content: rich_paragraph(text),
            });
        }
    }

    let block = BlockContent::Table {
        row_count: rows,
        column_count: columns,
        cells,
        widths_mm: widths,
        repeat_header_rows: header_row_count,
    };
    block
        .validate()
        .map_err(|error| format!("DOCX_TEMPLATE_GRID_INVALID: {error}"))?;
    Ok(block)
}

/// Preserve every supplied source segment, in policy order, as a separate
/// paragraph. Empty ranges are invalid; there is no title/body fallback.
pub fn rich_text_block(
    source_segments: &[String],
    notice: Option<&str>,
) -> Result<BlockContent, String> {
    if source_segments.is_empty() && notice.is_none() {
        return Err("DOCX_TEMPLATE_GRID_INVALID: source ranges missing".into());
    }
    let mut nodes = Vec::new();
    for segment in source_segments {
        if segment.is_empty() {
            return Err("DOCX_TEMPLATE_GRID_INVALID: source range is empty".into());
        }
        nodes.extend(rich_paragraph(segment));
    }
    if let Some(notice) = notice {
        if notice.is_empty() {
            return Err("DOCX_TEMPLATE_GRID_INVALID: notice is empty".into());
        }
        nodes.extend(rich_paragraph(notice));
    }
    let block = BlockContent::RichText { nodes };
    block
        .validate()
        .map_err(|error| format!("DOCX_TEMPLATE_GRID_INVALID: {error}"))?;
    Ok(block)
}

pub fn signature_placeholder(kind: &str) -> Result<BlockContent, String> {
    let signature_kind = match kind {
        "seal" => SignatureKind::Seal,
        "date" => SignatureKind::Date,
        "legal_representative_signature" | "authorized_representative_signature" => {
            SignatureKind::Signature
        }
        _ => {
            return Err("DOCX_TEMPLATE_GRID_INVALID: placeholder kind is invalid".into());
        }
    };
    let block = BlockContent::SignaturePlaceholder {
        signature_kind,
        width_mm: 50.0,
        height_mm: 20.0,
        // The label is the closed protocol value, not inferred business text.
        label: kind.to_owned(),
    };
    block
        .validate()
        .map_err(|error| format!("DOCX_TEMPLATE_GRID_INVALID: {error}"))?;
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn objective_grid_policy_preserves_widths_merges_and_blanks_only_body() {
        let definition = json!({
            "kind": "grid",
            "row_count": 2,
            "column_count": 2,
            "widths_mm": [45.0, 135.0],
            "cells": [
                {"row":0,"column":0,"row_span":1,"col_span":2,"text":"列甲"},
                {"row":1,"column":0,"row_span":1,"col_span":1,"text":"值甲"},
                {"row":1,"column":1,"row_span":1,"col_span":1,"text":"值乙"}
            ]
        });
        let policies = vec![
            json!({"column":0,"role":"copy_verbatim"}),
            json!({"column":1,"role":"blank_bidder_response"}),
        ];
        let block = table_block_from_grid(&definition, &policies, 1).expect("grid");
        let BlockContent::Table {
            widths_mm, cells, ..
        } = block
        else {
            panic!("table")
        };
        assert_eq!(widths_mm, vec![45.0, 135.0]);
        assert!(cells.iter().any(|cell| {
            cell.row == 0 && cell.column == 0 && cell.rowspan == 1 && cell.colspan == 2
        }));
        let bidder_cell = cells
            .iter()
            .find(|cell| cell.row == 1 && cell.column == 1)
            .expect("body cell");
        assert!(matches!(
            &bidder_cell.content[0],
            RichNode::Paragraph { content }
                if matches!(&content[0], Inline::Text { text, .. } if text.is_empty())
        ));
    }

    #[test]
    fn missing_geometry_has_no_even_width_fallback() {
        let definition = json!({
            "kind":"grid","row_count":1,"column_count":1,
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"甲"}]
        });
        assert!(
            table_block_from_grid(
                &definition,
                &[json!({"column":0,"role":"copy_verbatim"})],
                1
            )
            .is_err()
        );
    }

    #[test]
    fn missing_or_invalid_widths_fail_closed() {
        let base = json!({"kind":"grid","row_count":1,"column_count":2,
            "widths_mm":[45.0,135.0],
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"甲"},
                     {"row":0,"column":1,"row_span":1,"col_span":1,"text":"乙"}]});
        let policies = [
            json!({"column":0,"role":"copy_verbatim"}),
            json!({"column":1,"role":"copy_verbatim"}),
        ];
        assert!(table_block_from_grid(&base, &policies, 0).is_ok());
        for mutate in ["width_count", "width_zero", "width_missing"] {
            let mut invalid = base.clone();
            match mutate {
                "width_count" => invalid["widths_mm"] = json!([180.0]),
                "width_zero" => invalid["widths_mm"] = json!([0.0, 180.0]),
                "width_missing" => {
                    invalid.as_object_mut().unwrap().remove("widths_mm");
                }
                _ => unreachable!(),
            }
            assert!(
                table_block_from_grid(&invalid, &policies, 0).is_err(),
                "{mutate}"
            );
        }
    }
}
