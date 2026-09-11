use super::*;
use crate::docx_template::{self as render, *};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub section_id: String,
    pub bookmark: String,
    pub cell: Option<CellRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub reference: Reference,
    pub location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceExcerpt {
    pub reference: Reference,
    pub parts: Vec<SourcePart>,
    pub location: Location,
    pub response_location: Location,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub analysis_sha256: String,
    pub draft_sha256: String,
    pub docx_sha256: String,
    pub sections: Vec<Location>,
    pub placements: Vec<Placement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_excerpts: Vec<SourceExcerpt>,
    pub omissions: Vec<Omission>,
    pub relation_omissions: Vec<RelationOmission>,
    pub source_quality: String,
    pub source_open_items: Vec<SourceOpenItem>,
    /// Rendering is a structural check, never semantic or bidder approval.
    pub status: String,
}

impl Manifest {
    pub fn reviewed_status(&self) -> &'static str {
        if self.source_open_items.is_empty() {
            "reviewed_template"
        } else {
            "reviewed_template_with_open_items"
        }
    }
}

pub struct Compiled {
    pub docx: Vec<u8>,
    pub manifest: Manifest,
    pub rendered: Vec<super::document::RenderedBlock>,
}

fn block(kind: &str) -> TemplateBlock {
    TemplateBlock {
        kind: kind.into(),
        source_parts: vec![],
        source_id: None,
        quote: None,
        condition: None,
        form_id: None,
        header_rows: 0,
        blank_cells: vec![],
        blank_ranges: vec![],
        columns: vec![],
        blank_rows: 0,
    }
}

fn ordered_sections(draft: &Draft) -> Result<Vec<(&Section, usize)>, String> {
    let mut siblings = BTreeSet::new();
    for (id, section) in &draft.sections {
        if !section.placement.is_body()
            && (section.parent.is_some()
                || draft
                    .sections
                    .values()
                    .any(|s| s.parent.as_ref() == Some(id)))
        {
            return Err("front matter must be a root section without child chapters".into());
        }
        if section.id != *id
            || section.title.trim().is_empty()
            || section
                .parent
                .as_ref()
                .is_some_and(|p| !draft.sections.contains_key(p))
            || !siblings.insert((section.parent.clone(), section.order))
        {
            return Err(
                "invalid section identity, parent, title or duplicate sibling order".into(),
            );
        }
    }
    fn visit<'a>(
        draft: &'a Draft,
        parent: Option<&str>,
        depth: usize,
        out: &mut Vec<(&'a Section, usize)>,
    ) -> Result<(), String> {
        if depth >= 9 {
            return Err("DOCX heading hierarchy exceeds supported depth".into());
        }
        let mut children: Vec<_> = draft
            .sections
            .values()
            .filter(|s| s.parent.as_deref() == parent)
            .collect();
        children.sort_by_key(|s| s.order);
        for s in children {
            out.push((s, depth));
            if draft
                .sections
                .values()
                .any(|c| c.parent.as_deref() == Some(&s.id))
            {
                visit(draft, Some(&s.id), depth + 1, out)?;
            }
        }
        Ok(())
    }
    let mut out = vec![];
    visit(draft, None, 0, &mut out)?;
    if out.len() != draft.sections.len() || out.is_empty() {
        return Err("empty or cyclic section tree".into());
    }
    Ok(out)
}

pub(super) fn need(input: &FrozenInput, analysis: &Analysis, r: &Reference) -> Result<(), String> {
    validate_reference(input, analysis, r)?;
    if !matches!(
        r.target,
        RelationTarget::Response { .. } | RelationTarget::Proof { .. }
    ) {
        return Err(
            "bidder placeholder must identify an actual response or proof obligation".into(),
        );
    }
    if matches!(&analysis.records[&r.record_id].data, RecordData::Requirement { applicability, .. }
        if applicability.state == ApplicabilityState::NotApplicable)
    {
        return Err("not-applicable obligation cannot be placed as bidder work".into());
    }
    Ok(())
}

