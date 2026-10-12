use super::*;

fn header_input(body_repeats: usize) -> FrozenInput {
    let mut input = crate::outline::tests::input();
    input.source_units[0].text.clear();
    input.structured_forms = vec![json!({"source_unit_revision_id":"source",
    "form_definition_revision_id":"synthetic-table","definition":{
    "row_count":2,"column_count":2,"cells":[
        {"row":0,"column":0,"text":"Complete left header","header_role":"column_header"},
        {"row":0,"column":1,"text":"Complete right header","header_role":"column_header"},
        {"row":1,"column":0,"text":"Synthetic body clause. ".repeat(body_repeats),"header_role":"none"},
        {"row":1,"column":1,"text":"Another body clause. ".repeat(body_repeats),"header_role":"none"}
    ]}})];
    input
}

fn full_wire_size(state: &crate::analysis::agent::Checkpoint, sessions: &[Value]) -> usize {
    let mut request = json!({"model":"synthetic-budget-model","messages":[
        {"role":"system","content":crate::outline::agent::OUTLINE_PROMPT},
        {"role":"user","content":json!({"reading_packs":sessions}).to_string()}
    ],"tools":crate::outline::agent::schemas_for(crate::outline::agent::Duty::Discover)});
    compact_request(&mut request).unwrap();
    crate::outline::source_wire::request(state, &mut request).unwrap();
    crate::outline::model_wire::project(state, &mut request).unwrap();
    crate::agent_runtime::canonical_request_bytes(&request)
        .unwrap()
        .len()
}

#[test]
fn repeated_grid_split_preserves_complete_support_headers_without_owning_them_twice() {
    let input = header_input(10);
    let definition =
        super::super::super::evidence::table_definition(&input, "synthetic-table").unwrap();
    let original = grid_carrier(definition, "synthetic-table").unwrap();
    let first = split_carrier(&input, &original).unwrap();
    let body = first.iter().find(|part|matches!(part,PackCarrier::Grid{cells,..} if cells.iter().all(|cell|cell.header_role=="none"))).unwrap();
    let second = split_carrier(&input, body).unwrap();
    assert_eq!(
        second.len(),
        2,
        "tight budget must split body rather than erase its headers"
    );
    for part in second {
        let PackCarrier::Grid {
            cells,
            header_context,
            ..
        } = part
        else {
            unreachable!()
        };
        assert!(cells.iter().all(|cell| cell.header_role == "none"));
        assert_eq!(header_context.len(), 1);
        assert!(header_context.iter().all(|cell| cell.start_byte == 0));
    }
    let header_part = first.iter().find(|part|matches!(part,PackCarrier::Grid{cells,..} if cells.iter().all(|cell|cell.header_role!="none"))).unwrap();
    for part in split_carrier(&input, header_part).unwrap() {
        let PackCarrier::Grid {
            cells,
            header_context,
            ..
        } = part
        else {
            unreachable!()
        };
        assert_eq!(cells.len(), 1);
        assert!(
            header_context.is_empty(),
            "unrelated column header is not required support"
        );
    }
}

#[test]
fn tight_full_wire_budget_keeps_header_text_and_unique_primary_coverage() {
    let input = header_input(1000);
    let state = crate::outline::acceptance::checkpoint(&input);
    // A synthetic byte ceiling exercises the production full-wire fit callback,
    // not a guessed tokenizer profile or an old fixed source byte chunk rule.
    let ceiling = full_wire_size(&state, &[]) + 6000;
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(full_wire_size(&state, sessions) <= ceiling)
    });
    assert!(
        work.planning_error().is_none(),
        "{:?}",
        work.planning_error()
    );
    assert!(work.pack_ids().len() > 1);
    work.claim(usize::MAX);
    let mut ranges = BTreeMap::<(usize, usize), Vec<(usize, usize)>>::new();
    for id in work.pack_ids() {
        let pack = work.canonical_pack(&id).unwrap();
        for atom in &pack.atoms {
            let PackCarrier::Grid {
                cells,
                header_context,
                ..
            } = &atom.carrier
            else {
                continue;
            };
            for column in cells
                .iter()
                .map(|cell| cell.anchor_column)
                .collect::<BTreeSet<_>>()
            {
                assert!(
                    cells
                        .iter()
                        .chain(header_context)
                        .any(|cell| cell.anchor_row == 0
                            && cell.anchor_column == column
                            && cell.start_byte == 0
                            && cell.end_byte == if column == 0 { 20 } else { 21 }),
                    "full native header missing from a budget fragment"
                );
            }
            for cell in cells {
                ranges
                    .entry((cell.anchor_row, cell.anchor_column))
                    .or_default()
                    .push((cell.start_byte, cell.end_byte));
            }
        }
        let session = work.session(&input, &id).unwrap();
        assert!(full_wire_size(&state, std::slice::from_ref(&session)) <= ceiling);
        let wire = serde_json::to_string(&session).unwrap();
        assert!(wire.contains("Complete left header") || wire.contains("Complete right header"));
    }
    let definition =
        super::super::super::evidence::table_definition(&input, "synthetic-table").unwrap();
    for cell in definition["cells"].as_array().unwrap() {
        let coordinate = (
            cell["row"].as_u64().unwrap() as usize,
            cell["column"].as_u64().unwrap() as usize,
        );
        let pieces = ranges.get_mut(&coordinate).unwrap();
        pieces.sort();
        let mut end = 0;
        for &(start, next) in pieces.iter() {
            assert_eq!(
                start, end,
                "primary ownership must be disjoint and gap-free"
            );
            end = next;
        }
        assert_eq!(end, cell["text"].as_str().unwrap().len());
    }
}

#[test]
fn impossible_header_budget_fails_explicitly_instead_of_erasing_context() {
    let input = header_input(1);
    let work = DiscoverWork::plan_with_budget(&input, &|_| Ok(false));
    assert!(
        work.planning_error()
            .is_some_and(|error| error.contains("required grid header context"))
    );
    assert!(work.pack_ids().is_empty());
}
