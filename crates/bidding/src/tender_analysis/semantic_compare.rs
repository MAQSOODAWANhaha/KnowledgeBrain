//! Exact source/field and blank-policy projections, never semantic approval.
//! Only independently delivered evidence in the caller's projection scope can be quoted.
use super::*;
use crate::template_grid::blank_cell_text;
use serde_json::{Value, json};

struct Evidence<'a> {
    input: &'a FrozenInput,
    coverage: &'a Coverage,
    scope: &'a [String],
}

impl Evidence<'_> {
    fn excerpt(&self, span: &Span) -> Option<&str> {
        if span.view_id.is_some()
            || !self.scope.contains(&span.source_id)
            || tools::validate_span(self.input, self.coverage, span).is_err()
        {
            return None;
        }
        if let Some(cell) = &span.grid_cell {
            let form = self
                .input
                .structured_forms
                .iter()
                .find(|form| form["form_definition_revision_id"] == cell.form_id)?;
            return form["definition"]["cells"]
                .as_array()?
                .iter()
                .find(|entry| entry["row"] == cell.row && entry["column"] == cell.column)?["text"]
                .as_str();
        }
        self.input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == span.source_id)?
            .text
            .get(span.start..span.end)
    }

    fn grounds_row(&self, span: &Span) -> Value {
        let text = self.excerpt(span);
        json!({"source":span,"cited_text":text,
            "status":if text.is_some() {"delivered"} else {"not_delivered"}})
    }

    fn field_check(&self, path: &str, value: Value, grounds: &[Span]) -> Value {
        json!({"path":path,"field_value":value,
            "grounds":grounds.iter().map(|span| self.grounds_row(span)).collect::<Vec<_>>(),
            "instruction":"Check whether this field is supported by its own cited grounds, including conditions. Other correct text in the packet cannot repair a wrong citation. Null cited_text is not evidence: retrieve the exact frozen source with the existing reading tools under the current role's access rules. Empty optional values need no invented wording."})
    }
}

