//! Tender-side semantic analysis. Source geometry is immutable; interpretations
//! are versioned records, reviewed independently before publication.
pub mod agent;
pub mod evidence_refs;
pub mod postgres;
pub mod relations;
pub mod rule_contract;
pub mod semantic_compare;
pub mod source_review;
pub mod tools;
pub mod views;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub use rule_contract::{
    ANALYSIS_GLOBAL_CHECK_KEYS, GlobalCheck, GlobalCheckConclusion, RuleItem, RuleItemKind,
    RuleItemTarget,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub source_id: String,
    pub start: usize,
    pub end: usize,
    /// A visual citation has start=end=0 and references a delivered original
    /// view. It must not fabricate a quotation in the parser's text layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_id: Option<String>,
    /// Exact frozen grid evidence, with zero text offsets and no visual claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_cell: Option<GridCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GridCitation {
    pub form_id: String,
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub source_unit_revision_id: String,
    pub document_id: String,
    pub text: String,
    pub locator: Value,
    pub ordinal: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenInput {
    pub schema_version: u32,
    pub project_id: String,
    pub document_set_id: String,
    pub documents: Vec<Value>,
    pub document_relations: Vec<Value>,
    pub source_units: Vec<Source>,
    pub structured_forms: Vec<Value>,
    pub decisions: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApplicabilityState {
    Applicable,
    NotApplicable,
    Conditional,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Applicability {
    pub state: ApplicabilityState,
    pub condition: String,
    pub scope: String,
    pub grounds: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Mandatory,
    Optional,
    Informational,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Compliance {
    MustComply,
    ExplicitResponse,
    DeviationAllowed,
    Scored,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComplianceClaim {
    pub policy: Compliance,
    pub condition: String,
    pub grounds: Vec<Span>,
}

/// Source-defined criteria, never an industry-specific parameter dictionary.
/// Keep original notation (including units and operators); normalization must
/// not silently change a threshold or turn an alternative into a conjunction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub subject: String,
    pub aspect: String,
    pub operator: String,
    pub value: String,
    pub unit: String,
    pub condition: String,
    pub grounds: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseChannel {
    NarrativeContent,
    ResponseTable,
    DeviationStatement,
    StructuredForm,
    EvidenceAttachment,
    Quotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseNeed {
    pub channel: ResponseChannel,
    pub description: String,
    /// Additional response trigger. Empty means no extra condition beyond the
    /// parent requirement's applicability and governing compliance conditions.
    pub condition: String,
    pub grounds: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Qualification,
    Rejection,
    Evaluation,
    Commercial,
    Technical,
    Pricing,
    Personnel,
    Delivery,
    Format,
    Attachment,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofNeed {
    pub description: String,
    pub subject: String,
    pub validity: String,
    pub issuer: String,
    pub condition: String,
    pub grounds: Vec<Span>,
    pub name_required: bool,
    pub page_required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionRole {
    FixedText,
    TenderValue,
    BidderBlank,
    Instruction,
    Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRegion {
    pub source: Span,
    pub role: RegionRole,
    pub form_id: Option<String>,
    pub cells: Vec<Cell>,
    /// Optional partial blanks; nonempty means every selected cell uses ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blank_ranges: Vec<crate::template_grid::CellTextRange>,
    pub instruction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordData {
    Fact {
        name: String,
        value: String,
        scope: String,
    },
    Rule {
        text: String,
        scope: String,
        applicability: Applicability,
        #[serde(default)]
        items: Vec<RuleItem>,
    },
    Requirement {
        text: String,
        categories: Vec<Category>,
        strength: Strength,
        compliance: Vec<ComplianceClaim>,
        applicability: Applicability,
        response: Vec<ResponseNeed>,
        scoring_rule: Option<String>,
        proofs: Vec<ProofNeed>,
        criteria: Vec<Criterion>,
    },
    Template {
        label: String,
        title: String,
        parent: Option<String>,
        order: Option<usize>,
        purpose: String,
        applicability: Applicability,
        regions: Vec<TemplateRegion>,
    },
    Unresolved {
        problem: String,
        affected: Vec<String>,
        candidates: Vec<String>,
    },
}

impl RecordData {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Fact { .. } => "fact",
            Self::Rule { .. } => "rule",
            Self::Requirement { .. } => "requirement",
            Self::Template { .. } => "template",
            Self::Unresolved { .. } => "unresolved",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub sources: Vec<Span>,
    pub data: RecordData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    References,
    RequiresTemplate,
    Contains,
    RequiresProof,
    SameValue,
    Aggregates,
    RespondsTo,
    Amends,
    Replaces,
    Withdraws,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationState {
    Explicit,
    Inferred,
    Unresolved,
}

/// Locations within a semantic record, never appendix labels or grid ordinals
/// from a different document. Record digests on Relation bind these selectors
/// to the exact reviewed interpretation, including ordered region/need arrays.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelationTarget {
    Record,
    TemplateRegion {
        index: usize,
    },
    TemplateCell {
        form_id: String,
        row: usize,
        column: usize,
    },
    Response {
        index: usize,
    },
    Proof {
        index: usize,
    },
    Criterion {
        index: usize,
    },
    RuleItem {
        item_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub id: String,
    pub from: String,
    pub to: String,
    pub from_target: RelationTarget,
    pub to_target: RelationTarget,
    /// Assigned by the tool, not accepted as a model assertion.
    pub from_record_sha256: String,
    pub to_record_sha256: String,
    pub kind: RelationKind,
    pub state: RelationState,
    pub scope: String,
    pub explanation: String,
    pub grounds: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispositionState {
    Requirement,
    NonRequirement,
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disposition {
    pub state: DispositionState,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedField {
    pub id: String,
    /// JSON Pointer within the current record or relation; empty means the whole object.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: String,
    pub message: String,
    pub correction: String,
    pub affected: Vec<ReviewedField>,
    pub sources: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub metadata: BTreeMap<String, Vec<(usize, usize)>>,
    pub text: BTreeMap<String, Vec<(usize, usize)>>,
    pub form_cells: BTreeMap<String, Vec<(usize, usize)>>,
    /// Candidate objects actually returned by inspect_analysis, bound to their
    /// content digests. Source reading cannot stand in for result review.
    pub candidate: BTreeMap<String, String>,
    pub views: BTreeMap<String, views::ViewIdentity>,
    pub view_failures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    pub records: BTreeMap<String, Record>,
    pub relations: BTreeMap<String, Relation>,
    pub dispositions: BTreeMap<String, Disposition>,
    pub coverage: Coverage,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub main_global_checks: BTreeMap<String, GlobalCheck>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub review_global_checks: BTreeMap<String, GlobalCheck>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub analysis_sha256: String,
    pub coverage: Coverage,
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contract_sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub global_checks: Vec<GlobalCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisResult {
    pub schema_version: u32,
    pub frozen_input_sha256: String,
    pub analysis: Analysis,
    pub review: Review,
    pub quality: String,
    pub source_views: BTreeMap<String, views::SourceView>,
}

/// Known source-side uncertainty, retained in the separate composition report.
/// These items never stand in for unresolved independent-review findings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceOpenItem {
    Document {
        index: usize,
        document: Value,
    },
    Source {
        source_id: String,
        reason: String,
    },
    Record {
        record: Record,
    },
    Relation {
        relation: Relation,
    },
    View {
        source_id: String,
        role: agent::Role,
        error: String,
    },
}

impl AnalysisResult {
    pub fn open_items(&self, input: &FrozenInput) -> Vec<SourceOpenItem> {
        let mut items = Vec::new();
        for (index, document) in input.documents.iter().enumerate() {
            if document["disposition"]
                .as_str()
                .is_some_and(|s| s != "ready")
            {
                items.push(SourceOpenItem::Document {
                    index,
                    document: document.clone(),
                });
            }
        }
        for (source_id, disposition) in &self.analysis.dispositions {
            if disposition.state == DispositionState::Unresolved {
                items.push(SourceOpenItem::Source {
                    source_id: source_id.clone(),
                    reason: disposition.reason.clone(),
                });
            }
        }
        for record in self.analysis.records.values() {
            let unresolved = match &record.data {
                RecordData::Unresolved { .. } => true,
                RecordData::Requirement {
                    strength,
                    compliance,
                    applicability,
                    ..
                } => {
                    *strength == Strength::Unknown
                        || compliance.iter().any(|c| c.policy == Compliance::Unknown)
                        || applicability.state == ApplicabilityState::Unknown
                }
                RecordData::Rule { applicability, .. }
                | RecordData::Template { applicability, .. } => {
                    applicability.state == ApplicabilityState::Unknown
                }
                _ => false,
            };
            if unresolved {
                items.push(SourceOpenItem::Record {
                    record: record.clone(),
                });
            }
        }
        for relation in self.analysis.relations.values() {
            if relation.state == RelationState::Unresolved {
                items.push(SourceOpenItem::Relation {
                    relation: relation.clone(),
                });
            }
        }
        for (role, coverage) in [
            (agent::Role::Main, &self.analysis.coverage),
            (agent::Role::Reviewer, &self.review.coverage),
        ] {
            for (source_id, error) in &coverage.view_failures {
                items.push(SourceOpenItem::View {
                    source_id: source_id.clone(),
                    role: role.clone(),
                    error: error.clone(),
                });
            }
        }
        items
    }

    pub fn expected_quality(&self, input: &FrozenInput) -> &'static str {
        if self.review.findings.is_empty() && self.open_items(input).is_empty() {
            "verified"
        } else {
            "needs_review"
        }
    }
}

pub fn digest<T: Serialize>(value: &T) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    serde_json_canonicalizer::to_vec(value)
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
