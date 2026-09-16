//! Host projections for field-grounds and compiler blank effects.
//! These are comparison inputs, never clean conclusions or extra reading receipts.
use super::*;
use crate::template_grid::blank_cell_text;
use serde_json::{Value, json};

fn excerpt(input: &FrozenInput, span: &Span) -> Option<String> {
    if span.view_id.is_some() {
        return None;
    }
    if let Some(cell) = &span.grid_cell {
        let form = input.structured_forms.iter().find(|form| {
            form["form_definition_revision_id"] == cell.form_id
        })?;
        let cells = form["definition"]["cells"].as_array()?;
        let text = cells.iter().find(|entry| {
            entry["row"] == cell.row && entry["column"] == cell.column
        })?["text"]
            .as_str()?;
        return Some(text.to_owned());
    }
    let source = input
        .source_units
        .iter()
        .find(|source| source.source_unit_revision_id == span.source_id)?;
    source.text.get(span.start..span.end).map(str::to_owned)
}

fn grounds_row(input: &FrozenInput, span: &Span) -> Value {
    json!({
        "source_id": span.source_id,
        "start": span.start,
        "end": span.end,
        "cited_text": excerpt(input, span),
    })
}

fn field_check(input: &FrozenInput, path: &str, field_value: &str, grounds: &[Span]) -> Value {
    json!({
        "path": path,
        "field_value": field_value,
        "grounds": grounds.iter().map(|span| grounds_row(input, span)).collect::<Vec<_>>(),
        "instruction": "Compare field_value only with cited_text of these grounds. A correct statement elsewhere in the packet does not prove this citation."
    })
}

fn collect_record_checks(input: &FrozenInput, analysis: &Analysis, record: &Record) -> Vec<Value> {
    let mut checks = Vec::new();
    if !record.sources.is_empty() {
        checks.push(field_check(
            input,
            "/sources",
            &record.id,
            &record.sources,
        ));
    }
    match &record.data {
        RecordData::Fact { name, value, scope } => {
            checks.push(field_check(
                input,
                "/data/value",
                &format!("{name}={value}@{scope}"),
                &record.sources,
            ));
        }
        RecordData::Rule {
            text,
            applicability,
            ..
        } => {
            checks.push(field_check(input, "/data/text", text, &record.sources));
            checks.push(field_check(
                input,
                "/data/applicability/condition",
                &applicability.condition,
                &applicability.grounds,
            ));
        }
        RecordData::Requirement {
            text,
            compliance,
            applicability,
            response,
            scoring_rule,
            proofs,
            criteria,
            ..
        } => {
            checks.push(field_check(input, "/data/text", text, &record.sources));
            checks.push(field_check(
                input,
                "/data/applicability/condition",
                &applicability.condition,
                &applicability.grounds,
            ));
            if let Some(rule) = scoring_rule {
                checks.push(field_check(
                    input,
                    "/data/scoring_rule",
                    rule,
                    &record.sources,
                ));
            }
            for (index, claim) in compliance.iter().enumerate() {
                checks.push(field_check(
                    input,
                    &format!("/data/compliance/{index}/condition"),
                    &claim.condition,
                    &claim.grounds,
                ));
            }
            for (index, need) in response.iter().enumerate() {
                checks.push(field_check(
                    input,
                    &format!("/data/response/{index}/description"),
                    &need.description,
                    &need.grounds,
                ));
            }
            for (index, proof) in proofs.iter().enumerate() {
                checks.push(field_check(
                    input,
                    &format!("/data/proofs/{index}/description"),
                    &proof.description,
                    &proof.grounds,
                ));
            }
            for (index, criterion) in criteria.iter().enumerate() {
                checks.push(field_check(
                    input,
                    &format!("/data/criteria/{index}/value"),
                    &format!("{} {} {}", criterion.operator, criterion.value, criterion.unit),
                    &criterion.grounds,
                ));
            }
        }
        RecordData::Template { applicability, .. } => {
            checks.push(field_check(
                input,
                "/data/applicability/condition",
                &applicability.condition,
                &applicability.grounds,
            ));
        }
        RecordData::Unresolved {
            problem,
            candidates,
            ..
        } => {
            let present_records: Vec<_> = candidates
                .iter()
                .filter(|id| analysis.records.contains_key(*id))
                .cloned()
                .collect();
            checks.push(json!({
                "path": "/data",
                "field_value": problem,
                "grounds": [],
                "listed_candidates": candidates,
                "present_records": present_records,
                "instruction": "If listed targets already exist as current records, this is unfinished graph work, not source absence."
            }));
        }
    }
    checks
}

