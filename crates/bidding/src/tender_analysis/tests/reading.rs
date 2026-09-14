use super::*;

#[test]
fn form_literal_matches_return_original_utf8_offsets_without_selecting_a_blank_policy() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["cells"][1]["text"] = json!("名称：甲甲甲；备注：甲甲");
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let args = json!({"form_id":"grid","offset":0,"limit":4,"find_text":"甲甲"});
    let schema = tools::schemas(false)
        .into_iter()
        .find(|t| t["function"]["name"] == "read_form")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &args,
        8192,
    )
    .unwrap();
    assert_eq!(
        read["matches"],
        json!([[],null,[
        {"row":1,"column":0,"start":9,"end":15},
        {"row":1,"column":0,"start":12,"end":18},
        {"row":1,"column":0,"start":30,"end":36}
    ],[]])
    );
    assert!(analysis.records.is_empty());
    let no_match = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_form",
        &json!({"form_id":"grid","offset":2,"limit":1,"find_text":"名称:甲甲"}),
        8192,
    )
    .unwrap();
    assert_eq!(no_match["matches"], json!([[]]));
    let original = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_form",
        &json!({"form_id":"grid","offset":2,"limit":1}),
        8192,
    )
    .unwrap();
    assert!(original.get("matches").is_none());
    assert_eq!(original["cells"][0], read["cells"][2]);
}

#[test]
fn form_literal_match_failures_do_not_acknowledge_reading() {
    let mut input = grid_citation_input();
    input.structured_forms[0]["definition"]["cells"][1]["text"] = json!("甲".repeat(100));
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    for (query, budget) in [
        (json!(""), 8192),
        (Value::Null, 8192),
        (json!(7), 8192),
        (json!("甲"), 512),
    ] {
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "read_form",
                &json!({"form_id":"grid","offset":2,"limit":1,"find_text":query}),
                budget
            )
            .is_err()
        );
        assert!(coverage.form_cells.is_empty());
        assert!(analysis.records.is_empty());
    }
}

#[test]
fn compact_evidence_references_preserve_records_and_do_not_grant_reading() {
    let input = grid_citation_input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let cite = json!({"ref":"g:0:1:0"});
    let mut args = requirement();
    args["sources"] = json!([cite]);
    args["data"]["applicability"]["grounds"] = json!([cite]);
    args["data"]["response"][0]["grounds"] = json!([cite]);
    args["data"]["compliance"][0]["grounds"] = json!([cite]);
    args["data"]["text"] = json!("逐项响应；普通文字 {\"ref\":\"g:0:1:0\"} 不应转换。");
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &args,
            8192
        )
        .is_err()
    );
    assert!(analysis.records.is_empty());
    assert!(coverage.form_cells.is_empty());
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":0,"limit":4}),
        8192,
    )
    .unwrap();
    let id = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_record",
        &args,
        8192,
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(read["citation_refs"][2], cite);
    assert!(read["citation_refs"][1].is_null());
    let mut expected = args.clone();
    for path in [
        "/sources/0",
        "/data/applicability/grounds/0",
        "/data/response/0/grounds/0",
        "/data/compliance/0/grounds/0",
    ] {
        *expected.pointer_mut(path).unwrap() = read["citations"][2].clone();
    }
    expected["id"] = json!(id);
    assert_eq!(json!(analysis.records[&id]), expected);
    let schema = tools::schemas(false)
        .into_iter()
        .find(|s| s["function"]["name"] == "put_record")
        .unwrap();
    assert!(
        jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
            .unwrap()
            .is_valid(&args)
    );
    let before = digest(&analysis).unwrap();
    for bad in [
        "g:99:1:0",
        "g:0:0:1",
        "g:0:99:0",
        "g:0:1:0:extra",
        "g:-1:1:0",
        "t:0:0:1",
    ] {
        let mut bad_args = args.clone();
        bad_args["sources"][0] = json!({"ref":bad});
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "put_record",
                &bad_args,
                8192
            )
            .is_err(),
            "{bad}"
        );
        assert_eq!(digest(&analysis).unwrap(), before);
    }
    let mut reviewer = Coverage::default();
    let span: Span = serde_json::from_value(read["citations"][2].clone()).unwrap();
    assert!(tools::validate_span(&input, &reviewer, &span).is_err());
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut reviewer,
            true,
            "put_record",
            &args,
            8192
        )
        .is_err()
    );
}