fn place(
    placements: &mut Vec<Placement>,
    record_id: &str,
    target: RelationTarget,
    location: &Location,
) {
    placements.push(Placement {
        reference: Reference {
            record_id: record_id.into(),
            target,
        },
        location: location.clone(),
    });
}

fn validate_excerpt(
    input: &FrozenInput,
    analysis: &Analysis,
    reference: &Reference,
    paragraphs: &[Vec<SourcePart>],
) -> Result<(), String> {
    need(input, analysis, reference)?;
    let record = &analysis.records[&reference.record_id];
    let (
        RecordData::Requirement {
            response,
            applicability,
            ..
        },
        RelationTarget::Response { index },
    ) = (&record.data, &reference.target)
    else {
        return Err("source response requires a response obligation".into());
    };
    if applicability.state == ApplicabilityState::Unknown {
        return Err("unknown applicability cannot authorize a source response".into());
    }
    if paragraphs.is_empty() || paragraphs.iter().any(Vec::is_empty) {
        return Err("source response requires nonempty source paragraphs".into());
    }
    for part in paragraphs.iter().flatten() {
        let supported = |spans: &[Span]| {
            spans.iter().any(|s| {
                s.source_id == part.source_id()
                    && s.view_id.is_none()
                    && match part {
                        SourcePart::Text { start, end, .. } => {
                            s.grid_cell.is_none() && s.start <= *start && *end <= s.end
                        }
                        SourcePart::GridCell {
                            form_id,
                            row,
                            column,
                            ..
                        } => s.grid_cell.as_ref().is_some_and(|c| {
                            c.form_id == *form_id && c.row == *row && c.column == *column
                        }),
                    }
            })
        };
        if !supported(&record.sources) || !supported(&response[*index].grounds) {
            return Err("excerpt is outside the response obligation's frozen evidence".into());
        }
        // Template regions include fixed declarations and sample bidder values;
        // only the complete template path may apply their reviewed copy policy.
        if analysis.records.values().any(|r| match &r.data {
            RecordData::Template { regions, .. } => regions.iter().any(|region| {
                if region.source.source_id != part.source_id() {
                    return false;
                }
                match part {
                    SourcePart::Text { start, end, .. } => {
                        // A flattened grid has no trustworthy cell-to-byte
                        // mapping. Do not bypass its copy policy via text.
                        region.form_id.is_some()
                            || (region.source.view_id.is_none()
                                && *start < region.source.end
                                && region.source.start < *end)
                    }
                    SourcePart::GridCell {
                        form_id,
                        row,
                        column,
                        ..
                    } => {
                        region.form_id.as_ref() == Some(form_id)
                            && region
                                .cells
                                .iter()
                                .any(|c| c.row == *row && c.column == *column)
                    }
                }
            }),
            _ => false,
        }) {
            return Err("source response cannot copy a reviewed template region".into());
        }
    }
    Ok(())
}

