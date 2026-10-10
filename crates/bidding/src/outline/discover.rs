//! Ordered reading packs and atomic, revision-fenced discovery submissions.
use super::evidence::{EvidenceRef, input_digest, resolve_evidence, validate_evidence};
use crate::analysis::{FrozenInput, Source};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[path = "discover_planner.rs"]
mod planner;
#[path = "discover_projection.rs"]
mod projection;
pub use planner::plan_packs_with_budget;
pub(crate) use projection::compact_request;

pub type SessionFits<'a> = dyn Fn(&[Value]) -> Result<bool, String> + 'a;

pub const DEFAULT_PACK_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridFragment {
    pub anchor_row: usize,
    pub anchor_column: usize,
    pub row_span: usize,
    pub column_span: usize,
    pub header_role: String,
    pub start_byte: usize,
    pub end_byte: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackCarrier {
    Text {
        evidence: EvidenceRef,
    },
    Grid {
        table_id: String,
        row_count: usize,
        column_count: usize,
        cells: Vec<GridFragment>,
        header_context: Vec<GridFragment>,
    },
    Image {
        evidence: EvidenceRef,
        vision_required: bool,
    },
    /// Structural/empty units are still acknowledged by a negative submission.
    Structure {
        unit_id: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackAtom {
    pub id: String,
    pub document_id: String,
    pub source_unit_revision_id: String,
    pub section_id: String,
    pub heading_path: String,
    pub unit_ordinal: usize,
    pub fragment_ordinal: usize,
    pub previous_fragment_id: Option<String>,
    pub next_fragment_id: Option<String>,
    pub context_only: bool,
    pub carrier: PackCarrier,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParsePack {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) condition_support_options: Vec<ConditionSupport>,
    pub id: String,
    pub document_ids: Vec<String>,
    pub input_digest: String,
    pub order: usize,
    pub pack_revision: u64,
    pub claim_token: String,
    pub atoms: Vec<PackAtom>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackStatus {
    Pending,
    Running,
    Failed,
    Committed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldError {
    pub path: String,
    pub code: String,
    pub message: String,
    pub owner_atom_keys: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFeedback {
    pub pack_id: String,
    pub call_id: String,
    pub arguments_sha256: String,
    pub errors: Vec<FieldError>,
    pub total: usize,
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackRecord {
    pack: ParsePack,
    status: PackStatus,
    attempt: u64,
    feedback: Option<PackFeedback>,
    no_requirement_reason: Option<String>,
    inspected_atom_ids: Vec<String>,
    visual_receipts: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    pack_id: String,
    claim_hash_after: String,
    revision_after: u64,
    arguments_sha256: String,
    result: Result<(), PackFeedback>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReopenReceipt {
    arguments_sha256: String,
    pack: ParsePack,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenOutcome {
    pub pack: ParsePack,
    pub replayed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverWork {
    run_scope: String,
    wire_bindings: BTreeMap<String, WireBinding>,
    pub plan_sha256: String,
    input_digest: String,
    planning_error: Option<String>,
    packs: BTreeMap<String, PackRecord>,
    requirements: BTreeMap<String, RequirementRecord>,
    receipts: BTreeMap<String, Receipt>,
    reopen_receipts: BTreeMap<String, ReopenReceipt>,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBinding {
    run_scope: String,
    pack_id: String,
    revision: u64,
    generation: u64,
    claim_hash: String,
    kind: String,
    canonical: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionSupport {
    pub origin: ConditionSupportOrigin,
    pub review_version: String,
    pub evidence: Vec<EvidenceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConditionSupportOrigin {
    ReviewedRequirement { requirement_id: String },
    ConfirmedRelation { relation_id: String },
}

fn support_key(pack: &ParsePack, support: &ConditionSupport) -> Result<String, String> {
    Ok(format!(
        "cs_{}",
        &super::canonical_sha256(&(
            "condition-support-v1",
            &pack.input_digest,
            &pack.id,
            pack.pack_revision,
            &pack.claim_token,
            support
        ))?[..24]
    ))
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementRecord {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub condition_support: Vec<ConditionSupport>,
    pub obligation_strength: String,
    pub extraction_quality: String,
    pub description: String,
    pub evidence: Vec<EvidenceRef>,
    pub source_section_id: String,
    pub kind: String,
    /// Exact evidence identity is a candidate dedup group, never semantic merging.
    pub dedup_group: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackCounts {
    pub total: usize,
    pub pending: usize,
    pub running: usize,
    pub failed: usize,
    pub committed: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRequirement {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub condition_support: Vec<ConditionSupport>,
    pub obligation_strength: String,
    pub extraction_quality: String,
    pub description: String,
    pub evidence: Vec<EvidenceRef>,
    pub source_section_id: String,
    pub kind: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackSubmit {
    pub call_id: String,
    pub claim_token: String,
    pub pack_revision: u64,
    pub requirements: Vec<PackRequirement>,
    pub no_requirement_reason: Option<String>,
    pub inspected_atom_ids: Vec<String>,
}

const SUBMISSION_INSTRUCTION: &str = "Copy the current short claim_token and submission_operation_id exactly. They are opaque host-issued handles. A rejected submission is not committed. Read all owned atoms and extract actual response obligations. Only genuinely no obligations permits an empty requirements array, with a specific reason and every owned atom_key inspected; never invent a reason or inspection.";

impl DiscoverWork {
    pub fn plan_with_budget(input: &FrozenInput, fits: &SessionFits<'_>) -> Self {
        let result = plan_packs_with_budget(input, fits);
        let planning_error = result.as_ref().err().cloned();
        let packs = result.unwrap_or_default();
        let plan_sha256 = super::canonical_sha256(&packs).expect("serializable pack plan");
        Self {
            run_scope: uuid::Uuid::new_v4().to_string(),
            wire_bindings: BTreeMap::new(),
            plan_sha256,
            input_digest: input_digest(input).expect("serializable frozen input"),
            planning_error,
            packs: packs
                .into_iter()
                .map(|pack| {
                    (
                        pack.id.clone(),
                        PackRecord {
                            pack,
                            status: PackStatus::Pending,
                            attempt: 0,
                            feedback: None,
                            no_requirement_reason: None,
                            inspected_atom_ids: vec![],
                            visual_receipts: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
            requirements: BTreeMap::new(),
            receipts: BTreeMap::new(),
            reopen_receipts: BTreeMap::new(),
            revision: 0,
        }
    }
    #[cfg(test)]
    pub(crate) fn plan(input: &FrozenInput, max_tokens: usize) -> Self {
        Self::plan_with_budget(input, &|sessions| test_sessions_fit(sessions, max_tokens))
    }
    pub(crate) fn wire_scope_identity(&self) -> Value {
        json!({"run":self.run_scope,"revision":self.revision,"packs":self.packs.iter().map(|(id,record)| json!([id,record.pack.pack_revision,record.attempt,record.pack.claim_token])).collect::<Vec<_>>()})
    }
    pub(crate) fn pack_wire_identity(&self, id: &str) -> Result<Value, String> {
        let record = self.packs.get(id).ok_or("unknown pack")?;
        Ok(
            json!({"run":self.run_scope,"pack":id,"revision":record.pack.pack_revision,"generation":record.attempt,"claim":record.pack.claim_token}),
        )
    }
    pub(crate) fn confirm_pack_image(
        &mut self,
        input: &FrozenInput,
        pack_id: &str,
        image_id: &str,
        hash: &str,
    ) -> Result<(), String> {
        if input_digest(input)? != self.input_digest
            || hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("invalid visual receipt".into());
        }
        let record = self.packs.get_mut(pack_id).ok_or("unknown pack")?;
        if !record.pack.atoms.iter().any(|atom| matches!(&atom.carrier,PackCarrier::Image{evidence:EvidenceRef::ImageRegion{image_id:id,..},..} if id==image_id)) {return Err("image outside pack".into());}
        record.visual_receipts.insert(image_id.into(), hash.into());
        Ok(())
    }
    fn binding(&self, record: &PackRecord, kind: &str) -> Result<WireBinding, String> {
        let canonical = if kind == "claim" {
            record.pack.claim_token.clone()
        } else {
            let mut value = json!({});
            add_submission_identity(&mut value, &record.pack, record.feedback.as_ref())?;
            value["submission_operation_id"]
                .as_str()
                .ok_or("operation missing")?
                .to_string()
        };
        Ok(WireBinding {
            run_scope: self.run_scope.clone(),
            pack_id: record.pack.id.clone(),
            revision: record.pack.pack_revision,
            generation: record.attempt,
            claim_hash: record.pack.claim_token.clone(),
            kind: kind.into(),
            canonical,
        })
    }
    fn register_binding(&mut self, handle: String, binding: WireBinding) -> Result<(), String> {
        if self
            .wire_bindings
            .get(&handle)
            .is_some_and(|old| old != &binding)
        {
            return Err("opaque handle collision; no binding replaced".into());
        }
        self.wire_bindings.insert(handle, binding);
        Ok(())
    }
    fn ensure_wire_bindings(&mut self) -> Result<(), String> {
        if self.run_scope.is_empty() {
            return Err("run scope missing; a new protocol run is required".into());
        }
        let bindings = self
            .packs
            .values()
            .filter(|r| !r.pack.claim_token.is_empty())
            .flat_map(|r| [self.binding(r, "claim"), self.binding(r, "operation")])
            .collect::<Result<Vec<_>, _>>()?;
        let mut next = self.clone();
        for binding in bindings {
            let prefix = if binding.kind == "claim" { "cl" } else { "op" };
            let handle = format!("{prefix}_{}", &super::canonical_sha256(&binding)?[..16]);
            next.register_binding(handle, binding)?;
        }
        self.wire_bindings = next.wire_bindings;
        Ok(())
    }
    pub(crate) fn model_handles(&self, id: &str) -> Result<(String, String), String> {
        let record = self.packs.get(id).ok_or("unknown pack")?;
        let find = |kind| {
            let binding = self.binding(record, kind)?;
            self.wire_bindings
                .iter()
                .find(|(_, v)| **v == binding)
                .map(|(k, _)| k.clone())
                .ok_or_else(|| "current opaque binding missing".to_string())
        };
        Ok((find("claim")?, find("operation")?))
    }
    fn resolve_model_envelope(
        &self,
        id: &str,
        body: &mut Value,
        historical: bool,
    ) -> Result<ParsePack, String> {
        let record = self.packs.get(id).ok_or("unknown pack")?;
        let lookup = |field: &str, kind: &str| -> Result<&WireBinding, String> {
            let key = body[field]
                .as_str()
                .ok_or_else(|| format!("{field} required"))?;
            let binding=self.wire_bindings.get(key).ok_or_else(||format!("invalid or stale {field}: copy current host-issued short handle exactly; submission not committed"))?;
            if binding.run_scope != self.run_scope || binding.pack_id != id || binding.kind != kind
            {
                return Err(format!("cross-run or cross-pack {field} rejected"));
            }
            Ok(binding)
        };
        let claim = lookup("claim_token", "claim")?;
        let operation = lookup("call_id", "operation")?;
        if claim.claim_hash != operation.claim_hash
            || claim.revision != operation.revision
            || claim.generation != operation.generation
            || body["pack_revision"].as_u64() != Some(claim.revision)
        {
            return Err(
                "claim and operation must identify the same current pack revision and generation"
                    .into(),
            );
        }
        let replay = self
            .receipts
            .get(&operation.canonical)
            .is_some_and(|receipt| {
                historical
                    || (receipt.claim_hash_after == record.pack.claim_token
                        && receipt.revision_after == record.pack.pack_revision)
            });
        if !replay
            && (claim.claim_hash != record.pack.claim_token
                || claim.revision != record.pack.pack_revision
                || claim.generation != record.attempt
                || *operation != self.binding(record, "operation")?)
        {
            return Err("stale claim or operation; use the current reading session".into());
        }
        let mut scope = record.pack.clone();
        scope.claim_token = claim.claim_hash.clone();
        scope.pack_revision = claim.revision;
        let canonical_call = operation.canonical.clone();
        let canonical_claim = claim.canonical.clone();
        body["call_id"] = json!(canonical_call);
        body["claim_token"] = json!(canonical_claim);
        Ok(scope)
    }
    pub fn planning_error(&self) -> Option<&str> {
        self.planning_error.as_deref()
    }
    pub fn claim(&mut self, limit: usize) -> Vec<ParsePack> {
        let inflight = self
            .packs
            .values()
            .filter(|r| matches!(r.status, PackStatus::Running | PackStatus::Failed))
            .count();
        let mut pending: Vec<_> = self
            .packs
            .values()
            .filter(|r| r.status == PackStatus::Pending)
            .map(|r| (r.pack.order, r.pack.id.clone()))
            .collect();
        pending.sort();
        let claimed = pending
            .into_iter()
            .take(limit.saturating_sub(inflight))
            .map(|(_, id)| {
                let record = self.packs.get_mut(&id).expect("pending pack");
                claim_record(record, false);
                record.pack.clone()
            })
            .collect();
        if let Err(error) = self.ensure_wire_bindings() {
            self.planning_error = Some(error);
            return vec![];
        }
        claimed
    }
    pub fn release_highest_inflight(&mut self) -> bool {
        let running = self
            .packs
            .values()
            .filter(|r| r.status == PackStatus::Running)
            .count();
        let failed = self
            .packs
            .values()
            .filter(|r| r.status == PackStatus::Failed)
            .count();
        if running == 0 || running + failed <= 1 {
            return false;
        }
        let id = self
            .packs
            .values()
            .filter(|r| r.status == PackStatus::Running)
            .max_by_key(|r| r.pack.order)
            .map(|r| r.pack.id.clone())
            .expect("running pack");
        let r = self.packs.get_mut(&id).expect("running pack");
        r.status = PackStatus::Pending;
        r.pack.claim_token.clear();
        r.feedback = None;
        true
    }
    /// Host-only explicit reopening; callers must invalidate any organized draft
    /// on a non-replayed outcome before allowing another outline operation.
    pub(crate) fn reopen_committed(
        &mut self,
        input: &FrozenInput,
        pack_id: &str,
        expected_revision: u64,
        operation_id: &str,
    ) -> Result<ReopenOutcome, String> {
        if input_digest(input)? != self.input_digest {
            return Err("frozen input changed".into());
        }
        if operation_id.trim().is_empty() || self.receipts.contains_key(operation_id) {
            return Err("reopen operation identity is missing or reused".into());
        }
        let hash = super::canonical_sha256(&(pack_id, expected_revision))?;
        if let Some(receipt) = self.reopen_receipts.get(operation_id) {
            if receipt.arguments_sha256 != hash {
                return Err("reopen operation was reused with different arguments".into());
            }
            return Ok(ReopenOutcome {
                pack: receipt.pack.clone(),
                replayed: true,
            });
        }
        let record = self.packs.get(pack_id).ok_or("unknown reading pack")?;
        if record.status != PackStatus::Committed || record.pack.pack_revision != expected_revision
        {
            return Err("reopen requires the current committed revision".into());
        }
        let mut next = self.clone();
        let record = next.packs.get_mut(pack_id).expect("validated pack");
        claim_record(record, true);
        record.feedback = None;
        record.no_requirement_reason = None;
        record.visual_receipts.clear();
        record.inspected_atom_ids.clear();
        let pack = record.pack.clone();
        next.revision = next.revision.saturating_add(1);
        next.reopen_receipts.insert(
            operation_id.into(),
            ReopenReceipt {
                arguments_sha256: hash,
                pack: pack.clone(),
            },
        );
        next.ensure_wire_bindings()?;
        *self = next;
        Ok(ReopenOutcome {
            pack,
            replayed: false,
        })
    }
    /// Called only at the completed-response boundary after the reserved wire
    /// body and actual image bytes have been checked. Caching is never a receipt.
    pub(crate) fn confirm_visual_delivery(
        &mut self,
        input: &FrozenInput,
        image_id: &str,
        view_hash: &str,
    ) -> Result<(), String> {
        if input_digest(input)? != self.input_digest {
            return Err("frozen input changed".into());
        }
        if view_hash.len() != 64
            || !view_hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("invalid delivered image hash".into());
        }
        let known=self.packs.values().any(|record|record.pack.atoms.iter().any(|atom|matches!(&atom.carrier,PackCarrier::Image{evidence:EvidenceRef::ImageRegion{image_id:id,..},..} if id==image_id)));
        if !known {
            return Err("delivered image does not belong to a frozen reading pack".into());
        }
        for record in self
            .packs
            .values_mut()
            .filter(|record| matches!(record.status, PackStatus::Running | PackStatus::Failed))
        {
            if record.pack.atoms.iter().any(|atom|matches!(&atom.carrier,PackCarrier::Image{evidence:EvidenceRef::ImageRegion{image_id:id,..},..} if id==image_id)) {
                record.visual_receipts.insert(image_id.into(),view_hash.into());
            }
        }
        Ok(())
    }
    pub fn submit(
        &mut self,
        input: &FrozenInput,
        pack_id: &str,
        submit: PackSubmit,
    ) -> Result<(), PackFeedback> {
        self.commit_submission(input, pack_id, submit, false)
    }
    pub fn repair(
        &mut self,
        input: &FrozenInput,
        pack_id: &str,
        submit: PackSubmit,
    ) -> Result<(), PackFeedback> {
        self.commit_submission(input, pack_id, submit, true)
    }
    fn commit_submission(
        &mut self,
        input: &FrozenInput,
        pack_id: &str,
        submit: PackSubmit,
        repair: bool,
    ) -> Result<(), PackFeedback> {
        if input_digest(input).as_deref() != Ok(self.input_digest.as_str()) {
            return Err(feedback(
                pack_id,
                &submit,
                vec![field(
                    "/input_digest",
                    "digest_mismatch",
                    "frozen input changed",
                )],
            ));
        }
        let hash =
            super::canonical_sha256(&(pack_id, repair, &submit)).expect("serializable submission");
        if self.reopen_receipts.contains_key(&submit.call_id) {
            return Err(feedback(
                pack_id,
                &submit,
                vec![field(
                    "/call_id",
                    "operation_conflict",
                    "operation ID belongs to a reopen receipt",
                )],
            ));
        }
        if let Some(receipt) = self.receipts.get(&submit.call_id) {
            if receipt.pack_id == pack_id && receipt.arguments_sha256 == hash {
                return receipt.result.clone();
            }
            return Err(feedback(
                pack_id,
                &submit,
                vec![field(
                    "/call_id",
                    "operation_conflict",
                    "operation ID was already used with different arguments",
                )],
            ));
        }
        // Envelope, identity and status checks precede every mutation, including repair.
        let record = self.packs.get(pack_id).ok_or_else(|| {
            feedback(
                pack_id,
                &submit,
                vec![field(
                    "/pack_id",
                    "unknown_pack",
                    "reading pack is not in the frozen plan",
                )],
            )
        })?;
        let mut gate = Vec::new();
        if submit.call_id.trim().is_empty() {
            gate.push(field(
                "/call_id",
                "missing_operation",
                "call_id is required",
            ));
        }
        if submit.claim_token.is_empty() || submit.claim_token != record.pack.claim_token {
            gate.push(field(
                "/claim_token",
                "stale_claim",
                "claim token does not own this pack",
            ));
        }
        if submit.pack_revision != record.pack.pack_revision {
            gate.push(field(
                "/pack_revision",
                "stale_revision",
                "pack revision changed",
            ));
        }
        let expected = if repair {
            PackStatus::Failed
        } else {
            PackStatus::Running
        };
        if record.status != expected {
            gate.push(field(
                "/pack_id",
                "invalid_status",
                if repair {
                    "repair requires a failed reading pack"
                } else {
                    "submit requires a running reading pack"
                },
            ));
        }
        if input_digest(input).as_deref() != Ok(self.input_digest.as_str()) {
            gate.push(field(
                "/input_digest",
                "digest_mismatch",
                "frozen input changed",
            ));
        }
        if !gate.is_empty() {
            return Err(feedback(pack_id, &submit, gate));
        }
        let errors = self.validation_errors(input, &record.pack, &submit);
        // Validate the complete payload before changing either indexes or pack state.
        let mut next = self.clone();
        let record = next.packs.get_mut(pack_id).expect("validated pack");
        if repair {
            claim_record(record, true);
        }
        let result = if errors.is_empty() {
            let prefix = format!("{pack_id}:");
            next.requirements.retain(|id, _| !id.starts_with(&prefix));
            for (index, requirement) in submit.requirements.iter().enumerate() {
                let mut identity = requirement
                    .evidence
                    .iter()
                    .map(|r| super::canonical_sha256(r).expect("evidence"))
                    .collect::<Vec<_>>();
                identity.sort();
                identity.dedup();
                next.requirements.insert(
                    format!("{pack_id}:{index}"),
                    RequirementRecord {
                        obligation_strength: requirement.obligation_strength.clone(),
                        extraction_quality: requirement.extraction_quality.clone(),
                        description: requirement.description.clone(),
                        evidence: requirement.evidence.clone(),
                        condition_support: requirement.condition_support.clone(),
                        source_section_id: requirement.source_section_id.clone(),
                        kind: requirement.kind.clone(),
                        dedup_group: super::canonical_sha256(&identity).expect("identity"),
                    },
                );
            }
            record.status = PackStatus::Committed;
            record.feedback = None;
            record.no_requirement_reason = submit.no_requirement_reason.clone();
            record.inspected_atom_ids = submit.inspected_atom_ids.clone();
            next.revision = next.revision.saturating_add(1);
            Ok(())
        } else {
            let diagnostic = feedback(pack_id, &submit, errors);
            record.status = PackStatus::Failed;
            record.feedback = Some(diagnostic.clone());
            Err(diagnostic)
        };
        next.receipts.insert(
            submit.call_id.clone(),
            Receipt {
                pack_id: pack_id.into(),
                claim_hash_after: record.pack.claim_token.clone(),
                revision_after: record.pack.pack_revision,
                arguments_sha256: hash,
                result: result.clone(),
            },
        );
        if let Err(error) = next.ensure_wire_bindings() {
            return Err(feedback(
                pack_id,
                &submit,
                vec![field("/claim_token", "handle_collision", &error)],
            ));
        }
        *self = next;
        result
    }
    fn validation_errors(
        &self,
        input: &FrozenInput,
        pack: &ParsePack,
        submit: &PackSubmit,
    ) -> Vec<FieldError> {
        let receipts = &self.packs[&pack.id].visual_receipts;
        let scope = pack_scope(pack, true)
            .into_iter()
            .filter(|reference| match reference {
                EvidenceRef::ImageRegion { image_id, .. } => receipts.contains_key(image_id),
                _ => true,
            })
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        for atom in &pack.atoms {
            if let PackCarrier::Image {
                evidence: EvidenceRef::ImageRegion { image_id, .. },
                ..
            } = &atom.carrier
            {
                let blank = input
                    .source_units
                    .iter()
                    .find(|source| source.source_unit_revision_id == *image_id)
                    .is_some_and(|source| source.locator["blank_image"] == true);
                if !blank && !receipts.contains_key(image_id) {
                    errors.push(field("/pack_id","original_image_not_delivered","read_source_view must deliver the original image in a completed model request before this whole-pack scan can commit"));
                }
            }
        }
        for (index, req) in submit.requirements.iter().enumerate() {
            let path = format!("/requirements/{index}");
            for support in &req.condition_support {
                if !pack.condition_support_options.contains(support)
                    || resolve_evidence(input, &support.evidence).is_err()
                {
                    errors.push(field(
                        &format!("{path}/condition_support"),
                        "invalid_support",
                        "support must be a host-authorized current review option",
                    ));
                }
            }
            if req.description.trim().is_empty() {
                errors.push(field(
                    &format!("{path}/description"),
                    "required",
                    "description is required",
                ));
            }
            if ![
                "qualification",
                "material",
                "format",
                "scoring",
                "technical",
                "commercial",
                "unknown",
            ]
            .contains(&req.kind.as_str())
            {
                errors.push(field(
                    &format!("{path}/kind"),
                    "invalid_kind",
                    "use a supported requirement kind",
                ));
            }
            if !["mandatory", "optional", "informational", "unknown"]
                .contains(&req.obligation_strength.as_str())
            {
                errors.push(field(
                    &format!("{path}/obligation_strength"),
                    "invalid_strength",
                    "use a supported obligation strength",
                ));
            }
            if !["explicit", "inferred", "uncertain"].contains(&req.extraction_quality.as_str()) {
                errors.push(field(
                    &format!("{path}/extraction_quality"),
                    "invalid_quality",
                    "use a supported extraction quality",
                ));
            }
            let section_scope = pack
                .atoms
                .iter()
                .filter(|a| a.section_id == req.source_section_id)
                .flat_map(|a| atom_scope(a, &pack.input_digest, false))
                .collect::<Vec<_>>();
            if section_scope.is_empty()
                || !req
                    .evidence
                    .iter()
                    .any(|r| super::evidence::covered_by_union(r, &section_scope))
            {
                let mut error = field(
                    &format!("{path}/source_section_id"),
                    "outside_section",
                    "source section must own cited evidence; select an owner_atom_keys candidate only when it matches the intended obligation",
                );
                error.owner_atom_keys = pack
                    .atoms
                    .iter()
                    .filter(|atom| {
                        let scope = atom_scope(atom, &pack.input_digest, false);
                        req.evidence
                            .iter()
                            .any(|reference| super::evidence::covered_by_union(reference, &scope))
                    })
                    .map(|atom| atom_key(pack, atom))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap_or_default();
                errors.push(error);
            }
            if req.evidence.is_empty() {
                errors.push(field(
                    &format!("{path}/evidence"),
                    "required",
                    "a requirement needs frozen evidence",
                ));
            } else if let Err(error) = validate_evidence(&req.evidence, input, &scope) {
                errors.push(field(
                    &format!("{path}/evidence"),
                    error.split(':').next().unwrap_or("invalid_evidence"),
                    &error,
                ));
            }
        }
        if submit.requirements.is_empty() {
            if submit
                .no_requirement_reason
                .as_deref()
                .is_none_or(|r| r.trim().is_empty())
            {
                errors.push(field(
                    "/no_requirement_reason",
                    "required",
                    "empty requirements rejected: extract obligations from all owned text and tables; only a genuinely negative result permits a specific reviewable reason",
                ));
            }
            let expected: BTreeSet<_> = pack
                .atoms
                .iter()
                .filter(|a| !a.context_only)
                .map(|a| a.id.as_str())
                .collect();
            let inspected: BTreeSet<_> = submit
                .inspected_atom_ids
                .iter()
                .map(String::as_str)
                .collect();
            if inspected != expected || inspected.len() != submit.inspected_atom_ids.len() {
                errors.push(field(
                    "/inspected_atom_ids",
                    "incomplete_inspection",
                    "empty requirements rejected: every owned atom must actually be inspected and acknowledged exactly once; do not invent inspection to bypass this gate",
                ));
            }
        } else if submit.no_requirement_reason.is_some() {
            errors.push(field(
                "/no_requirement_reason",
                "contradictory_result",
                "positive results cannot include a no-requirement reason",
            ));
        }
        errors
    }
    /// Host-only structure. Never derive Check obligations from a lossy wire view.
    pub(super) fn canonical_session(&self, pack_id: &str) -> Result<Value, String> {
        let record = self.packs.get(pack_id).ok_or("unknown reading pack")?;
        let mut pack = serde_json::to_value(&record.pack).map_err(|e| e.to_string())?;
        for atom in pack["atoms"].as_array_mut().ok_or("pack atoms missing")? {
            if let Some(image_id) = atom["carrier"]["evidence"]["image_id"]
                .as_str()
                .map(str::to_string)
            {
                atom["visual_evidence_delivered"] =
                    json!(record.visual_receipts.contains_key(&image_id));
            }
        }
        Ok(
            json!({"duty":"discover","pack":pack,"status":record.status,"feedback":record.feedback,"no_requirement_reason":record.no_requirement_reason}),
        )
    }
    pub(crate) fn bind_condition_support(
        &mut self,
        pack_id: &str,
        options: Vec<ConditionSupport>,
    ) -> Result<(), String> {
        let record = self.packs.get_mut(pack_id).ok_or("unknown pack")?;
        if record.status != PackStatus::Running {
            return Err("condition support requires reopened running pack".into());
        }
        let owned = pack_scope(&record.pack, true);
        record.pack.condition_support_options = options
            .into_iter()
            .filter(|option| option.evidence.iter().any(|e| !owned.contains(e)))
            .collect();
        Ok(())
    }
    /// Build read output without granting any credit. The host must confirm
    /// exact response delivery in this worker's own completed request first.
    pub(crate) fn related_read(
        &self,
        input: &FrozenInput,
        pack_id: &str,
        refs: &[EvidenceRef],
    ) -> Result<(Value, Vec<ConditionSupport>), String> {
        use crate::analysis::source_manifest::{DocumentRelationKind, DocumentRelationStatus};
        input.validate_document_relations()?;
        if refs.is_empty() || input_digest(input)? != self.input_digest {
            return Err("related read needs current nonempty evidence".into());
        }
        let pack = &self.packs.get(pack_id).ok_or("unknown pack")?.pack;
        let excerpts = resolve_evidence(input, refs)?;
        let mut grouped = BTreeMap::<String, Vec<EvidenceRef>>::new();
        for reference in refs {
            let unit_id = match reference {
                EvidenceRef::Text { unit_id, .. } => unit_id.as_str(),
                EvidenceRef::GridCell { table_id, .. } => input
                    .structured_forms
                    .iter()
                    .find(|form| form["form_definition_revision_id"] == *table_id)
                    .and_then(|form| form["source_unit_revision_id"].as_str())
                    .ok_or("related table owner missing")?,
                EvidenceRef::ImageRegion { .. } => {
                    return Err("related image needs original pixel delivery".into());
                }
            };
            let source = input
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == unit_id)
                .ok_or("related source missing")?;
            let relation = input
                .document_relations
                .iter()
                .find(|relation| {
                    relation.kind == DocumentRelationKind::ExplicitReference
                        && relation.status == DocumentRelationStatus::Confirmed
                        && pack.atoms.iter().any(|atom| {
                            atom.document_id == relation.from.document_id
                                && relation.from.unit_id.as_ref().is_none_or(|key| {
                                    input.source_units.iter().any(|source| {
                                        source.source_unit_revision_id
                                            == atom.source_unit_revision_id
                                            && source.locator["unit_id"] == *key
                                    })
                                })
                        })
                        && relation.to.as_ref().is_some_and(|target| {
                            target.document_id == source.document_id
                                && target
                                    .unit_id
                                    .as_ref()
                                    .is_none_or(|key| source.locator["unit_id"] == *key)
                        })
                })
                .ok_or("source is not an explicitly confirmed dependency of this pack")?;
            grouped
                .entry(relation.id.clone())
                .or_default()
                .push(reference.clone());
        }
        let options = grouped
            .into_iter()
            .map(|(relation_id, evidence)| {
                Ok(ConditionSupport {
                    review_version: super::canonical_sha256(&(
                        &self.input_digest,
                        &self.run_scope,
                        pack_id,
                        pack.pack_revision,
                        &relation_id,
                        &evidence,
                    ))?,
                    origin: ConditionSupportOrigin::ConfirmedRelation { relation_id },
                    evidence,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let handles = options
            .iter()
            .map(|option| {
                Ok(json!({"support_key":support_key(pack, option)?,"origin":option.origin}))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((
            json!({"excerpts":excerpts,"condition_support_options":handles,"delivery_policy":"These options become usable only after this exact read output is delivered in a completed request for the same worker."}),
            options,
        ))
    }

    pub(crate) fn related_read_already_delivered(
        &self,
        pack_id: &str,
        options: &[ConditionSupport],
    ) -> bool {
        self.packs.get(pack_id).is_some_and(|record| {
            !options.is_empty()
                && options
                    .iter()
                    .all(|option| record.pack.condition_support_options.contains(option))
        })
    }

    pub(crate) fn confirm_related_read(
        &mut self,
        input: &FrozenInput,
        pack_id: &str,
        options: &[ConditionSupport],
    ) -> Result<(), String> {
        for option in options {
            let (_, current) = self.related_read(input, pack_id, &option.evidence)?;
            if !current.contains(option) {
                return Err("stale related read scope".into());
            }
        }
        let record = self.packs.get_mut(pack_id).ok_or("unknown pack")?;
        if record.status != PackStatus::Running {
            return Err("related reader is not running".into());
        }
        for option in options {
            if !record.pack.condition_support_options.contains(option) {
                record.pack.condition_support_options.push(option.clone());
            }
        }
        Ok(())
    }

    pub fn session(&self, input: &FrozenInput, pack_id: &str) -> Result<Value, String> {
        let record = self
            .packs
            .get(pack_id)
            .ok_or_else(|| format!("unknown reading pack {pack_id}"))?;
        if input_digest(input)? != self.input_digest {
            return Err("frozen input changed".into());
        }
        let mut pack = materialize_pack(input, &record.pack)?;
        for atom in pack["atoms"].as_array_mut().ok_or("pack atoms missing")? {
            if let Some(image_id) = atom["carrier"]["evidence"]["image_id"]
                .as_str()
                .map(str::to_string)
            {
                atom["visual_evidence_delivered"] =
                    json!(record.visual_receipts.contains_key(&image_id));
            }
        }
        let mut session = json!({"duty":"discover","pack":pack,"status":record.status,"feedback":record.feedback,"no_requirement_reason":record.no_requirement_reason});
        add_submission_identity(&mut session, &record.pack, record.feedback.as_ref())?;
        let (claim, operation) = self.model_handles(pack_id)?;
        session["pack"]["claim_token"] = json!(claim);
        session["submission_operation_id"] = json!(operation);
        session["submission_instruction"] = json!(SUBMISSION_INSTRUCTION);
        Ok(session)
    }
    /// Recheck the caller's complete request token policy before transport.
    pub fn session_fitting(
        &self,
        input: &FrozenInput,
        pack_id: &str,
        fits: &SessionFits<'_>,
    ) -> Result<Value, String> {
        let session = self.session(input, pack_id)?;
        if !fits(std::slice::from_ref(&session))? {
            return Err("pack_context_exceeded: reading session does not fit the configured context token budget".into());
        }
        Ok(session)
    }
    pub fn inflight_sessions(&self, input: &FrozenInput) -> Vec<Value> {
        if let Some(error) = &self.planning_error {
            return vec![json!({"error":error,"recoverable":true})];
        }
        let mut records: Vec<_> = self
            .packs
            .values()
            .filter(|r| matches!(r.status, PackStatus::Running | PackStatus::Failed))
            .collect();
        records.sort_by_key(|r| r.pack.order);
        records
            .into_iter()
            .map(|r| {
                self.session(input, &r.pack.id)
                    .unwrap_or_else(|e| json!({"pack_id":r.pack.id,"error":e}))
            })
            .collect()
    }
    pub fn pack_evidence(
        &self,
        input: &FrozenInput,
        pack_id: &str,
    ) -> Result<Vec<EvidenceRef>, String> {
        if input_digest(input)? != self.input_digest {
            return Err("frozen input changed".into());
        }
        let pack = &self.packs.get(pack_id).ok_or("unknown pack")?.pack;
        let refs = pack_scope(pack, false);
        resolve_evidence(input, &refs)?;
        Ok(refs)
    }
    pub fn status(&self, id: &str) -> Option<PackStatus> {
        self.packs.get(id).map(|r| r.status)
    }
    pub fn failed_pack_ids(&self) -> Vec<String> {
        let mut found: Vec<_> = self
            .packs
            .values()
            .filter(|r| r.status == PackStatus::Failed)
            .collect();
        found.sort_by_key(|r| r.pack.order);
        found.into_iter().map(|r| r.pack.id.clone()).collect()
    }
    pub fn pack_ids(&self) -> BTreeSet<String> {
        self.packs.keys().cloned().collect()
    }
    pub fn requirement_count(&self) -> usize {
        self.requirements.len()
    }
    pub fn requirement(&self, id: &str) -> Option<&RequirementRecord> {
        self.requirements.get(id)
    }
    pub fn requirement_ids(&self) -> BTreeSet<String> {
        self.requirements.keys().cloned().collect()
    }
    pub fn requirement_records(&self) -> &BTreeMap<String, RequirementRecord> {
        &self.requirements
    }
    pub fn pack_counts(&self) -> PackCounts {
        let mut c = PackCounts {
            total: self.packs.len(),
            pending: 0,
            running: 0,
            failed: 0,
            committed: 0,
        };
        for r in self.packs.values() {
            match r.status {
                PackStatus::Pending => c.pending += 1,
                PackStatus::Running => c.running += 1,
                PackStatus::Failed => c.failed += 1,
                PackStatus::Committed => c.committed += 1,
            }
        }
        c
    }
    pub fn turn_only_committed(&self, ids: &[String]) -> bool {
        !ids.is_empty()
            && ids
                .iter()
                .all(|id| self.status(id) == Some(PackStatus::Committed))
    }
    /// Only exact journaled submissions can replace their verbose transcript.
    /// Envelope/identity failures never create a receipt and do not qualify.
    pub(crate) fn retains_submission(&self, args: &Value) -> bool {
        let Some(pack_id) = args["pack_id"].as_str() else {
            return false;
        };
        let Some(repair) = args["repair"].as_bool() else {
            return false;
        };
        let mut body = args.clone();
        let Some(fields) = body.as_object_mut() else {
            return false;
        };
        fields.remove("pack_id");
        fields.remove("repair");
        let Some(record) = self.packs.get(pack_id) else {
            return false;
        };
        // A completed repair rotates the live claim. Historical eviction must
        // verify the exact journaled submission under its original scope, not
        // authorize its old keys against the new live pack.
        let Some(call_id) = body["call_id"].as_str() else {
            return false;
        };
        let canonical_call = self
            .wire_bindings
            .get(call_id)
            .map(|b| b.canonical.as_str())
            .unwrap_or(call_id);
        if !self.receipts.contains_key(canonical_call) {
            return false;
        }
        if body["call_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("op_"))
        {
            let Ok(scope) = self.resolve_model_envelope(pack_id, &mut body, true) else {
                return false;
            };
            let _ = scope;
        }
        let mut receipt_scope = record.pack.clone();
        let Some(claim) = body["claim_token"].as_str() else {
            return false;
        };
        let Some(revision) = body["pack_revision"].as_u64() else {
            return false;
        };
        receipt_scope.claim_token = claim.into();
        receipt_scope.pack_revision = revision;
        if resolve_submission_handles(&receipt_scope, &mut body).is_err() {
            return false;
        }
        let Ok(submit) = serde_json::from_value::<PackSubmit>(body) else {
            return false;
        };
        let Some(receipt) = self.receipts.get(&submit.call_id) else {
            return false;
        };
        let Ok(hash) = super::canonical_sha256(&(pack_id, repair, &submit)) else {
            return false;
        };
        receipt.pack_id == pack_id
            && receipt.arguments_sha256 == hash
            && self.packs.get(pack_id).is_some_and(|record| {
                record.status == PackStatus::Committed
                    || (record.status == PackStatus::Failed && record.feedback.is_some())
            })
    }
    pub fn complete(&self) -> bool {
        self.planning_error.is_none()
            && self
                .packs
                .values()
                .all(|r| r.status == PackStatus::Committed)
    }
}
fn add_submission_identity(
    session: &mut Value,
    pack: &ParsePack,
    feedback: Option<&PackFeedback>,
) -> Result<(), String> {
    session["submission_operation_id"] = json!(format!(
        "submit-{}",
        super::canonical_sha256(&(
            &pack.id,
            pack.pack_revision,
            &pack.claim_token,
            feedback.map(|feedback| &feedback.arguments_sha256),
        ))?
    ));
    session["operation_instruction"] = json!(
        "Use submission_operation_id as call_id for this submission. A transport retry reuses the exact operation and arguments. A new reopened or repaired attempt gets a new host-derived identity; never recycle an earlier operation ID."
    );
    Ok(())
}

fn claim_record(record: &mut PackRecord, repair: bool) {
    record.attempt += 1;
    if repair {
        record.pack.pack_revision += 1;
        record.pack.condition_support_options.retain(|option| {
            matches!(
                option.origin,
                ConditionSupportOrigin::ReviewedRequirement { .. }
            )
        });
    }
    record.pack.claim_token = claim_token(&record.pack, record.attempt, repair);
    record.status = PackStatus::Running;
}
fn claim_token(pack: &ParsePack, attempt: u64, repair: bool) -> String {
    super::canonical_sha256(&(
        &pack.input_digest,
        &pack.id,
        pack.pack_revision,
        attempt,
        repair,
        &pack.atoms,
    ))
    .expect("serializable claim")
}

pub fn claim_turn_with_budget(
    slot: &mut Option<DiscoverWork>,
    input: &FrozenInput,
    fits: &SessionFits<'_>,
    limit: usize,
) -> Vec<Value> {
    let work = slot.get_or_insert_with(|| DiscoverWork::plan_with_budget(input, fits));
    work.claim(limit);
    work.inflight_sessions(input)
}
#[cfg(test)]
pub(crate) fn claim_turn(
    slot: &mut Option<DiscoverWork>,
    input: &FrozenInput,
    max_tokens: usize,
    limit: usize,
) -> Vec<Value> {
    claim_turn_with_budget(
        slot,
        input,
        &|sessions| test_sessions_fit(sessions, max_tokens),
        limit,
    )
}
#[cfg(test)]
pub(crate) fn test_sessions_fit(sessions: &[Value], max_tokens: usize) -> Result<bool, String> {
    use crate::agent_runtime::chat::{TokenEncoding, TokenizerProfile};
    let profile = TokenizerProfile {
        model_id: "gpt-4o-2024-08-06".into(),
        encoding: TokenEncoding::O200kBase,
        calibration: None,
    };
    let text = serde_json::to_string(sessions).map_err(|e| e.to_string())?;
    Ok(profile.count_text_tokens(&text).map_err(|e| e.message)? <= max_tokens)
}
#[cfg(test)]
fn plan_packs(input: &FrozenInput, max_tokens: usize) -> Result<Vec<ParsePack>, String> {
    plan_packs_with_budget(input, &|sessions| test_sessions_fit(sessions, max_tokens))
}
/// Transport handles select immutable host-issued references. The normal
/// digest, claim, UTF-8, section, scope and visual-receipt checks still run.
fn atom_key(pack: &ParsePack, atom: &PackAtom) -> Result<String, String> {
    Ok(format!(
        "atom_{}",
        super::canonical_sha256(&(
            &pack.input_digest,
            &pack.id,
            pack.pack_revision,
            &pack.claim_token,
            &atom.id
        ))?
    ))
}
fn evidence_key(
    pack: &ParsePack,
    atom: &PackAtom,
    reference: &EvidenceRef,
) -> Result<String, String> {
    Ok(format!(
        "ev_{}",
        &super::canonical_sha256(&(
            "pack-evidence-v1",
            &pack.input_digest,
            &pack.id,
            pack.pack_revision,
            &pack.claim_token,
            &atom.id,
            reference
        ))?[..24]
    ))
}
fn resolve_submission_handles(pack: &ParsePack, body: &mut Value) -> Result<(), String> {
    let mut keys = BTreeMap::new();
    let mut atoms = BTreeMap::new();
    for atom in &pack.atoms {
        let handle = atom_key(pack, atom)?;
        if atoms.insert(handle, atom).is_some() {
            return Err("atom key collision".into());
        }
        for reference in atom_scope(atom, &pack.input_digest, true) {
            let key = evidence_key(pack, atom, &reference)?;
            if keys.get(&key).is_some_and(|old| old != &reference) {
                return Err("evidence key collision".into());
            }
            keys.insert(key, reference);
        }
    }

    let atom = |value: &Value| -> Result<&PackAtom, String> {
        if value.as_object().is_none_or(|fields| fields.len() != 1) {
            return Err("atom_key must be the only field".into());
        }
        let key = value["atom_key"].as_str().ok_or("atom_key required")?;
        atoms
            .get(key)
            .copied()
            .ok_or("invalid or stale current-pack atom_key".into())
    };
    if let Some(ids) = body["inspected_atom_ids"].as_array_mut() {
        for id in ids.iter_mut() {
            *id = json!(atom(id)?.id);
        }
        let unique = ids.iter().map(Value::to_string).collect::<BTreeSet<_>>();
        if unique.len() != ids.len() {
            return Err("inspection handles must select distinct atoms".into());
        }
    }
    for (requirement_index, requirement) in body["requirements"]
        .as_array_mut()
        .ok_or("requirements missing")?
        .iter_mut()
        .enumerate()
    {
        if let Some(supports) = requirement
            .get_mut("condition_support")
            .and_then(Value::as_array_mut)
        {
            for support in supports {
                if support.as_object().is_none_or(|object| object.len() != 1) {
                    return Err("condition_support requires only support_key".into());
                }
                let key = support["support_key"]
                    .as_str()
                    .ok_or("condition_support requires support_key")?;
                let option = pack
                    .condition_support_options
                    .iter()
                    .find(|option| support_key(pack, option).as_deref() == Ok(key))
                    .ok_or("invalid or stale condition support key")?;
                *support = json!(option);
            }
        }
        requirement["source_section_id"] = json!(
            atom(&requirement["source_section_id"])
                .map_err(|error| format!(
                    "/requirements/{requirement_index}/source_section_id: {error}"
                ))?
                .section_id
        );
        for (reference_index, reference) in requirement["evidence"]
            .as_array_mut()
            .ok_or("evidence missing")?
            .iter_mut()
            .enumerate()
        {
            if let Some(key) = reference.get("evidence_key") {
                let path = format!("requirements[{requirement_index}].evidence[{reference_index}]");
                if reference.as_object().is_none_or(|fields| fields.len() != 1) {
                    return Err(format!("{path}: evidence_key must be the only field"));
                }
                let selected = key
                    .as_str()
                    .and_then(|key| keys.get(key))
                    .ok_or_else(|| format!("{path}: invalid or stale current-pack evidence_key"))?;
                *reference = serde_json::to_value(selected).map_err(|e| e.to_string())?;
            } else {
                return Err("evidence_key required; raw references are not model inputs".into());
            }
        }
    }
    Ok(())
}

pub fn apply_pack_tool(
    slot: &mut Option<DiscoverWork>,
    input: &FrozenInput,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    if !["submit_pack_scan", "repair_pack_scan"].contains(&name) {
        return Err("unknown discovery tool".into());
    }
    let id = args["pack_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("pack_id is required")?;
    let mut body = args.clone();
    body.as_object_mut()
        .ok_or("submission must be an object")?
        .remove("pack_id");
    // Malformed envelopes never create or mutate work, even when the id exists.
    let work = slot.as_ref().ok_or("no reading plan has been claimed")?;
    if [
        "call_id",
        "claim_token",
        "pack_revision",
        "requirements",
        "no_requirement_reason",
        "inspected_atom_ids",
    ]
    .iter()
    .any(|field| body.get(*field).is_none())
    {
        return Err("incomplete submission envelope".into());
    }
    let record = work.packs.get(id).ok_or("unknown reading pack")?;
    if body["requirements"].as_array().is_some_and(Vec::is_empty) {
        let expected = record
            .pack
            .atoms
            .iter()
            .filter(|a| !a.context_only)
            .map(|a| atom_key(&record.pack, a))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let inspected = body["inspected_atom_ids"].as_array();
        let actual = inspected
            .into_iter()
            .flatten()
            .filter_map(|a| a["atom_key"].as_str().map(str::to_string))
            .collect::<BTreeSet<_>>();
        let reason = body["no_requirement_reason"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty());
        if !reason || actual != expected || inspected.is_none_or(|v| v.len() != actual.len()) {
            return Ok(json!({"ok":false,"state_changed":false,"feedback":{
                "pack_id":id,"code":"INVALID_NEGATIVE_SUBMISSION","reason_required":!reason,
                "inspection_complete":actual==expected && inspected.is_some_and(|v| v.len()==actual.len()),
                "owned_atom_count":expected.len(),"inspected_atom_count":actual.len(),
                "next_action":"Read the current owned text and tables and extract their actual response obligations with evidence. Empty requirements are allowed only after genuinely finding none, with a specific reason and every owned atom_key inspected. Do not invent a reason or inspection; copy both current host-issued short handles exactly."}}));
        }
    }
    let pack = work.resolve_model_envelope(id, &mut body, false)?;
    resolve_submission_handles(&pack, &mut body)?;
    let submit: PackSubmit =
        serde_json::from_value(body).map_err(|e| format!("invalid submission envelope: {e}"))?;
    let work = slot
        .as_mut()
        .ok_or("no reading plan has been claimed; submit cannot create one")?;
    if let Some(error) = work.planning_error() {
        return Err(error.into());
    }
    let result = if name == "repair_pack_scan" {
        work.repair(input, id, submit)
    } else {
        work.submit(input, id, submit)
    };
    match result {
        Ok(()) => Ok(json!({"ok":true,"pack_id":id,"status":"committed","revision":work.revision})),
        Err(feedback) => Ok(json!({"ok":false,"feedback":feedback})),
    }
}
fn field(path: &str, code: &str, message: &str) -> FieldError {
    FieldError {
        path: path.into(),
        code: code.into(),
        message: message.into(),
        owner_atom_keys: Vec::new(),
    }
}
fn feedback(id: &str, submit: &PackSubmit, errors: Vec<FieldError>) -> PackFeedback {
    PackFeedback {
        pack_id: id.into(),
        call_id: submit.call_id.clone(),
        arguments_sha256: super::canonical_sha256(submit).expect("submission"),
        total: errors.len(),
        errors,
        truncated: false,
    }
}

fn grid_reference(digest: &str, table: &str, cell: &GridFragment) -> EvidenceRef {
    EvidenceRef::GridCell {
        input_digest: digest.into(),
        table_id: table.into(),
        anchor_row: cell.anchor_row,
        anchor_column: cell.anchor_column,
        start_byte: cell.start_byte,
        end_byte: cell.end_byte,
    }
}
fn atom_scope(atom: &PackAtom, digest: &str, context: bool) -> Vec<EvidenceRef> {
    if atom.context_only && !context {
        return vec![];
    }
    match &atom.carrier {
        PackCarrier::Text { evidence } => vec![evidence.clone()],
        // This is required scope, not a delivery receipt. Submission and Check
        // independently intersect original images with confirmed wire delivery.
        PackCarrier::Image {
            evidence,
            vision_required,
        } => {
            if *vision_required {
                vec![evidence.clone()]
            } else {
                vec![]
            }
        }
        PackCarrier::Grid {
            table_id,
            cells,
            header_context,
            ..
        } => cells
            .iter()
            .chain(if context {
                header_context.as_slice()
            } else {
                &[]
            })
            .filter(|c| c.start_byte < c.end_byte)
            .map(|c| grid_reference(digest, table_id, c))
            .collect(),
        PackCarrier::Structure { .. } => vec![],
    }
}
fn pack_scope(pack: &ParsePack, context: bool) -> Vec<EvidenceRef> {
    pack.atoms
        .iter()
        .flat_map(|a| atom_scope(a, &pack.input_digest, context))
        .collect()
}
fn section_id(source: &Source) -> String {
    source.locator["section_id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| {
            if let Some(n) = source.locator["section_ordinal"].as_u64() {
                format!("{}:section:{n}", source.document_id)
            } else {
                format!(
                    "{}:heading:{}",
                    source.document_id,
                    source.locator["heading_path"].as_str().unwrap_or("")
                )
            }
        })
}

fn materialize_pack(input: &FrozenInput, pack: &ParsePack) -> Result<Value, String> {
    // Validate the immutable input and every reference in one batch. Resolving
    // each atom separately would canonicalize the entire tender once per atom.
    let references = pack
        .atoms
        .iter()
        .filter_map(|atom| match &atom.carrier {
            PackCarrier::Text { evidence } | PackCarrier::Image { evidence, .. } => {
                Some(evidence.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut excerpts = if references.is_empty() {
        Vec::new()
    } else {
        resolve_evidence(input, &references)?
    }
    .into_iter();
    let mut atoms = Vec::new();
    for atom in &pack.atoms {
        let mut rendered = serde_json::to_value(atom).map_err(|e| e.to_string())?;
        rendered["atom_key"] = json!(atom_key(pack, atom)?);
        let source = input
            .source_units
            .iter()
            .find(|source| {
                source.source_unit_revision_id == atom.source_unit_revision_id
                    && source.document_id == atom.document_id
            })
            .ok_or("pack atom source ownership mismatch")?;
        let mut native = source.locator.clone();
        if let Some(object) = native.as_object_mut() {
            object.remove("cells");
            object.remove("rendered_spans");
            if let Some(physical) = object
                .get_mut("physical_locator")
                .and_then(Value::as_object_mut)
            {
                physical.remove("cells");
            }
        }
        rendered["source_identity"] = json!({
            "document_id":source.document_id,
            "document_revision":source.locator["document_revision"],
            "parser_version":source.locator["parser_version"],
            "source_unit_revision_id":source.source_unit_revision_id,
            "native_locator":native
        });
        match &atom.carrier {
            PackCarrier::Text { .. } | PackCarrier::Image { .. } => {
                if matches!(&atom.carrier, PackCarrier::Image { .. }) {
                    rendered["visual_evidence_delivered"] = json!(false);
                }
                rendered["excerpts"] =
                    json!([excerpts.next().ok_or("missing validated atom excerpt")?]);
                rendered["excerpts"][0]["evidence_index"] = json!(0);
            }
            PackCarrier::Grid {
                table_id,
                cells,
                header_context,
                ..
            } => {
                projection::render_grid(
                    &mut rendered,
                    input,
                    table_id,
                    cells,
                    header_context,
                    &pack.input_digest,
                )?;
            }
            PackCarrier::Structure { .. } => {}
        }
        let references = atom_scope(atom, &pack.input_digest, true);
        for field in ["excerpts", "cells", "header_context"] {
            if let Some(items) = rendered[field].as_array_mut() {
                for item in items {
                    if let Some(index) = item["evidence_index"].as_u64()
                        && let Some(reference) = references.get(index as usize)
                    {
                        item["evidence_key"] = json!(evidence_key(pack, atom, reference)?);
                    }
                }
            }
        }
        atoms.push(rendered);
    }
    let supports = pack.condition_support_options.iter().map(|option| {
        let mut value = json!({"support_key":support_key(pack,option)?,"type":"external_condition_support","origin":option.origin,"review_version":option.review_version});
        match option.origin {
            ConditionSupportOrigin::ReviewedRequirement { .. } => value["excerpts"] = json!(resolve_evidence(input,&option.evidence)?),
            ConditionSupportOrigin::ConfirmedRelation { .. } => {
                value["delivered_evidence"] = json!(option.evidence);
                value["reading_status"] = json!("Original ranges were delivered to this worker; use read_evidence to reread as needed. These are not fresh Check receipts.");
            }
        }
        Ok(value)
    }).collect::<Result<Vec<_>,String>>()?;
    let read_dependencies = input.document_relations.iter()
        .filter(|relation| pack.document_ids.contains(&relation.from.document_id))
        .map(|relation| json!({"relation":relation,
            "target_in_pack":relation.to.as_ref().is_some_and(|target| pack.document_ids.contains(&target.document_id)),
            "available_originals": if relation.kind == crate::analysis::source_manifest::DocumentRelationKind::ExplicitReference
                && relation.status == crate::analysis::source_manifest::DocumentRelationStatus::Confirmed {
                input.source_units.iter().filter(|source| !source.text.is_empty() && relation.to.as_ref().is_some_and(|target|
                    source.document_id == target.document_id && target.unit_id.as_ref().is_none_or(|key| source.locator["unit_id"] == *key)))
                    .map(|source| json!({"document_id":source.document_id,"unit_id":source.source_unit_revision_id,
                        "evidence":EvidenceRef::Text { input_digest:pack.input_digest.clone(), unit_id:source.source_unit_revision_id.clone(), start_byte:0, end_byte:source.text.len() }}))
                    .collect::<Vec<_>>()
            } else { vec![] },
            "read_policy":"Read original target atoms in their own planned packs when not present here. Relation metadata grants no source-reading or semantic-review credit."}))
        .collect::<Vec<_>>();
    Ok(
        json!({"read_dependencies":read_dependencies,"condition_support_options":supports,"id":pack.id,"document_ids":pack.document_ids,"input_digest":pack.input_digest,"order":pack.order,"pack_revision":pack.pack_revision,"claim_token":pack.claim_token,"atoms":atoms}),
    )
}

#[cfg(test)]
pub(crate) fn fixture_key_submission(
    input: &FrozenInput,
    work: &DiscoverWork,
    mut args: Value,
) -> Result<Value, String> {
    let id = args["pack_id"].as_str().ok_or("pack missing")?;
    let pack = &work.packs.get(id).ok_or("pack missing")?.pack;
    work.session(input, id)?;
    for requirement in args["requirements"]
        .as_array_mut()
        .ok_or("requirements missing")?
    {
        let section = requirement["source_section_id"]
            .as_str()
            .ok_or("fixture canonical section missing")?;
        let atom = pack
            .atoms
            .iter()
            .find(|a| a.section_id == section)
            .ok_or("fixture source section outside pack")?;
        requirement["source_section_id"] = json!({"atom_key":atom_key(pack,atom)?});
        for value in requirement["evidence"]
            .as_array_mut()
            .ok_or("fixture evidence missing")?
        {
            let reference: EvidenceRef =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            let atom = pack
                .atoms
                .iter()
                .find(|a| atom_scope(a, &pack.input_digest, true).contains(&reference))
                .ok_or("fixture reference outside pack")?;
            *value = json!({"evidence_key":evidence_key(pack,atom,&reference)?});
        }
    }
    for value in args["inspected_atom_ids"]
        .as_array_mut()
        .ok_or("fixture atoms missing")?
    {
        let atom = pack
            .atoms
            .iter()
            .find(|a| value.as_str() == Some(a.id.as_str()))
            .ok_or("fixture atom outside pack")?;
        *value = json!({"atom_key":atom_key(pack,atom)?});
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_claim_and_operation_handles_are_scoped_persistent_and_transactional() {
        let input = input(
            vec![source("a", 0, "Submit a signed declaration.", "one")],
            vec![],
        );
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        let (claim, op) = work.model_handles(&pack.id).unwrap();
        assert_eq!(claim.len(), 19);
        assert_eq!(op.len(), 19);
        let mut slot = Some(serde_json::from_value::<DiscoverWork>(json!(work)).unwrap());
        assert_eq!(
            slot.as_ref().unwrap().model_handles(&pack.id).unwrap(),
            (claim.clone(), op.clone())
        );
        let reference = atom_scope(&pack.atoms[0], &pack.input_digest, true).remove(0);
        let args = json!({"pack_id":pack.id,"call_id":op,"claim_token":claim,"pack_revision":pack.pack_revision,
            "requirements":[{"description":"Submit a signed declaration.","kind":"material","obligation_strength":"mandatory","extraction_quality":"explicit",
            "source_section_id":{"atom_key":atom_key(&pack,&pack.atoms[0]).unwrap()},"evidence":[{"evidence_key":evidence_key(&pack,&pack.atoms[0],&reference).unwrap()}]}],
            "no_requirement_reason":null,"inspected_atom_ids":[]});
        let before = slot.clone();
        let mut empty = args.clone();
        empty["requirements"] = json!([]);
        let rejected = apply_pack_tool(&mut slot, &input, "submit_pack_scan", &empty).unwrap();
        assert_eq!(rejected["ok"], false);
        assert_eq!(rejected["feedback"]["code"], "INVALID_NEGATIVE_SUBMISSION");
        assert_eq!(slot, before);
        for field in ["claim_token", "call_id"] {
            let mut bad = args.clone();
            bad[field] = json!("unknown");
            assert!(apply_pack_tool(&mut slot, &input, "submit_pack_scan", &bad).is_err());
            assert_eq!(slot, before);
        }
        let mut other = DiscoverWork::plan(&input, 8192);
        other.claim(1);
        assert!(apply_pack_tool(&mut Some(other), &input, "submit_pack_scan", &args).is_err());
        let mut collision = slot.clone().unwrap();
        let binding = collision.wire_bindings[&claim].clone();
        let mut wrong = binding.clone();
        wrong.pack_id = "foreign".into();
        let snapshot = collision.clone();
        assert!(collision.register_binding(claim.clone(), wrong).is_err());
        assert_eq!(collision, snapshot);
        let result = apply_pack_tool(&mut slot, &input, "submit_pack_scan", &args).unwrap();
        assert_eq!(result["ok"], true);
        let committed = slot.clone();
        assert_eq!(
            apply_pack_tool(&mut slot, &input, "submit_pack_scan", &args).unwrap()["ok"],
            true
        );
        assert_eq!(slot, committed);
        let mut retained = args.clone();
        retained["repair"] = json!(false);
        assert!(slot.as_ref().unwrap().retains_submission(&retained));
        slot.as_mut()
            .unwrap()
            .reopen_committed(&input, &pack.id, pack.pack_revision, "reopen")
            .unwrap();
        let reopened = slot.clone();
        assert!(apply_pack_tool(&mut slot, &input, "submit_pack_scan", &args).is_err());
        assert_eq!(
            slot, reopened,
            "reopened claims reject even an exact old receipt on the live model wire"
        );
        let fresh = slot.as_ref().unwrap().model_handles(&pack.id).unwrap();
        assert_ne!(fresh.0, claim);
        assert_ne!(fresh.1, op);
        let mut stale = args.clone();
        stale["call_id"] = json!(fresh.1);
        let snapshot = slot.clone();
        assert!(apply_pack_tool(&mut slot, &input, "submit_pack_scan", &stale).is_err());
        assert_eq!(slot, snapshot);
        // Canonical claim integrity is unchanged; model handles never replace it.
        assert_eq!(binding.canonical, pack.claim_token);
    }

    include!("discover_projection_tests.rs");
    fn source(id: &str, ordinal: usize, text: &str, section: &str) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: "doc".into(),
            text: text.into(),
            locator: json!({"section_id":section,"heading_path":section,"completeness":"complete"}),
            ordinal,
        }
    }
    fn input(sources: Vec<Source>, forms: Vec<Value>) -> FrozenInput {
        FrozenInput {
            schema_version: crate::outline::frozen::FROZEN_SCHEMA_VERSION,
            project_id: "p".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: sources,
            structured_forms: forms,
            decisions: vec![],
        }
    }
    fn text_ref(input: &FrozenInput, id: &str, start: usize, end: usize) -> EvidenceRef {
        EvidenceRef::Text {
            input_digest: input_digest(input).unwrap(),
            unit_id: id.into(),
            start_byte: start,
            end_byte: end,
        }
    }
    fn req(
        input: &FrozenInput,
        id: &str,
        start: usize,
        end: usize,
        section: &str,
    ) -> PackRequirement {
        PackRequirement {
            condition_support: Vec::new(),
            obligation_strength: "mandatory".into(),
            extraction_quality: "explicit".into(),
            description: "要求".into(),
            evidence: vec![text_ref(input, id, start, end)],
            source_section_id: section.into(),
            kind: "material".into(),
        }
    }
    fn submission(pack: &ParsePack, call: &str, requirements: Vec<PackRequirement>) -> PackSubmit {
        let negative = requirements.is_empty();
        PackSubmit {
            call_id: call.into(),
            claim_token: pack.claim_token.clone(),
            pack_revision: pack.pack_revision,
            requirements,
            no_requirement_reason: negative.then(|| "已检查全部载体，仅为说明".into()),
            inspected_atom_ids: if negative {
                pack.atoms.iter().map(|a| a.id.clone()).collect()
            } else {
                vec![]
            },
        }
    }
    #[test]
    fn token_planner_measures_the_exact_first_running_claim() {
        let input = input(
            vec![source("a", 0, "甲😀", "一"), source("b", 1, "乙", "二")],
            vec![],
        );
        let admitted = std::cell::RefCell::new(Vec::new());
        let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
            let fits = test_sessions_fit(sessions, 8192)?;
            if fits {
                admitted.borrow_mut().extend_from_slice(sessions);
            }
            Ok(fits)
        });
        assert!(work.planning_error().is_none());
        let packs = work.claim(4);
        assert_eq!(packs.len(), 1);
        let actual = work.session(&input, &packs[0].id).unwrap();
        assert!(test_sessions_fit(std::slice::from_ref(&actual), 8192).unwrap());
        assert_eq!(actual["status"], "running");
        assert_eq!(actual["pack"]["claim_token"].as_str().unwrap().len(), 19);
    }
    #[test]
    fn adjacent_small_chapters_merge_but_keep_identity() {
        let input = input(
            vec![source("a", 0, "A", "一"), source("b", 1, "B", "二")],
            vec![],
        );
        let packs = plan_packs(&input, 8192).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(
            packs[0]
                .atoms
                .iter()
                .map(|a| a.section_id.as_str())
                .collect::<Vec<_>>(),
            vec!["一", "二"]
        );
    }
    #[test]
    fn mixed_prose_table_prose_order_survives_splitting() {
        let input = input(
            vec![
                source("a", 0, "AA", "一"),
                source("t", 1, "", "一"),
                source("b", 2, "BB", "一"),
            ],
            vec![
                json!({"source_unit_revision_id":"t","form_definition_revision_id":"table","definition":{"row_count":1,"column_count":1,"cells":[{"row":0,"column":0,"text":"TT"}]}}),
            ],
        );
        for separate in [false, true] {
            let packs = plan_packs_with_budget(&input, &|sessions| {
                Ok(test_sessions_fit(sessions, 8192)?
                    && (!separate
                        || sessions.iter().all(|session| {
                            session["pack"]["atoms"].as_array().unwrap().len() <= 1
                        })))
            })
            .unwrap();
            let ids = packs
                .iter()
                .flat_map(|p| p.atoms.iter())
                .map(|a| a.unit_ordinal)
                .collect::<Vec<_>>();
            assert_eq!(ids, vec![0, 1, 2]);
        }
    }
    #[test]
    fn oversized_grid_cell_splits_utf8_and_is_table_only_evidence() {
        let original = "甲😀乙".repeat(1000);
        let input = input(
            vec![source("t", 0, "", "一")],
            vec![
                json!({"source_unit_revision_id":"t","form_definition_revision_id":"table","definition":{"row_count":1,"column_count":1,"cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":original}]}}),
            ],
        );
        let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
            // The planner must budget the same instruction that the claimed session sends.
            for session in sessions {
                assert_eq!(session["submission_instruction"], SUBMISSION_INSTRUCTION);
            }
            test_sessions_fit(sessions, 2048)
        });
        let packs = work.claim(100);
        let mut covered = String::new();
        for (n, pack) in packs.iter().enumerate() {
            let scope = work.pack_evidence(&input, &pack.id).unwrap();
            let quotes = resolve_evidence(&input, &scope).unwrap();
            assert!(test_sessions_fit(&[work.session(&input, &pack.id).unwrap()], 2048).unwrap());
            for q in quotes {
                covered.push_str(&q.quote);
            }
            let requirement = PackRequirement {
                condition_support: Vec::new(),
                obligation_strength: "mandatory".into(),
                extraction_quality: "explicit".into(),
                description: "表内条件".into(),
                evidence: scope,
                source_section_id: "一".into(),
                kind: "technical".into(),
            };
            work.submit(
                &input,
                &pack.id,
                submission(pack, &format!("op{n}"), vec![requirement]),
            )
            .unwrap();
        }
        assert_eq!(covered, original);
        assert!(work.complete());
    }
    #[test]
    fn invalid_state_and_stale_claim_never_mutate() {
        let input = input(vec![source("a", 0, "甲😀乙", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let unclaimed = work.packs["pack-0"].pack.clone();
        let before = work.clone();
        assert!(
            work.submit(
                &input,
                "pack-0",
                submission(&unclaimed, "pending", vec![req(&input, "a", 0, 3, "一")])
            )
            .is_err()
        );
        assert_eq!(work, before);
        let pack = work.claim(1).remove(0);
        let mut stale = submission(&pack, "stale", vec![req(&input, "a", 0, 3, "一")]);
        stale.claim_token = "old".into();
        let before = work.clone();
        assert!(work.submit(&input, "pack-0", stale).is_err());
        assert_eq!(work, before);
    }
    #[test]
    fn business_validation_is_atomic_and_repair_has_new_revision() {
        let input = input(vec![source("a", 0, "甲😀乙", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        let bad = submission(
            &pack,
            "bad",
            vec![req(&input, "a", 0, 3, "一"), req(&input, "a", 3, 5, "一")],
        );
        assert!(work.submit(&input, &pack.id, bad.clone()).is_err());
        assert_eq!(work.requirement_count(), 0);
        assert_eq!(work.status(&pack.id), Some(PackStatus::Failed));
        let before = work.clone();
        assert!(work.submit(&input, &pack.id, bad).is_err());
        assert_eq!(work, before);
        let good = submission(&pack, "repair", vec![req(&input, "a", 3, 7, "一")]);
        work.repair(&input, &pack.id, good.clone()).unwrap();
        assert_eq!(work.requirement_count(), 1);
        assert_eq!(work.packs[&pack.id].pack.pack_revision, 2);
        let committed = work.clone();
        work.repair(&input, &pack.id, good.clone()).unwrap();
        assert_eq!(work, committed);
        let mut changed = good;
        changed.requirements.clear();
        assert!(work.repair(&input, &pack.id, changed).is_err());
        assert_eq!(work, committed);
        assert!(
            work.submit(
                &input,
                &pack.id,
                submission(&pack, "overwrite", vec![req(&input, "a", 0, 3, "一")])
            )
            .is_err()
        );
        assert_eq!(work, committed);
    }
    #[test]
    fn replacement_removes_all_old_requirement_indexes() {
        let input = input(vec![source("a", 0, "ABCDE", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        work.submit(
            &input,
            &pack.id,
            submission(
                &pack,
                "first",
                vec![
                    req(&input, "a", 0, 1, "一"),
                    req(&input, "a", 1, 2, "一"),
                    req(&input, "a", 2, 3, "一"),
                ],
            ),
        )
        .unwrap();
        let reopened = work
            .reopen_committed(&input, &pack.id, pack.pack_revision, "explicit-reopen")
            .unwrap();
        assert!(!reopened.replayed);
        assert!(
            work.reopen_committed(&input, &pack.id, pack.pack_revision, "explicit-reopen")
                .unwrap()
                .replayed
        );
        work.submit(
            &input,
            &pack.id,
            submission(
                &reopened.pack,
                "replacement",
                vec![req(&input, "a", 3, 4, "一")],
            ),
        )
        .unwrap();
        assert_eq!(work.requirement_count(), 1);
        assert!(work.requirement("pack-0:2").is_none());
    }
    #[test]
    fn negative_result_requires_reason_and_all_carriers() {
        let input = input(vec![source("empty", 0, "", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        let mut bad = submission(&pack, "bad", vec![]);
        bad.no_requirement_reason = None;
        bad.inspected_atom_ids.clear();
        assert!(work.submit(&input, &pack.id, bad).is_err());
        work.repair(&input, &pack.id, submission(&pack, "good", vec![]))
            .unwrap();
        assert!(work.complete());
    }
    #[test]
    fn malformed_envelope_and_wrong_digest_do_not_mutate_state() {
        let input = input(vec![source("a", 0, "A", "一")], vec![]);
        let mut slot = None;
        claim_turn(&mut slot, &input, 8192, 1);
        let before = slot.clone();
        assert!(
            apply_pack_tool(
                &mut slot,
                &input,
                "submit_pack_scan",
                &json!({"pack_id":"pack-0","call_id":"x","requirements":[]})
            )
            .is_err()
        );
        assert_eq!(slot, before);
        let pack = slot.as_ref().unwrap().packs["pack-0"].pack.clone();
        let mut other = input.clone();
        other.source_units[0].text = "B".into();
        assert!(
            slot.as_mut()
                .unwrap()
                .submit(&other, &pack.id, submission(&pack, "changed", vec![]))
                .is_err()
        );
        assert_eq!(slot, before);
    }
    #[test]
    fn bounded_session_bills_wire_metadata_and_reserve() {
        let input = input(vec![source("a", 0, "A", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        assert!(
            work.session_fitting(&input, &pack.id, &|_| Ok(false))
                .unwrap_err()
                .contains("pack_context_exceeded")
        );
        assert!(
            work.session_fitting(&input, &pack.id, &|sessions| test_sessions_fit(
                sessions, 8192
            ))
            .is_ok()
        );
    }
    #[test]
    fn all_adjacent_text_slices_in_same_pack_validate() {
        let input = input(vec![source("a", 0, "甲😀乙", "一")], vec![]);
        let digest = input_digest(&input).unwrap();
        let mut packs = plan_packs_with_budget(&input, &|sessions| {
            Ok(test_sessions_fit(sessions, 8192)?
                && sessions
                    .iter()
                    .flat_map(|session| session["pack"]["atoms"].as_array().unwrap())
                    .flat_map(|atom| atom["excerpts"].as_array().into_iter().flatten())
                    .map(|excerpt| excerpt["quote"].as_str().unwrap().chars().count())
                    .sum::<usize>()
                    <= 1)
        })
        .unwrap();
        let atoms = packs.iter().flat_map(|p| p.atoms.clone()).collect();
        packs[0].atoms = atoms;
        let scope = pack_scope(&packs[0], false);
        assert!(
            validate_evidence(
                &[EvidenceRef::Text {
                    input_digest: digest,
                    unit_id: "a".into(),
                    start_byte: 0,
                    end_byte: 10
                }],
                &input,
                &scope
            )
            .is_ok()
        );
    }
    #[test]
    fn requirements_inventory_preserves_identity_and_revision() {
        let input = input(vec![source("a", 0, "AB", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        work.submit(
            &input,
            &pack.id,
            submission(
                &pack,
                "op",
                vec![req(&input, "a", 0, 1, "一"), req(&input, "a", 1, 2, "一")],
            ),
        )
        .unwrap();
        assert_eq!(work.requirement_records().len(), 2);
        assert_eq!(work.revision, 1);
        let records = work.requirement_records().values().collect::<Vec<_>>();
        assert_ne!(records[0].evidence, records[1].evidence);
        assert_eq!(records[0].source_section_id, "一");
        assert_eq!(records[1].source_section_id, "一");
        // Bounded transport is covered by the shared runtime continuation tests,
        // not by a second discovery-specific byte pager.
    }
    #[test]
    fn claim_cap_keeps_failed_sessions_and_uses_source_order() {
        let input = input(
            (0..12)
                .map(|n| source(&format!("s{n}"), n, "X", &format!("section{n}")))
                .collect(),
            vec![],
        );
        let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(test_sessions_fit(sessions, 8192)?
                && sessions
                    .iter()
                    .all(|session| session["pack"]["atoms"].as_array().unwrap().len() <= 1))
        });
        let packs = work.claim(3);
        assert_eq!(
            packs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["pack-0", "pack-1", "pack-2"]
        );
        assert!(work.claim(3).is_empty());
        work.submit(&input, &packs[0].id, submission(&packs[0], "ok", vec![]))
            .unwrap();
        assert!(
            work.submit(
                &input,
                &packs[1].id,
                submission(&packs[1], "bad", vec![req(&input, "s1", 0, 2, "section1")])
            )
            .is_err()
        );
        assert_eq!(work.claim(3)[0].id, "pack-3");
        let sessions = work.inflight_sessions(&input);
        assert_eq!(
            sessions
                .iter()
                .map(|s| s["pack"]["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["pack-1", "pack-2", "pack-3"]
        );
        assert_eq!(sessions[0]["status"], "failed");
        assert!(!sessions[0]["feedback"].is_null());
    }
    #[test]
    fn release_invalidates_the_old_claim_without_reordering() {
        let input = input(
            vec![source("a", 0, "A", "一"), source("b", 1, "B", "二")],
            vec![],
        );
        let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(test_sessions_fit(sessions, 8192)?
                && sessions
                    .iter()
                    .all(|session| session["pack"]["atoms"].as_array().unwrap().len() <= 1))
        });
        let packs = work.claim(2);
        assert!(work.release_highest_inflight());
        let fresh = work.claim(2).remove(0);
        assert_eq!(fresh.id, packs[1].id);
        assert_ne!(fresh.claim_token, packs[1].claim_token);
        let before = work.clone();
        assert!(
            work.submit(&input, &fresh.id, submission(&packs[1], "stale", vec![]))
                .is_err()
        );
        assert_eq!(work, before);
    }
    #[test]
    fn repeated_headers_are_context_with_original_anchor_identity() {
        let input = input(
            vec![source("t", 0, "", "一")],
            vec![
                json!({"source_unit_revision_id":"t","form_definition_revision_id":"table","definition":{"row_count":3,"column_count":1,"cells":[{"row":0,"column":0,"text":"H","header_role":"column_header"},{"row":1,"column":0,"text":"B","header_role":"none"},{"row":2,"column":0,"text":"C","header_role":"none"}]}}),
            ],
        );
        let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(test_sessions_fit(sessions, 8192)?
                && sessions
                    .iter()
                    .flat_map(|session| session["pack"]["atoms"].as_array().unwrap())
                    .map(|atom| atom["cells"].as_array().map_or(0, Vec::len))
                    .sum::<usize>()
                    <= 1)
        });
        let packs = work.claim(10);
        assert_eq!(packs.len(), 3);
        let owned = pack_scope(&packs[1], false);
        let displayed = pack_scope(&packs[1], true);
        assert_eq!(owned.len(), 1);
        assert_eq!(displayed.len(), 2);
        assert_eq!(
            resolve_evidence(&input, &displayed)
                .unwrap()
                .iter()
                .map(|e| e.quote.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "H"]
        );
        let requirement = PackRequirement {
            condition_support: Vec::new(),
            obligation_strength: "mandatory".into(),
            extraction_quality: "explicit".into(),
            description: "Header-scoped row".into(),
            evidence: displayed,
            source_section_id: "一".into(),
            kind: "technical".into(),
        };
        work.submit(
            &input,
            &packs[1].id,
            submission(&packs[1], "row", vec![requirement]),
        )
        .unwrap();
    }
    #[test]
    fn empty_grid_cells_still_have_bounded_structural_transport() {
        let cells = (0..20)
            .map(|n| json!({"row":0,"column":n,"text":"","header_role":"none"}))
            .collect::<Vec<_>>();
        let input = input(
            vec![source("t", 0, "", "一")],
            vec![
                json!({"source_unit_revision_id":"t","form_definition_revision_id":"table","definition":{"row_count":1,"column_count":20,"cells":cells}}),
            ],
        );
        let packs = plan_packs(&input, 1024).unwrap();
        assert!(!packs.is_empty());
        for pack in &packs {
            assert!(planner::test_pack_fits(&input, pack, 1024));
        }
        assert_eq!(
            packs
                .iter()
                .flat_map(|p| p.atoms.iter())
                .map(|a| match &a.carrier {
                    PackCarrier::Grid { cells, .. } => cells.len(),
                    _ => 0,
                })
                .sum::<usize>(),
            20
        );
    }
    #[test]
    fn document_transport_preserves_frozen_order_not_lexical_ids() {
        let mut first = source("first", 0, "A", "one");
        first.document_id = "z-document".into();
        let mut second = source("second", 0, "B", "two");
        second.document_id = "a-document".into();
        let mut input = input(vec![second, first], vec![]);
        input.documents = vec![
            crate::analysis::FrozenDocument::fixture("z-document"),
            crate::analysis::FrozenDocument::fixture("a-document"),
        ];
        let packs = plan_packs(&input, 8192).unwrap();
        assert_eq!(packs[0].document_ids, vec!["z-document"]);
        assert_eq!(packs[1].document_ids, vec!["a-document"]);
    }
    #[test]
    fn oversized_requirement_is_preserved_for_runtime_continuation() {
        let input = input(vec![source("a", 0, "A", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        let mut long = req(&input, "a", 0, 1, "一");
        long.description = "x".repeat(20000);
        work.submit(
            &input,
            &pack.id,
            submission(&pack, "large", vec![long.clone()]),
        )
        .unwrap();
        assert_eq!(work.requirement_count(), 1);
        assert_eq!(work.status(&pack.id), Some(PackStatus::Committed));
        assert_eq!(
            work.requirement_records()
                .values()
                .next()
                .unwrap()
                .description,
            long.description
        );
    }
    #[test]
    fn unknown_valid_submission_does_not_create_an_unclaimed_plan() {
        let input = input(vec![source("a", 0, "A", "一")], vec![]);
        let mut slot = None;
        let args = json!({"pack_id":"unknown","call_id":"operation","claim_token":"forged","pack_revision":1,"requirements":[],"no_requirement_reason":"none","inspected_atom_ids":[]});
        assert!(apply_pack_tool(&mut slot, &input, "submit_pack_scan", &args).is_err());
        assert!(slot.is_none());
    }
    #[test]
    fn batched_materialization_matches_each_reference_and_rechecks_changed_input() {
        let mut image = source("image", 1, "", "chapter");
        image.locator["locator_kind"] = json!("image");
        image.locator["image_available"] = json!(true);
        let input = input(
            vec![
                source("before", 0, "甲😀", "chapter"),
                image,
                source("after", 2, "乙", "chapter"),
            ],
            vec![],
        );
        let mut work = DiscoverWork::plan(&input, 8192);
        let packs = work.claim(4);
        assert_eq!(packs.len(), 1);
        let pack = &packs[0];
        let materialized = materialize_pack(&input, pack).unwrap();
        assert_eq!(pack.atoms.len(), 3);
        for (atom, actual) in pack
            .atoms
            .iter()
            .zip(materialized["atoms"].as_array().unwrap())
        {
            let reference = match &atom.carrier {
                PackCarrier::Text { evidence } | PackCarrier::Image { evidence, .. } => evidence,
                _ => panic!("fixture must contain text and image atoms"),
            };
            let mut excerpts = actual["excerpts"].clone();
            assert_eq!(
                excerpts[0]
                    .as_object_mut()
                    .unwrap()
                    .remove("evidence_index"),
                Some(json!(0))
            );
            excerpts[0].as_object_mut().unwrap().remove("evidence_key");
            assert_eq!(
                excerpts,
                json!(resolve_evidence(&input, std::slice::from_ref(reference)).unwrap())
            );
            if matches!(reference, EvidenceRef::ImageRegion { .. }) {
                assert_eq!(actual["visual_evidence_delivered"], false);
            }
        }
        let mut changed = input;
        changed.source_units[0].text.push('!');
        assert!(
            materialize_pack(&changed, pack)
                .unwrap_err()
                .contains("digest_mismatch")
        );
    }
    #[test]
    fn image_markers_never_grant_visual_evidence_without_pixels() {
        let mut image = source("i", 0, "", "一");
        image.locator["locator_kind"] = json!("image");
        image.locator["image_available"] = json!(true);
        let input = input(vec![image], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        assert!(
            work.pack_evidence(&input, &pack.id)
                .unwrap()
                .iter()
                .any(|reference| matches!(reference, EvidenceRef::ImageRegion { .. }))
        );
        let error = work
            .submit(&input, &pack.id, submission(&pack, "metadata-only", vec![]))
            .unwrap_err();
        assert!(
            error
                .errors
                .iter()
                .any(|error| error.code == "original_image_not_delivered")
        );
        work.confirm_visual_delivery(&input, "i", &"a".repeat(64))
            .unwrap();
        work.repair(
            &input,
            &pack.id,
            submission(&pack, "pixels-confirmed", vec![]),
        )
        .unwrap();
        assert!(work.complete());
    }

    #[test]
    fn persisted_commit_receipt_replays_without_duplicate_requirements() {
        let input = input(vec![source("a", 0, "A", "一")], vec![]);
        let mut work = DiscoverWork::plan(&input, 8192);
        let pack = work.claim(1).remove(0);
        let operation = submission(
            &pack,
            "journal-operation",
            vec![req(&input, "a", 0, 1, "一")],
        );
        work.submit(&input, &pack.id, operation.clone()).unwrap();
        let mut recovered: DiscoverWork =
            serde_json::from_slice(&serde_json::to_vec(&work).unwrap()).unwrap();
        recovered.submit(&input, &pack.id, operation).unwrap();
        assert_eq!(recovered, work);
        assert_eq!(recovered.requirement_count(), 1);
    }
}