fn collect_relation_checks(input: &FrozenInput, relation: &Relation) -> Vec<Value> {
    vec![field_check(
        input,
        "/explanation",
        &relation.explanation,
        &relation.grounds,
    )]
}

fn text_blank_effect(original: &str, role: RegionRole) -> (String, String) {
    if role == RegionRole::BidderBlank {
        (original.to_owned(), String::new())
    } else {
        (String::new(), original.to_owned())
    }
}

fn collect_blank_effects(input: &FrozenInput, record: &Record) -> Vec<Value> {
    let RecordData::Template { regions, .. } = &record.data else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    for (index, region) in regions.iter().enumerate() {
        if let Some(form_id) = &region.form_id {
            let Some(form) = input.structured_forms.iter().find(|form| {
                form["form_definition_revision_id"] == *form_id
            }) else {
                continue;
            };
            let Some(cells) = form["definition"]["cells"].as_array() else {
                continue;
            };
            for cell in &region.cells {
                let original = cells
                    .iter()
                    .find(|entry| entry["row"] == cell.row && entry["column"] == cell.column)
                    .and_then(|entry| entry["text"].as_str())
                    .unwrap_or("");
                let retained = if region.role == RegionRole::BidderBlank {
                    if region.blank_ranges.is_empty() {
                        String::new()
                    } else {
                        blank_cell_text(original, cell.row, cell.column, &region.blank_ranges)
                            .unwrap_or_default()
                    }
                } else {
                    original.to_owned()
                };
                let removed: String = if region.role == RegionRole::BidderBlank {
                    if region.blank_ranges.is_empty() {
                        original.to_owned()
                    } else {
                        original
                            .chars()
                            .zip(retained.chars().chain(std::iter::repeat('\0')))
                            .filter(|(a, b)| *a != *b)
                            .map(|(a, _)| a)
                            .collect()
                    }
                } else {
                    String::new()
                };
                effects.push(json!({
                    "path": format!("/data/regions/{index}"),
                    "role": region.role,
                    "instruction": region.instruction,
                    "form_id": form_id,
                    "row": cell.row,
                    "column": cell.column,
                    "original": original,
                    "removed": removed,
                    "retained": retained,
                    "instruction_note": "instruction never overrides role or ranges. Compare removed/retained original bytes with the compiler blank policy."
                }));
            }
            continue;
        }
        let Some(original) = excerpt(input, &region.source) else {
            continue;
        };
        let (removed, retained) = text_blank_effect(&original, region.role);
        effects.push(json!({
            "path": format!("/data/regions/{index}"),
            "role": region.role,
            "instruction": region.instruction,
            "original": original,
            "removed": removed,
            "retained": retained,
            "instruction_note": "instruction never overrides role or ranges. A bidder_blank text region removes these original bytes from the compiled DOCX."
        }));
    }
    effects
}