#[test]
fn grid_citations_require_actual_cell_reading_and_preserve_exact_source_identity() {
    let input = grid_citation_input();
    let citation: Span = serde_json::from_value(json!({"source_id":"source","start":0,"end":0,
        "grid_cell":{"form_id":"grid","row":1,"column":1}}))
    .unwrap();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    assert!(tools::validate_span(&input, &coverage, &citation).is_err());
    let first = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":0,"limit":2}),
        8192,
    )
    .unwrap();
    assert!(first["citations"][1].is_null());
    assert!(tools::validate_span(&input, &coverage, &citation).is_err());
    let args = json!({"source_id":"source","state":"requirement","reason":"原始网格明确约定义务"});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "set_disposition",
            &args,
            8192
        )
        .is_err()
    );
    let before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_form",
            &json!({"form_id":"grid","offset":2,"limit":2}),
            40
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), before);
    let second = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":2,"limit":2}),
        8192,
    )
    .unwrap();
    assert_eq!(second["citations"][1], json!(citation));
    tools::validate_span(&input, &coverage, &citation).unwrap();
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "set_disposition",
        &args,
        8192,
    )
    .unwrap();
    // Reading grid evidence never establishes text or independent reviewer coverage.
    assert!(coverage.text.is_empty());
    assert!(tools::validate_span(&input, &Coverage::default(), &citation).is_err());
    let mut malformed = input.clone();
    malformed.structured_forms[0]["definition"]["row_count"] = json!(1);
    assert!(tools::validate_span(&malformed, &coverage, &citation).is_err());
    for patch in [
        json!({"source_id":"foreign"}),
        json!({"end":1}),
        json!({"view_id":"view"}),
        json!({"grid_cell":{"form_id":"grid","row":0,"column":1}}),
        json!({"grid_cell":{"form_id":"foreign","row":1,"column":1}}),
    ] {
        let mut bad = json!(citation);
        for (k, v) in patch.as_object().unwrap() {
            bad[k] = v.clone();
        }
        let bad: Span = serde_json::from_value(bad).unwrap();
        assert!(tools::validate_span(&input, &coverage, &bad).is_err());
    }
}

#[test]
fn source_line_spans_preserve_original_bytes_and_support_direct_citations() {
    let mut input = input();
    input.source_units[0].text = "标题\r\n第一项需\n要跨行保留。😀\n\n末行".into();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_source",
        &json!({"source_id":"source","start":0,"max_bytes":4096}),
        8192,
    )
    .unwrap();
    let lines = out["line_spans"]
        .as_array()
        .expect("read source needs directly usable line spans");
    assert_eq!(
        lines
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect::<String>(),
        input.source_units[0].text
    );
    for line in lines {
        let start = line["start"].as_u64().unwrap() as usize;
        let end = line["end"].as_u64().unwrap() as usize;
        assert_eq!(&input.source_units[0].text[start..end], line["text"]);
        let expanded = evidence_refs::expand(&input, &line["citation_ref"]).unwrap();
        assert_eq!(
            expanded,
            json!({"source_id":"source","start":start,"end":end})
        );
        tools::validate_span(
            &input,
            &coverage,
            &Span {
                source_id: "source".into(),
                start,
                end,
                view_id: None,
                grid_cell: None,
            },
        )
        .unwrap();
    }
    assert_eq!(lines[0]["text"], "标题\r\n");
    let start = lines[1]["start"].as_u64().unwrap() as usize;
    let end = lines[2]["end"].as_u64().unwrap() as usize;
    assert_eq!(
        &input.source_units[0].text[start..end],
        "第一项需\n要跨行保留。😀\n"
    );
    tools::validate_span(
        &input,
        &coverage,
        &Span {
            source_id: "source".into(),
            start,
            end,
            view_id: None,
            grid_cell: None,
        },
    )
    .unwrap();
}

#[test]
fn annotated_source_pages_remain_bounded_and_never_skip_utf8_fragments() {
    let mut input = input();
    input.source_units[0].text = "甲😀\r\n\n乙\n".repeat(200);
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut start = 0;
    let mut read = String::new();
    while start < input.source_units[0].text.len() {
        let out = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "read_source",
            &json!({"source_id":"source","start":start,"max_bytes":4096}),
            1024,
        )
        .unwrap();
        assert!(serde_json::to_vec(&out).unwrap().len() <= 1024);
        assert_eq!(out["start"], start);
        let end = out["end"].as_u64().unwrap() as usize;
        assert!(end > start);
        let joined: String = out["line_spans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect();
        assert_eq!(joined, out["text"]);
        assert_eq!(joined, input.source_units[0].text[start..end]);
        read.push_str(&joined);
        start = end;
    }
    assert_eq!(read, input.source_units[0].text);
    assert!(
        analysis.coverage.text.is_empty(),
        "reviewer annotations cannot mark main evidence read"
    );
    assert!(tools::contains(coverage.text.get("source"), 0, start));
    let before = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "read_source",
            &json!({"source_id":"source","start":0,"max_bytes":4096}),
            1
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), before);
    let eof = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "read_source",
        &json!({"source_id":"source","start":start,"max_bytes":4096}),
        1024,
    )
    .unwrap();
    assert_eq!(eof["line_spans"], json!([]));
}

#[test]
fn read_coverage_is_exact_utf8_and_oversized_results_do_not_mark_read() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_source",
        &json!({"source_id":"source","start":0,"max_bytes":4}),
        1024,
    )
    .unwrap();
    assert_eq!(out["end"], 3);
    assert_eq!(out["text"], "提");
    assert!(tools::validate_span(&input, &coverage, &span()).is_err());
    let prior = digest(&coverage).unwrap();
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":1,"max_bytes":10}),
            1024
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), prior);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":3,"max_bytes":10}),
            5
        )
        .is_err()
    );
    assert_eq!(digest(&coverage).unwrap(), prior);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "set_disposition",
            &json!({"source_id":"source","state":"non_requirement","reason":"read only prefix"}),
            1024
        )
        .is_err()
    );
}

#[test]
fn search_and_index_do_not_establish_reading_coverage() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    for (name, args) in [
        ("source_index", json!({"offset":0,"limit":10})),
        (
            "search_sources",
            json!({"query":"附表甲","offset":0,"limit":10}),
        ),
    ] {
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            name,
            &args,
            4096,
        )
        .unwrap();
    }
    assert_eq!(tools::reading_gaps(&input, &coverage).len(), 1);
    assert!(coverage.text.is_empty());
}

