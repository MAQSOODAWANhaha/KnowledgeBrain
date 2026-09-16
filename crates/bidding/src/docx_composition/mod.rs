//! Incremental composition of a new DOCX from one frozen, independently
//! reviewed tender analysis. This workspace contains no bidder facts.
pub mod agent;
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
        validate_basis(input, result)?;
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
        validate_basis(input, result)?;
        if self.schema_version != 1 || self.analysis_sha256 != digest(result)? {
            return Err("composition belongs to another frozen analysis version".into());
        }
        Ok(())
    }
}

pub fn validate_basis(input: &FrozenInput, result: &AnalysisResult) -> Result<(), String> {
    crate::tender_analysis::tools::validate_input(input)?;
    if !matches!(result.schema_version, 1 | 2)
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
    if result.schema_version == 2 {
        crate::tender_analysis::rule_contract::validate_inventory(
            input,
            &result.analysis,
            &result.review.global_checks,
            &result.review.findings,
        )?;
    }
    Ok(())
}

pub fn obligation_inventory(result: &AnalysisResult) -> Vec<Reference> {
    compiler::required_references(result)
}

pub fn plan_complete(result: &AnalysisResult, draft: &Draft) -> Result<bool, String> {
    let mut accounted = std::collections::BTreeSet::new();
    for item in draft.plan.values() {
        if item
            .parent
            .as_ref()
            .is_some_and(|parent| !draft.plan.contains_key(parent))
        {
            return Ok(false);
        }
        for r in &item.obligation_refs {
            accounted.insert(r.clone());
        }
        if let Some(exception) = &item.exception
            && exception.trim().is_empty()
        {
            return Ok(false);
        }
    }
    for omission in draft.omissions.values() {
        accounted.insert(reference_key(&omission.reference)?);
    }
    for r in obligation_inventory(result) {
        if !accounted.contains(&reference_key(&r)?) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn implementation_complete(draft: &Draft, artifact: Option<&compiler::Compiled>) -> bool {
    if draft.plan.is_empty() {
        return false;
    }
    let Some(compiled) = artifact else {
        return false;
    };
    if compiled.manifest.plan_sha256 != digest(&draft.plan).ok().unwrap_or_default() {
        return false;
    }
    draft.plan.values().all(|item| match item.kind {
        PlanItemKind::Section => draft.sections.contains_key(&item.id),
        PlanItemKind::Presentation => draft.presentation.is_some(),
        PlanItemKind::ReportNote => true,
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
