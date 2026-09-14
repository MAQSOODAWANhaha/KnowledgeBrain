use super::*;

fn frozen_grid() -> FrozenInput {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["schema_version"] = json!(3);
    input
}

#[test]
fn frozen_input_accepts_sparse_merge_anchors_without_changing_source() {
    let mut input = frozen_grid();
    input.structured_forms[0]["definition"]["cells"][2]["text"] = json!("");
    let original = serde_json::to_value(&input).unwrap();
    tools::validate_input(&input).unwrap();
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    input.structured_forms[0]["definition"]
        .as_object_mut()
        .unwrap()
        .remove("widths_mm");
    tools::validate_input(&input).unwrap();
}

#[test]
fn frozen_input_rejects_unavailable_or_incompatible_grids_at_entry() {
    for definition in [
        json!({"cells":[]}),
        json!({"schema_version":3,"kind":"grid","cells":[]}),
        json!({"schema_version":3,"kind":"grid","row_count":0,"column_count":2,"cells":[]}),
        json!({"schema_version":3,"kind":"grid","row_count":1,"column_count":1,"cells":[]}),
    ] {
        let mut input = frozen_grid();
        input.structured_forms[0]["definition"] = definition.clone();
        assert!(tools::validate_input(&input).is_err(), "{definition}");
    }
    for (field, value) in [
        ("schema_version", json!(1)),
        ("schema_version", json!(2)),
        ("schema_version", Value::Null),
        ("kind", json!("table")),
        ("row_count", json!(-1)),
        ("row_count", json!(u64::from(u32::MAX) + 1)),
        ("cells", json!({})),
        ("widths_mm", json!([40])),
        ("widths_mm", json!([40, 0])),
    ] {
        let mut input = frozen_grid();
        input.structured_forms[0]["definition"][field] = value.clone();
        assert!(tools::validate_input(&input).is_err(), "{field}: {value}");
    }
}

#[test]
fn frozen_input_rejects_missing_duplicate_or_overlapping_source_cells() {
    let input = frozen_grid();
    let cells = input.structured_forms[0]["definition"]["cells"]
        .as_array()
        .unwrap();
    let mut missing = cells.clone();
    missing.pop();
    let mut duplicate = cells.clone();
    let mut conflicting = cells[1].clone();
    conflicting["text"] = json!("must not be hidden by an earlier anchor");
    duplicate.push(conflicting);
    let mut overlapping = cells.clone();
    overlapping.push(
        json!({"row":0,"column":1,"row_span":1,"col_span":1,"text":"covered source content"}),
    );
    for invalid in [missing, duplicate, overlapping] {
        let mut input = frozen_grid();
        input.structured_forms[0]["definition"]["cells"] = json!(invalid);
        assert!(tools::validate_input(&input).is_err());
    }
}

#[test]
fn frozen_input_rejects_invalid_cell_shape_before_reading_receipts() {
    for (field, value) in [
        ("row", json!(2)),
        ("column", json!(2)),
        ("row_span", json!(0)),
        ("row_span", json!(u32::MAX)),
        ("col_span", json!(3)),
        ("col_span", Value::Null),
        ("text", json!(42)),
    ] {
        let mut input = frozen_grid();
        input.structured_forms[0]["definition"]["cells"][1][field] = value.clone();
        assert!(tools::validate_input(&input).is_err(), "{field}: {value}");
    }
}