#[test]
fn reading_gaps_keep_interleaved_source_order_without_acknowledging_evidence() {
    let mut input = grid_citation_input();
    input.documents.push(json!({"id":"document"}));
    input.source_units.push(Source {
        source_unit_revision_id: "later-text".into(),
        document_id: "document".into(),
        text: "后续正文".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let mut later_grid = input.source_units[0].clone();
    later_grid.source_unit_revision_id = "later-grid".into();
    later_grid.ordinal = 2;
    input.source_units.push(later_grid);
    let mut form = input.structured_forms[0].clone();
    form["form_definition_revision_id"] = json!("grid-later");
    form["source_unit_revision_id"] = json!("later-grid");
    // The form collection's order must not override the frozen source order.
    input.structured_forms.insert(0, form);
    let mut coverage = Coverage::default();
    coverage.form_cells.insert("grid".into(), vec![(0, 2)]);
    let before = digest(&coverage).unwrap();
    let gaps = tools::reading_gaps(&input, &coverage);
    assert_eq!(gaps.len(), 4);
    assert_eq!(gaps[0]["kind"], "unread_metadata");
    assert_eq!(gaps[1]["form_id"], "grid");
    assert_eq!(gaps[1]["read_ranges"], json!([[0, 2]]));
    assert_eq!(gaps[2]["source_id"], "later-text");
    assert_eq!(gaps[3]["form_id"], "grid-later");
    assert_eq!(digest(&coverage).unwrap(), before);
    tools::cover(coverage.form_cells.get_mut("grid").unwrap(), 2, 4);
    let gaps = tools::reading_gaps(&input, &coverage);
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[1]["source_id"], "later-text");
    assert_eq!(gaps[2]["form_id"], "grid-later");
}

#[test]
fn search_finds_sparse_grid_text_with_dense_read_offsets_and_no_evidence() {
    let mut input = grid_citation_input();
    input.source_units[0].text = "证书".into();
    input.structured_forms[0]["definition"]["cells"][0]["text"] = json!("证书");
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let before = digest(&coverage).unwrap();
    let mut hits = vec![];
    let mut offset = 0;
    loop {
        let page = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "search_sources",
            &json!({"query":"证书","offset":offset,"limit":100}),
            220,
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 220);
        assert_eq!(page["total"], 3);
        let next = page["next"].as_u64().unwrap();
        assert!(next > offset);
        hits.extend(page["items"].as_array().unwrap().clone());
        offset = next;
        if page["next"] == page["total"] {
            break;
        }
    }
    assert_eq!(
        hits,
        vec![
            json!({"source_id":"source","start":0,"end":6}),
            json!({"source_id":"source","grid_cell":{"form_id":"grid","row":0,"column":0},"form_offset":0,"cell_match":{"start":0,"end":6}}),
            json!({"source_id":"source","grid_cell":{"form_id":"grid","row":1,"column":1},"form_offset":3,"cell_match":{"start":15,"end":21}}),
        ]
    );
    assert_eq!(digest(&coverage).unwrap(), before);
    let citation: Span = serde_json::from_value(
        json!({"source_id":"source","start":0,"end":0,"grid_cell":hits[2]["grid_cell"]}),
    )
    .unwrap();
    assert!(tools::validate_span(&input, &coverage, &citation).is_err());
    let read = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "read_form",
        &json!({"form_id":"grid","offset":hits[2]["form_offset"],"limit":1}),
        4096,
    )
    .unwrap();
    assert_eq!(read["cells"][0]["text"], "供货时提供证书");
    tools::validate_span(&input, &coverage, &citation).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_FROZEN_SOURCE from the shared parser; no model calls"]
fn frozen_parser_grids_remain_searchable_and_readable_as_exact_evidence() {
    let bytes = std::fs::read(std::env::var("KB_TENDER_FROZEN_SOURCE").unwrap()).unwrap();
    let input: FrozenInput = serde_json::from_slice(&bytes).unwrap();
    tools::validate_input(&input).unwrap();
    assert!(!input.structured_forms.is_empty());
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut searched = 0;
    let mut read_cells = 0;
    for f in &input.structured_forms {
        let definition = &f["definition"];
        assert_eq!(definition["schema_version"], 3);
        let columns = definition["column_count"].as_u64().unwrap();
        for cell in definition["cells"].as_array().unwrap() {
            let coordinate = json!({"form_id":f["form_definition_revision_id"],"row":cell["row"],"column":cell["column"]});
            let index = cell["row"].as_u64().unwrap() * columns + cell["column"].as_u64().unwrap();
            let text = cell["text"].as_str().unwrap();
            if !text.trim().is_empty() {
                let before = digest(&coverage).unwrap();
                let mut offset = 0;
                let mut found = false;
                loop {
                    let page = tools::invoke(
                        &input,
                        &mut analysis,
                        &mut coverage,
                        false,
                        "search_sources",
                        &json!({"query":text,"offset":offset,"limit":100}),
                        2048,
                    )
                    .unwrap();
                    found |= page["items"].as_array().unwrap().iter().any(|hit| {
                        hit["source_id"] == f["source_unit_revision_id"]
                            && hit["grid_cell"] == coordinate
                            && hit["form_offset"] == index
                            && hit["cell_match"] == json!({"start":0,"end":text.len()})
                    });
                    if page["next"] == page["total"] {
                        break;
                    }
                    offset = page["next"].as_u64().unwrap();
                }
                assert!(found, "missing grid match: {coordinate}");
                assert_eq!(digest(&coverage).unwrap(), before);
                searched += 1;
            }
            let read = tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                false,
                "read_form",
                &json!({"form_id":f["form_definition_revision_id"],"offset":index,"limit":1}),
                65536,
            )
            .unwrap();
            assert_eq!(read["cells"][0], *cell);
            let citation: Span = serde_json::from_value(read["citations"][0].clone()).unwrap();
            tools::validate_span(&input, &coverage, &citation).unwrap();
            read_cells += 1;
        }
    }
    println!(
        "{}",
        json!({"sources":input.source_units.len(),"forms":input.structured_forms.len(),"searched_nonempty_anchors":searched,"read_anchors":read_cells})
    );
    assert!(analysis.records.is_empty());
}

