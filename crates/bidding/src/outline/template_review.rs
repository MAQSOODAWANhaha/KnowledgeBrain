//! Version-bound judgments about a fillable template, not filled bidder facts.
//! Semantic judgments are submitted by the reviewer; the host checks identity,
//! complete claim coverage, visible field anchors and current read receipts.
use super::{TargetRef, claim_review, tools::Draft};
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    TemplateReady,
    Insufficient,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Field {
    Slot {
        slot_id: String,
        label: String,
    },
    FormRange {
        form_id: String,
        start_row: usize,
        end_row: usize,
        start_column: usize,
        end_column: usize,
        label: String,
    },
    ManualTask {
        task_id: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimCoverage {
    pub claim_handle: String,
    pub verdict: Verdict,
    pub reason: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub requirement_id: String,
    pub version: String,
    pub claims: Vec<ClaimCoverage>,
}

pub fn version(input: &FrozenInput, draft: &Draft, id: &str) -> Result<String, String> {
    let record = draft
        .requirements
        .get(id)
        .ok_or("template requirement missing")?;
    let fulfillment = draft
        .fulfillments
        .iter()
        .find(|f| f.requirement_id == id)
        .ok_or("template fulfillment missing")?;
    let comparison = draft
        .claim_comparisons
        .get(id)
        .ok_or("template review needs claim comparison")?;
    super::canonical_sha256(&(
        super::evidence::input_digest(input)?,
        draft.read_epoch,
        &draft.discovery_revision,
        record,
        fulfillment,
        &draft.slots,
        &draft.bindings,
        comparison,
    ))
}

pub fn validate(input: &FrozenInput, draft: &Draft, review: &Review) -> Result<bool, String> {
    if review.version != version(input, draft, &review.requirement_id)? {
        return Err("template review version is stale".into());
    }
    let comparison = &draft.claim_comparisons[&review.requirement_id];
    let record = &draft.requirements[&review.requirement_id];
    let unit = claim_review::build(input, &review.requirement_id, record)?;
    let source_supported = claim_review::validate(&unit, comparison)?;
    let expected: BTreeSet<_> = comparison
        .declared_claims
        .iter()
        .map(|c| c.claim_handle.as_str())
        .collect();
    let actual: BTreeSet<_> = review
        .claims
        .iter()
        .map(|c| c.claim_handle.as_str())
        .collect();
    if expected.is_empty() || expected != actual || actual.len() != review.claims.len() {
        return Err("template coverage must judge every declared claim exactly once".into());
    }
    let fulfillment = draft
        .fulfillments
        .iter()
        .find(|f| f.requirement_id == review.requirement_id)
        .ok_or("template fulfillment missing")?;
    for claim in &review.claims {
        if claim.reason.trim().is_empty()
            || (claim.verdict == Verdict::TemplateReady && claim.fields.is_empty())
        {
            return Err(
                "template judgment needs a reason and ready claims need concrete fields".into(),
            );
        }
        let mut unique = BTreeSet::new();
        for field in &claim.fields {
            if !unique.insert(super::canonical_sha256(field)?) {
                return Err("duplicate template field in one claim".into());
            }
            match field {
                Field::Slot { slot_id, label } => {
                    if !fulfillment.target_refs.contains(&TargetRef::TextSlot { slot_id: slot_id.clone() }) {
                        return Err("template field is outside requirement targets".into());
                    }
                    let slot = draft.slots.iter().find(|s| s.slot_id == *slot_id).ok_or("template slot missing")?;
                    if label.trim().is_empty() || !(slot.text.contains(label) || slot.match_query.contains(label)) {
                        return Err("template field label is not anchored in visible slot content".into());
                    }
                    if !super::tools::check_read_slot_complete(draft, slot) {
                        return Err("template field needs a complete fresh slot read".into());
                    }
                }
                Field::FormRange { form_id, start_row, end_row, start_column, end_column, label } => {
                    if !fulfillment.target_refs.contains(&TargetRef::FormBinding { form_id: form_id.clone() }) {
                        return Err("template form is outside requirement targets".into());
                    }
                    let form = input.structured_forms.iter().find(|f| f["form_definition_revision_id"] == *form_id)
                        .ok_or("template form missing")?;
                    let cells = form["definition"]["cells"].as_array().ok_or("template form has no native cells")?;
                    if start_row > end_row || start_column > end_column || label.trim().is_empty() {
                        return Err("template form range is invalid".into());
                    }
                    let row_end = cells.iter().filter_map(|c| c["row"].as_u64()
                        .map(|r| r.saturating_add(c["row_span"].as_u64().unwrap_or(1).max(1))))
                        .max().ok_or("template form has no rows")?;
                    let column_end = cells.iter().filter_map(|c| c["column"].as_u64()
                        .map(|col| col.saturating_add(c.get("column_span").or_else(|| c.get("col_span")).and_then(|v| v.as_u64()).unwrap_or(1).max(1))))
                        .max().ok_or("template form has no columns")?;
                    if *end_row as u64 >= row_end || *end_column as u64 >= column_end {
                        return Err("template form range exceeds native grid bounds".into());
                    }
                    let selected: Vec<_> = cells.iter().filter(|c| {
                        c["row"].as_u64().is_some_and(|r| (*start_row..=*end_row).contains(&(r as usize))) &&
                        c["column"].as_u64().is_some_and(|col| (*start_column..=*end_column).contains(&(col as usize)))
                    }).collect();
                    if selected.is_empty() || !selected.iter().any(|c| c["text"].as_str().is_some_and(|t| t.contains(label))) {
                        return Err("template form range lacks its visible field label".into());
                    }
                    // A form ID alone never proves its labels were read.
                    let digest = super::evidence::input_digest(input)?;
                    for cell in selected {
                        if let Some(text) = cell["text"].as_str().filter(|s| !s.is_empty()) {
                            let evidence = super::evidence::EvidenceRef::GridCell {
                                input_digest: digest.clone(), table_id: form_id.clone(),
                                anchor_row: cell["row"].as_u64().ok_or("cell row missing")? as usize,
                                anchor_column: cell["column"].as_u64().ok_or("cell column missing")? as usize,
                                start_byte: 0, end_byte: text.len(),
                            };
                            super::evidence::validate_evidence(&[evidence], input, &draft.check_reads.evidence)?;
                        }
                    }
                }
                Field::ManualTask { task_id, label } => {
                    if label.trim().is_empty() || !fulfillment.target_refs.iter().any(|t| matches!(t,
                        TargetRef::ManualTask { task_id: id, description } if id == task_id && description.contains(label))) {
                        return Err("manual field is outside its visible requirement task".into());
                    }
                }
            }
        }
    }
    Ok(source_supported
        && review
            .claims
            .iter()
            .all(|c| c.verdict == Verdict::TemplateReady))
}

pub fn ready(input: &FrozenInput, draft: &Draft) -> Result<(), String> {
    for id in &draft.required_requirement_ids {
        let review = draft
            .template_reviews
            .get(id)
            .ok_or("template coverage review is incomplete")?;
        if !validate(input, draft, review)? {
            return Err("template field coverage remains insufficient or unresolved".into());
        }
    }
    Ok(())
}

/// Frozen review evidence travels with the artifact so direct storage can apply
/// the same gate. Requirements and template content remain on the artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub read_epoch: u64,
    pub discovery_revision: Option<String>,
    pub claim_comparisons: std::collections::BTreeMap<String, claim_review::Comparison>,
    pub template_reviews: std::collections::BTreeMap<String, Review>,
    pub check_reads: super::tools::CheckReads,
    pub required_pack_ids: BTreeSet<String>,
    pub source_scopes: BTreeMap<String, super::source_review::Scope>,
    pub source_dispositions: BTreeMap<String, super::source_review::Disposition>,
    pub reviewed_requirement_ids: BTreeSet<String>,
    pub reviewed_pack_ids: BTreeSet<String>,
}
impl Proof {
    pub fn from_draft(draft: &Draft) -> Self {
        Self {
            read_epoch: draft.read_epoch,
            discovery_revision: draft.discovery_revision.clone(),
            claim_comparisons: draft.claim_comparisons.clone(),
            template_reviews: draft.template_reviews.clone(),
            check_reads: draft.check_reads.clone(),
            required_pack_ids: draft.required_pack_ids.clone(),
            source_scopes: draft.source_scopes.clone(),
            source_dispositions: draft.source_dispositions.clone(),
            reviewed_requirement_ids: draft.reviewed_requirement_ids.clone(),
            reviewed_pack_ids: draft.reviewed_pack_ids.clone(),
        }
    }
    pub fn restore(&self, draft: &mut Draft) {
        draft.read_epoch = self.read_epoch;
        draft.discovery_revision = self.discovery_revision.clone();
        draft.claim_comparisons = self.claim_comparisons.clone();
        draft.template_reviews = self.template_reviews.clone();
        draft.check_reads = self.check_reads.clone();
        draft.required_pack_ids = self.required_pack_ids.clone();
        draft.source_scopes = self.source_scopes.clone();
        draft.source_dispositions = self.source_dispositions.clone();
        draft.reviewed_requirement_ids = self.reviewed_requirement_ids.clone();
        draft.reviewed_pack_ids = self.reviewed_pack_ids.clone();
    }
}
