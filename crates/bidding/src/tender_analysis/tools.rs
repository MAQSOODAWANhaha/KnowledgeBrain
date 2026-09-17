use super::*;
use serde_json::json;
use std::collections::BTreeSet;

pub fn cover(ranges: &mut Vec<(usize, usize)>, start: usize, end: usize) {
    if start == end {
        return;
    }
    ranges.push((start, end));
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for &(a, b) in ranges.iter() {
        if let Some(last) = merged.last_mut().filter(|last| a <= last.1) {
            last.1 = last.1.max(b);
        } else {
            merged.push((a, b));
        }
    }
    *ranges = merged;
}

pub(super) fn contains(ranges: Option<&Vec<(usize, usize)>>, start: usize, end: usize) -> bool {
    ranges.is_some_and(|rs| rs.iter().any(|&(a, b)| a <= start && b >= end))
}

fn source<'a>(input: &'a FrozenInput, id: &str) -> Result<&'a Source, String> {
    input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == id)
        .ok_or_else(|| "unknown source in frozen collection".into())
}

fn form<'a>(input: &'a FrozenInput, id: &str) -> Result<&'a Value, String> {
    input
        .structured_forms
        .iter()
        .find(|f| f["form_definition_revision_id"] == id)
        .ok_or_else(|| "unknown frozen form".into())
}

pub fn validate_input(input: &FrozenInput) -> Result<(), String> {
    if input.schema_version != 1 || input.document_set_id.is_empty() {
        return Err("invalid frozen analysis identity".into());
    }
    let ids: BTreeSet<_> = input
        .source_units
        .iter()
        .map(|s| &s.source_unit_revision_id)
        .collect();
    if ids.len() != input.source_units.len() {
        return Err("duplicate source identity".into());
    }
    let mut forms = BTreeSet::new();
    for f in &input.structured_forms {
        let id = f["form_definition_revision_id"]
            .as_str()
            .ok_or("form identity missing")?;
        if !forms.insert(id) {
            return Err("duplicate form identity".into());
        }
        source(
            input,
            f["source_unit_revision_id"]
                .as_str()
                .ok_or("form source missing")?,
        )?;
        let definition = &f["definition"];
        if definition["schema_version"] != 3 || definition["kind"] != "grid" {
            return Err(format!("form {id}: schema 3 grid definition required"));
        }
        let grid: docparser::TableGrid = serde_json::from_value(definition.clone())
            .map_err(|error| format!("form {id}: {error}"))?;
        docparser::validate_table_grid(&grid).map_err(|error| format!("form {id}: {error}"))?;
    }
    Ok(())
}

pub fn validate_span(input: &FrozenInput, coverage: &Coverage, span: &Span) -> Result<(), String> {
    source(input, &span.source_id)?;
    if let Some(cell) = &span.grid_cell {
        let index = validate_grid_span(input, span)?;
        if !contains(coverage.form_cells.get(&cell.form_id), index, index + 1) {
            return Err("read the cited grid cell before using it".into());
        }
        return Ok(());
    }
    if let Some(id) = &span.view_id {
        if span.start != 0
            || span.end != 0
            || coverage
                .views
                .get(id)
                .is_none_or(|v| v.source_id != span.source_id)
        {
            return Err(
                "visual citation requires a delivered source view and zero text offsets".into(),
            );
        }
        return Ok(());
    }
    validate_text_span(input, span)?;
    if !contains(coverage.text.get(&span.source_id), span.start, span.end) {
        return Err("read the cited source range before using it".into());
    }
    Ok(())
}

/// Validate a planned grid location without granting a reading receipt.
pub(super) fn validate_grid_span(input: &FrozenInput, span: &Span) -> Result<usize, String> {
    source(input, &span.source_id)?;
    let cell = span.grid_cell.as_ref().ok_or("grid cell missing")?;
    if span.start != 0 || span.end != 0 || span.view_id.is_some() {
        return Err("grid citation requires zero text offsets and no visual citation".into());
    }
    let f = form(input, &cell.form_id)?;
    if f["source_unit_revision_id"] != span.source_id
        || !super::relations::grid_cell_is_anchor(&f["definition"], cell.row, cell.column)
    {
        return Err("grid citation must identify an anchor in this source".into());
    }
    let columns = super::relations::form_dimensions(&f["definition"])
        .ok_or("form grid unavailable")?
        .1;
    super::relations::dense_index(cell.row, cell.column, columns)
        .ok_or_else(|| "grid citation cell missing".into())
}

/// A planned text location is not a delivered citation.
pub(super) fn validate_text_span(input: &FrozenInput, span: &Span) -> Result<(), String> {
    let text = &source(input, &span.source_id)?.text;
    if span.start >= span.end {
        return Err(format!(
            "text start {} must be less than end {}; for locate on an unread grid-only source, use source_spans=[] and read_form",
            span.start, span.end
        ));
    }
    if span.end > text.len() {
        return Err(format!(
            "text end {} exceeds source length {} bytes; use read_source line_spans for exact offsets",
            span.end,
            text.len()
        ));
    }
    for (field, offset) in [("start", span.start), ("end", span.end)] {
        if !text.is_char_boundary(offset) {
            return Err(format!(
                "text {field} {offset} is not a UTF-8 boundary; adjacent boundaries are {} and {}; use read_source line_spans for exact offsets, not character counts",
                text.floor_char_boundary(offset),
                text.ceil_char_boundary(offset)
            ));
        }
    }
    Ok(())
}

pub(super) fn field_error(path: &str, detail: impl std::fmt::Display) -> String {
    format!("INVALID_FIELD {path}: {detail}")
}

fn nonempty(value: &str, path: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(field_error(
            path,
            "empty semantic value; provide source-grounded text",
        ))
    } else {
        Ok(())
    }
}

fn validate_grounds(
    input: &FrozenInput,
    coverage: &Coverage,
    spans: &[Span],
    path: &str,
) -> Result<(), String> {
    if spans.is_empty() {
        return Err(field_error(path, "source evidence must not be empty"));
    }
    for (index, span) in spans.iter().enumerate() {
        validate_span(input, coverage, span)
            .map_err(|error| field_error(&format!("{path}/{index}"), error))?;
    }
    Ok(())
}

fn validate_applicability(
    input: &FrozenInput,
    coverage: &Coverage,
    a: &Applicability,
) -> Result<(), String> {
    nonempty(&a.scope, "/data/applicability/scope")?;
    nonempty(&a.condition, "/data/applicability/condition")?;
    validate_grounds(input, coverage, &a.grounds, "/data/applicability/grounds")
}