#[test]
fn reading_ranges_merge_but_do_not_hide_internal_holes() {
    let mut ranges = vec![];
    tools::cover(&mut ranges, 5, 8);
    tools::cover(&mut ranges, 0, 3);
    assert_eq!(ranges, vec![(0, 3), (5, 8)]);
    tools::cover(&mut ranges, 2, 6);
    assert_eq!(ranges, vec![(0, 8)]);
}

#[test]
fn reviewer_must_inspect_current_candidate_and_oversize_is_not_coverage() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let mut main = analysis.coverage.clone();
    let result = tools::invoke(
        &input,
        &mut analysis,
        &mut main,
        false,
        "put_record",
        &requirement(),
        16000,
    )
    .unwrap();
    let mut review = main.clone();
    assert_eq!(tools::review_gaps(&input, &analysis, &review).len(), 1);
    let query = json!({"view":"detail","kind":"all","offset":0,"limit":10});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut review,
            true,
            "inspect_analysis",
            &query,
            100
        )
        .is_err()
    );
    assert!(review.candidate.is_empty());
    tools::invoke(
        &input,
        &mut analysis,
        &mut review,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert!(tools::review_gaps(&input, &analysis, &review).is_empty());
    let mut change = requirement();
    change["id"] = result["id"].clone();
    change["data"]["text"] = json!("修订后的要求");
    tools::invoke(
        &input,
        &mut analysis,
        &mut main,
        false,
        "put_record",
        &change,
        16000,
    )
    .unwrap();
    assert_eq!(
        tools::review_gaps(&input, &analysis, &review).len(),
        1,
        "old inspection cannot approve changed content"
    );
}

#[test]
fn inspect_analysis_exact_ids_and_source_filter_do_not_review_neighbours() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    let query = json!({"view":"detail","kind":"all","offset":0,"limit":1,"ids":[ids[1]]});
    for reviewer in [false, true] {
        let schema = tools::schemas(reviewer)
            .into_iter()
            .find(|t| t["function"]["name"] == "inspect_analysis")
            .unwrap();
        assert!(
            jsonschema::JSONSchema::compile(&schema["function"]["parameters"])
                .unwrap()
                .is_valid(&query)
        );
    }
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert_eq!(out["total"], 1);
    assert_eq!(out["items"][0]["id"], ids[1]);
    assert_eq!(coverage.candidate.len(), 1);
    assert!(
        coverage
            .candidate
            .contains_key(&format!("record:{}", ids[1]))
    );
    assert!(
        coverage.text.is_empty(),
        "candidate inspection is not source reading"
    );

    let mut found = Vec::new();
    for offset in 0..2 {
        let out = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"view":"detail","kind":"fact","source_id":"source","offset":offset,"limit":1}),
            16000,
        )
        .unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["next"], offset + 1);
        found.push(out["items"][0]["id"].as_str().unwrap().to_owned());
    }
    found.sort();
    let mut expected = vec![ids[0].clone(), ids[2].clone()];
    expected.sort();
    assert_eq!(
        found, expected,
        "same labels in another document must not leak into this source"
    );
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"view":"detail","kind":"all","source_id":"source","ids":[ids[1]],"offset":0,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(
        out["total"], 0,
        "filters intersect rather than broaden the query"
    );
}

#[test]
fn inspect_all_resolves_mixed_candidate_ids_and_tracks_only_delivered_details() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    let mut query =
        json!({"kind":"all","ids":[ids[0],ids[3],"source"],"view":"index","offset":0,"limit":10});
    let index = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert_eq!(index["total"], 3);
    assert!(coverage.candidate.is_empty());
    query["view"] = json!("detail");
    query["limit"] = json!(1);
    let mut offset = 0;
    while offset < 3 {
        query["offset"] = json!(offset);
        let page = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            16000,
        )
        .unwrap();
        offset = page["next"].as_u64().unwrap();
        assert_eq!(coverage.candidate.len(), offset as usize);
    }
    assert!(
        coverage
            .candidate
            .contains_key(&format!("record:{}", ids[0]))
    );
    assert!(
        coverage
            .candidate
            .contains_key(&format!("relation:{}", ids[3]))
    );
    assert!(coverage.candidate.contains_key("disposition:source"));
    let delivered = coverage.candidate.clone();
    query["offset"] = json!(0);
    query["ids"] = json!([ids[0], "missing"]);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            16000
        )
        .is_err()
    );
    assert_eq!(
        coverage.candidate, delivered,
        "invalid mixed queries cannot partially receipt objects"
    );
    let mut collision = analysis.records[&ids[0]].clone();
    collision.id = "source".into();
    analysis.records.insert("source".into(), collision);
    query["ids"] = json!(["source"]);
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            16000
        )
        .unwrap_err()
        .contains("ambiguous")
    );
    assert_eq!(coverage.candidate, delivered);
    query["kind"] = json!("record");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert!(coverage.candidate.contains_key("record:source"));
}

