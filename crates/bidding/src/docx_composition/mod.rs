//! Deterministic DOCX compilation and optional filling of saved chapters.
pub mod compiler;
pub mod document;
pub mod fill;
pub mod postgres;
pub mod runtime;
#[cfg(test)]
#[path = "tests/compiler_fixture.rs"]
mod tools;

use crate::{
    docx_template::{SectionPlacement, SourcePart, TemplateStyle},
    tender_analysis::*,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub record_id: String,
    pub target: RelationTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldBinding {
    pub need: Reference,
    pub field: RelationTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormHeader {
    pub form_id: String,
    pub header_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    /// Expands the complete ordered template, not an arbitrary model rewrite.
    Template {
        record_id: String,
        headers: Vec<FormHeader>,
        bindings: Vec<FieldBinding>,
    },
    /// A pending bidder response/proof, deliberately without invented text.
    Placeholder { needs: Vec<Reference> },
    /// A chapter listed in the outline whose body the bidder still writes.
    /// Draft-only: it carries no obligation, so official composition rejects it.
    BidderBlank,
    /// The body the user already wrote, read back from the saved document and
    /// re-emitted verbatim. Draft-only, and the text comes from the readback
    /// receipt rather than from this plan, so refilling never rewrites it.
    Preserved {
        blocks: Vec<crate::tender_analysis::readback::Preserved>,
    },
    /// Verbatim requirement paragraphs followed by a separate empty response.
    SourceResponse {
        need: Reference,
        paragraphs: Vec<Vec<SourcePart>>,
    },
    ResponseTable {
        needs: Vec<Reference>,
        columns: Vec<String>,
        blank_rows: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: String,
    #[serde(default, skip_serializing_if = "SectionPlacement::is_body")]
    pub placement: SectionPlacement,
    pub parent: Option<String>,
    pub order: usize,
    pub title: String,
    pub grounds: Vec<Span>,
    pub content: Vec<Content>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Omission {
    pub reference: Reference,
    pub reason: String,
    pub grounds: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationOmission {
    pub relation_id: String,
    pub reason: String,
    pub grounds: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub title: String,
    pub toc_title: String,
    pub style: TemplateStyle,
    pub grounds: Vec<Span>,
    /// Explicit explanation of tender requirements / configured presentation.
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanItemKind {
    Section,
    Presentation,
    ReportNote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanItem {
    pub id: String,
    pub kind: PlanItemKind,
    pub parent: Option<String>,
    pub order: usize,
    pub title: String,
    #[serde(default, skip_serializing_if = "SectionPlacement::is_body")]
    pub placement: SectionPlacement,
    pub prescribed: bool,
    pub grounds: Vec<Span>,
    #[serde(default)]
    pub obligation_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub schema_version: u32,
    pub analysis_sha256: String,
    pub presentation: Option<Presentation>,
    pub sections: BTreeMap<String, Section>,
    pub omissions: BTreeMap<String, Omission>,
    pub relation_omissions: BTreeMap<String, RelationOmission>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plan: BTreeMap<String, PlanItem>,
}

impl Draft {
    pub fn new(input: &FrozenInput, result: &AnalysisResult) -> Result<Self, String> {
        if result.review.draft {
            validate_draft_basis(input, result)?;
        } else {
            validate_basis(input, result)?;
        }
        Ok(Self {
            schema_version: 1,
            analysis_sha256: digest(result)?,
            presentation: None,
            sections: BTreeMap::new(),
            omissions: BTreeMap::new(),
            relation_omissions: BTreeMap::new(),
            plan: BTreeMap::new(),
        })
    }

    pub fn validate_basis(
        &self,
        input: &FrozenInput,
        result: &AnalysisResult,
    ) -> Result<(), String> {
        if result.review.draft {
            validate_draft_basis(input, result)?;
        } else {
            validate_basis(input, result)?;
        }
        if self.schema_version != 1 || self.analysis_sha256 != digest(result)? {
            return Err("composition belongs to another frozen analysis version".into());
        }
        Ok(())
    }
}

pub fn validate_draft_basis(input: &FrozenInput, result: &AnalysisResult) -> Result<(), String> {
    crate::tender_analysis::tools::validate_input(input)?;
    if !result.review.draft {
        return Err("draft composition requires review.draft".into());
    }
    if result.schema_version != 2
        || result.quality != "needs_review"
        || result.frozen_input_sha256 != digest(input)?
        || result.review.analysis_sha256 != digest(&result.analysis)?
        || !crate::tender_analysis::draft::plan_ready(&result.analysis.draft_plan)
        || !crate::tender_analysis::draft::filled_templates_present(&result.analysis)
    {
        return Err("draft composition requires a frozen draft_plan analysis marked draft".into());
    }
    Ok(())
}

/// 每个 grid region 的 header 行数由填章时的作者声明，宿主只做搬运。
///
/// 编译器要求 header policy 与模板里的 grid **一一对应**，缺一个就整篇编译失
/// 败；带表格的章是投标文件的主场景（报价表、业绩表、人员表），所以这里不允
/// 许静默给默认值。
fn form_headers(record: &crate::tender_analysis::Record) -> Result<Vec<FormHeader>, String> {
    use crate::tender_analysis::RecordData;
    let RecordData::Template { regions, .. } = &record.data else {
        return Err("filled chapter record is not a template".into());
    };
    let mut headers: std::collections::BTreeMap<String, usize> = Default::default();
    for region in regions {
        let Some(form_id) = region.form_id.as_deref() else {
            continue;
        };
        let header_rows = region
            .header_rows
            .ok_or("grid region has no header policy to compile")?;
        if headers
            .insert(form_id.to_string(), header_rows)
            .is_some_and(|prior| prior != header_rows)
        {
            return Err("grid regions disagree on the form header policy".into());
        }
    }
    Ok(headers
        .into_iter()
        .map(|(form_id, header_rows)| FormHeader {
            form_id,
            header_rows,
        })
        .collect())
}

/// 宿主从 draft_plan + Template 合成编制稿，无编制 Agent。
///
/// 每个活节点都进文档：Filled 章带模板正文，未填章保留为空 heading。只有
/// `omitted` 章允许不出现，否则「填一部分也能出稿」会产出缺章的 Word，用户
/// 会以为章节被删了。
pub fn synthesize_draft_document(
    input: &FrozenInput,
    result: &AnalysisResult,
) -> Result<Draft, String> {
    use crate::tender_analysis::draft::{DraftPlanItem, DraftStatus};
    validate_draft_basis(input, result)?;
    let mut draft = Draft::new(input, result)?;
    let plan = &result.analysis.draft_plan;
    crate::tender_analysis::readback::validate_seed(&result.analysis)?;
    let retained = !result.analysis.fill_seed_chapters.is_empty();
    crate::tender_analysis::outline_flow::tree_valid(plan)?;
    // The compiler validates every ground against what the run actually read.
    // A large tender only reads its anchor windows, so a whole-source span would
    // fail; the skeleton cites a read range instead.
    let read = |source_id: &str| {
        result
            .analysis
            .coverage
            .text
            .get(source_id)
            .and_then(|ranges| ranges.first())
            .map(|(start, end)| Span {
                source_id: source_id.to_string(),
                start: *start,
                end: *end,
                view_id: None,
                grid_cell: None,
            })
    };
    let unbound = result
        .analysis
        .coverage
        .text
        .iter()
        .find(|(_, ranges)| !ranges.is_empty())
        .map(|(id, _)| id.clone());
    if unbound.is_none() && !retained {
        return Err("draft skeleton needs a read source range".into());
    }
    let retained_grounds = |item: &DraftPlanItem| -> Result<Vec<Span>, String> {
        if retained {
            return Ok(item.grounds.clone());
        }
        let bound: Vec<_> = if !item.grounds.is_empty() {
            item.grounds.clone()
        } else {
            item.source_ids.iter().filter_map(|id| read(id)).collect()
        };
        if !bound.is_empty() {
            Ok(bound)
        } else {
            Ok(vec![
                unbound
                    .as_deref()
                    .and_then(read)
                    .ok_or("read source range vanished")?,
            ])
        }
    };
    let node_of = |id: &str| plan.iter().find(|node| node.id == id);
    // An omitted volume still holds its live children, so it keeps a heading;
    // an omitted leaf is the only node allowed to leave the document.
    let carries_live_child = |item: &DraftPlanItem| {
        plan.iter().any(|node| {
            let mut current = node.parent.clone();
            while let Some(id) = current {
                if id == item.id {
                    return node.status != DraftStatus::Omitted;
                }
                current = node_of(&id).and_then(|node| node.parent.clone());
            }
            false
        })
    };
    let mut sources = std::collections::BTreeSet::new();
    for item in plan {
        if item.status == DraftStatus::Omitted && !retained && !carries_live_child(item) {
            continue;
        }
        let mut parent = item.parent.clone();
        if parent.as_ref().is_some_and(|id| node_of(id).is_none()) {
            parent = None;
        }
        let (content, grounds, obligation_refs) = match item.status {
            // The user's own body outranks everything else: a chapter read back
            // with text keeps that text, whatever the plan says about filling it.
            _ if !item.preserved.is_empty() => {
                let grounds = retained_grounds(item)?;
                (
                    vec![Content::Preserved {
                        blocks: item.preserved.clone(),
                    }],
                    grounds,
                    vec![],
                )
            }
            DraftStatus::Filled => {
                let record_id = item
                    .template_id
                    .clone()
                    .ok_or("filled chapter missing template")?;
                let record = result
                    .analysis
                    .records
                    .get(&record_id)
                    .ok_or("filled chapter template missing from records")?;
                let span = record
                    .sources
                    .first()
                    .cloned()
                    .ok_or("template needs source grounds")?;
                let headers = form_headers(record)?;
                let obligation = reference_key(&Reference {
                    record_id: record_id.clone(),
                    target: RelationTarget::Record,
                })?;
                (
                    vec![Content::Template {
                        record_id,
                        headers,
                        bindings: vec![],
                    }],
                    vec![span],
                    vec![obligation],
                )
            }
            // An unfilled chapter keeps its heading. A parent holds its children
            // instead of a body; a leaf declares the body as bidder work. Either
            // way it cites a read range so the compiler can ground it.
            _ => {
                let grounds = retained_grounds(item)?;
                let content = match carries_live_child(item) {
                    true => vec![],
                    false => vec![Content::BidderBlank],
                };
                (content, grounds, vec![])
            }
        };
        sources.extend(grounds.iter().map(|span: &Span| span.source_id.clone()));
        draft.plan.insert(
            item.id.clone(),
            PlanItem {
                id: item.id.clone(),
                kind: PlanItemKind::Section,
                parent: parent.clone(),
                order: item.order,
                title: item.title.clone(),
                placement: Default::default(),
                prescribed: item.prescribed,
                grounds: grounds.clone(),
                obligation_refs,
                exception: None,
            },
        );
        draft.sections.insert(
            item.id.clone(),
            Section {
                id: item.id.clone(),
                placement: Default::default(),
                parent,
                order: item.order,
                title: item.title.clone(),
                grounds,
                content,
            },
        );
    }
    let mut grounds: Vec<Span> = sources.iter().filter_map(|id| read(id)).collect();
    if grounds.is_empty() && !retained {
        grounds.push(
            unbound
                .as_deref()
                .and_then(read)
                .ok_or("read source range vanished")?,
        );
    }
    let title = input
        .documents
        .first()
        .and_then(|doc| doc.get("file_name").and_then(|value| value.as_str()))
        .unwrap_or("投标文件草稿")
        .to_string();
    draft.presentation = Some(Presentation {
        title: title.clone(),
        toc_title: "目录".into(),
        style: TemplateStyle {
            width_mm: 210.0,
            height_mm: 297.0,
            top_mm: 25.0,
            right_mm: 25.0,
            bottom_mm: 25.0,
            left_mm: 25.0,
            font_family: "SimSun".into(),
            body_font_pt: 12.0,
            line_spacing: 1.5,
        },
        grounds,
        explanation: "草稿默认版式".into(),
    });
    Ok(draft)
}

pub fn validate_basis(input: &FrozenInput, result: &AnalysisResult) -> Result<(), String> {
    crate::tender_analysis::tools::validate_input(input)?;
    if result.review.draft {
        return Err("official composition rejects draft analysis; use validate_draft_basis".into());
    }
    if result.schema_version != 2
        || result.quality != result.expected_quality(input)
        || result.frozen_input_sha256 != digest(input)?
        || result.review.analysis_sha256 != digest(&result.analysis)?
        || !result.review.findings.is_empty()
        || !crate::tender_analysis::tools::gaps(input, &result.analysis).is_empty()
        || !crate::tender_analysis::tools::review_gaps(
            input,
            &result.analysis,
            &result.review.coverage,
        )
        .is_empty()
    {
        return Err("composition requires a frozen, independently reviewed analysis with no outstanding review findings and an accurate source quality".into());
    }
    crate::tender_analysis::rule_contract::validate_review(
        input,
        &result.analysis,
        &result.review,
    )?;
    Ok(())
}

pub fn obligation_inventory(result: &AnalysisResult) -> Vec<Reference> {
    compiler::required_references(result)
}

pub fn validate_plan(result: &AnalysisResult, draft: &Draft) -> Result<(), String> {
    if !plan_complete(result, draft)? {
        return Err("composition plan must account for every obligation before compilation".into());
    }
    Ok(())
}

pub fn plan_complete(result: &AnalysisResult, draft: &Draft) -> Result<bool, String> {
    use std::collections::BTreeSet;
    if draft.plan.is_empty() {
        return Ok(false);
    }
    let required: BTreeSet<_> = obligation_inventory(result)
        .iter()
        .map(reference_key)
        .collect::<Result<_, _>>()?;
    let mut accounted = BTreeSet::new();
    let mut positions = BTreeSet::new();
    for (id, item) in &draft.plan {
        if id != &item.id
            || id.trim().is_empty()
            || item.title.trim().is_empty()
            || item.grounds.is_empty()
            || item.exception.as_ref().is_some_and(|e| e.trim().is_empty())
        {
            return Err("invalid composition plan item".into());
        }
        if !positions.insert((
            item.parent.clone(),
            format!("{:?}:{:?}", item.kind, item.placement),
            item.order,
        )) {
            return Err("plan siblings must have distinct order".into());
        }
        let mut ancestors = BTreeSet::from([id.as_str()]);
        let mut parent = item.parent.as_deref();
        while let Some(key) = parent {
            if !ancestors.insert(key) {
                return Err("composition plan parent cycle".into());
            }
            let ancestor = draft.plan.get(key).ok_or("unknown plan parent")?;
            if ancestor.kind != PlanItemKind::Section {
                return Err("plan parent must be a section".into());
            }
            parent = ancestor.parent.as_deref();
        }
        let mut seen = BTreeSet::new();
        for reference in &item.obligation_refs {
            if !required.contains(reference) || !seen.insert(reference) {
                return Err(
                    "plan obligation must identify a distinct current required reference".into(),
                );
            }
            accounted.insert(reference.clone());
        }
    }
    for omission in draft.omissions.values() {
        let key = reference_key(&omission.reference)?;
        if !required.contains(&key) {
            return Err("omission is not a current obligation".into());
        }
        accounted.insert(key);
    }
    Ok(required.is_subset(&accounted))
}

pub fn validate_plan_sections(draft: &Draft) -> Result<(), String> {
    for (id, section) in &draft.sections {
        let item = draft
            .plan
            .get(id)
            .ok_or("section has no composition plan item")?;
        if item.kind != PlanItemKind::Section
            || section.id != item.id
            || section.parent != item.parent
            || section.order != item.order
            || section.title != item.title
            || section.placement != item.placement
        {
            return Err(
                "section identity, parent, order, title and placement must match its plan".into(),
            );
        }
        if section.content.is_empty()
            && !draft
                .sections
                .values()
                .any(|s| s.parent.as_deref() == Some(id))
        {
            return Err("planned section needs content or child sections".into());
        }
    }
    Ok(())
}

pub fn implementation_complete(draft: &Draft, artifact: Option<&compiler::Compiled>) -> bool {
    artifact.is_some_and(|compiled| implementation_manifest_complete(draft, &compiled.manifest))
}

pub(crate) fn implementation_manifest_complete(
    draft: &Draft,
    manifest: &compiler::Manifest,
) -> bool {
    if draft.plan.is_empty()
        || validate_plan_sections(draft).is_err()
        || digest(&draft.plan).ok().as_deref() != Some(manifest.plan_sha256.as_str())
        || digest(draft).ok().as_deref() != Some(manifest.draft_sha256.as_str())
    {
        return false;
    }
    draft.plan.values().all(|item| match item.kind {
        PlanItemKind::Section => {
            draft.sections.contains_key(&item.id)
                && manifest
                    .sections
                    .iter()
                    .any(|location| location.section_id == item.id)
                && item.obligation_refs.iter().all(|key| {
                    manifest.placements.iter().any(|placement| placement.location.section_id == item.id
                        && reference_key(&placement.reference).ok().as_ref() == Some(key))
                    || manifest.rule_implementations.iter().any(|implementation| implementation.plan_item_id == item.id
                        && reference_key(&implementation.reference).ok().as_ref() == Some(key)
                        && matches!(&implementation.implementation, compiler::RuleImplementationTarget::Section { location, .. } if location.section_id == item.id))
                })
        }
        PlanItemKind::Presentation => draft
            .presentation
            .as_ref()
            .is_some_and(|p| p.title == item.title)
            && item.obligation_refs.iter().all(|key| manifest.rule_implementations.iter().any(|implementation|
                implementation.plan_item_id == item.id && reference_key(&implementation.reference).ok().as_ref() == Some(key)
                    && matches!(implementation.implementation, compiler::RuleImplementationTarget::Presentation { .. }))),
        PlanItemKind::ReportNote => item.exception.as_ref().is_some_and(|reason| {
            !item.obligation_refs.is_empty()
                && item.obligation_refs.iter().all(|key| {
                    manifest.omissions.iter().any(|omission| {
                        reference_key(&omission.reference).ok().as_ref() == Some(key)
                            && &omission.reason == reason
                    })
                })
        }),
    })
}

pub fn reference_key(reference: &Reference) -> Result<String, String> {
    digest(reference)
}

pub fn validate_reference(
    input: &FrozenInput,
    analysis: &Analysis,
    r: &Reference,
) -> Result<(), String> {
    let record = analysis
        .records
        .get(&r.record_id)
        .ok_or("unknown composition record")?;
    relations::validate_target(input, record, &r.target)
}

pub fn validate_grounds(
    input: &FrozenInput,
    analysis: &Analysis,
    grounds: &[Span],
) -> Result<(), String> {
    if grounds.is_empty() {
        return Err("composition decision needs source grounds".into());
    }
    for span in grounds {
        crate::tender_analysis::tools::validate_span(input, &analysis.coverage, span)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