fn validate_rule_items(
    input: &FrozenInput,
    analysis: &Analysis,
    record_id: &str,
    items: &[RuleItem],
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        let path = format!("/data/items/{index}");
        nonempty(&item.id, &format!("{path}/id"))?;
        if !seen.insert(&item.id) {
            return Err(field_error(&format!("{path}/id"), "duplicate rule item id"));
        }
        nonempty(&item.text, &format!("{path}/text"))?;
        validate_grounds(
            input,
            &analysis.coverage,
            &item.grounds,
            &format!("{path}/grounds"),
        )?;
        if item.kind != RuleItemKind::Order && !item.sequence.is_empty() {
            return Err(field_error(
                &format!("{path}/sequence"),
                "sequence is only valid on order items",
            ));
        }
        if item.kind != RuleItemKind::Format
            && (item.format_key.is_some() || item.format_value.is_some())
        {
            return Err(field_error(
                &path,
                "format fields are only valid on format items",
            ));
        }
        match item.kind {
            RuleItemKind::Order => {
                if item.sequence.is_empty() {
                    return Err(field_error(
                        &format!("{path}/sequence"),
                        "order items need a sequence of item ids; first save the source-backed component items without this new order item, then add it using their returned IDs from the same rule. Document precedence is not output order; do not relabel a genuine output-order requirement to bypass this check",
                    ));
                }
                let mut sequence = BTreeSet::new();
                for (position, id) in item.sequence.iter().enumerate() {
                    if id == &item.id
                        || !items.iter().any(|other| &other.id == id)
                        || !sequence.insert(id)
                    {
                        return Err(field_error(
                            &format!("{path}/sequence/{position}"),
                            "sequence must reference distinct other items in this rule; retrieve this saved rule's returned component IDs, not placeholders, predicted IDs, self references or IDs from another rule. A rejected write does not allocate usable IDs",
                        ));
                    }
                }
            }
            RuleItemKind::Format => {
                nonempty(
                    item.format_key.as_deref().unwrap_or(""),
                    &format!("{path}/format_key"),
                )?;
                nonempty(
                    item.format_value.as_deref().unwrap_or(""),
                    &format!("{path}/format_value"),
                )?;
            }
            _ => {}
        }
        for (t_index, target) in item.targets.iter().enumerate() {
            let tpath = format!("{path}/targets/{t_index}");
            match target {
                RuleItemTarget::Record { id } => {
                    if id != record_id && !analysis.records.contains_key(id) {
                        return Err(field_error(&tpath, "unknown record target"));
                    }
                }
                RuleItemTarget::RuleItem {
                    record_id: rid,
                    item_id,
                } => {
                    let known = rid == record_id
                        && item_id != &item.id
                        && items.iter().any(|other| other.id == *item_id);
                    if !known {
                        return Err(field_error(
                            &tpath,
                            "target must reference another current item in the same rule",
                        ));
                    }
                }
                RuleItemTarget::Unresolved { reason } => {
                    nonempty(reason, &format!("{tpath}/reason"))?;
                }
            }
        }
    }
    Ok(())
}

