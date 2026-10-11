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
    #[cfg(test)]
    let _profile = crate::analysis::agent::profile::enter("planner_source_index");
    let mut document_order = BTreeMap::new();
    for document in &input.documents {
        if !document.document_id.is_empty() {
            let id = &document.document_id;
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
                    document_id: source.document_id.clone(),
                    source_unit_revision_id: source.source_unit_revision_id.clone(),
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
    #[cfg(test)]
    let _profile = crate::analysis::agent::profile::enter("planner_assign_links");
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

fn pack(digest: &str, order: usize, atoms: Vec<PackAtom>) -> ParsePack {
    let mut document_ids = Vec::new();
    for atom in &atoms {
        if !document_ids.contains(&atom.document_id) {
            document_ids.push(atom.document_id.clone());
        }
    }
    ParsePack {
        condition_support_options: Vec::new(),
        id: format!("pack-{order}"),
        document_ids,
        input_digest: digest.into(),
        order,
        pack_revision: 1,
        claim_token: String::new(),
        atoms,
    }
}

/// Exact first-running-session shape, including the actual content-bound claim.
fn fits_pack(
    resolver: &super::super::evidence::EvidenceResolver<'_>,
    pack: &ParsePack,
    fits: &PlanningFits<'_>,
) -> Result<PlanningFit, String> {
    #[cfg(test)]
    let _profile = crate::analysis::agent::profile::enter("planner_full_probe");
    #[cfg(test)]
    let materialize_profile = crate::analysis::agent::profile::enter("planner_materialize");
    #[cfg(test)]
    {
        use crate::analysis::agent::profile;
        profile::observe("candidate_atoms_total", pack.atoms.len());
        profile::observe_max("candidate_atoms_max", pack.atoms.len());
        for atom in &pack.atoms {
            profile::observe(
                match atom.carrier {
                    PackCarrier::Text { .. } => "candidate_text_atoms",
                    PackCarrier::Grid { .. } => "candidate_grid_atoms",
                    PackCarrier::Image { .. } => "candidate_image_atoms",
                    PackCarrier::Structure { .. } => "candidate_structure_atoms",
                },
                1,
            );
        }
    }
    let mut running = pack.clone();
    #[cfg(test)]
    let claim_profile = crate::analysis::agent::profile::enter("planner_claim_identity");
    running.claim_token = claim_token(&running, 1, false);
    #[cfg(test)]
    drop(claim_profile);
    let mut session = json!({"duty":"discover","pack":materialize_pack_with_resolver(resolver,&running)?,"status":"running","feedback":null,"no_requirement_reason":null,"issues":[]});
    add_submission_identity(&mut session, &running, None)?;
    session["submission_instruction"] = json!(SUBMISSION_INSTRUCTION);
    #[cfg(test)]
    drop(materialize_profile);
    #[cfg(test)]
    let _budget_profile = crate::analysis::agent::profile::enter("planner_request_budget");
    fits(std::slice::from_ref(&session))
}

#[cfg(test)]
pub(super) fn test_pack_fits(input: &FrozenInput, pack: &ParsePack, max_tokens: usize) -> bool {
    let resolver = super::super::evidence::EvidenceResolver::new(input).unwrap();
    fits_pack(&resolver, pack, &|sessions| {
        test_sessions_fit(sessions, max_tokens).map(PlanningFit::Unmeasured)
    })
    .unwrap()
    .fits()
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
            ..
        } => {
            // Budget pressure may shrink primary cell ranges, never erase the
            // native headers needed to interpret them. Context is support-only.
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
                ).map_err(|error| {
                    if headers.is_empty() { error } else {
                        format!("required grid header context and minimum cell range exceed the context budget: {error}")
                    }
                })?;
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
                    let header_context = headers
                        .iter()
                        .filter(|header| !cells.contains(header))
                        .cloned()
                        .collect();
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

/// Cross-document co-location follows actual native endpoint ownership, not
/// another relation elsewhere in either file. Same-document adjacency remains.
fn related_atoms(input: &FrozenInput, owned: &[PackAtom], incoming: &[PackAtom]) -> bool {
    let matches = |endpoint: &crate::analysis::source_manifest::RelationEndpoint,
                   atom: &PackAtom| {
        endpoint.document_id == atom.document_id
            && endpoint.unit_id.as_ref().is_none_or(|key| {
                input.source_units.iter().any(|source| {
                    source.source_unit_revision_id == atom.source_unit_revision_id
                        && source.locator["unit_id"] == *key
                })
            })
    };
    owned.iter().any(|left| {
        incoming.iter().any(|right| {
            left.document_id == right.document_id
                || input.document_relations.iter().any(|relation| {
                    relation.kind
                        == crate::analysis::source_manifest::DocumentRelationKind::ExplicitReference
                        && relation.status
                            == crate::analysis::source_manifest::DocumentRelationStatus::Confirmed
                        && relation.to.as_ref().is_some_and(|target| {
                            (matches(&relation.from, left) && matches(target, right))
                                || (matches(target, left) && matches(&relation.from, right))
                        })
                })
        })
    })
}

// Frozen byte weights are boundary hints, never token counts or admission.
// Serialize each source/initial carrier once; fragments inherit a range share.
struct FrozenWeight {
    bytes: usize,
    extent: usize,
}
fn carrier_weight_key(atom: &PackAtom) -> (String, String) {
    let kind = match &atom.carrier {
        PackCarrier::Text { .. } => "text".into(),
        PackCarrier::Grid { table_id, .. } => format!("grid:{table_id}"),
        PackCarrier::Image { .. } => "image".into(),
        PackCarrier::Structure { .. } => "structure".into(),
    };
    (atom.source_unit_revision_id.clone(), kind)
}
fn carrier_extent(carrier: &PackCarrier) -> usize {
    match carrier {
        PackCarrier::Text { evidence } => evidence
            .range()
            .map(|(start, end)| end - start)
            .unwrap_or(0),
        PackCarrier::Grid {
            cells,
            header_context,
            ..
        } => cells.iter().chain(header_context).fold(0usize, |sum, c| {
            sum.saturating_add(c.end_byte - c.start_byte)
                .saturating_add(1)
        }),
        _ => 1,
    }
    .max(1)
}
fn frozen_weights(
    input: &FrozenInput,
    units: &[Unit],
) -> Result<BTreeMap<(String, String), FrozenWeight>, String> {
    #[cfg(test)]
    let _profile = crate::analysis::agent::profile::enter("planner_source_weights");
    let mut weights = BTreeMap::new();
    let mut table_sizes = BTreeMap::new();
    for unit in units {
        #[cfg(test)]
        crate::analysis::agent::profile::observe("source_units_weighted", 1);
        let source = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == unit.source_id)
            .ok_or("weight source missing")?;
        let structure = serde_json::to_vec(&source.locator)
            .map_err(|e| e.to_string())?
            .len();
        for atom in &unit.atoms {
            #[cfg(test)]
            crate::analysis::agent::profile::observe("initial_atoms_weighted", 1);
            let extent = carrier_extent(&atom.carrier);
            let content = match &atom.carrier {
                PackCarrier::Grid { table_id, .. } => {
                    if let Some(&size) = table_sizes.get(table_id) {
                        size
                    } else {
                        #[cfg(test)]
                        crate::analysis::agent::profile::observe("table_definitions_weighted", 1);
                        let definition = super::super::evidence::table_definition(input, table_id)?;
                        let size = serde_json::to_vec(definition)
                            .map_err(|e| e.to_string())?
                            .len();
                        table_sizes.insert(table_id.clone(), size);
                        size
                    }
                }
                _ => extent,
            };
            let bytes = structure
                .saturating_add(
                    serde_json::to_vec(&atom.carrier)
                        .map_err(|e| e.to_string())?
                        .len(),
                )
                .saturating_add(content);
            weights.insert(carrier_weight_key(atom), FrozenWeight { bytes, extent });
        }
    }
    Ok(weights)
}
fn atom_weight(weights: &BTreeMap<(String, String), FrozenWeight>, atom: &PackAtom) -> usize {
    let base = &weights[&carrier_weight_key(atom)];
    ((base.bytes as u128)
        .saturating_mul(carrier_extent(&atom.carrier) as u128)
        .div_ceil(base.extent as u128)
        .min(usize::MAX as u128) as usize)
        .max(1)
}
fn shrink_prefix(prefix: &[usize], count: usize, fit: &PlanningFit) -> usize {
    let base = prefix[0];
    let target = fit.shrink_weight(prefix[count].saturating_sub(base));
    prefix[1..count]
        .partition_point(|&weight| weight.saturating_sub(base) <= target)
        .max(1)
}
fn hinted_prefix(prefix: &[usize], capacity: usize) -> usize {
    let base = prefix[0];
    prefix[1..]
        .partition_point(|&weight| weight.saturating_sub(base) <= capacity)
        .max(1)
}

enum PrefixOutcome {
    Admitted(usize, ParsePack),
    Oversize(PlanningFit),
}

/// Select a final pack using a plan-local measured weight hint, or the whole
/// related range before the first measurement. Only a rejected full request
/// adjusts the current boundary; successful requests only guide later packs.
/// Weight-guided shrinking is not a monotonic-fit inference: every smaller final
/// proposal is checked too. Return immediately on success, without fill probes.
fn fitted_chapter_prefix(
    input: &FrozenInput,
    digest: &str,
    order: usize,
    chapters: &[Vec<PackAtom>],
    chapter_prefix: &[usize],
    capacity_hint: &std::cell::Cell<Option<usize>>,
    fits: &impl Fn(&ParsePack) -> Result<PlanningFit, String>,
) -> Result<PrefixOutcome, String> {
    let mut atoms = Vec::new();
    let mut ends = Vec::new();
    // A measured hint already bounds the first candidate. Do not clone the
    // unused suffix merely to discard it below; relation checks still apply.
    let proposed = capacity_hint
        .get()
        .map(|capacity| hinted_prefix(chapter_prefix, capacity))
        .unwrap_or(chapters.len());
    for chapter in chapters.iter().take(proposed) {
        if !atoms.is_empty() && !related_atoms(input, &atoms, chapter) {
            break;
        }
        #[cfg(test)]
        crate::analysis::agent::profile::observe("chapter_atoms_copied_before_hint", chapter.len());
        atoms.extend_from_slice(chapter);
        ends.push(atoms.len());
    }
    if ends.is_empty() {
        return Err("empty chapter proposal".into());
    }
    let candidate = |count: usize| pack(digest, order, atoms[..ends[count - 1]].to_vec());
    let prefix = &chapter_prefix[..=ends.len()];
    let mut count = capacity_hint
        .get()
        .map(|capacity| hinted_prefix(prefix, capacity))
        .unwrap_or(ends.len());
    loop {
        let proposal = candidate(count);
        let fit = fits(&proposal)?;
        if fit.fits() {
            return Ok(PrefixOutcome::Admitted(count, proposal));
        }
        if count == 1 {
            return Ok(PrefixOutcome::Oversize(fit));
        }
        count = shrink_prefix(prefix, count, &fit);
    }
}

/// Merge complete adjacent chapters when the *real request* predicate admits
/// them. Large chapters preserve atom order and split only failed carriers.
pub fn plan_packs_with_budget(
    input: &FrozenInput,
    fits: &SessionFits<'_>,
) -> Result<Vec<ParsePack>, String> {
    plan_packs_with_measured_budget(input, &|sessions| {
        fits(sessions).map(PlanningFit::Unmeasured)
    })
}
pub fn plan_packs_with_measured_budget(
    input: &FrozenInput,
    fits: &PlanningFits<'_>,
) -> Result<Vec<ParsePack>, String> {
    #[cfg(test)]
    let _profile = crate::analysis::agent::profile::enter("planner_total");
    input.validate_document_relations()?;
    let resolver = super::super::evidence::EvidenceResolver::new(input)?;
    let digest = resolver.digest().to_owned();
    let mut units = source_units(input, &digest)?;
    let weights = frozen_weights(input, &units)?;
    let capacity_hint = std::cell::Cell::new(None);
    let fits_candidate = |candidate: &ParsePack| -> Result<PlanningFit, String> {
        #[cfg(test)]
        let _profile = crate::analysis::agent::profile::enter("planner_logical_probe");
        let fit = fits_pack(&resolver, candidate, fits)?;
        if matches!(&fit, PlanningFit::Measured { estimated_input, .. } if *estimated_input > 0) {
            let weight = candidate.atoms.iter().fold(0usize, |sum, atom| {
                sum.saturating_add(atom_weight(&weights, atom))
            });
            capacity_hint.set(Some(fit.shrink_weight(weight).max(1)));
        }
        Ok(fit)
    };
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
        #[cfg(test)]
        crate::analysis::agent::profile::observe("section_groups_across_replans", chapters.len());
        let chapter_atoms: Vec<Vec<PackAtom>> = chapters
            .iter()
            .map(|chapter| {
                chapter
                    .iter()
                    .map(|&(unit, part)| units[unit].atoms[part].clone())
                    .collect()
            })
            .collect();
        // One cumulative chapter index per replan; later packs reuse slices
        // including their base. Relation scanning remains independent of it.
        let mut chapter_prefix = vec![0usize];
        for atoms in &chapter_atoms {
            let weight = atoms.iter().fold(0usize, |sum, atom| {
                sum.saturating_add(atom_weight(&weights, atom))
            });
            chapter_prefix.push(chapter_prefix.last().unwrap().saturating_add(weight));
        }
        let mut out = Vec::new();
        let mut chapter_index = 0;
        while chapter_index < chapters.len() {
            let rejected = match fitted_chapter_prefix(
                input,
                &digest,
                out.len(),
                &chapter_atoms[chapter_index..],
                &chapter_prefix[chapter_index..],
                &capacity_hint,
                &fits_candidate,
            )? {
                PrefixOutcome::Admitted(consumed, admitted) => {
                    out.push(admitted);
                    chapter_index += consumed;
                    continue;
                }
                PrefixOutcome::Oversize(fit) => fit,
            };
            // The first whole chapter was actually rejected. Preserve the
            // existing ordered atom/UTF-8/grid fallback and replan split links.
            #[cfg(test)]
            crate::analysis::agent::profile::observe("standalone_chapter_fallbacks", 1);
            let chapter = &chapter_atoms[chapter_index];
            let mut atom_prefix = vec![0usize];
            for atom in chapter {
                atom_prefix.push(
                    atom_prefix
                        .last()
                        .unwrap()
                        .saturating_add(atom_weight(&weights, atom)),
                );
            }
            let mut offset = 0;
            while offset < chapter.len() {
                let prefix = &atom_prefix[offset..];
                let mut count = if offset == 0 && chapter.len() > 1 {
                    shrink_prefix(prefix, chapter.len(), &rejected)
                } else {
                    capacity_hint
                        .get()
                        .map(|capacity| hinted_prefix(prefix, capacity))
                        .unwrap_or(chapter.len() - offset)
                };
                loop {
                    let candidate =
                        pack(&digest, out.len(), chapter[offset..offset + count].to_vec());
                    let already_rejected = offset == 0 && count == chapter.len();
                    let fit = if already_rejected {
                        None
                    } else {
                        Some(fits_candidate(&candidate)?)
                    };
                    if fit.as_ref().is_some_and(PlanningFit::fits) {
                        out.push(candidate);
                        offset += count;
                        break;
                    }
                    if count > 1 {
                        count = shrink_prefix(prefix, count, fit.as_ref().unwrap_or(&rejected));
                        continue;
                    }
                    let (unit_index, atom_index) = chapters[chapter_index][offset];
                    let atom = units[unit_index].atoms[atom_index].clone();
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
            chapter_index += 1;
        }
        return Ok(out);
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;

    fn synthetic_input(count: usize, same_section: bool) -> FrozenInput {
        FrozenInput {
            schema_version: crate::outline::frozen::FROZEN_SCHEMA_VERSION,
            project_id: "profile".into(), document_set_id: "set".into(),
            documents: vec![], document_relations: vec![],
            source_units: (0..count).map(|ordinal| Source {
                source_unit_revision_id: format!("unit-{ordinal}"), document_id: "doc".into(),
                text: "Synthetic obligation".into(),
                locator: json!({"section_id":(if same_section {0} else {ordinal}).to_string(),"completeness":"complete"}),
                ordinal,
            }).collect(), structured_forms: vec![], decisions: vec![],
        }
    }
    #[test]
    fn split_chapter_tail_is_not_backfilled_with_next_chapter() {
        let mut input = synthetic_input(4, true);
        input.source_units[3].locator["section_id"] = json!("next-section");
        let packs = plan_packs_with_measured_budget(&input, &|sessions| {
            Ok(PlanningFit::Measured {
                estimated_input: sessions[0]["pack"]["atoms"].as_array().unwrap().len() * 100,
                available_input: 200,
            })
        })
        .unwrap();
        let ordinals: Vec<Vec<_>> = packs
            .iter()
            .map(|pack| pack.atoms.iter().map(|atom| atom.unit_ordinal).collect())
            .collect();
        assert_eq!(ordinals, vec![vec![0, 1], vec![2], vec![3]]);
    }

    #[test]
    fn weighted_hint_bounds_remaining_chapter_copy_scaling() {
        for count in [64, 128, 256] {
            let input = synthetic_input(count, false);
            crate::analysis::agent::profile::reset();
            let start = std::time::Instant::now();
            let packs = plan_packs_with_measured_budget(&input, &|sessions| {
                let atoms = sessions[0]["pack"]["atoms"].as_array().unwrap().len();
                Ok(PlanningFit::Measured {
                    estimated_input: atoms * 100,
                    available_input: 400,
                })
            })
            .unwrap();
            let micros = start.elapsed().as_micros();
            let counters = crate::analysis::agent::profile::finish_counters();
            crate::analysis::agent::profile::finish();
            let copied = counters["chapter_atoms_copied_before_hint"];
            assert!(packs.iter().all(|pack| pack.atoms.len() <= 4));
            assert_eq!(
                packs.iter().map(|pack| pack.atoms.len()).sum::<usize>(),
                count
            );
            assert!(
                copied as usize <= count * 3,
                "measured hints must avoid repeatedly cloning the entire unused suffix"
            );
            eprintln!(
                "synthetic_copy_audit n={count} k=4 copied_atoms={copied} elapsed_us={micros}"
            );
        }
    }

    #[test]
    fn bounded_prefix_keeps_exact_coverage_with_nonmonotone_fits() {
        let input = synthetic_input(17, false);
        for accepted in [vec![1, 3, 5], vec![1, 2, 8], vec![1, 2, 3, 6]] {
            let admitted = std::cell::RefCell::new(Vec::new());
            let packs = plan_packs_with_budget(&input, &|sessions| {
                let pack = &sessions[0]["pack"];
                let ok = accepted.contains(&pack["atoms"].as_array().unwrap().len());
                if ok {
                    admitted.borrow_mut().push(pack.clone());
                }
                Ok(ok)
            })
            .unwrap();
            assert_eq!(
                packs
                    .iter()
                    .flat_map(|p| p.atoms.iter().map(|a| a.unit_ordinal))
                    .collect::<Vec<_>>(),
                (0..17).collect::<Vec<_>>()
            );
            let resolver = super::super::super::evidence::EvidenceResolver::new(&input).unwrap();
            for pack in &packs {
                let mut running = pack.clone();
                running.claim_token = claim_token(&running, 1, false);
                let exact = materialize_pack_with_resolver(&resolver, &running).unwrap();
                assert!(
                    admitted.borrow().contains(&exact),
                    "exact returned pack must have passed the real predicate"
                );
                assert!(accepted.contains(&pack.atoms.len()));
            }
        }
        assert_eq!(
            plan_packs_with_budget(&input, &|_| Err("probe failed".into())),
            Err("probe failed".into())
        );
    }
    #[test]
    fn measured_weight_ratio_selects_boundary_then_validates_full_request() {
        let input = synthetic_input(8, false);
        let probes = std::cell::RefCell::new(Vec::new());
        let packs = plan_packs_with_measured_budget(&input, &|sessions| {
            let count = sessions[0]["pack"]["atoms"].as_array().unwrap().len();
            probes.borrow_mut().push(count);
            Ok(PlanningFit::Measured {
                estimated_input: count * 100,
                available_input: 500,
            })
        })
        .unwrap();
        assert_eq!(*probes.borrow(), vec![8, 5, 3]);
        assert_eq!(
            packs
                .iter()
                .map(|pack| pack.atoms.len())
                .collect::<Vec<_>>(),
            vec![5, 3]
        );
        assert_eq!(
            packs
                .iter()
                .flat_map(|pack| pack.atoms.iter().map(|atom| atom.unit_ordinal))
                .collect::<Vec<_>>(),
            (0..8).collect::<Vec<_>>()
        );
    }
    #[test]
    fn later_packs_use_measured_weight_capacity_without_full_remaining_probe() {
        let input = synthetic_input(9, false);
        let probes = std::cell::RefCell::new(Vec::new());
        let packs = plan_packs_with_measured_budget(&input, &|sessions| {
            let count = sessions[0]["pack"]["atoms"].as_array().unwrap().len();
            probes.borrow_mut().push(count);
            Ok(PlanningFit::Measured {
                estimated_input: count * 100,
                available_input: 300,
            })
        })
        .unwrap();
        assert_eq!(*probes.borrow(), vec![9, 3, 3, 3]);
        assert_eq!(
            packs
                .iter()
                .map(|pack| pack.atoms.len())
                .collect::<Vec<_>>(),
            vec![3, 3, 3]
        );
    }
    #[test]
    fn unequal_frozen_weights_avoid_count_half_overflow() {
        let mut input = synthetic_input(8, false);
        for (index, source) in input.source_units.iter_mut().enumerate() {
            source.text = if index < 2 {
                "x".repeat(5000)
            } else {
                "z".into()
            };
        }
        let probes = std::cell::RefCell::new(Vec::new());
        let packs = plan_packs_with_measured_budget(&input, &|sessions| {
            let atoms = sessions[0]["pack"]["atoms"].as_array().unwrap();
            let bytes: usize = atoms
                .iter()
                .flat_map(|atom| atom["excerpts"].as_array().unwrap())
                .map(|excerpt| excerpt["quote"].as_str().unwrap().len())
                .sum();
            probes.borrow_mut().push((atoms.len(), bytes));
            Ok(PlanningFit::Measured {
                estimated_input: bytes,
                available_input: 6000,
            })
        })
        .unwrap();
        assert_eq!(&probes.borrow()[..2], &[(8, 10006), (1, 5000)]);
        assert_eq!(packs.iter().map(|pack| pack.atoms.len()).sum::<usize>(), 8);
        for pack in &packs {
            let text_bytes: usize = pack
                .atoms
                .iter()
                .map(|atom| input.source_units[atom.unit_ordinal].text.len())
                .sum();
            assert!(text_bytes <= 6000);
        }
        assert!(
            input.source_units[..4]
                .iter()
                .map(|s| s.text.len())
                .sum::<usize>()
                > 6000,
            "count halving would still exceed this synthetic full-request predicate"
        );
    }
    #[test]
    fn measured_nonmonotone_rejection_always_shrinks_and_checks_again() {
        let input = synthetic_input(8, false);
        let admitted = std::cell::RefCell::new(Vec::new());
        let packs = plan_packs_with_measured_budget(&input, &|sessions| {
            let count = sessions[0]["pack"]["atoms"].as_array().unwrap().len();
            let fits = matches!(count, 1 | 3 | 7);
            if fits {
                admitted.borrow_mut().push(count);
            }
            Ok(PlanningFit::Measured {
                estimated_input: if fits { 50 } else { 101 },
                available_input: 100,
            })
        })
        .unwrap();
        assert_eq!(packs.iter().map(|pack| pack.atoms.len()).sum::<usize>(), 8);
        for pack in packs {
            assert!(admitted.borrow().contains(&pack.atoms.len()));
        }
    }
    #[test]
    fn bounded_prefix_reduces_full_probes_without_a_candidate_cache() {
        let input = synthetic_input(128, false);
        let probes = std::cell::Cell::new(0usize);
        let packs = plan_packs_with_budget(&input, &|_| {
            probes.set(probes.get() + 1);
            Ok(true)
        })
        .unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].atoms.len(), 128);
        assert_eq!(
            probes.get(),
            1,
            "an admitted whole range needs no candidate probes"
        );
    }
    #[test]
    fn bounded_prefix_preserves_oversize_chapter_atom_fallback() {
        let input = synthetic_input(3, true);
        let packs = plan_packs_with_budget(&input, &|sessions| {
            Ok(sessions[0]["pack"]["atoms"].as_array().unwrap().len() == 1)
        })
        .unwrap();
        let digest = input_digest(&input).unwrap();
        let mut units = source_units(&input, &digest).unwrap();
        assign_links(&mut units);
        let expected: Vec<_> = units
            .into_iter()
            .enumerate()
            .map(|(order, unit)| pack(&digest, order, unit.atoms))
            .collect();
        assert_eq!(packs, expected);
        assert_eq!(
            plan_packs_with_budget(&input, &|_| Ok(true)).unwrap().len(),
            1
        );
    }
    #[test]
    fn bounded_prefix_preserves_utf8_split_replan_links() {
        let mut input = synthetic_input(1, true);
        input.source_units[0].text = "甲😀乙😀丙😀丁😀".into();
        let packs = plan_packs_with_budget(&input, &|sessions| {
            Ok(sessions[0]["pack"]["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .map(|atom| {
                    atom["excerpts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|excerpt| excerpt["quote"].as_str().unwrap().len())
                        .sum::<usize>()
                })
                .sum::<usize>()
                <= 7)
        })
        .unwrap();
        let atoms: Vec<_> = packs.iter().flat_map(|p| &p.atoms).collect();
        assert!(atoms.len() > 1);
        let mut cursor = 0;
        for (index, atom) in atoms.iter().enumerate() {
            let PackCarrier::Text {
                evidence:
                    EvidenceRef::Text {
                        start_byte,
                        end_byte,
                        ..
                    },
            } = &atom.carrier
            else {
                panic!("text carrier expected")
            };
            assert_eq!(*start_byte, cursor);
            assert!(
                input.source_units[0]
                    .text
                    .get(*start_byte..*end_byte)
                    .is_some()
            );
            cursor = *end_byte;
            assert_eq!(
                atom.previous_fragment_id.as_deref(),
                index.checked_sub(1).map(|i| atoms[i].id.as_str())
            );
            assert_eq!(
                atom.next_fragment_id.as_deref(),
                atoms.get(index + 1).map(|a| a.id.as_str())
            );
        }
        assert_eq!(cursor, input.source_units[0].text.len());
    }
    #[test]
    fn cross_document_co_location_requires_the_native_relation_endpoints() {
        let input = crate::outline::frozen::tests::related_native_input();
        let packs = plan_packs_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
        })
        .unwrap();
        let left = &packs[0].atoms;
        let right = &packs[1].atoms;
        assert!(related_atoms(&input, left, right));
        let mut wrong = left[0].clone();
        wrong.source_unit_revision_id = "another-unit-in-same-file".into();
        let mut precise = input.clone();
        let from = precise.document_relations[0].from.clone();
        let original = precise
            .source_units
            .iter()
            .find(|source| {
                source.document_id == from.document_id
                    && left
                        .iter()
                        .any(|atom| atom.source_unit_revision_id == source.source_unit_revision_id)
            })
            .unwrap();
        precise.document_relations[0].from.unit_id =
            Some(original.locator["unit_id"].as_str().unwrap().to_owned());
        assert!(
            !related_atoms(&precise, &[wrong], right),
            "another unit's relation cannot co-locate this chapter"
        );
        let mut wrong_target = right[0].clone();
        wrong_target.source_unit_revision_id = "other-native-grid".into();
        assert!(
            !related_atoms(&precise, left, &[wrong_target]),
            "target endpoint must also match"
        );
    }
}

#[cfg(test)]
#[path = "private_planner_profile.rs"]
mod private_planner_profile;

#[cfg(test)]
#[path = "discover_header_budget_tests.rs"]
mod header_budget_tests;