fn template(
    input: &FrozenInput,
    result: &AnalysisResult,
    record_id: &str,
    headers: &[FormHeader],
    chapter: (&Section, usize),
    blocks: &mut Vec<TemplateBlock>,
    placements: &mut Vec<Placement>,
) -> Result<(), String> {
    let (section, ordinal) = chapter;
    let record = result
        .analysis
        .records
        .get(record_id)
        .ok_or("unknown template record")?;
    let RecordData::Template {
        regions,
        applicability,
        ..
    } = &record.data
    else {
        return Err("template content requires a template record".into());
    };
    if matches!(
        applicability.state,
        ApplicabilityState::Unknown | ApplicabilityState::NotApplicable
    ) {
        return Err("unknown or not-applicable template cannot be placed".into());
    }
    if applicability.state == ApplicabilityState::Conditional {
        let mut annotation = block("condition");
        annotation.source_id = Some(
            applicability
                .grounds
                .first()
                .ok_or("conditional template needs source grounds")?
                .source_id
                .clone(),
        );
        annotation.condition = Some(applicability.condition.clone());
        place(
            placements,
            record_id,
            RelationTarget::Record,
            &Location {
                section_id: section.id.clone(),
                bookmark: render::bookmark_name(ordinal, Some(blocks.len())),
                cell: None,
            },
        );
        blocks.push(annotation);
    }
    let mut header_map = BTreeMap::new();
    for header in headers {
        if header_map
            .insert(header.form_id.as_str(), header.header_rows)
            .is_some()
        {
            return Err("duplicate form header policy".into());
        }
    }
    let forms: BTreeSet<_> = regions
        .iter()
        .filter_map(|r| r.form_id.as_deref())
        .collect();
    if forms != header_map.keys().copied().collect() {
        return Err("explicit header policy required for exactly this template's grids".into());
    }
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < regions.len() {
        let region = &regions[index];
        let location = Location {
            section_id: section.id.clone(),
            bookmark: render::bookmark_name(ordinal, Some(blocks.len())),
            cell: None,
        };
        place(placements, record_id, RelationTarget::Record, &location);
        if let Some(form_id) = &region.form_id {
            if !seen.insert(form_id.clone()) {
                return Err("interleaved grid regions need an explicit source-order repair".into());
            }
            let form = input
                .structured_forms
                .iter()
                .find(|f| f["form_definition_revision_id"] == *form_id)
                .ok_or("frozen form missing")?;
            let definition = &form["definition"];
            let columns = definition["column_count"]
                .as_u64()
                .ok_or("grid columns missing")? as usize;
            let policies: Vec<_> = (0..columns)
                .map(|c| json!({"column":c,"role":"copy_verbatim"}))
                .collect();
            let table = crate::template_grid::table_block_from_grid(
                definition,
                &policies,
                header_map[form_id.as_str()],
            )?;
            let crate::content_block::BlockContent::Table { cells, .. } = table else {
                return Err("grid expected".into());
            };
            let anchors: BTreeSet<_> = cells.iter().map(|c| (c.row, c.column)).collect();
            let mut assigned = BTreeMap::new();
            let mut out = block("table");
            out.source_id = Some(region.source.source_id.clone());
            out.form_id = Some(form_id.clone());
            out.header_rows = header_map[form_id.as_str()];
            while index < regions.len() && regions[index].form_id.as_ref() == Some(form_id) {
                let r = &regions[index];
                for cell in &r.cells {
                    if !anchors.contains(&(cell.row, cell.column))
                        || assigned.insert((cell.row, cell.column), &r.role).is_some()
                    {
                        return Err(
                            "template grid has foreign, covered or multiply assigned cells".into(),
                        );
                    }
                    let mut field = location.clone();
                    field.cell = Some(CellRef {
                        row: cell.row,
                        column: cell.column,
                    });
                    // A grid region owns these exact anchors, not the entire
                    // table. Preserve every location when binding a need to
                    // the region, including single-cell region/cell aliases.
                    place(
                        placements,
                        record_id,
                        RelationTarget::TemplateRegion { index },
                        &field,
                    );
                    place(
                        placements,
                        record_id,
                        RelationTarget::TemplateCell {
                            form_id: form_id.clone(),
                            row: cell.row,
                            column: cell.column,
                        },
                        &field,
                    );
                    if r.role == RegionRole::BidderBlank && r.blank_ranges.is_empty() {
                        out.blank_cells.push(CellRef {
                            row: cell.row,
                            column: cell.column,
                        });
                    }
                }
                out.blank_ranges.extend(r.blank_ranges.iter().cloned());
                index += 1;
            }
            if assigned.keys().copied().collect::<BTreeSet<_>>() != anchors {
                return Err("every output grid anchor needs an explicit fixed/blank/instruction/signature policy".into());
            }
            blocks.push(out);
        } else {
            if region.source.view_id.is_some() || region.source.grid_cell.is_some() {
                return Err("visual-only wording needs a reviewed editable source; screenshots are not editable templates".into());
            }
            let source = input
                .source_units
                .iter()
                .find(|s| s.source_unit_revision_id == region.source.source_id)
                .ok_or("template source missing")?;
            let quote = source
                .text
                .get(region.source.start..region.source.end)
                .filter(|s| !s.is_empty())
                .ok_or("template region text missing")?;
            let mut out = block(if region.role == RegionRole::BidderBlank {
                "blank"
            } else {
                "quote"
            });
            if region.role != RegionRole::BidderBlank {
                out.source_id = Some(region.source.source_id.clone());
                out.quote = Some(quote.into());
            }
            // Fixed labels/instructions/signatures are separate reviewed regions.
            // Bidder slots must not copy example names or sample filled values.
            blocks.push(out);
            place(
                placements,
                record_id,
                RelationTarget::TemplateRegion { index },
                &location,
            );
            index += 1;
        }
    }
    Ok(())
}

