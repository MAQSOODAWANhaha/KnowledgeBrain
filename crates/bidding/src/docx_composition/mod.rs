//! Incremental composition of a new DOCX from one frozen, independently
//! reviewed tender analysis. This workspace contains no bidder facts.
pub mod agent;
mod agent_scope;
mod agent_work;
pub mod compiler;
pub mod document;
pub mod postgres;
pub mod runtime;
pub mod tools;

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

/// 宿主从 draft_plan + Template 合成编制稿，无编制 Agent。
pub fn synthesize_draft_document(
    input: &FrozenInput,
    result: &AnalysisResult,
) -> Result<Draft, String> {
    validate_draft_basis(input, result)?;
    let mut draft = Draft::new(input, result)?;
    let mut grounds = Vec::new();
    let plan = &result.analysis.draft_plan;
    for item in plan {
        if item.status != crate::tender_analysis::draft::DraftStatus::Filled {
            continue;
        }
        let template_id = item
            .template_id
            .clone()
            .ok_or("filled chapter missing template")?;
        let record = result
            .analysis
            .records
            .get(&template_id)
            .ok_or("filled chapter template missing from records")?;
        let span = record
            .sources
            .first()
            .cloned()
            .ok_or("template needs source grounds")?;
        grounds.push(span.clone());
        let obligation = reference_key(&Reference {
            record_id: template_id.clone(),
            target: RelationTarget::Record,
        })?;
        let mut parent = item.parent.clone();
        if parent
            .as_ref()
            .is_some_and(|id| !plan.iter().any(|node| node.id == *id))
        {
            parent = None;
        }
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
                grounds: vec![span.clone()],
                obligation_refs: vec![obligation],
                exception: None,
            },
        );
        draft.sections.insert(
            item.id.clone(),
            Section {
                id: item.id.clone(),
                placement: Default::default(),
                parent: parent.clone(),
                order: item.order,
                title: item.title.clone(),
                grounds: vec![span.clone()],
                content: vec![Content::Template {
                    record_id: template_id,
                    headers: vec![],
                    bindings: vec![],
                }],
            },
        );
        let mut ancestor = parent;
        while let Some(id) = ancestor {
            if draft.sections.contains_key(&id) {
                break;
            }
            let Some(node) = plan.iter().find(|node| node.id == id) else {
                break;
            };
            let mut node_parent = node.parent.clone();
            if node_parent
                .as_ref()
                .is_some_and(|pid| !plan.iter().any(|n| n.id == *pid))
            {
                node_parent = None;
            }
            draft.plan.insert(
                node.id.clone(),
                PlanItem {
                    id: node.id.clone(),
                    kind: PlanItemKind::Section,
                    parent: node_parent.clone(),
                    order: node.order,
                    title: node.title.clone(),
                    placement: Default::default(),
                    prescribed: node.prescribed,
                    grounds: vec![span.clone()],
                    obligation_refs: vec![],
                    exception: None,
                },
            );
            draft.sections.insert(
                node.id.clone(),
                Section {
                    id: node.id.clone(),
                    placement: Default::default(),
                    parent: node_parent.clone(),
                    order: node.order,
                    title: node.title.clone(),
                    grounds: vec![span.clone()],
                    content: vec![],
                },
            );
            ancestor = node_parent;
        }
    }
    if grounds.is_empty() {
        let source = input
            .source_units
            .first()
            .ok_or("draft skeleton needs a frozen source")?;
        grounds.push(Span {
            source_id: source.source_unit_revision_id.clone(),
            start: 0,
            end: source.text.len(),
            view_id: None,
            grid_cell: None,
        });
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