pub fn validate_record(
    input: &FrozenInput,
    analysis: &Analysis,
    record: &Record,
) -> Result<(), String> {
    validate_grounds(input, &analysis.coverage, &record.sources, "/sources")?;
    match &record.data {
        RecordData::Fact { name, value, scope } => {
            nonempty(name, "/data/name")?;
            nonempty(value, "/data/value")?;
            nonempty(scope, "/data/scope")?;
        }
        RecordData::Rule {
            text,
            scope,
            applicability,
            items,
        } => {
            nonempty(text, "/data/text")?;
            nonempty(scope, "/data/scope")?;
            validate_applicability(input, &analysis.coverage, applicability)?;
            validate_rule_items(input, analysis, &record.id, items)?;
        }
        RecordData::Requirement {
            text,
            categories,
            applicability,
            response,
            compliance,
            criteria,
            proofs,
            ..
        } => {
            nonempty(text, "/data/text")?;
            if categories.is_empty() {
                return Err(field_error(
                    "/data/categories",
                    "requirement needs semantic categories",
                ));
            }
            if response.is_empty()
                && compliance
                    .iter()
                    .any(|claim| claim.policy == Compliance::ExplicitResponse)
            {
                return Err(field_error(
                    "/data/response",
                    "explicit_response policy needs a grounded response obligation",
                ));
            }
            for (index, need) in response.iter().enumerate() {
                let path = format!("/data/response/{index}");
                nonempty(&need.description, &format!("{path}/description"))?;
                // The response may inherit its parent's conditions without
                // repeating them or inventing an additional trigger.
                if !need.condition.is_empty() {
                    nonempty(&need.condition, &format!("{path}/condition"))?;
                }
                validate_grounds(
                    input,
                    &analysis.coverage,
                    &need.grounds,
                    &format!("{path}/grounds"),
                )?;
            }
            validate_applicability(input, &analysis.coverage, applicability)?;
            if compliance.is_empty() {
                return Err(field_error(
                    "/data/compliance",
                    "requirement needs explicit compliance assessment",
                ));
            }
            let mut claims = BTreeMap::new();
            for (index, claim) in compliance.iter().enumerate() {
                let path = format!("/data/compliance/{index}");
                nonempty(&claim.condition, &format!("{path}/condition"))?;
                if let Some(first) = claims.insert(digest(claim)?, index) {
                    return Err(field_error(
                        &path,
                        format!(
                            "duplicate compliance claim; identical policy, condition and grounds already occur at /data/compliance/{first}"
                        ),
                    ));
                }
                validate_grounds(
                    input,
                    &analysis.coverage,
                    &claim.grounds,
                    &format!("{path}/grounds"),
                )?;
            }
            for (index, criterion) in criteria.iter().enumerate() {
                let path = format!("/data/criteria/{index}");
                for (name, value) in [
                    ("subject", &criterion.subject),
                    ("aspect", &criterion.aspect),
                    ("operator", &criterion.operator),
                    ("value", &criterion.value),
                    ("condition", &criterion.condition),
                ] {
                    nonempty(value, &format!("{path}/{name}"))?;
                }
                validate_grounds(
                    input,
                    &analysis.coverage,
                    &criterion.grounds,
                    &format!("{path}/grounds"),
                )?;
            }
            for (index, proof) in proofs.iter().enumerate() {
                let path = format!("/data/proofs/{index}");
                for (name, value) in [
                    ("description", &proof.description),
                    ("subject", &proof.subject),
                    ("condition", &proof.condition),
                ] {
                    nonempty(value, &format!("{path}/{name}"))?;
                }
                validate_grounds(
                    input,
                    &analysis.coverage,
                    &proof.grounds,
                    &format!("{path}/grounds"),
                )?;
            }
        }
        RecordData::Template {
            title,
            parent,
            purpose,
            applicability,
            regions,
            ..
        } => {
            nonempty(title, "/data/title")?;
            nonempty(purpose, "/data/purpose")?;
            validate_applicability(input, &analysis.coverage, applicability)?;
            let mut visited = BTreeSet::from([record.id.as_str()]);
            let mut next = parent.as_deref();
            while let Some(id) = next {
                if !visited.insert(id) {
                    return Err(field_error("/data/parent", "cyclic appendix parent"));
                }
                let p = analysis
                    .records
                    .get(id)
                    .ok_or_else(|| field_error("/data/parent", "unknown appendix parent"))?;
                let RecordData::Template { parent, .. } = &p.data else {
                    return Err(field_error("/data/parent", "parent is not a template"));
                };
                next = parent.as_deref();
            }
            if regions.is_empty() {
                return Err(field_error(
                    "/data/regions",
                    "template needs actual source regions",
                ));
            }
            let mut assigned = BTreeMap::new();
            let mut seen_forms = BTreeSet::new();
            let mut text_ranges: BTreeMap<&str, Vec<(usize, usize, usize)>> = BTreeMap::new();
            for (region_index, region) in regions.iter().enumerate() {
                let path = format!("/data/regions/{region_index}");
                let fail =
                    |field: &str, message: &str| field_error(&format!("{path}/{field}"), message);
                validate_span(input, &analysis.coverage, &region.source)
                    .map_err(|error| fail("source", &error))?;
                if !region.blank_ranges.is_empty()
                    && (region.role != RegionRole::BidderBlank || region.form_id.is_none())
                {
                    return Err(fail(
                        "blank_ranges",
                        "partial cell blanks require a bidder_blank grid region",
                    ));
                }
                if let Some(cell) = &region.source.grid_cell
                    && (region.form_id.as_ref() != Some(&cell.form_id)
                        || !region
                            .cells
                            .iter()
                            .any(|c| c.row == cell.row && c.column == cell.column))
                {
                    return Err(fail(
                        "cells",
                        "grid-cited template region must include that cell in its grid policy",
                    ));
                }
                if let Some(id) = &region.form_id {
                    // The compiler emits one complete table per consecutive
                    // group. Source order must be resolved by the Agent.
                    if (region_index == 0 || regions[region_index - 1].form_id.as_ref() != Some(id))
                        && !seen_forms.insert(id)
                    {
                        return Err(fail(
                            "form_id",
                            "regions for the same grid must be contiguous; inspect source order before revising the template",
                        ));
                    }
                    let f = form(input, id).map_err(|error| fail("form_id", &error))?;
                    if f["source_unit_revision_id"] != region.source.source_id {
                        return Err(fail("source/source_id", "grid and region source disagree"));
                    }
                    let columns = super::relations::form_dimensions(&f["definition"])
                        .ok_or_else(|| fail("form_id", "form grid unavailable"))?
                        .1;
                    if region.cells.is_empty() {
                        return Err(fail("cells", "grid region must identify cells"));
                    }
                    if region.blank_ranges.iter().any(|range| {
                        !region
                            .cells
                            .iter()
                            .any(|cell| cell.row == range.row && cell.column == range.column)
                    }) {
                        return Err(fail(
                            "blank_ranges",
                            "cell blank range is outside its region's cells",
                        ));
                    }
                    for (cell_index, cell) in region.cells.iter().enumerate() {
                        let cell_path = format!("cells/{cell_index}");
                        if !super::relations::grid_cell_is_anchor(
                            &f["definition"],
                            cell.row,
                            cell.column,
                        ) {
                            return Err(fail(
                                &cell_path,
                                "template cell must be an actual grid anchor, not a covered merged position",
                            ));
                        }
                        let index = super::relations::dense_index(cell.row, cell.column, columns)
                            .ok_or_else(|| fail(&cell_path, "foreign grid cell"))?;
                        if !contains(analysis.coverage.form_cells.get(id), index, index + 1) {
                            return Err(fail(&cell_path, "read grid cell before assigning it"));
                        }
                        if !region.blank_ranges.is_empty() {
                            if !region
                                .blank_ranges
                                .iter()
                                .any(|r| r.row == cell.row && r.column == cell.column)
                            {
                                return Err(fail(
                                    &cell_path,
                                    "each cell in a partial-blank region needs explicit ranges",
                                ));
                            }
                            let text = super::relations::sparse_cell(
                                &f["definition"],
                                cell.row,
                                cell.column,
                            )
                            .and_then(|cell| cell["text"].as_str())
                            .ok_or_else(|| fail(&cell_path, "grid cell text missing"))?;
                            crate::template_grid::blank_cell_text(
                                text,
                                cell.row,
                                cell.column,
                                &region.blank_ranges,
                            )
                            .map_err(|error| fail("blank_ranges", &error))?;
                        }
                        let key = (id, cell.row, cell.column);
                        if assigned.insert(key, &region.role).is_some() {
                            return Err(fail(
                                &cell_path,
                                "multiple policies for the same template cell",
                            ));
                        }
                    }
                } else if !region.cells.is_empty() {
                    return Err(fail("form_id", "cell policy without a form"));
                } else if region.source.view_id.is_some() {
                    return Err(fail(
                        "source",
                        "template wording needs editable source text; retain visual-only wording as unresolved until the shared parser supplies it",
                    ));
                } else {
                    let span = &region.source;
                    let prior = text_ranges.entry(&span.source_id).or_default();
                    if let Some((_, _, previous_index)) = prior
                        .iter()
                        .find(|(start, end, _)| span.start < *end && *start < span.end)
                    {
                        return Err(fail(
                            "source",
                            &format!(
                                "template text regions {previous_index} and {region_index} overlap; split fixed wording, bidder blanks and signatures into distinct UTF-8 source ranges"
                            ),
                        ));
                    }
                    prior.push((span.start, span.end, region_index));
                }
            }
            // Compiler refuses incomplete grids. Extract must fail at put_record
            // rather than after an independently reviewed analysis is frozen.
            for id in regions
                .iter()
                .filter_map(|region| region.form_id.as_deref())
                .collect::<BTreeSet<_>>()
            {
                let f = form(input, id)?;
                let definition = &f["definition"];
                let cells = definition["cells"].as_array().ok_or("grid cells missing")?;
                let mut anchors = BTreeSet::new();
                for cell in cells {
                    let row = cell["row"].as_u64().ok_or("grid cell row")? as usize;
                    let column = cell["column"].as_u64().ok_or("grid cell column")? as usize;
                    if super::relations::grid_cell_is_anchor(definition, row, column) {
                        anchors.insert((row, column));
                    }
                }
                let owned: BTreeSet<_> = assigned
                    .iter()
                    .filter_map(|((form_id, row, column), _)| {
                        (form_id.as_str() == id).then_some((*row, *column))
                    })
                    .collect();
                if owned != anchors {
                    return Err(field_error(
                        "/data/regions",
                        format!(
                            "every output grid anchor needs an explicit fixed/blank/instruction/signature policy; form {id}, first missing anchor {:?}",
                            anchors.difference(&owned).next()
                        ),
                    ));
                }
            }
        }
        RecordData::Unresolved { problem, .. } => nonempty(problem, "/data/problem")?,
    }
    Ok(())
}

pub fn validate_relation(
    input: &FrozenInput,
    analysis: &Analysis,
    relation: &Relation,
) -> Result<(), String> {
    for (path, id) in [("/from", &relation.from), ("/to", &relation.to)] {
        if !analysis.records.contains_key(id) {
            return Err(field_error(
                path,
                "relation endpoints must be existing records",
            ));
        }
    }
    super::relations::validate_endpoints(input, analysis, relation)?;
    nonempty(&relation.scope, "/scope")?;
    nonempty(&relation.explanation, "/explanation")?;
    readable_relation_prose(&relation.scope, "/scope")?;
    readable_relation_prose(&relation.explanation, "/explanation")?;
    validate_grounds(input, &analysis.coverage, &relation.grounds, "/grounds")?;
    if relation.kind == RelationKind::RequiresTemplate
        && !matches!(
            analysis.records[&relation.to].data,
            RecordData::Template { .. }
        )
    {
        return Err(field_error(
            "/to",
            "requires_template target must be a template",
        ));
    }
    if relation.kind == RelationKind::Contains {
        let RecordData::Template { parent, .. } = &analysis.records[&relation.to].data else {
            return Err(field_error("/to", "contains target must be a template"));
        };
        if parent.as_deref() != Some(&relation.from) {
            return Err(field_error(
                "/from",
                "contains relation and template parent disagree",
            ));
        }
    }
    Ok(())
}