#[test]
fn inspect_analysis_category_errors_explain_reference_mapping_without_receipting_a_partial_batch() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    for query_ids in [json!([ids[0], ids[3]]), json!(["disposition:source"])] {
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"kind":"record","ids":query_ids,"offset":0,"limit":10}),
            16000,
        )
        .unwrap_err();
        assert!(error.contains("/ids"));
        assert!(error.contains("kind=all for mixed categories"));
        assert!(error.contains("kind=relation"));
        assert!(error.contains("kind=disposition"));
        assert!(error.contains("without the reference prefix"));
        assert!(coverage.candidate.is_empty());
    }
}

#[test]
fn inspect_analysis_filtered_relations_and_dispositions_keep_exact_review_identity() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"view":"detail","kind":"relation","source_id":"other-source","ids":[ids[3]],"offset":0,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(
        out["items"][0]["id"], ids[3],
        "endpoint provenance must locate cross-source links"
    );
    assert_eq!(coverage.candidate.len(), 1);
    assert!(
        coverage
            .candidate
            .contains_key(&format!("relation:{}", ids[3]))
    );
    let out = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"view":"detail","kind":"disposition","ids":["other-source"],"offset":0,"limit":1}),
        16000,
    )
    .unwrap();
    assert_eq!(out["items"][0]["source_id"], "other-source");
    assert!(coverage.candidate.contains_key("disposition:other-source"));
    assert!(!coverage.candidate.contains_key("disposition:source"));
    assert!(
        tools::review_gaps(&input, &analysis, &coverage)
            .iter()
            .any(|g| g["kind"] == "unreviewed_candidate")
    );
}

#[test]
fn inspect_analysis_budgeted_pages_preserve_records_and_only_receipt_returned_items() {
    let (input, analysis, _) = analysis_query_fixture();
    for reviewer in [false, true] {
        let mut analysis = analysis.clone();
        let mut coverage = Coverage::default();
        let expected = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "inspect_analysis",
            &json!({"view":"detail","kind":"all","offset":0,"limit":100}),
            16000,
        )
        .unwrap();
        let records = expected["items"].as_array().unwrap();
        coverage = Coverage::default();
        assert!(records.len() > 1);
        let budget = records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                serde_json::to_vec(
                    &json!({"view":"detail","total":records.len(),"next":index+1,"items":[record]}),
                )
                .unwrap()
                .len()
            })
            .max()
            .unwrap();
        let mut offset = 0;
        let mut actual = Vec::new();
        let mut pages = 0;
        while offset < records.len() {
            let page = tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                reviewer,
                "inspect_analysis",
                &json!({"view":"detail","kind":"all","offset":offset,"limit":100}),
                budget,
            )
            .unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= budget);
            assert_eq!(page["total"], records.len());
            let next = page["next"].as_u64().unwrap() as usize;
            assert!(next > offset);
            actual.extend(page["items"].as_array().unwrap().iter().cloned());
            assert_eq!(coverage.candidate.len(), actual.len());
            offset = next;
            pages += 1;
        }
        assert!(
            pages > 1,
            "fixture must exercise a page too large for one response"
        );
        assert_eq!(
            &actual, records,
            "every record must survive pagination intact and in order"
        );
    }
}

#[test]
fn candidate_detail_receipts_are_role_local_and_invalidated_by_edits() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let query = json!({"kind":"all","ids":[ids[0]],"offset":0,"limit":1});
    let mut index_query = query.clone();
    index_query["view"] = json!("index");
    for reviewer in [false, true] {
        let mut coverage = Coverage::default();
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(index["items"][0]["detail_received"], false);
        assert!(coverage.candidate.is_empty());
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &query,
            16000,
        )
        .unwrap();
        let restored: Coverage = serde_json::from_value(json!(coverage)).unwrap();
        assert_eq!(restored.candidate.len(), 1);
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(index["items"][0]["detail_received"], true);
        assert_eq!(coverage.candidate, restored.candidate);
        let other_role_index = tools::invoke(
            &input,
            &mut analysis,
            &mut Coverage::default(),
            !reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(other_role_index["items"][0]["detail_received"], false);
        let RecordData::Fact { value, .. } = &mut analysis.records.get_mut(&ids[0]).unwrap().data
        else {
            panic!("fixture fact")
        };
        value.push_str(" changed");
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            reviewer,
            "inspect_analysis",
            &index_query,
            16000,
        )
        .unwrap();
        assert_eq!(index["items"][0]["detail_received"], false);
        assert_eq!(
            coverage.candidate, restored.candidate,
            "index cannot refresh a stale receipt"
        );
    }
}