fn collect_record_checks(
    evidence: &Evidence<'_>,
    analysis: &Analysis,
    record: &Record,
) -> Vec<Value> {
    let mut checks = Vec::new();
    match &record.data {
        RecordData::Fact { name, value, scope } => {
            checks.push(evidence.field_check(
                "/data/value",
                json!({"name":name,"value":value,"scope":scope}),
                &record.sources,
            ));
        }
        RecordData::Rule {
            text,
            applicability,
            items,
            ..
        } => {
            checks.push(evidence.field_check("/data/text", json!(text), &record.sources));
            checks.push(evidence.field_check(
                "/data/applicability",
                json!(applicability),
                &applicability.grounds,
            ));
            for (index, item) in items.iter().enumerate() {
                checks.push(evidence.field_check(
                    &format!("/data/items/{index}"),
                    json!(item),
                    &item.grounds,
                ));
            }
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
            checks.push(evidence.field_check("/data/text", json!(text), &record.sources));
            checks.push(evidence.field_check(
                "/data/applicability",
                json!(applicability),
                &applicability.grounds,
            ));
            if let Some(rule) = scoring_rule {
                checks.push(evidence.field_check(
                    "/data/scoring_rule",
                    json!(rule),
                    &record.sources,
                ));
            }
            for (index, claim) in compliance.iter().enumerate() {
                checks.push(evidence.field_check(
                    &format!("/data/compliance/{index}/condition"),
                    json!(claim.condition),
                    &claim.grounds,
                ));
            }
            for (index, need) in response.iter().enumerate() {
                checks.push(evidence.field_check(
                    &format!("/data/response/{index}"),
                    json!(need),
                    &need.grounds,
                ));
            }
            for (index, proof) in proofs.iter().enumerate() {
                checks.push(evidence.field_check(
                    &format!("/data/proofs/{index}"),
                    json!(proof),
                    &proof.grounds,
                ));
            }
            for (index, criterion) in criteria.iter().enumerate() {
                checks.push(evidence.field_check(
                    &format!("/data/criteria/{index}"),
                    json!(criterion),
                    &criterion.grounds,
                ));
            }
        }
        RecordData::Template { applicability, .. } => {
            checks.push(evidence.field_check(
                "/data/applicability",
                json!(applicability),
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
                .collect();
            checks.push(json!({"path":"/data","field_value":problem,
                "grounds":record.sources.iter().map(|span|evidence.grounds_row(span)).collect::<Vec<_>>(),
                "listed_candidates":candidates,"present_records":present_records,
                "instruction":"Inspect available targets and compare the claimed uncertainty. Existing records may still be ambiguous; their presence alone proves neither resolution nor source absence. Unfinished extraction or linking must not be labelled missing source evidence."}));
        }
    }
    checks
}

fn collect_blank_effects(evidence: &Evidence<'_>, record: &Record) -> Vec<Value> {
    let RecordData::Template { regions, .. } = &record.data else {
        return vec![];
    };
    let mut effects = Vec::new();
    let text_effects = collect_text_effects(evidence, regions);
    for (index, region) in regions.iter().enumerate() {
        if let Some(form_id) = &region.form_id {
            for cell in &region.cells {
                let source = Span {
                    source_id: region.source.source_id.clone(),
                    start: 0,
                    end: 0,
                    view_id: None,
                    grid_cell: Some(GridCitation {
                        form_id: form_id.clone(),
                        row: cell.row,
                        column: cell.column,
                    }),
                };
                let mut effect = json!({"path":format!("/data/regions/{index}"),"source":source,
                    "role":region.role,"instruction":region.instruction,
                    "instruction_note":"instruction never overrides role or blank_ranges. Compare actual selected bytes with required fixed wording."});
                if let Some(original) = evidence.excerpt(&source) {
                    let ranges: Vec<_> = region
                        .blank_ranges
                        .iter()
                        .filter(|r| r.row == cell.row && r.column == cell.column)
                        .collect();
                    let retained = if region.role.preserves_source_text() {
                        Ok(original.to_owned())
                    } else if region.blank_ranges.is_empty() {
                        Ok(String::new())
                    } else {
                        blank_cell_text(original, cell.row, cell.column, &region.blank_ranges)
                    };
                    effect["original"] = json!(original);
                    match retained {
                        Ok(retained) => {
                            let removed = if region.role.preserves_source_text() {
                                vec![]
                            } else if region.blank_ranges.is_empty() {
                                vec![original]
                            } else {
                                let mut ordered = ranges;
                                ordered.sort_by_key(|r| r.start);
                                ordered
                                    .into_iter()
                                    .map(|r| &original[r.start..r.end])
                                    .collect()
                            };
                            effect["removed"] = json!(removed.concat());
                            effect["retained"] = json!(retained);
                            effect["status"] = json!("delivered");
                        }
                        Err(error) => {
                            effect["status"] = json!("invalid_blank_ranges");
                            effect["error"] = json!(error);
                        }
                    }
                } else {
                    effect["status"] = json!("not_delivered");
                }
                effects.push(effect);
            }
        } else {
            effects.push(text_effects[&index].clone());
        }
    }
    effects
}

fn collect_text_effects(
    evidence: &Evidence<'_>,
    regions: &[TemplateRegion],
) -> BTreeMap<usize, Value> {
    let mut effects = BTreeMap::new();
    let mut index = 0;
    while index < regions.len() {
        if regions[index].form_id.is_some() {
            index += 1;
            continue;
        }
        // Match the compiler's contiguous text block boundaries. A single
        // region uses quote/blank; two or more use the shared inline stream.
        let mut end = index + 1;
        while end < regions.len()
            && regions[end].form_id.is_none()
            && regions[end].source.source_id == regions[index].source.source_id
            && regions[end - 1].source.end == regions[end].source.start
        {
            end += 1;
        }
        let originals: Vec<_> = regions[index..end]
            .iter()
            .map(|region| evidence.excerpt(&region.source))
            .collect();
        let delivered = originals.iter().all(Option::is_some);
        let inline = end > index + 1;
        let mut prior_cr = false;
        for (offset, region) in regions[index..end].iter().enumerate() {
            let original = originals[offset];
            let blank = !region.role.preserves_source_text();
            let mut effect = json!({"path":format!("/data/regions/{}",index+offset),
                "source":region.source,"role":region.role,"instruction":region.instruction,
                "selected_source_text":original,
                "status":if delivered {"delivered"}else{"not_delivered"},
                "operation":if blank {"replace_with_bidder_blank"}else{"preserve_source_text"},
                "render_group":{"start_region":index,"end_region_exclusive":end,
                    "primitive":if inline {"text_regions"}else if blank {"blank"}else{"quote"}},
                "instruction_note":"generated_text is this region's initial compiler fragment, not the whole template. Concatenate fragments within each text_regions group; quote/blank groups are separate paragraph blocks. Compare removed_ranges and generated_text with required fixed wording. Newlines are normalized and adjacent CRLF is shared. instruction cannot override the policy. Missing group evidence yields no generated text or removal claim."});
            if delivered {
                let raw = original.expect("all group evidence independently delivered");
                let (generated, removed) =
                    crate::docx_template::initial_text_fragment(raw, blank, inline, &mut prior_cr);
                effect["generated_text"] = json!(generated);
                effect["removed_ranges"] = json!(
                    removed
                        .into_iter()
                        .map(|range| {
                            json!({"start":region.source.start+range.start,
                        "end":region.source.start+range.end,"text":&raw[range]})
                        })
                        .collect::<Vec<_>>()
                );
            }
            effects.insert(index + offset, effect);
        }
        index = end;
    }
    effects
}

/// Attach optional comparisons without changing candidate digests or reading receipts.
pub fn attach(
    input: &FrozenInput,
    analysis: &Analysis,
    coverage: &Coverage,
    scope: &[String],
    key: &str,
    mut wrapper: Value,
) -> Value {
    let evidence = Evidence {
        input,
        coverage,
        scope,
    };
    if let Some(id) = key.strip_prefix("record:")
        && let Some(record) = analysis.records.get(id)
    {
        let checks = collect_record_checks(&evidence, analysis, record);
        if !checks.is_empty() {
            wrapper["field_ground_checks"] = json!(checks);
        }
        let blanks = collect_blank_effects(&evidence, record);
        if !blanks.is_empty() {
            wrapper["blank_effects"] = json!(blanks);
        }
    }
    if let Some(id) = key.strip_prefix("relation:")
        && let Some(relation) = analysis.relations.get(id)
    {
        wrapper["field_ground_checks"] = json!([evidence.field_check(
            "/explanation",
            json!(relation.explanation),
            &relation.grounds
        )]);
    }
    wrapper
}

/// Add optional explanations only after every admitted raw group is fixed.
/// An explanation can use spare bytes, never evict another complete candidate.
pub(super) fn annotate_candidates(
    input: &FrozenInput,
    analysis: &Analysis,
    coverage: &Coverage,
    scope: &[String],
    packet: &mut Value,
    budget: usize,
) -> Result<(), String> {
    let count = packet["assigned_evidence"]["candidates"]
        .as_array()
        .ok_or("candidate packet missing")?
        .len();
    for index in 0..count {
        let original = packet["assigned_evidence"]["candidates"][index].clone();
        let key = original["reference"]
            .as_str()
            .ok_or("candidate reference missing")?;
        let enriched = attach(input, analysis, coverage, scope, key, original.clone());
        packet["assigned_evidence"]["candidates"][index] = enriched;
        if serde_json::to_vec(packet).map_err(|e| e.to_string())?.len() > budget {
            packet["assigned_evidence"]["candidates"][index] = original;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delivered(input: &FrozenInput) -> Coverage {
        let mut coverage = Coverage::default();
        for source in &input.source_units {
            tools::cover(
                coverage
                    .text
                    .entry(source.source_unit_revision_id.clone())
                    .or_default(),
                0,
                source.text.len(),
            );
        }
        for form in &input.structured_forms {
            let count = form["definition"]["row_count"].as_u64().unwrap() as usize
                * form["definition"]["column_count"].as_u64().unwrap() as usize;
            tools::cover(
                coverage
                    .form_cells
                    .entry(form["form_definition_revision_id"].as_str().unwrap().into())
                    .or_default(),
                0,
                count,
            );
        }
        coverage
    }

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
        let coverage = delivered(&frozen);
        let scope = vec!["indicators".into(), "control".into()];
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let checks = collect_record_checks(&evidence, &Analysis::default(), &record);
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
                    header_rows: None,
                    cells: vec![],
                    blank_ranges: vec![],
                    instruction: "keep labels 投标人名称 and blank the underscores".into(),
                }],
            },
        };
        let coverage = delivered(&frozen);
        let scope = vec!["source".into()];
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_blank_effects(&evidence, &record);
        assert_eq!(
            effects[0]["source"],
            json!(span("source", 0, original.len()))
        );
        assert_eq!(effects[0]["selected_source_text"], original);
        assert_eq!(effects[0]["operation"], "replace_with_bidder_blank");
        assert_eq!(effects[0]["role"], "bidder_blank");
        assert_eq!(effects[0]["generated_text"], "");
        assert_eq!(
            effects[0]["removed_ranges"],
            json!([{
                "start":0,"end":original.len(),"text":original
            }])
        );
        assert!(
            effects[0]["instruction"]
                .as_str()
                .unwrap()
                .contains("keep labels")
        );
        let mut analysis = Analysis::default();
        analysis.records.insert(record.id.clone(), record.clone());
        let mut packet = json!({"assigned_evidence":{"candidates":[{
            "reference":"record:tpl","value":record,"sha256":digest(&record).unwrap()
        }]}});
        let unchanged = packet.clone();
        let budget = serde_json::to_vec(&packet).unwrap().len();
        annotate_candidates(&frozen, &analysis, &coverage, &scope, &mut packet, budget).unwrap();
        assert_eq!(
            packet, unchanged,
            "optional output effects cannot evict raw candidates"
        );
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
                        header_rows: None,
                        cells: vec![],
                        blank_ranges: vec![],
                        instruction: String::new(),
                    },
                    TemplateRegion {
                        source: span("source", label.len(), original.len()),
                        role: RegionRole::BidderBlank,
                        form_id: None,
                        header_rows: None,
                        cells: vec![],
                        blank_ranges: vec![],
                        instruction: String::new(),
                    },
                ],
            },
        };
        let coverage = delivered(&frozen);
        let scope = vec!["source".into()];
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_blank_effects(&evidence, &record);
        assert_eq!(effects[0]["operation"], "preserve_source_text");
        assert_eq!(effects[0]["selected_source_text"], label);
        assert_eq!(effects[1]["selected_source_text"], blank);
        assert_eq!(effects[1]["operation"], "replace_with_bidder_blank");
        assert_eq!(effects[0]["generated_text"], label);
        assert_eq!(effects[1]["generated_text"], " ");
        assert_eq!(
            effects[1]["removed_ranges"],
            json!([{
                "start":label.len(),"end":original.len(),"text":blank
            }])
        );
    }

    #[test]
    fn text_effects_preserve_roles_and_report_exact_utf8_removal_in_shared_crlf_stream() {
        let parts = [
            ("固定字\r", RegionRole::FixedText),
            ("\n样例甲\r\n \t\n样例乙", RegionRole::BidderBlank),
            ("\r", RegionRole::Signature),
            ("\n签章日期", RegionRole::Signature),
            ("说明", RegionRole::Instruction),
            ("编号", RegionRole::TenderValue),
        ];
        let text: String = parts.iter().map(|(text, _)| *text).collect();
        let frozen = input(vec![("source", &text)]);
        let mut start = 0;
        let regions: Vec<_> = parts
            .iter()
            .map(|(text, role)| {
                let end = start + text.len();
                let region = TemplateRegion {
                    source: span("source", start, end),
                    role: *role,
                    form_id: None,
                    header_rows: None,
                    cells: vec![],
                    blank_ranges: vec![],
                    instruction: String::new(),
                };
                start = end;
                region
            })
            .collect();
        let scope = vec!["source".into()];
        let coverage = delivered(&frozen);
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_text_effects(&evidence, &regions);
        let generated: String = effects
            .values()
            .map(|effect| effect["generated_text"].as_str().unwrap())
            .collect();
        assert_eq!(generated, "固定字\n \n \t\n \n签章日期说明编号");
        for index in [0, 2, 3, 4, 5] {
            assert_eq!(effects[&index]["removed_ranges"], json!([]));
        }
        assert_eq!(
            effects[&1]["removed_ranges"],
            json!([
                {"start":parts[0].0.len()+1,"end":parts[0].0.len()+1+"样例甲".len(),"text":"样例甲"},
                {"start":parts[0].0.len()+parts[1].0.len()-"样例乙".len(),"end":parts[0].0.len()+parts[1].0.len(),"text":"样例乙"}
            ])
        );
    }

    #[test]
    fn text_effects_do_not_infer_unread_group_context_or_invalid_utf8_bytes() {
        let frozen = input(vec![("source", "已读\r\n未交付")]);
        let mut regions = vec![
            TemplateRegion {
                source: span("source", 0, "已读\r".len()),
                role: RegionRole::FixedText,
                form_id: None,
                header_rows: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: String::new(),
            },
            TemplateRegion {
                source: span("source", "已读\r".len(), frozen.source_units[0].text.len()),
                role: RegionRole::BidderBlank,
                form_id: None,
                header_rows: None,
                cells: vec![],
                blank_ranges: vec![],
                instruction: String::new(),
            },
        ];
        let mut coverage = Coverage::default();
        coverage
            .text
            .insert("source".into(), vec![(0, "已读\r".len())]);
        let scope = vec!["source".into()];
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_text_effects(&evidence, &regions);
        for effect in effects.values() {
            assert_eq!(effect["status"], "not_delivered");
            assert!(effect["generated_text"].is_null());
            assert!(effect["removed_ranges"].is_null());
            assert!(!effect.to_string().contains("未交付"));
        }
        let coverage = delivered(&frozen);
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        regions[0].source.start = 1;
        let effects = collect_text_effects(&evidence, &regions);
        assert_eq!(effects[&0]["status"], "not_delivered");
        assert!(effects[&0]["selected_source_text"].is_null());
        assert!(effects[&1]["generated_text"].is_null());
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &[],
        };
        assert!(
            collect_text_effects(&evidence, &regions)
                .values()
                .all(|effect| effect["selected_source_text"].is_null()
                    && effect["generated_text"].is_null())
        );
    }
    #[test]
    fn projection_never_quotes_unread_or_out_of_scope_originals() {
        let frozen = input(vec![("source", "已读"), ("private", "未交付原文")]);
        let coverage = delivered(&frozen);
        let scope = vec!["source".into()];
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let private = span("private", 0, frozen.source_units[1].text.len());
        assert!(
            evidence.excerpt(&private).is_none(),
            "even a previous delivery is outside this work scope"
        );
        let all_scope = vec!["source".into(), "private".into()];
        let mut unread = coverage.clone();
        unread.text.remove("private");
        let evidence = Evidence {
            input: &frozen,
            coverage: &unread,
            scope: &all_scope,
        };
        let row = evidence.grounds_row(&private);
        assert_eq!(row["status"], "not_delivered");
        assert!(row["cited_text"].is_null());
        assert!(!row.to_string().contains("未交付原文"));
        assert_eq!(row["source"], json!(private));
    }

    fn grid_fixture() -> (FrozenInput, Record) {
        let text = "名称：示例单位";
        let mut frozen = input(vec![("source", "")]);
        frozen.structured_forms = vec![json!({
            "form_definition_revision_id":"form", "source_unit_revision_id":"source",
            "definition":{"schema_version":3,"kind":"grid","row_count":1,"column_count":1,
                "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":text}]}
        })];
        let source = Span {
            source_id: "source".into(),
            start: 0,
            end: 0,
            view_id: None,
            grid_cell: Some(GridCitation {
                form_id: "form".into(),
                row: 0,
                column: 0,
            }),
        };
        let record = Record {
            id: "tpl".into(),
            sources: vec![source.clone()],
            data: RecordData::Template {
                label: "format".into(),
                title: "format".into(),
                parent: None,
                order: None,
                purpose: "submission".into(),
                applicability: applicability(vec![source.clone()]),
                regions: vec![TemplateRegion {
                    source,
                    role: RegionRole::BidderBlank,
                    form_id: Some("form".into()),
                    header_rows: Some(1),
                    cells: vec![Cell { row: 0, column: 0 }],
                    blank_ranges: vec![crate::template_grid::CellTextRange {
                        row: 0,
                        column: 0,
                        start: "名称：".len(),
                        end: "名称：示例".len(),
                    }],
                    instruction: "retain fixed wording".into(),
                }],
            },
        };
        (frozen, record)
    }

    #[test]
    fn partial_grid_blank_uses_exact_utf8_ranges_not_shifted_character_diff() {
        let (frozen, record) = grid_fixture();
        let coverage = delivered(&frozen);
        let scope = vec!["source".into()];
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_blank_effects(&evidence, &record);
        assert_eq!(effects[0]["removed"], "示例");
        assert_eq!(effects[0]["retained"], "名称：单位");
        assert_eq!(effects[0]["source"]["grid_cell"]["form_id"], "form");
    }

    #[test]
    fn invalid_grid_ranges_and_undelivered_cells_cannot_claim_rendered_results() {
        let (frozen, mut record) = grid_fixture();
        let mut coverage = delivered(&frozen);
        let scope = vec!["source".into()];
        if let RecordData::Template { regions, .. } = &mut record.data {
            regions[0].blank_ranges[0].start += 1; // Middle of a Chinese UTF-8 character.
        }
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_blank_effects(&evidence, &record);
        assert_eq!(effects[0]["status"], "invalid_blank_ranges");
        assert!(effects[0]["retained"].is_null());
        coverage.form_cells.clear();
        let evidence = Evidence {
            input: &frozen,
            coverage: &coverage,
            scope: &scope,
        };
        let effects = collect_blank_effects(&evidence, &record);
        assert_eq!(effects[0]["status"], "not_delivered");
        assert!(effects[0]["original"].is_null());
    }

    #[test]
    fn optional_comparisons_do_not_evict_complete_candidate_groups() {
        let frozen = input(vec![("source", "a source paragraph")]);
        let coverage = delivered(&frozen);
        let scope = vec!["source".into()];
        let mut analysis = Analysis::default();
        for id in ["a", "b"] {
            analysis.records.insert(
                id.into(),
                Record {
                    id: id.into(),
                    sources: vec![span("source", 0, 18)],
                    data: RecordData::Fact {
                        name: id.into(),
                        value: "source paragraph".into(),
                        scope: "project".into(),
                    },
                },
            );
        }
        let rows: Vec<_> = analysis.records.values().map(|r|json!({"reference":format!("record:{}",r.id),"value":r,"sha256":digest(r).unwrap()})).collect();
        let mut packet = json!({"assigned_evidence":{"candidates":rows}});
        let before = packet.clone();
        let budget = serde_json::to_vec(&packet).unwrap().len();
        annotate_candidates(&frozen, &analysis, &coverage, &scope, &mut packet, budget).unwrap();
        assert_eq!(packet, before);
        annotate_candidates(
            &frozen,
            &analysis,
            &coverage,
            &scope,
            &mut packet,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(
            packet["assigned_evidence"]["candidates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(packet["assigned_evidence"]["candidates"][0]["field_ground_checks"].is_array());
        for index in 0..2 {
            assert_eq!(
                packet["assigned_evidence"]["candidates"][index]["value"],
                before["assigned_evidence"]["candidates"][index]["value"]
            );
            assert_eq!(
                packet["assigned_evidence"]["candidates"][index]["sha256"],
                before["assigned_evidence"]["candidates"][index]["sha256"]
            );
        }
    }
}