/// These two fields are authored explanations, not source quotations or code.
/// Recognize a wholly encoded prose value without decoding or rewriting it.
/// Contextual literal examples, ordinary prose, paths and regexes are retained.
fn readable_relation_prose(value: &str, path: &str) -> Result<(), String> {
    let mut rest = value.trim();
    let mut encoded_non_ascii = false;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(r"\u") {
            let Some(hex) = after
                .get(..4)
                .filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return Ok(());
            };
            encoded_non_ascii |= u16::from_str_radix(hex, 16).is_ok_and(|unit| unit > 127);
            rest = &after[4..];
        } else {
            let first = rest.as_bytes()[0];
            if first == b'\\'
                || !(first.is_ascii_digit()
                    || first.is_ascii_punctuation()
                    || first.is_ascii_whitespace())
            {
                return Ok(());
            }
            rest = &rest[1..];
        }
    }
    if encoded_non_ascii {
        return Err(field_error(
            path,
            "explanation consists of literal JSON Unicode escapes; write readable source-grounded prose. If describing a literal code example, explain its purpose and retain the literal example. Sources are never decoded again.",
        ));
    }
    Ok(())
}

pub fn reading_gaps(input: &FrozenInput, coverage: &Coverage) -> Vec<Value> {
    let mut gaps = Vec::new();
    for (kind, values) in [
        ("documents", &input.documents),
        ("document_relations", &input.document_relations),
        ("decisions", &input.decisions),
    ] {
        if !values.is_empty() && !contains(coverage.metadata.get(kind), 0, values.len()) {
            gaps.push(json!({"kind":"unread_metadata","collection":kind,"total":values.len()}));
        }
    }
    for s in &input.source_units {
        if s.text.is_empty() {
            continue;
        }
        let mut offset = 0;
        for &(a, b) in coverage
            .text
            .get(&s.source_unit_revision_id)
            .into_iter()
            .flatten()
        {
            if a > offset {
                gaps.push(json!({"kind":"unread_source","source_id":s.source_unit_revision_id,"start":offset,"end":a}));
            }
            offset = offset.max(b);
        }
        if offset < s.text.len() {
            gaps.push(json!({"kind":"unread_source","source_id":s.source_unit_revision_id,"start":offset,"end":s.text.len()}));
        }
    }
    for f in &input.structured_forms {
        let id = f["form_definition_revision_id"]
            .as_str()
            .unwrap_or_default();
        let n = super::relations::form_total(&f["definition"]).unwrap_or(0);
        if n > 0 && !contains(coverage.form_cells.get(id), 0, n) {
            gaps.push(json!({"kind":"unread_grid","form_id":id,"cell_count":n,"read_ranges":coverage.form_cells.get(id)}));
        }
    }
    // Keep metadata first, then interleave text and grids in frozen source order.
    // Grouping by representation postpones every table until all text is read.
    gaps.sort_by_cached_key(|gap| {
        if gap["kind"] == "unread_metadata" {
            return (0, 0);
        }
        let source_id = gap["source_id"].as_str().or_else(|| {
            input
                .structured_forms
                .iter()
                .find(|form| form["form_definition_revision_id"] == gap["form_id"])
                .and_then(|form| form["source_unit_revision_id"].as_str())
        });
        (
            1,
            input
                .source_units
                .iter()
                .position(|source| Some(source.source_unit_revision_id.as_str()) == source_id)
                .unwrap_or(usize::MAX),
        )
    });
    gaps
}

pub fn gaps(input: &FrozenInput, analysis: &Analysis) -> Vec<Value> {
    let mut gaps = reading_gaps(input, &analysis.coverage);
    for s in &input.source_units {
        match analysis.dispositions.get(&s.source_unit_revision_id) {
            None => gaps.push(json!({"kind":"missing_disposition","source_id":s.source_unit_revision_id})),
            Some(d) if d.state == DispositionState::Requirement && !analysis.records.values().any(|r|
                matches!(r.data,RecordData::Requirement{..}|RecordData::Rule{..}|RecordData::Template{..}) &&
                r.sources.iter().any(|span|span.source_id == s.source_unit_revision_id)) => {
                gaps.push(json!({"kind":"disposition_without_record","source_id":s.source_unit_revision_id}));
            }
            Some(d) if d.state == DispositionState::NonRequirement && analysis.records.values().any(|r|
                matches!(&r.data,RecordData::Requirement{applicability,..} if applicability.state==ApplicabilityState::Applicable)
                && r.sources.iter().any(|span|span.source_id==s.source_unit_revision_id)) => {
                gaps.push(json!({"kind":"disposition_contradicts_requirement","source_id":s.source_unit_revision_id}));
            }
            _ => {}
        }
    }
    for r in analysis.records.values() {
        if let Err(error) = validate_record(input, analysis, r) {
            gaps.push(json!({"kind":"invalid_record","id":r.id,"error":error}));
        }
    }
    for r in analysis.relations.values() {
        if let Err(error) = validate_relation(input, analysis, r) {
            gaps.push(json!({"kind":"invalid_relation","id":r.id,"error":error}));
        }
    }
    gaps
}

fn candidate_rows(analysis: &Analysis, kind: &str) -> Result<Vec<(String, Value)>, String> {
    filtered_candidate_rows(analysis, kind, None, None).map_err(String::from)
}

/// Preserve identity failures for host navigation without interpreting prose.
#[derive(Debug)]
pub(super) enum InspectionError {
    CandidateIdentity(String),
    Other(String),
}

impl From<String> for InspectionError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for InspectionError {
    fn from(message: &str) -> Self {
        Self::Other(message.into())
    }
}

impl From<InspectionError> for String {
    fn from(error: InspectionError) -> Self {
        match error {
            InspectionError::CandidateIdentity(message) | InspectionError::Other(message) => {
                message
            }
        }
    }
}

fn selected_entries<'a, T>(
    entries: &'a BTreeMap<String, T>,
    ids: Option<&BTreeSet<String>>,
) -> Result<Vec<(&'a String, &'a T)>, InspectionError> {
    match ids {
        None => Ok(entries.iter().collect()),
        Some(ids) => ids
            .iter()
            .map(|id| {
                entries
                    .get_key_value(id)
                    .ok_or_else(|| InspectionError::CandidateIdentity(field_error("/ids", format!(
                        "unknown candidate id in requested category: {id}. Use kind=all for mixed categories, kind=record for all record kinds, kind=relation for relations, or kind=disposition for source dispositions. Pass IDs without the reference prefix; use the current index if the ID is stale."
                    ))))
            })
            .collect(),
    }
}