pub fn compile(
    input: &FrozenInput,
    result: &AnalysisResult,
    draft: &Draft,
    max_docx_bytes: usize,
) -> Result<Compiled, String> {
    draft.validate_basis(input, result)?;
    let p = draft
        .presentation
        .as_ref()
        .ok_or("document presentation is not configured")?;
    validate_grounds(input, &result.analysis, &p.grounds)?;
    if p.explanation.trim().is_empty() {
        return Err("presentation decision needs explanation".into());
    }
    let mut plan = TemplatePlan {
        title: p.title.clone(),
        toc_title: p.toc_title.clone(),
        style: p.style.clone(),
        sections: vec![],
        excluded_sources: vec![],
        excluded_forms: vec![],
        notices: vec![],
    };
    let mut placements = vec![];
    let mut source_excerpts = vec![];
    let mut section_locations = vec![];
    for (ordinal, (section, depth)) in ordered_sections(draft)?.into_iter().enumerate() {
        validate_grounds(input, &result.analysis, &section.grounds)?;
        let mut blocks = vec![];
        let before = placements.len();
        for content in &section.content {
            match content {
                Content::SourceResponse {
                    need: reference,
                    paragraphs,
                } => {
                    validate_excerpt(input, &result.analysis, reference, paragraphs)?;
                    let response_location = Location {
                        section_id: section.id.clone(),
                        bookmark: render::bookmark_name(
                            ordinal,
                            Some(blocks.len() + paragraphs.len()),
                        ),
                        cell: None,
                    };
                    for parts in paragraphs {
                        let location = Location {
                            section_id: section.id.clone(),
                            bookmark: render::bookmark_name(ordinal, Some(blocks.len())),
                            cell: None,
                        };
                        let mut out = block("source_excerpt");
                        out.source_parts = parts.clone();
                        blocks.push(out);
                        source_excerpts.push(SourceExcerpt {
                            reference: reference.clone(),
                            parts: parts.clone(),
                            location,
                            response_location: response_location.clone(),
                        });
                    }
                    blocks.push(block("blank"));
                    placements.push(Placement {
                        reference: reference.clone(),
                        location: response_location,
                    });
                }
                Content::Template {
                    record_id,
                    headers,
                    bindings,
                } => {
                    let start = placements.len();
                    template(
                        input,
                        result,
                        record_id,
                        headers,
                        (section, ordinal),
                        &mut blocks,
                        &mut placements,
                    )
                    .map_err(|e| format!("section {} template {record_id}: {e}", section.id))?;
                    let own = placements[start..].to_vec();
                    for binding in bindings {
                        need(input, &result.analysis, &binding.need)?;
                        let destinations: Vec<_> = own
                            .iter()
                            .filter(|p| p.reference.target == binding.field)
                            .collect();
                        if destinations.is_empty() {
                            return Err(
                                "response binding is not a rendered template location".into()
                            );
                        }
                        for destination in destinations {
                            placements.push(Placement {
                                reference: binding.need.clone(),
                                location: destination.location.clone(),
                            });
                        }
                    }
                }
                Content::Placeholder { needs } | Content::ResponseTable { needs, .. } => {
                    if needs.is_empty() {
                        return Err("empty bidder work placeholder".into());
                    }
                    let location = Location {
                        section_id: section.id.clone(),
                        bookmark: render::bookmark_name(ordinal, Some(blocks.len())),
                        cell: None,
                    };
                    for r in needs {
                        need(input, &result.analysis, r)?;
                        placements.push(Placement {
                            reference: r.clone(),
                            location: location.clone(),
                        });
                    }
                    if let Content::ResponseTable {
                        columns,
                        blank_rows,
                        ..
                    } = content
                    {
                        let source =
                            &result.analysis.records[&needs[0].record_id].sources[0].source_id;
                        let mut out = block("response_table");
                        out.source_id = Some(source.clone());
                        out.columns = columns.clone();
                        out.blank_rows = *blank_rows;
                        out.header_rows = 1;
                        blocks.push(out);
                        plan.notices.push(
                            "Proposed response grid; verify against tender-prescribed formats"
                                .into(),
                        );
                    } else {
                        blocks.push(block("blank"));
                    }
                }
            }
        }
        if blocks.is_empty() {
            if !draft
                .sections
                .values()
                .any(|s| s.parent.as_deref() == Some(&section.id))
            {
                return Err("leaf chapter has no source template or bidder work".into());
            }
            blocks.push(block("blank"));
        }
        let mut sources: BTreeSet<_> = section
            .grounds
            .iter()
            .map(|s| s.source_id.clone())
            .collect();
        for placement in &placements[before..] {
            sources.extend(
                result.analysis.records[&placement.reference.record_id]
                    .sources
                    .iter()
                    .map(|s| s.source_id.clone()),
            );
        }
        for b in &blocks {
            sources.extend(b.source_parts.iter().map(|p| p.source_id().to_owned()));
            if let Some(source) = &b.source_id {
                sources.insert(source.clone());
            }
        }
        plan.sections.push(TemplateSection {
            title: section.title.clone(),
            placement: section.placement,
            depth,
            source_ids: sources.into_iter().collect(),
            blocks,
        });
        section_locations.push(Location {
            section_id: section.id.clone(),
            bookmark: render::bookmark_name(ordinal, None),
            cell: None,
        });
    }
    account(input, result, draft, &placements)?;
    let used_sources: BTreeSet<_> = plan
        .sections
        .iter()
        .flat_map(|s| s.source_ids.iter().cloned())
        .collect();
    let used_forms: BTreeSet<_> = plan
        .sections
        .iter()
        .flat_map(|s| {
            s.blocks.iter().flat_map(|b| {
                b.form_id.iter().cloned().chain(
                    b.source_parts
                        .iter()
                        .filter_map(|p| p.form_id().map(str::to_owned)),
                )
            })
        })
        .collect();
    for source in &input.source_units {
        if !used_sources.contains(&source.source_unit_revision_id) {
            plan.excluded_sources.push(ExcludedSource {
                source_id: source.source_unit_revision_id.clone(),
                reason: "Retained in frozen analysis; not copied into this document".into(),
            });
        }
    }
    for form in &input.structured_forms {
        let id = form["form_definition_revision_id"]
            .as_str()
            .ok_or("form identity missing")?;
        if !used_forms.contains(id) {
            plan.excluded_forms.push(ExcludedForm {
                form_id: id.into(),
                reason: "Retained in frozen analysis; not copied into this document".into(),
            });
        }
    }
    let value = serde_json::to_value(input).map_err(|e| e.to_string())?;
    let docx = render::compile_template(&value, &plan).map_err(|e| e.to_string())?;
    if docx.len() > max_docx_bytes {
        return Err("compiled DOCX exceeds configured byte budget".into());
    }
    let rendered = super::document::verify(&docx, &value, &plan)?;
    use sha2::{Digest, Sha256};
    let manifest = Manifest {
        schema_version: 1,
        analysis_sha256: draft.analysis_sha256.clone(),
        draft_sha256: digest(draft)?,
        docx_sha256: hex::encode(Sha256::digest(&docx)),
        sections: section_locations,
        placements,
        source_excerpts,
        omissions: draft.omissions.values().cloned().collect(),
        relation_omissions: draft.relation_omissions.values().cloned().collect(),
        source_quality: result.quality.clone(),
        source_open_items: result.open_items(input),
        status: "needs_review".into(),
    };
    Ok(Compiled {
        docx,
        manifest,
        rendered,
    })
}

