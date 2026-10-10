//! Token-policy-driven ordered packs. There is no character or cell weight.
use super::*;

#[derive(Clone)]
struct Unit {
    document_id: String,
    source_id: String,
    atoms: Vec<PackAtom>,
}

fn grid_carrier(def: &Value, table: &str) -> Result<PackCarrier, String> {
    let row_count = def["row_count"].as_u64().ok_or("grid rows missing")? as usize;
    let column_count = def["column_count"].as_u64().ok_or("grid columns missing")? as usize;
    if row_count == 0 || column_count == 0 {
        return Err("empty grid dimensions".into());
    }
    let mut cells = Vec::new();
    for cell in def["cells"].as_array().ok_or("grid cells missing")? {
        let row = cell["row"].as_u64().ok_or("grid row missing")? as usize;
        let column = cell["column"].as_u64().ok_or("grid column missing")? as usize;
        super::super::evidence::grid_cell(def, row, column)?;
        cells.push(GridFragment {
            anchor_row: row,
            anchor_column: column,
            row_span: cell["row_span"].as_u64().unwrap_or(1) as usize,
            column_span: cell
                .get("column_span")
                .or_else(|| cell.get("col_span"))
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize,
            header_role: cell["header_role"]
                .as_str()
                .unwrap_or(if row == 0 { "column_header" } else { "none" })
                .into(),
            start_byte: 0,
            end_byte: cell["text"].as_str().ok_or("grid text missing")?.len(),
        });
    }
    cells.sort_by_key(|cell| (cell.anchor_row, cell.anchor_column));
    Ok(PackCarrier::Grid {
        table_id: table.into(),
        row_count,
        column_count,
        cells,
        header_context: vec![],
    })
}