#[tokio::test]
async fn candidate_receipt_is_not_reported_received_before_model_delivery() {
    struct DetailThenIndex(String);
    #[async_trait]
    impl Model for DetailThenIndex {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: ["detail", "index"]
                    .into_iter()
                    .map(|view| ChatToolCall {
                        id: format!("inspect-{view}"),
                        name: "inspect_analysis".into(),
                        arguments:
                            json!({"kind":"all","ids":[self.0],"view":view,"offset":0,"limit":1})
                                .to_string(),
                    })
                    .collect(),
            })
        }
    }
    let (input, analysis, ids) = analysis_query_fixture();
    for role in [Role::Main, Role::Reviewer] {
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![("set_work_note", active_work("source"))]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut state = journal.load().await.unwrap().unwrap();
        state.analysis = analysis.clone();
        state.role = role.clone();
        if role == Role::Reviewer {
            state.source_review = Some(source_review::initialize(&input, &config()).unwrap());
            source_review::select_next(&input, &config(), &mut state).unwrap();
            state.reviewer_work = Some(serde_json::from_value(active_work("source")).unwrap());
        }
        *journal.state.lock().unwrap() = Some(state);
        *journal.interrupt_after.lock().unwrap() = Some(2);
        agent::run(
            &input,
            &config(),
            &journal,
            &DetailThenIndex(ids[0].clone()),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        assert!(
            if role == Role::Main {
                &saved.analysis.coverage
            } else {
                &saved.reviewer_coverage
            }
            .candidate
            .is_empty()
        );
        assert_eq!(saved.pending_coverage.as_ref().unwrap().candidate.len(), 1);
        let index: Value = serde_json::from_str(
            saved.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(index["result"]["items"][0]["detail_received"], false);
        let restored: Checkpoint = serde_json::from_value(json!(saved)).unwrap();
        *journal.state.lock().unwrap() = Some(restored);
        *journal.interrupt_after.lock().unwrap() = Some(3);
        agent::run(
            &input,
            &config(),
            &journal,
            &work_script(vec![(
                "inspect_analysis",
                json!({"kind":"all","ids":[ids[0]],"view":"index","offset":0,"limit":1}),
            )]),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let received = journal.load().await.unwrap().unwrap();
        assert_eq!(
            if role == Role::Main {
                &received.analysis.coverage
            } else {
                &received.reviewer_coverage
            }
            .candidate
            .len(),
            1
        );
        let index: Value = serde_json::from_str(
            received.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(index["result"]["items"][0]["detail_received"], true);
        let other = if role == Role::Main {
            &received.reviewer_coverage
        } else {
            &received.analysis.coverage
        };
        assert!(other.candidate.is_empty());
    }
}

#[test]
fn candidate_index_is_navigation_and_never_reviews_complete_objects() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    for kind in ["all", "relation", "disposition"] {
        let index = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"kind":kind,"offset":0,"limit":100}),
            16000,
        )
        .unwrap();
        assert_eq!(index["view"], "index");
        assert!(index["total"].as_u64().unwrap() > 0);
        assert!(coverage.candidate.is_empty());
        for row in index["items"].as_array().unwrap() {
            assert!(row["ref"].is_string());
            assert!(row.get("data").is_none());
            assert!(row.get("grounds").is_none());
            assert!(row.get("disposition").is_none());
        }
    }
    let query = json!({"kind":"all","ids":[ids[0]],"offset":0,"limit":1});
    let detail = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        16000,
    )
    .unwrap();
    assert_eq!(detail["view"], "detail");
    assert_eq!(detail["items"][0], json!(analysis.records[&ids[0]]));
    assert_eq!(coverage.candidate.len(), 1);
    let prior = coverage.candidate.clone();
    let mut indexed = query;
    indexed["view"] = json!("index");
    tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &indexed,
        16000,
    )
    .unwrap();
    assert_eq!(coverage.candidate, prior);
    assert!(
        tools::review_gaps(&input, &analysis, &coverage)
            .iter()
            .any(|g| g["kind"] == "unreviewed_candidate")
    );
}

#[test]
fn candidate_index_pages_fit_without_returning_full_candidate_content() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let record = analysis.records.get_mut(&ids[0]).unwrap();
    let RecordData::Fact { value, .. } = &mut record.data else {
        panic!("fixture fact")
    };
    *value = "large candidate body".repeat(1000);
    let mut coverage = Coverage::default();
    let full = json!({"kind":"all","ids":[ids[0]],"offset":0,"limit":1});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &full,
            4096
        )
        .is_err()
    );
    let mut query = full.clone();
    query["view"] = json!("index");
    let index = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &query,
        4096,
    )
    .unwrap();
    assert_eq!(index["items"][0]["id"], ids[0]);
    assert!(coverage.candidate.is_empty());
    // The envelope itself participates in the byte cap; pagination must
    // neither drop nor duplicate an ID, including the last short page.
    let all = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        true,
        "inspect_analysis",
        &json!({"kind":"all","offset":0,"limit":100}),
        16000,
    )
    .unwrap();
    let budget = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            serde_json::to_vec(
                &json!({"view":"index","total":all["total"],"next":all["total"],"items":[row]}),
            )
            .unwrap()
            .len()
        })
        .max()
        .unwrap();
    let mut offset = 0;
    let mut found = std::collections::BTreeSet::new();
    loop {
        let page = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &json!({"kind":"all","offset":offset,"limit":100}),
            budget,
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= budget);
        for row in page["items"].as_array().unwrap() {
            assert!(found.insert(row["ref"].as_str().unwrap().to_owned()));
        }
        let next = page["next"].as_u64().unwrap();
        assert!(next > offset);
        if next == page["total"].as_u64().unwrap() {
            break;
        }
        offset = next;
    }
    assert_eq!(found.len(), all["total"].as_u64().unwrap() as usize);
    assert!(found.iter().any(|key| key.starts_with("relation:")));
    assert!(found.iter().any(|key| key.starts_with("disposition:")));
    assert!(coverage.candidate.is_empty());
}