fn filtered_candidate_rows(
    analysis: &Analysis,
    kind: &str,
    ids: Option<&BTreeSet<String>>,
    source_ids: Option<&BTreeSet<&str>>,
) -> Result<Vec<(String, Value)>, InspectionError> {
    if kind == "all" {
        let groups = ["record", "relation", "disposition"];
        let contains = |category: &str, id: &str| match category {
            "record" => analysis.records.contains_key(id),
            "relation" => analysis.relations.contains_key(id),
            _ => analysis.dispositions.contains_key(id),
        };
        if let Some(ids) = ids {
            for id in ids {
                match groups
                    .iter()
                    .filter(|category| contains(category, id))
                    .count()
                {
                    1 => {}
                    0 => {
                        return Err(InspectionError::CandidateIdentity(field_error(
                            "/ids",
                            format!(
                                "unknown candidate ID: {id}; use the current index and pass IDs without the reference prefix"
                            ),
                        )));
                    }
                    _ => {
                        return Err(field_error(
                            "/ids",
                            format!(
                                "ambiguous candidate ID: {id}; select kind=record, kind=relation or kind=disposition"
                            ),
                        ).into());
                    }
                }
            }
        }
        let mut rows = Vec::new();
        for category in groups {
            let selected = ids.map(|ids| {
                ids.iter()
                    .filter(|id| contains(category, id))
                    .cloned()
                    .collect::<BTreeSet<_>>()
            });
            rows.extend(filtered_candidate_rows(
                analysis,
                category,
                selected.as_ref(),
                source_ids,
            )?);
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(rows);
    }
    let record_matches = |record: &Record| {
        source_ids.is_none_or(|ids| {
            record
                .sources
                .iter()
                .any(|s| ids.contains(s.source_id.as_str()))
        })
    };
    Ok(match kind {
        "relation" => selected_entries(&analysis.relations, ids)?
            .into_iter()
            .filter(|(_, relation)| {
                source_ids.is_none_or(|ids| {
                    relation
                        .grounds
                        .iter()
                        .any(|s| ids.contains(s.source_id.as_str()))
                        || [&relation.from, &relation.to]
                            .iter()
                            .any(|id| analysis.records.get(*id).is_some_and(record_matches))
                })
            })
            .map(|(id, v)| (format!("relation:{id}"), json!(v)))
            .collect(),
        "disposition" => selected_entries(&analysis.dispositions, ids)?
            .into_iter()
            .filter(|(id, _)| source_ids.is_none_or(|ids| ids.contains(id.as_str())))
            .map(|(id, v)| {
                (
                    format!("disposition:{id}"),
                    json!({"source_id":id,"disposition":v}),
                )
            })
            .collect(),
        "record" | "fact" | "rule" | "requirement" | "template" | "unresolved" => {
            let entries = selected_entries(&analysis.records, ids)?;
            if ids.is_some()
                && kind != "record"
                && entries.iter().any(|(_, record)| record.data.kind() != kind)
            {
                return Err(InspectionError::CandidateIdentity(
                    "candidate id does not belong to requested record kind".into(),
                ));
            }
            entries
                .into_iter()
                .filter(|(_, v)| (kind == "record" || v.data.kind() == kind) && record_matches(v))
                .map(|(id, v)| (format!("record:{id}"), json!(v)))
                .collect()
        }
        _ => return Err("unknown analysis category".into()),
    })
}

pub fn review_gaps(input: &FrozenInput, analysis: &Analysis, coverage: &Coverage) -> Vec<Value> {
    let mut gaps = reading_gaps(input, coverage);
    for (id, view) in &analysis.coverage.views {
        if coverage.views.get(id) != Some(view) {
            gaps.push(
                json!({"kind":"unreviewed_source_view","view_id":id,"source_id":view.source_id}),
            );
        }
    }
    for kind in ["record", "relation", "disposition"] {
        for (key, value) in candidate_rows(analysis, kind).expect("known category") {
            if coverage.candidate.get(&key) != digest(&value).ok().as_ref() {
                gaps.push(json!({"kind":"unreviewed_candidate","key":key,"category":kind}));
            }
        }
    }
    gaps
}

fn object(args: &Value, allowed: &[&str]) -> Result<(), String> {
    let obj = args
        .as_object()
        .ok_or_else(|| field_error("", "tool arguments must be an object"))?;
    if let Some(key) = obj.keys().find(|key| !allowed.contains(&key.as_str())) {
        let path = format!("/{}", key.replace('~', "~0").replace('/', "~1"));
        return Err(field_error(&path, "unknown tool argument"));
    }
    Ok(())
}
fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str()
        .ok_or_else(|| field_error(&format!("/{k}"), "must be a string"))
}
fn number(v: &Value, k: &str) -> Result<usize, String> {
    v[k].as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| field_error(&format!("/{k}"), "must be a nonnegative integer"))
}

/// Exact fragments of the delivered parsed text, including original line
/// endings. These are byte locations, not inferred clauses or reading receipts.
pub(super) fn source_line_spans(text: &str, start: usize) -> Vec<Value> {
    let mut cursor = start;
    text.split_inclusive('\n')
        .map(|line| {
            let start = cursor;
            cursor += line.len();
            json!({"start":start,"end":cursor,"text":line})
        })
        .collect()
}

/// Return only complete items, with a stable continuation and a byte-bounded envelope.
pub(crate) fn bounded_page<T: Serialize>(
    rows: &[T],
    start: usize,
    limit: usize,
    max_bytes: usize,
) -> Result<Value, String> {
    if limit == 0 || start > rows.len() {
        return Err(
            "invalid page range; offset must not exceed total and limit must be positive".into(),
        );
    }
    let page = |end| json!({"total":rows.len(),"next":end,"items":&rows[start..end]});
    let mut end = start;
    let mut upper = start.saturating_add(limit).min(rows.len());
    while end < upper {
        let middle = end + (upper - end).div_ceil(2);
        if serde_json::to_vec(&page(middle))
            .map_err(|e| e.to_string())?
            .len()
            <= max_bytes
        {
            end = middle;
        } else {
            upper = middle - 1;
        }
    }
    let out = page(end);
    if (end == start && start < rows.len())
        || serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > max_bytes
    {
        return Err(format!(
            "one complete result at offset {start} cannot fit the tool result budget"
        ));
    }
    Ok(out)
}