fn source_units(input: &FrozenInput, digest: &str) -> Result<Vec<Unit>, String> {
    let mut document_order = BTreeMap::new();
    for document in &input.documents {
        if let Some(id) = document["document_id"].as_str() {
            let index = document_order.len();
            document_order.entry(id.to_string()).or_insert(index);
        }
    }
    for source in &input.source_units {
        let index = document_order.len();
        document_order
            .entry(source.document_id.clone())
            .or_insert(index);
    }
    let mut sources = input.source_units.iter().collect::<Vec<_>>();
    sources.sort_by_key(|source| (document_order[&source.document_id], source.ordinal));
    let mut seen = BTreeSet::new();
    let mut forms = BTreeSet::new();
    let mut units = Vec::new();
    for source in sources {
        if !seen.insert(&source.source_unit_revision_id) {
            return Err("duplicate source unit".into());
        }
        let mut carriers = Vec::new();
        if !source.text.is_empty() {
            carriers.push(PackCarrier::Text {
                evidence: EvidenceRef::Text {
                    input_digest: digest.into(),
                    unit_id: source.source_unit_revision_id.clone(),
                    start_byte: 0,
                    end_byte: source.text.len(),
                },
            });
        }
        for form in input
            .structured_forms
            .iter()
            .filter(|form| form["source_unit_revision_id"] == source.source_unit_revision_id)
        {
            let table = form["form_definition_revision_id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("table identity missing")?;
            if !forms.insert(table.to_string()) {
                return Err("duplicate table identity".into());
            }
            carriers.push(grid_carrier(&form["definition"], table)?);
        }
        if source.locator["locator_kind"] == "image" && source.locator["image_available"] == true {
            carriers.push(PackCarrier::Image {
                vision_required: source.locator["blank_image"] != true,
                evidence: EvidenceRef::ImageRegion {
                    input_digest: digest.into(),
                    image_id: source.source_unit_revision_id.clone(),
                    region: "original".into(),
                },
            });
        }
        if carriers.is_empty() {
            carriers.push(PackCarrier::Structure {
                unit_id: source.source_unit_revision_id.clone(),
            });
        }
        units.push(Unit {
            document_id: source.document_id.clone(),
            source_id: source.source_unit_revision_id.clone(),
            atoms: carriers
                .into_iter()
                .map(|carrier| PackAtom {
                    id: String::new(),
                    section_id: section_id(source),
                    heading_path: source.locator["heading_path"].as_str().unwrap_or("").into(),
                    unit_ordinal: source.ordinal,
                    fragment_ordinal: 0,
                    previous_fragment_id: None,
                    next_fragment_id: None,
                    context_only: false,
                    carrier,
                })
                .collect(),
        });
    }
    if forms.len() != input.structured_forms.len() {
        return Err("orphan frozen table".into());
    }
    Ok(units)
}

fn assign_links(units: &mut [Unit]) {
    for unit in units {
        let count = unit.atoms.len();
        for (index, atom) in unit.atoms.iter_mut().enumerate() {
            atom.id = format!("{}:{index}", unit.source_id);
            atom.fragment_ordinal = index;
            atom.previous_fragment_id = index
                .checked_sub(1)
                .map(|previous| format!("{}:{previous}", unit.source_id));
            atom.next_fragment_id =
                (index + 1 < count).then(|| format!("{}:{}", unit.source_id, index + 1));
        }
    }
}

fn pack(document: &str, digest: &str, order: usize, atoms: Vec<PackAtom>) -> ParsePack {
    ParsePack {
        condition_support_options: Vec::new(),
        id: format!("pack-{order}"),
        document_id: document.into(),
        input_digest: digest.into(),
        order,
        pack_revision: 1,
        claim_token: String::new(),
        atoms,
    }
}

/// Exact first-running-session shape, including the actual content-bound claim.
fn fits_pack(
    input: &FrozenInput,
    pack: &ParsePack,
    fits: &SessionFits<'_>,
) -> Result<bool, String> {
    let mut running = pack.clone();
    running.claim_token = claim_token(&running, 1, false);
    let mut session = json!({"duty":"discover","pack":materialize_pack(input,&running)?,"status":"running","feedback":null,"no_requirement_reason":null});
    add_submission_identity(&mut session, &running, None)?;
    fits(std::slice::from_ref(&session))
}

#[cfg(test)]
pub(super) fn test_pack_fits(input: &FrozenInput, pack: &ParsePack, max_tokens: usize) -> bool {
    fits_pack(input, pack, &|sessions| {
        test_sessions_fit(sessions, max_tokens)
    })
    .unwrap()
}

fn midpoint(text: &str, start: usize, end: usize) -> Result<usize, String> {
    let raw = text.get(start..end).ok_or("invalid UTF-8 evidence range")?;
    let middle = start + (end - start) / 2;
    let boundary = raw
        .char_indices()
        .filter(|(index, _)| start + index > start && start + index < end)
        .min_by_key(|(index, _)| (start + index).abs_diff(middle))
        .map(|(index, _)| start + index)
        .ok_or("indivisible source scalar exceeds the context token budget")?;
    let clause = raw
        .char_indices()
        .filter_map(|(index, ch)| {
            matches!(ch, '\n' | '。' | '；' | ';' | '.').then_some(start + index + ch.len_utf8())
        })
        .filter(|position| {
            *position > start + (end - start) / 4 && *position < end - (end - start) / 4
        })
        .min_by_key(|position| position.abs_diff(middle));
    Ok(clause.unwrap_or(boundary))
}

fn split_carrier(input: &FrozenInput, carrier: &PackCarrier) -> Result<Vec<PackCarrier>, String> {
    match carrier {
        PackCarrier::Text {
            evidence:
                EvidenceRef::Text {
                    input_digest,
                    unit_id,
                    start_byte,
                    end_byte,
                },
        } => {
            let source = input
                .source_units
                .iter()
                .find(|source| &source.source_unit_revision_id == unit_id)
                .ok_or("source missing")?;
            let middle = midpoint(&source.text, *start_byte, *end_byte)?;
            Ok([(*start_byte, middle), (middle, *end_byte)]
                .into_iter()
                .map(|(start_byte, end_byte)| PackCarrier::Text {
                    evidence: EvidenceRef::Text {
                        input_digest: input_digest.clone(),
                        unit_id: unit_id.clone(),
                        start_byte,
                        end_byte,
                    },
                })
                .collect())
        }
        PackCarrier::Grid {
            table_id,
            row_count,
            column_count,
            cells,
            header_context,
        } => {
            if !header_context.is_empty() {
                return Ok(vec![PackCarrier::Grid {
                    table_id: table_id.clone(),
                    row_count: *row_count,
                    column_count: *column_count,
                    cells: cells.clone(),
                    header_context: vec![],
                }]);
            }
            let def = super::super::evidence::table_definition(input, table_id)?;
            let original = grid_carrier(def, table_id)?;
            let PackCarrier::Grid {
                cells: original_cells,
                ..
            } = original
            else {
                unreachable!()
            };
            let headers = original_cells
                .into_iter()
                .filter(|cell| cell.header_role != "none")
                .collect::<Vec<_>>();
            let mut pieces = Vec::new();
            if cells.len() > 1 {
                let rows = cells
                    .iter()
                    .map(|cell| cell.anchor_row)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                let boundary = if rows.len() > 1 {
                    cells
                        .iter()
                        .position(|cell| cell.anchor_row >= rows[rows.len() / 2])
                        .unwrap()
                } else {
                    cells.len() / 2
                };
                pieces.push(cells[..boundary].to_vec());
                pieces.push(cells[boundary..].to_vec());
            } else if let Some(cell) = cells.first() {
                let source =
                    super::super::evidence::grid_cell(def, cell.anchor_row, cell.anchor_column)?;
                let middle = midpoint(
                    source["text"].as_str().ok_or("cell text missing")?,
                    cell.start_byte,
                    cell.end_byte,
                )?;
                let mut left = cell.clone();
                left.end_byte = middle;
                let mut right = cell.clone();
                right.start_byte = middle;
                pieces.push(vec![left]);
                pieces.push(vec![right]);
            } else {
                return Err("grid metadata exceeds the context token budget".into());
            }
            Ok(pieces
                .into_iter()
                .map(|cells| {
                    let header_context = if cells.iter().all(|cell| cell.header_role == "none") {
                        headers.clone()
                    } else {
                        vec![]
                    };
                    PackCarrier::Grid {
                        table_id: table_id.clone(),
                        row_count: *row_count,
                        column_count: *column_count,
                        cells,
                        header_context,
                    }
                })
                .collect())
        }
        _ => Err("indivisible image/structure metadata exceeds the context token budget".into()),
    }
}

/// Merge complete adjacent chapters when the *real request* predicate admits
/// them. Large chapters preserve atom order and split only failed carriers.
pub fn plan_packs_with_budget(
    input: &FrozenInput,
    fits: &SessionFits<'_>,
) -> Result<Vec<ParsePack>, String> {
    let digest = input_digest(input)?;
    let mut units = source_units(input, &digest)?;
    'replan: loop {
        assign_links(&mut units);
        let mut chapters = Vec::<Vec<(usize, usize)>>::new();
        for (unit_index, unit) in units.iter().enumerate() {
            for (atom_index, atom) in unit.atoms.iter().enumerate() {
                let same = chapters
                    .last()
                    .and_then(|chapter| chapter.last())
                    .is_some_and(|&(previous, part)| {
                        units[previous].document_id == unit.document_id
                            && units[previous].atoms[part].section_id == atom.section_id
                    });
                if same {
                    chapters.last_mut().unwrap().push((unit_index, atom_index));
                } else {
                    chapters.push(vec![(unit_index, atom_index)]);
                }
            }
        }
        let mut out = Vec::new();
        let mut current: Option<ParsePack> = None;
        for chapter in chapters {
            let document = &units[chapter[0].0].document_id;
            let atoms = chapter
                .iter()
                .map(|&(unit, part)| units[unit].atoms[part].clone())
                .collect::<Vec<_>>();
            let mut candidate = current
                .clone()
                .filter(|pack| &pack.document_id == document)
                .unwrap_or_else(|| {
                    pack(
                        document,
                        &digest,
                        out.len() + usize::from(current.is_some()),
                        vec![],
                    )
                });
            candidate.atoms.extend(atoms.clone());
            if fits_pack(input, &candidate, fits)? {
                if current
                    .as_ref()
                    .is_some_and(|pack| &pack.document_id != document)
                {
                    out.push(current.take().unwrap());
                }
                current = Some(candidate);
                continue;
            }
            if let Some(previous) = current.take() {
                out.push(previous);
            }
            let whole = pack(document, &digest, out.len(), atoms);
            if fits_pack(input, &whole, fits)? {
                current = Some(whole);
                continue;
            }
            for (unit_index, atom_index) in chapter {
                let atom = units[unit_index].atoms[atom_index].clone();
                let mut candidate = current
                    .clone()
                    .unwrap_or_else(|| pack(document, &digest, out.len(), vec![]));
                candidate.atoms.push(atom.clone());
                if fits_pack(input, &candidate, fits)? {
                    current = Some(candidate);
                    continue;
                }
                if let Some(previous) = current.take() {
                    out.push(previous);
                }
                let single = pack(document, &digest, out.len(), vec![atom.clone()]);
                if fits_pack(input, &single, fits)? {
                    current = Some(single);
                    continue;
                }
                let parts = split_carrier(input, &atom.carrier)?;
                let replacement = parts.into_iter().map(|carrier| PackAtom {
                    carrier,
                    ..atom.clone()
                });
                units[unit_index]
                    .atoms
                    .splice(atom_index..=atom_index, replacement);
                continue 'replan;
            }
        }
        if let Some(pack) = current {
            out.push(pack);
        }
        return Ok(out);
    }
}