fn account(
    input: &FrozenInput,
    result: &AnalysisResult,
    draft: &Draft,
    placements: &[Placement],
) -> Result<(), String> {
    let mut accounted: BTreeSet<String> = placements
        .iter()
        .map(|p| reference_key(&p.reference))
        .collect::<Result<_, _>>()?;
    for (key, omission) in &draft.omissions {
        validate_reference(input, &result.analysis, &omission.reference)?;
        validate_grounds(input, &result.analysis, &omission.grounds)?;
        if omission.reason.trim().is_empty()
            || *key != reference_key(&omission.reference)?
            || !accounted.insert(key.clone())
        {
            return Err("invalid or simultaneously placed and omitted obligation".into());
        }
    }
    let mut missing = vec![];
    for r in required_references(result) {
        if !accounted.contains(&reference_key(&r)?) {
            missing.push(r);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "{} template/response/proof dispositions missing; inspect composition coverage",
            missing.len()
        ));
    }
    // Explicit alternatives/conditional formats are reviewed decisions. Never
    // turn every source edge into an implicit AND, or infer OR from keywords.
    for (id, omission) in &draft.relation_omissions {
        if omission.relation_id != *id
            || omission.reason.trim().is_empty()
            || result
                .analysis
                .relations
                .get(id)
                .is_none_or(|r| r.kind != RelationKind::RequiresTemplate)
        {
            return Err("invalid prescribed-format relation omission".into());
        }
        validate_grounds(input, &result.analysis, &omission.grounds)?;
    }
    // A generic blank/proposed table cannot stand in for a selected prescribed format.
    for relation in result.analysis.relations.values().filter(|r| {
        r.kind == RelationKind::RequiresTemplate && r.state != RelationState::Unresolved
    }) {
        if draft.relation_omissions.contains_key(&relation.id) {
            continue;
        }
        let template_places: Vec<_> = placements
            .iter()
            .filter(|p| p.reference.record_id == relation.to)
            .collect();
        let need_places: Vec<_> = placements
            .iter()
            .filter(|p| {
                p.reference.record_id == relation.from
                    && (relation.from_target == RelationTarget::Record
                        || p.reference.target == relation.from_target)
            })
            .collect();
        if !need_places.is_empty()
            && !need_places.iter().any(|p| {
                template_places.iter().any(|t| {
                    t.location.bookmark == p.location.bookmark
                        && t.location.section_id == p.location.section_id
                })
            })
        {
            return Err(format!(
                "response {} must include prescribed template {} (relation {})",
                relation.from, relation.to, relation.id
            ));
        }
    }
    Ok(())
}

pub fn required_references(result: &AnalysisResult) -> Vec<Reference> {
    result
        .analysis
        .records
        .values()
        .flat_map(|record| {
            let targets = match &record.data {
                RecordData::Template { .. } => vec![RelationTarget::Record],
                RecordData::Requirement {
                    response, proofs, ..
                } => (0..response.len())
                    .map(|index| RelationTarget::Response { index })
                    .chain((0..proofs.len()).map(|index| RelationTarget::Proof { index }))
                    .collect(),
                _ => vec![],
            };
            targets.into_iter().map(move |target| Reference {
                record_id: record.id.clone(),
                target,
            })
        })
        .collect()
}