#[test]
fn inspect_analysis_rejects_invalid_selectors_and_oversize_without_partial_coverage() {
    let (input, mut analysis, ids) = analysis_query_fixture();
    let mut coverage = Coverage::default();
    for extra in [
        json!({"ids":[]}),
        json!({"ids":[ids[0],ids[0]]}),
        json!({"ids":[ids[0],"missing"]}),
        json!({"kind":"record","ids":[ids[3]]}),
        json!({"ids":null}),
        json!({"source_id":"missing"}),
        json!({"source_id":null}),
        json!({"view":null}),
        json!({"view":"summary"}),
    ] {
        let mut query = json!({"view":"detail","kind":"all","offset":0,"limit":10});
        query
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let before = digest(&analysis).unwrap();
        assert!(
            tools::invoke(
                &input,
                &mut analysis,
                &mut coverage,
                true,
                "inspect_analysis",
                &query,
                16000
            )
            .is_err(),
            "{query}"
        );
        assert!(coverage.candidate.is_empty());
        assert_eq!(before, digest(&analysis).unwrap());
    }
    let query = json!({"view":"detail","kind":"all","offset":0,"limit":1,"ids":[ids[0]]});
    assert!(
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            true,
            "inspect_analysis",
            &query,
            64
        )
        .is_err()
    );
    assert!(coverage.candidate.is_empty());
}

#[tokio::test]
async fn a_same_turn_read_cannot_authorize_a_write_before_the_model_sees_it() {
    struct ReadThenWrite;
    #[async_trait]
    impl Model for ReadThenWrite {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: vec![
                    ChatToolCall {
                        id: "work".into(),
                        name: "set_work_note".into(),
                        arguments: active_work("source").to_string(),
                    },
                    ChatToolCall {
                        id: "read".into(),
                        name: "read_source".into(),
                        arguments: json!({"source_id":"source","start":0,"max_bytes":1024})
                            .to_string(),
                    },
                    ChatToolCall {
                        id: "write".into(),
                        name: "put_record".into(),
                        arguments: requirement().to_string(),
                    },
                ],
            })
        }
    }
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(1);
    agent::run(
        &input(),
        &config(),
        &journal,
        &ReadThenWrite,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    assert!(
        saved.analysis.records.is_empty(),
        "unseen read results cannot ground a record"
    );
    assert!(saved.analysis.coverage.text.is_empty());
    let result: Value = serde_json::from_str(
        saved.transcript.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["ok"], false);
}

#[test]
fn read_source_errors_identify_repairable_offsets_without_claiming_coverage() {
    let input = input();
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let end = input.source_units[0].text.len();
    for (start, max_bytes, expected) in [
        (
            1,
            100,
            "INVALID_FIELD /start: byte 1 is inside a UTF-8 character; adjacent boundaries are 0 and 3",
        ),
        (end + 1, 100, "INVALID_FIELD /start: byte"),
        (0, 0, "INVALID_FIELD /max_bytes:"),
    ] {
        let error = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":start,"max_bytes":max_bytes}),
            4096,
        )
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(
            coverage.text.is_empty(),
            "a suggested boundary is not delivered text"
        );
    }
    // EOF remains a valid empty read. A caller can explicitly use a suggested
    // boundary; the tool never silently relocates the requested start.
    for start in [0, 3, end] {
        let result = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":"source","start":start,"max_bytes":100}),
            4096,
        )
        .unwrap();
        assert_eq!(result["start"], start);
    }
}

#[test]
fn text_span_errors_identify_the_bad_offset_without_adjusting_the_citation() {
    let input = input();
    let coverage = Coverage::default();
    for (start, end, expected) in [
        (0, 0, "start 0 must be less than end 0"),
        (
            0,
            input.source_units[0].text.len() + 1,
            "exceeds source length",
        ),
        (
            1,
            6,
            "start 1 is not a UTF-8 boundary; adjacent boundaries are 0 and 3",
        ),
        (
            0,
            4,
            "end 4 is not a UTF-8 boundary; adjacent boundaries are 3 and 6",
        ),
    ] {
        let citation = Span {
            start,
            end,
            ..span()
        };
        let error = tools::validate_span(&input, &coverage, &citation).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!((citation.start, citation.end), (start, end));
    }
    for (start, end) in [(0, 3), (3, 6)] {
        let citation = Span {
            start,
            end,
            ..span()
        };
        tools::validate_text_span(&input, &citation).unwrap();
        assert_eq!(
            tools::validate_span(&input, &coverage, &citation).unwrap_err(),
            "read the cited source range before using it",
            "choosing a suggested boundary must not establish delivery"
        );
    }
}