/// An active Agent scope supplies the default filter. Explicit IDs or a source
/// select cross-reference targets; collection scope navigates the whole analysis
/// without changing the active task or granting any reading receipt.
pub(super) fn inspect_analysis(
    input: &FrozenInput,
    analysis: &Analysis,
    coverage: &mut Coverage,
    received: &Coverage,
    args: &Value,
    max_bytes: usize,
    default_scope: Option<&[String]>,
) -> Result<Value, InspectionError> {
    object(
        args,
        &[
            "kind",
            "offset",
            "limit",
            "ids",
            "source_id",
            "view",
            "scope",
        ],
    )?;
    let scope = match args.get("scope") {
        None => "work",
        Some(value) => value.as_str().ok_or("scope must be work or collection")?,
    };
    if !matches!(scope, "work" | "collection") {
        return Err("scope must be work or collection".into());
    }
    let view = match args.get("view") {
        Some(value) => value.as_str().ok_or("view must be index or detail")?,
        None if args.get("ids").is_some() => "detail",
        None => "index",
    };
    if !matches!(view, "index" | "detail") {
        return Err("view must be index or detail".into());
    }
    let start = number(args, "offset")?;
    let limit = number(args, "limit")?.min(max_bytes);
    let ids = args
        .get("ids")
        .map(|value| {
            let values: Vec<String> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            let ids: BTreeSet<_> = values.iter().cloned().collect();
            if ids.is_empty()
                || ids.len() != values.len()
                || ids.iter().any(|id| id.trim().is_empty())
            {
                return Err("ids must be a nonempty array of distinct candidate IDs".into());
            }
            Ok::<_, String>(ids)
        })
        .transpose()?;
    let source_id = args
        .get("source_id")
        .map(|_| {
            let id = string(args, "source_id")?;
            source(input, id)?;
            Ok::<_, String>(id)
        })
        .transpose()?;
    // Select by frozen identity before serializing candidate content;
    // looking up one record must not clone the entire semantic graph.
    let source_ids: Option<BTreeSet<&str>> = if let Some(source_id) = source_id {
        Some(BTreeSet::from([source_id]))
    } else if ids.is_none() && scope == "work" {
        default_scope.map(|scope| scope.iter().map(String::as_str).collect())
    } else {
        None
    };
    let rows = filtered_candidate_rows(
        analysis,
        string(args, "kind")?,
        ids.as_ref(),
        source_ids.as_ref(),
    )?;
    if limit == 0 || start > rows.len() {
        return Err("invalid analysis range".into());
    }
    let index = (view == "index")
        .then(|| {
            rows.iter()
                .map(|(key, value)| {
                    let mut row = candidate_index_row(key, value);
                    row["detail_received"] = json!(match received.candidate.get(key) {
                        Some(prior) => prior == &digest(value)?,
                        None => false,
                    });
                    Ok::<_, String>(row)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let values: Vec<_> = match &index {
        Some(index) => index.iter().collect(),
        None => rows.iter().map(|(_, value)| value).collect(),
    };
    let query_scope = json!({
        "mode": if source_id.is_some() || ids.is_some() { "explicit" }
            else if source_ids.is_some() { "work" } else { "collection" },
        "source_id":source_id,
        "source_scope_count":source_ids.as_ref().map(BTreeSet::len),
        "candidate_id_count":ids.as_ref().map(BTreeSet::len),
    });
    let overhead = serde_json::to_vec(&json!({"view":view,"query_scope":query_scope}))
        .map_err(|e| e.to_string())?
        .len()
        - 1;
    let mut page = bounded_page(
        &values,
        start,
        limit,
        max_bytes
            .checked_sub(overhead)
            .ok_or("inspection envelope exceeds budget")?,
    )?;
    page["view"] = json!(view);
    page["query_scope"] = query_scope;
    let end = page["next"].as_u64().ok_or("invalid page continuation")? as usize;
    if view == "detail" {
        for (key, value) in &rows[start..end] {
            coverage.candidate.insert(key.clone(), digest(value)?);
        }
    }
    Ok(page)
}

/// Navigation only: no criterion arrays, template bodies or repeated citation
/// payloads. Labels are copied from the candidate, never generated summaries.
fn candidate_index_row(key: &str, value: &Value) -> Value {
    if key.starts_with("record:") {
        let data = &value["data"];
        let label = match data["kind"].as_str() {
            Some("fact") => &data["name"],
            Some("template") => &data["title"],
            Some("unresolved") => &data["problem"],
            _ => &data["text"],
        };
        let source_ids: BTreeSet<_> = value["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|span| span["source_id"].as_str())
            .collect();
        json!({"id":value["id"],"ref":key,"kind":data["kind"],"label":label,"source_ids":source_ids})
    } else if key.starts_with("relation:") {
        json!({"id":value["id"],"ref":key,"kind":value["kind"],"from":value["from"],"to":value["to"],"state":value["state"]})
    } else {
        json!({"source_id":value["source_id"],"ref":key,"state":value["disposition"]["state"]})
    }
}

pub fn invoke(
    input: &FrozenInput,
    analysis: &mut Analysis,
    coverage: &mut Coverage,
    reviewer: bool,
    name: &str,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    // Tool execution is transactional in memory. An oversized/invalid result
    // must neither mark unseen text read nor partially update the candidate.
    let expanded = evidence_refs::expand(input, args)?;
    let args = &expanded;
    let mut next_coverage = coverage.clone();
    // Reading a long tender must not copy the growing semantic graph on every
    // source page. These branches mutate only the temporary coverage ledger.
    if matches!(
        name,
        "collection_index"
            | "source_index"
            | "read_source"
            | "read_form"
            | "search_sources"
            | "inspect_analysis"
            | "check_gaps"
    ) {
        let out = execute(
            input,
            analysis,
            &mut next_coverage,
            reviewer,
            name,
            args,
            max_bytes,
        )?;
        if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > max_bytes {
            return Err("tool result exceeds budget; request a smaller range".into());
        }
        if !reviewer {
            analysis.coverage = next_coverage.clone();
        }
        *coverage = next_coverage;
        return Ok(out);
    }
    let mut next = analysis.clone();
    let out = execute(
        input,
        &mut next,
        &mut next_coverage,
        reviewer,
        name,
        args,
        max_bytes,
    )?;
    if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > max_bytes {
        return Err("tool result exceeds budget; request a smaller range".into());
    }
    if !reviewer {
        next.coverage = next_coverage.clone();
    }
    *analysis = next;
    *coverage = next_coverage;
    Ok(out)
}

fn execute(
    input: &FrozenInput,
    analysis: &mut Analysis,
    coverage: &mut Coverage,
    reviewer: bool,
    name: &str,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    match name {
        "collection_index" => {
            object(args, &["kind", "offset", "limit"])?;
            let kind = string(args, "kind")?;
            let values = match kind {
                "documents" => &input.documents,
                "document_relations" => &input.document_relations,
                "decisions" => &input.decisions,
                _ => return Err("unknown collection".into()),
            };
            let start = number(args, "offset")?;
            let limit = number(args, "limit")?.min(max_bytes);
            if start > values.len() || limit == 0 {
                return Err("invalid collection range".into());
            }
            let end = start.saturating_add(limit).min(values.len());
            cover(
                coverage.metadata.entry(kind.into()).or_default(),
                start,
                end,
            );
            Ok(json!({"total":values.len(),"next":end,"items":&values[start..end]}))
        }
        "source_index" => {
            object(args, &["offset", "limit"])?;
            let start = number(args, "offset")?;
            let limit = number(args, "limit")?.min(max_bytes);
            if limit == 0 {
                return Err("positive limit required".into());
            }
            let rows: Vec<_> = input.source_units.iter().skip(start).take(limit).map(|s|json!({
                "source_id":s.source_unit_revision_id,"document_id":s.document_id,"ordinal":s.ordinal,
                "bytes":s.text.len(),"heading_path":s.locator["heading_path"],"page_ordinal":s.locator["page_ordinal"],
                "forms":input.structured_forms.iter().filter(|f|f["source_unit_revision_id"]==s.source_unit_revision_id)
                    .map(|f|&f["form_definition_revision_id"]).collect::<Vec<_>>()
            })).collect();
            Ok(
                json!({"total":input.source_units.len(),"next":start.saturating_add(rows.len()),"items":rows}),
            )
        }
        "read_source" => {
            object(args, &["source_id", "start", "max_bytes"])?;
            let id = string(args, "source_id")?;
            let s = source(input, id)?;
            let start = number(args, "start")?;
            let size = number(args, "max_bytes")?.min(max_bytes / 2);
            if size == 0 {
                return Err(field_error("/max_bytes", "positive read size required"));
            }
            if start > s.text.len() {
                return Err(field_error(
                    "/start",
                    format!("byte {start} exceeds source length {} bytes", s.text.len()),
                ));
            }
            if !s.text.is_char_boundary(start) {
                return Err(field_error(
                    "/start",
                    format!(
                        "byte {start} is inside a UTF-8 character; adjacent boundaries are {} and {}; choose an exact byte offset from read_source line_spans or search_sources",
                        s.text.floor_char_boundary(start),
                        s.text.ceil_char_boundary(start)
                    ),
                ));
            }
            let mut end = start.saturating_add(size).min(s.text.len());
            loop {
                while end > start && !s.text.is_char_boundary(end) {
                    end -= 1;
                }
                if end == start && start < s.text.len() {
                    return Err(
                        "range too small for next character or annotated result budget".into(),
                    );
                }
                let text = &s.text[start..end];
                let mut out = json!({"source_id":id,"start":start,"end":end,"total_bytes":s.text.len(),
                    "text":text,"line_spans":source_line_spans(text,start)});
                evidence_refs::decorate(input, name, &mut out)?;
                if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() <= max_bytes {
                    cover(coverage.text.entry(id.into()).or_default(), start, end);
                    return Ok(out);
                }
                if end == start {
                    return Err("source metadata exceeds tool result budget".into());
                }
                // Include annotation overhead in the returned range budget.
                // The next read resumes at this exact returned end, without gaps.
                end = start + (end - start) / 2;
            }
        }
        "read_form" => {
            object(args, &["form_id", "offset", "limit", "find_text"])?;
            let find_text = args
                .get("find_text")
                .map(|_| string(args, "find_text"))
                .transpose()?;
            if find_text == Some("") {
                return Err("find_text must be a nonempty exact substring".into());
            }
            let id = string(args, "form_id")?;
            let f = form(input, id)?;
            let start = number(args, "offset")?;
            let limit = number(args, "limit")?.min(max_bytes);
            if limit == 0 {
                return Err("positive limit required".into());
            }
            let mut definition = f["definition"].clone();
            definition
                .as_object_mut()
                .ok_or("invalid form")?
                .remove("cells")
                .ok_or("cells missing")?;
            let (rows, columns) = super::relations::form_dimensions(&f["definition"])
                .ok_or("form grid unavailable")?;
            let total = rows.checked_mul(columns).ok_or("form grid unavailable")?;
            if start > total {
                return Err("cell offset outside form".into());
            }
            let end = start.saturating_add(limit).min(total);
            let mut window = Vec::with_capacity(end.saturating_sub(start));
            let mut citations = Vec::with_capacity(end.saturating_sub(start));
            for index in start..end {
                let row = index / columns;
                let column = index % columns;
                if !super::relations::grid_cell_is_anchor(&f["definition"], row, column) {
                    window.push(Value::Null);
                    citations.push(Value::Null);
                    continue;
                }
                let cell = super::relations::sparse_cell(&f["definition"], row, column)
                    .cloned()
                    .ok_or("grid citation cell missing")?;
                citations.push(json!({
                    "source_id": f["source_unit_revision_id"],
                    "start": 0,
                    "end": 0,
                    "grid_cell": {"form_id": id, "row": row, "column": column}
                }));
                window.push(cell);
            }
            let mut out = json!({"form_id":id,"source_id":f["source_unit_revision_id"],"definition":definition,
                "offset":start,"next":end,"total_cells":total,"cells":window,"citations":citations});
            evidence_refs::decorate(input, name, &mut out)?;
            if let Some(query) = find_text {
                let mut remaining = max_bytes;
                let mut matched = vec![];
                for (cell, citation) in out["cells"].as_array().unwrap().iter().zip(&citations) {
                    if citation.is_null() {
                        matched.push(Value::Null);
                        continue;
                    }
                    let value = cell["text"].as_str().ok_or("grid cell text missing")?;
                    let mut cursor = 0;
                    let mut ranges = vec![];
                    while let Some(relative) = value[cursor..].find(query) {
                        let start = cursor + relative;
                        let range = json!({"row":cell["row"],"column":cell["column"],"start":start,"end":start+query.len()});
                        remaining = remaining
                            .checked_sub(
                                serde_json::to_vec(&range).map_err(|e| e.to_string())?.len(),
                            )
                            .ok_or(
                                "cell matches exceed tool result budget; read a smaller cell slice",
                            )?;
                        ranges.push(range);
                        cursor = start + value[start..].chars().next().unwrap().len_utf8();
                    }
                    matched.push(json!(ranges));
                }
                out["matches"] = json!(matched);
            }
            cover(
                coverage.form_cells.entry(id.into()).or_default(),
                start,
                end,
            );
            Ok(out)
        }
        "search_sources" => {
            object(args, &["query", "offset", "limit"])?;
            let query = string(args, "query")?;
            if query.trim().is_empty() {
                return Err("query required".into());
            }
            let start = number(args, "offset")?;
            let limit = number(args, "limit")?.min(max_bytes);
            let mut all: Vec<_> = input.source_units.iter().flat_map(|s|s.text.match_indices(query).map(move |(offset,_)|
                json!({"source_id":s.source_unit_revision_id,"start":offset,"end":offset+query.len()}))).collect();
            // Grid text is stored separately from source text. Search sparse
            // anchors directly; a merged cell contributes once, and the read
            // offset is the dense position used by read_form, not this index.
            for f in &input.structured_forms {
                let definition = &f["definition"];
                let (_, columns) =
                    super::relations::form_dimensions(definition).ok_or("form grid unavailable")?;
                for cell in definition["cells"].as_array().ok_or("form cells missing")? {
                    let row = number(cell, "row")?;
                    let column = number(cell, "column")?;
                    if !super::relations::grid_cell_is_anchor(definition, row, column) {
                        continue;
                    }
                    let Some(text) = cell["text"].as_str().filter(|text| !text.is_empty()) else {
                        continue;
                    };
                    let form_offset = super::relations::dense_index(row, column, columns)
                        .ok_or("grid cell offset overflow")?;
                    for (offset, _) in text.match_indices(query) {
                        all.push(json!({
                            "source_id":f["source_unit_revision_id"],
                            "grid_cell":{"form_id":f["form_definition_revision_id"],"row":row,"column":column},
                            "form_offset":form_offset,
                            "cell_match":{"start":offset,"end":offset+query.len()}
                        }));
                    }
                }
            }
            bounded_page(&all, start, limit, max_bytes)
        }
        "inspect_analysis" => inspect_analysis(
            input,
            analysis,
            coverage,
            &coverage.clone(),
            args,
            max_bytes,
            None,
        )
        .map_err(String::from),
        "check_gaps" => {
            object(args, &["offset", "limit", "scope"])?;
            if string(args, "scope")? != "analysis" {
                return Err("scope must be analysis, or work through the Agent work state".into());
            }
            let rows = if reviewer {
                review_gaps(input, analysis, coverage)
            } else {
                gaps(input, analysis)
            };
            bounded_page(
                &rows,
                number(args, "offset")?,
                number(args, "limit")?,
                max_bytes,
            )
        }
        "put_record" if !reviewer => {
            object(args, &["id", "sources", "data"])?;
            if !args["data"].is_object() {
                return Err(field_error(
                    "/data",
                    "expected a record object with a kind field; send the object directly, not a JSON-encoded string",
                ));
            }
            let id = record_id(args, &analysis.records)?;
            let mut record = Record {
                id: id.clone(),
                sources: serde_json::from_value(args["sources"].clone())
                    .map_err(|e| field_error("/sources", e))?,
                data: serde_json::from_value(args["data"].clone())
                    .map_err(|e| field_error("/data", e))?,
            };
            let item_sequence = if let RecordData::Rule { items, .. } = &mut record.data {
                let previous = analysis
                    .records
                    .get(&id)
                    .and_then(|r| match &r.data {
                        RecordData::Rule { items, .. } => Some(items.as_slice()),
                        _ => None,
                    })
                    .unwrap_or_default();
                Some(crate::tender_analysis::rule_contract::assign_item_ids(
                    items,
                    previous,
                    analysis
                        .rule_item_sequences
                        .get(&id)
                        .copied()
                        .unwrap_or_default(),
                )?)
            } else {
                None
            };
            analysis.coverage = coverage.clone();
            validate_record(input, analysis, &record)?;
            if let Some(sequence) = item_sequence {
                analysis.rule_item_sequences.insert(id.clone(), sequence);
            }
            let item_ids = if let RecordData::Rule { items, .. } = &record.data {
                Some(items.iter().map(|item| item.id.clone()).collect::<Vec<_>>())
            } else {
                None
            };
            analysis.records.insert(id.clone(), record);
            let record_sha = digest(&analysis.records[&id])?;
            let recheck: Vec<_> = analysis
                .relations
                .values()
                .filter(|r| {
                    r.from == id && r.from_record_sha256 != record_sha
                        || r.to == id && r.to_record_sha256 != record_sha
                })
                .map(|r| &r.id)
                .collect();
            let mut output = json!({"id":id,"relations_to_recheck":recheck});
            if let Some(item_ids) = item_ids {
                output["item_ids"] = json!(item_ids);
            }
            Ok(output)
        }
        "put_relation" if !reviewer => {
            object(
                args,
                &[
                    "id",
                    "from",
                    "to",
                    "from_target",
                    "to_target",
                    "kind",
                    "state",
                    "scope",
                    "explanation",
                    "grounds",
                ],
            )?;
            let id = record_id(args, &analysis.relations)?;
            let mut raw = args.clone();
            raw["id"] = json!(id);
            for (endpoint, output) in [("from", "from_record_sha256"), ("to", "to_record_sha256")] {
                let record = analysis
                    .records
                    .get(string(args, endpoint)?)
                    .ok_or_else(|| {
                        field_error(&format!("/{endpoint}"), "unknown endpoint record")
                    })?;
                raw[output] = json!(digest(record)?);
            }
            let relation: Relation = serde_json::from_value(raw).map_err(|e| field_error("", e))?;
            analysis.coverage = coverage.clone();
            validate_relation(input, analysis, &relation)?;
            // Repeating the exact claim must not mint a new identity or renew
            // progress. Differing prose, scope, evidence or endpoint versions
            // remain distinct claims for the Agent to inspect and reconcile.
            if args["id"].is_null() {
                for existing in analysis
                    .relations
                    .values()
                    .filter(|existing| existing.from == relation.from && existing.to == relation.to)
                {
                    let mut repeated = relation.clone();
                    repeated.id.clone_from(&existing.id);
                    if repeated == *existing {
                        return Ok(json!({"id":existing.id,"unchanged":true}));
                    }
                }
            }
            analysis.relations.insert(id.clone(), relation);
            Ok(json!({"id":id}))
        }
        "delete_record" if !reviewer => {
            object(args, &["id"])?;
            let id = string(args, "id")?;
            if analysis
                .relations
                .values()
                .any(|r| r.from == id || r.to == id)
                || analysis
                    .records
                    .values()
                    .any(|r| matches!(&r.data,RecordData::Template {parent:Some(p),..} if p==id))
            {
                return Err("remove dependent relations/children before deleting record".into());
            }
            if analysis.records.remove(id).is_none() {
                return Err("unknown record".into());
            }
            Ok(json!({"deleted":id}))
        }
        "delete_relation" if !reviewer => {
            object(args, &["id"])?;
            let id = string(args, "id")?;
            if analysis.relations.remove(id).is_none() {
                return Err("unknown relation".into());
            }
            Ok(json!({"deleted":id}))
        }
        "set_disposition" if !reviewer => {
            object(args, &["source_id", "state", "reason"])?;
            let id = string(args, "source_id")?;
            let s = source(input, id)?;
            let state: DispositionState =
                serde_json::from_value(args["state"].clone()).map_err(|e| e.to_string())?;
            let reason = string(args, "reason")?.to_string();
            if reason.trim().is_empty() {
                return Err(field_error("/reason", "source disposition needs a reason"));
            }
            let grids: Vec<_> = input
                .structured_forms
                .iter()
                .filter(|f| f["source_unit_revision_id"] == id)
                .collect();
            let readable_grids = !grids.is_empty()
                && grids.iter().all(|f| {
                    super::relations::form_total(&f["definition"]).is_some_and(|total| {
                        contains(
                            coverage.form_cells.get(
                                f["form_definition_revision_id"]
                                    .as_str()
                                    .unwrap_or_default(),
                            ),
                            0,
                            total,
                        )
                    })
                })
                && grids.iter().any(|f| {
                    f["definition"]["cells"].as_array().is_some_and(|cells| {
                        cells.iter().any(|c| {
                            c["text"].as_str().is_some_and(|t| !t.trim().is_empty())
                                && c["row"].as_u64().zip(c["column"].as_u64()).is_some_and(
                                    |(r, col)| {
                                        super::relations::grid_cell_is_anchor(
                                            &f["definition"],
                                            r as usize,
                                            col as usize,
                                        )
                                    },
                                )
                        })
                    })
                });
            if s.text.is_empty()
                && state != DispositionState::Unresolved
                && !coverage.views.values().any(|v| v.source_id == id)
                && !readable_grids
            {
                return Err("empty source must remain unresolved".into());
            }
            if !s.text.is_empty() && !contains(coverage.text.get(id), 0, s.text.len()) {
                return Err("read the entire source before disposition".into());
            }
            analysis
                .dispositions
                .insert(id.into(), Disposition { state, reason });
            Ok(json!({"source_id":id}))
        }
        _ => Err("unknown or role-forbidden tool".into()),
    }
}

fn record_id<T>(args: &Value, records: &BTreeMap<String, T>) -> Result<String, String> {
    if args.get("id") == Some(&Value::Null) {
        return Ok(uuid::Uuid::new_v4().to_string());
    }
    let id = string(args, "id")?;
    if !records.contains_key(id) {
        return Err("unknown record id; use null to allocate a new identity".into());
    }
    Ok(id.into())
}

pub fn schemas_for(reviewer: bool, _limits: &super::agent::Limits) -> Vec<Value> {
    schemas(reviewer)
}

pub fn schemas(reviewer: bool) -> Vec<Value> {
    let contract: Value = serde_json::from_str(include_str!(
        "../../schemas/tender-analysis-tools-v1.schema.json"
    ))
    .expect("checked tender tool schemas");
    contract
        .as_array()
        .expect("tool array")
        .iter()
        .filter(|tool| {
            !reviewer
                || !matches!(
                    tool["function"]["name"].as_str(),
                    Some(
                        "put_record"
                            | "put_relation"
                            | "delete_record"
                            | "delete_relation"
                            | "set_disposition"
                            | "request_review"
                            | "put_repair_result"
                    )
                )
        })
        .filter(|tool| {
            reviewer
                || !matches!(
                    tool["function"]["name"].as_str(),
                    Some(
                        "read_review_task"
                            | "put_source_review"
                            | "put_review_finding"
                            | "delete_review_finding"
                            | "complete_review_check"
                    )
                )
        })
        .cloned()
        .collect()
}