/// Attach host comparison projections beside a candidate wrapper.
/// Digest of `value` is unchanged; projections are not reading receipts.
pub fn attach(input: &FrozenInput, analysis: &Analysis, key: &str, mut wrapper: Value) -> Value {
    if let Some(id) = key.strip_prefix("record:")
        && let Ok(record) = serde_json::from_value::<Record>(wrapper["value"].clone())
        && record.id == id
    {
        let checks = collect_record_checks(input, analysis, &record);
        if !checks.is_empty() {
            wrapper["field_ground_checks"] = json!(checks);
        }
        let blanks = collect_blank_effects(input, &record);
        if !blanks.is_empty() {
            wrapper["blank_effects"] = json!(blanks);
        }
    }
    if key.starts_with("relation:")
        && let Ok(relation) = serde_json::from_value::<Relation>(wrapper["value"].clone())
    {
        wrapper["field_ground_checks"] = json!(collect_relation_checks(input, &relation));
    }
    wrapper
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(source: &str, start: usize, end: usize) -> Span {
        Span {
            source_id: source.into(),
            start,
            end,
            view_id: None,
            grid_cell: None,
        }
    }

    fn input(units: Vec<(&str, &str)>) -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            decisions: vec![],
            structured_forms: vec![],
            source_units: units
                .into_iter()
                .enumerate()
                .map(|(ordinal, (id, text))| Source {
                    source_unit_revision_id: id.into(),
                    document_id: "document".into(),
                    text: text.into(),
                    locator: json!({}),
                    ordinal,
                })
                .collect(),
        }
    }

    fn applicability(grounds: Vec<Span>) -> Applicability {
        Applicability {
            state: ApplicabilityState::Applicable,
            condition: String::new(),
            scope: "project".into(),
            grounds,
        }
    }

    #[test]
    fn field_grounds_juxtapose_cited_bytes_not_a_neighbor_clause() {
        let control = "满足且证明完整得该条12.5分，否则该条不得分";
        let indicators = "指标与证明要求，不含分值";
        let frozen = input(vec![("indicators", indicators), ("control", control)]);
        let record = Record {
            id: "req".into(),
            sources: vec![span("indicators", 0, indicators.len())],
            data: RecordData::Requirement {
                text: "评分项".into(),
                categories: vec![Category::Evaluation],
                strength: Strength::Mandatory,
                compliance: vec![ComplianceClaim {
                    policy: Compliance::Scored,
                    condition: control.into(),
                    grounds: vec![span("indicators", 0, indicators.len())],
                }],
                applicability: applicability(vec![span("indicators", 0, indicators.len())]),
                response: vec![],
                scoring_rule: Some("见控制条款".into()),
                proofs: vec![],
                criteria: vec![],
            },
        };
        let checks = collect_record_checks(&frozen, &Analysis::default(), &record);
        let scoring = checks
            .iter()
            .find(|row| row["path"] == "/data/compliance/0/condition")
            .unwrap();
        assert_eq!(scoring["field_value"], control);
        assert_eq!(scoring["grounds"][0]["cited_text"], indicators);
        assert_ne!(scoring["field_value"], scoring["grounds"][0]["cited_text"]);
        assert!(!indicators.contains("12.5"));
        assert!(control.contains("12.5"));
    }

    #[test]
    fn whole_line_bidder_blank_projects_labels_as_removed() {
        let original = "投标人名称：________________";
        let frozen = input(vec![("source", original)]);
        let record = Record {
            id: "tpl".into(),
            sources: vec![span("source", 0, original.len())],
            data: RecordData::Template {
                label: "乙".into(),
                title: "乙".into(),
                parent: None,
                order: None,
                purpose: "附表".into(),
                applicability: applicability(vec![span("source", 0, original.len())]),
                regions: vec![TemplateRegion {
                    source: span("source", 0, original.len()),
                    role: RegionRole::BidderBlank,
                    form_id: None,
                    cells: vec![],
                    blank_ranges: vec![],
                    instruction: "keep labels 投标人名称 and blank the underscores".into(),
                }],
            },
        };
        let effects = collect_blank_effects(&frozen, &record);
        assert_eq!(effects[0]["original"], original);
        assert_eq!(effects[0]["removed"], original);
        assert_eq!(effects[0]["retained"], "");
        assert_eq!(effects[0]["role"], "bidder_blank");
        assert!(effects[0]["instruction"].as_str().unwrap().contains("keep labels"));
    }

    #[test]
    fn split_label_and_underscore_retains_fixed_text() {
        let label = "投标人名称：";
        let blank = "________________";
        let original = format!("{label}{blank}");
        let frozen = input(vec![("source", &original)]);
        let record = Record {
            id: "tpl".into(),
            sources: vec![span("source", 0, original.len())],
            data: RecordData::Template {
                label: "乙".into(),
                title: "乙".into(),
                parent: None,
                order: None,
                purpose: "附表".into(),
                applicability: applicability(vec![span("source", 0, original.len())]),
                regions: vec![
                    TemplateRegion {
                        source: span("source", 0, label.len()),
                        role: RegionRole::FixedText,
                        form_id: None,
                        cells: vec![],
                        blank_ranges: vec![],
                        instruction: String::new(),
                    },
                    TemplateRegion {
                        source: span("source", label.len(), original.len()),
                        role: RegionRole::BidderBlank,
                        form_id: None,
                        cells: vec![],
                        blank_ranges: vec![],
                        instruction: String::new(),
                    },
                ],
            },
        };
        let effects = collect_blank_effects(&frozen, &record);
        assert_eq!(effects[0]["retained"], label);
        assert_eq!(effects[0]["removed"], "");
        assert_eq!(effects[1]["removed"], blank);
        assert_eq!(effects[1]["retained"], "");
    }
}