#[tokio::test]
async fn known_evidence_writes_can_complete_in_one_batch_but_failed_or_unseen_writes_cannot() {
    struct Batch(Vec<(&'static str, Value)>);
    #[async_trait]
    impl Model for Batch {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: self
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, (name, args))| ChatToolCall {
                        id: format!("batch-{i}"),
                        name: (*name).into(),
                        arguments: args.to_string(),
                    })
                    .collect(),
            })
        }
    }
    for (delivered, valid_write, compact) in [
        (true, true, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
        (false, true, true),
        (true, false, true),
    ] {
        let mut source = input();
        source.source_units[0].text = "项目名称为测试项目。".into();
        let journal = MemoryJournal::default();
        let mut setup = vec![("set_work_note", active_work("source"))];
        if delivered {
            setup.push((
                "read_source",
                json!({"source_id":"source","start":0,"max_bytes":1024}),
            ));
        }
        let setup_turns = setup.len();
        *journal.interrupt_after.lock().unwrap() = Some(setup_turns);
        agent::run(
            &source,
            &config(),
            &journal,
            &work_script(setup),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let mut complete = active_work("source");
        complete["status"] = json!("complete");
        let mut calls = vec![];
        if !delivered {
            calls.push((
                "read_source",
                json!({"source_id":"source","start":0,"max_bytes":1024}),
            ));
        }
        let start = if valid_write { 0 } else { 1 };
        let end = source.source_units[0].text.len();
        let citation = if compact {
            json!({"ref":format!("t:0:{start}:{end}")})
        } else {
            json!({"source_id":"source","start":start,"end":end})
        };
        calls.extend([
            ("put_record", json!({"id":null,"sources":[citation],
                "data":{"kind":"fact","name":"项目名称","value":"测试项目","scope":"本项目"}})),
            ("set_disposition", json!({"source_id":"source","state":"non_requirement","reason":"项目名称事实，无独立投标义务"})),
            ("set_work_note", complete),
        ]);
        *journal.interrupt_after.lock().unwrap() = Some(setup_turns + 1);
        agent::run(
            &source,
            &config(),
            &journal,
            &Batch(calls),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.turn, setup_turns + 1);
        let accepted = delivered && valid_write;
        assert_eq!(saved.analysis.records.len(), usize::from(accepted));
        assert_eq!(
            json!(saved.main_work.as_ref().unwrap().status) == "complete",
            accepted,
            "failed or not-yet-delivered writes cannot be hidden by same-batch completion"
        );
        assert!(
            !saved.done,
            "local completion never authorizes the whole analysis"
        );
        if !delivered {
            assert!(saved.analysis.coverage.text.is_empty());
            assert!(
                saved
                    .pending_coverage
                    .as_ref()
                    .unwrap()
                    .text
                    .contains_key("source")
            );
        }
    }
}

#[tokio::test]
async fn tool_batch_overflow_returns_feedback_and_only_retains_fitting_read_receipts() {
    struct BatchRead;
    #[async_trait]
    impl Model for BatchRead {
        async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
            let mut calls = vec![ChatToolCall {
                id: "work".into(),
                name: "set_work_note".into(),
                arguments: active_work("source").to_string(),
            }];
            for i in 0..6 {
                calls.push(ChatToolCall {
                    id: format!("read-{i}"),
                    name: "read_source".into(),
                    arguments: json!({"source_id":"source","start":i*12000,"max_bytes":12000})
                        .to_string(),
                });
            }
            Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls: calls,
            })
        }
    }
    for token_limited in [false, true] {
        let mut input = input();
        input.source_units[0].text = "x".repeat(72000);
        let mut config = config();
        config.limits.max_context_bytes = if token_limited { 200000 } else { 80000 };
        if token_limited {
            config.limits.max_context_tokens = 100000;
        }
        let journal = MemoryJournal::default();
        *journal.interrupt_after.lock().unwrap() = Some(1);
        let err = agent::run(
            &input,
            &config,
            &journal,
            &BatchRead,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.code, "INTERNAL",
            "a read overflow should save feedback, not strand the whole batch"
        );
        let saved = journal.load().await.unwrap().unwrap();
        assert!(saved.analysis.coverage.text.is_empty());
        let mut successes = 0;
        let mut rejected = 0;
        let mut delivered_ranges = Vec::new();
        for message in &saved.transcript {
            if message["role"] != "tool"
                || !message["tool_call_id"]
                    .as_str()
                    .unwrap()
                    .starts_with("read-")
            {
                continue;
            }
            let output: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
            if output["ok"] == true {
                successes += 1;
                let start = output["result"]["start"].as_u64().unwrap() as usize;
                let end = output["result"]["end"].as_u64().unwrap() as usize;
                assert_eq!(
                    output["result"]["text"],
                    input.source_units[0].text[start..end]
                );
                delivered_ranges.push((start, end));
            } else {
                rejected += 1;
                assert!(
                    output["error"]
                        .as_str()
                        .unwrap()
                        .contains("remaining batch context")
                );
            }
        }
        assert!(
            successes > 0 && rejected > 0,
            "token_limited={token_limited} successes={successes} rejected={rejected}"
        );
        assert_eq!(successes + rejected, 6);
        assert_eq!(
            saved.pending_coverage.as_ref().unwrap().text["source"],
            delivered_ranges
        );
        let mut state = saved.clone();
        let body = agent::request(&input, &config, &mut state).await.unwrap();
        assert!(body.len() <= config.limits.max_context_bytes);
        assert_eq!(
            digest(&saved.pending_coverage).unwrap(),
            digest(&state.pending_coverage).unwrap()
        );
        assert_eq!(saved.tool_calls, 7);
        assert_eq!(saved.turn, 1);
    }
}
