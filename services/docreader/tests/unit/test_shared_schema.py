"""One strict field dictionary across sources, plans, model stages and receipts."""

import importlib.util
import json

import pytest
from pydantic import TypeAdapter, ValidationError

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.schema import (
    ApplicabilityDecision, ArtifactRef, CommandStatus, DispositionStatus,
    DocumentIdentity, Evidence, Extraction, ModelRequestStatus, OCRStatus,
    PackDescriptor, PackPlan, Plan, RangeSelector, Requirement, RequirementRef,
    RunIdentity, RunStatus, ScopeDecision, ScopeStatus, SourceRangeIdentity,
    SourceReceiptIdentity, SourceView, Task, Verification,
)


def artifact():
    return {"sha256": "a" * 64, "relative_path": "aa/" + "a" * 64,
            "size": 120, "media_type": "application/json"}


def selector():
    return {"range_id": "range-1", "source_range_id": "range-1", "start": 0, "end": 7}


def descriptor():
    return {"schema_version": 2, "pack_id": "pack-1", "task": "extract",
            "input_digest": "1" * 64, "budget_digest": "2" * 64, "plan_digest": "3" * 64,
            "document_id": "document-1", "document_version": "amendment-A",
            "raw_ref": artifact(), "source_index_ref": artifact(),
            "body": [selector()], "supporting": [], "pending_scope_context": [],
            "issues": [], "compacted_blank_ranges": [],
            "planning_details": {"planner_version": "native-sections-full-request-v1",
                                 "boundary_reason": "complete_adjacent_sections", "section_ids": ["section-1"],
                                 "budget": {"input_tokens": 100, "fits": True}, "budget_spec_ref": artifact()}}


def aggregate_plan():
    return {"schema_version": 2, "task": "extract",
            "input_digest": "1" * 64, "budget_digest": "2" * 64, "plan_digest": "3" * 64,
            "frozen_ref": artifact(), "budget_spec_ref": artifact(),
            "documents": [{"document_id": "document-1", "document_version": "amendment-A",
                           "role": "main", "ordinal": 0, "issues": [], "file_name": "fixture.docx",
                           "original_ref": artifact(), "raw_ref": artifact(), "source_index_ref": artifact()}],
            "packs": [{"pack_id": "pack-1", "document_id": "document-1", "artifact_ref": artifact(), "range_count": 1}],
            "range_count": 1, "native_range_count": 1, "issues": [], "scope_decisions": [],
            "stats": {"document_count": 1, "pack_count": 1, "native_range_count": 1, "probe_count": 1},
            "run_identity": {"run_id": "run-1", "source_version": 2, "domain_revision": 3}}


def test_sources_extraction_verification_and_plan_share_json_identities(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    identity = SourceRangeIdentity(
        document_id="document-1", document_version="amendment-A", unit_id="unit-1",
        range_id="range-1", source_range_id="range-1", section_id="section-1",
        heading_path="Delivery", start=0, end=7,
    )
    original_identity = identity.model_dump(mode="json")
    identity_ref = artifacts.put_json(original_identity)
    assert SourceRangeIdentity.model_validate(artifacts.read_json(identity_ref)) == identity
    requirement = Requirement(
        requirement_id="requirement-1", text="Deliver in 30 days.", subject="Supplier",
        evidence=[Evidence(range_id=identity.range_id, quote="30 days")], conditions=[], exceptions=[],
        numbers=[{"value": 30, "unit": "days", "quote": "30 days"}], scoring={},
        veto_risk=False, cross_references=[], applicable=True, applicability_basis="All suppliers.",
    )
    extracted = Extraction(requirements=[requirement], dispositions=[{
        "range_id": identity.range_id, "status": "requirements", "reason": "Explicit delivery duty.",
        "requirement_ids": [requirement.requirement_id],
    }])
    requirements_ref = artifacts.put_json(extracted.model_dump(mode="json"))
    restored = Extraction.model_validate(artifacts.read_json(requirements_ref))
    checked = Verification(reviewed_range_ids=[identity.range_id], findings=[])
    checked = Verification.model_validate_json(checked.model_dump_json())
    reference = RequirementRef(artifact_ref=ArtifactRef.model_validate(requirements_ref),
                               requirement_id=requirement.requirement_id)
    response_plan = Plan(items=[{
        "requirement_id": reference.requirement_id, "target_kind": "body", "target": "2.1 Delivery",
        "action": "Commit to delivering in 30 days.", "material_needs": ["Approved delivery schedule"],
        "constraint_digest": "b" * 64,
    }])
    response_plan = Plan.model_validate_json(response_plan.model_dump_json())
    assert restored.requirements[0].evidence[0].range_id == checked.reviewed_range_ids[0] == identity.range_id
    assert response_plan.items[0].requirement_id == reference.requirement_id == restored.requirements[0].requirement_id
    assert RequirementRef.model_validate_json(reference.model_dump_json()) == reference
    assert artifacts.read_json(identity_ref) == original_identity


def test_source_view_roundtrip_preserves_blank_row_identity_and_rejects_storage_aliases():
    payload = {"pack_id": "p", "document_id": "d", "document_version": "rev-A",
               "body": [{"range_id": "r", "text": "Field", "section_id": "s"}], "supporting": [],
               "sections": [{"section_id": "s", "heading_path": "Application form"}],
               "blank_table_templates": [{"table_id": "t", "column_count": 2,
                                          "rows": [{"row_start": 1, "row_end": 3, "range_ids": ["b1", "b2"]}]}]}
    parsed = SourceView.model_validate(payload)
    assert SourceView.model_validate_json(parsed.model_dump_json()) == parsed
    with pytest.raises(ValidationError):
        SourceView.model_validate({**payload, "version": "rev-A"})
    payload["body"][0]["physical_locator"] = {"page": 1}
    with pytest.raises(ValidationError):
        SourceView.model_validate(payload)
    payload["body"][0].pop("physical_locator")
    payload["blank_table_templates"][0]["rows"][0]["range_ids"] = ["b1", "b1"]
    with pytest.raises(ValidationError):
        SourceView.model_validate(payload)


@pytest.mark.parametrize("model,payload", [(PackDescriptor, descriptor()), (PackPlan, aggregate_plan())])
def test_both_plan_layers_round_trip_with_canonical_fields(model, payload):
    parsed = model.model_validate(payload)
    assert model.model_validate_json(parsed.model_dump_json()) == parsed
    encoded = parsed.model_dump_json()
    assert '"version"' not in encoded and '"ref"' not in encoded and '"plan"' not in encoded
    assert parsed.plan_digest == "3" * 64


@pytest.mark.parametrize("model,payload,alias", [
    (DocumentIdentity, {"document_id": "d", "document_version": "v1"}, "version"),
    (RunIdentity, {"run_id": "r", "source_version": 1, "domain_revision": 1}, "version"),
    (ArtifactRef, artifact(), "ref"),
    (SourceRangeIdentity, {"document_id": "d", "document_version": "v1", "unit_id": "u", "range_id": "r",
                           "source_range_id": "r", "start": 0, "end": 1}, "key"),
    (RangeSelector, selector(), "text"),
    (PackDescriptor, descriptor(), "plan"),
    (PackPlan, aggregate_plan(), "selectors"),
    (RequirementRef, {"artifact_ref": artifact(), "requirement_id": "q"}, "ref"),
])
def test_old_aliases_and_undeclared_content_are_rejected(model, payload, alias):
    with pytest.raises(ValidationError):
        model.model_validate({**payload, alias: "old value"})


def test_nested_aliases_cannot_hide_in_plan_documents_or_pack_indexes():
    plan = aggregate_plan()
    plan["documents"][0]["version"] = "old-document-version"
    with pytest.raises(ValidationError):
        PackPlan.model_validate(plan)
    plan = aggregate_plan()
    plan["packs"][0]["ref"] = plan["packs"][0].pop("artifact_ref")
    with pytest.raises(ValidationError):
        PackPlan.model_validate(plan)


@pytest.mark.parametrize("field,value", [("source_version", "2"), ("source_version", True),
                                          ("source_version", 0), ("domain_revision", "3"),
                                          ("domain_revision", False), ("domain_revision", 0)])
def test_run_version_numbers_are_strict_positive_integers(field, value):
    with pytest.raises(ValidationError):
        RunIdentity.model_validate({"run_id": "r", "source_version": 2, "domain_revision": 3, field: value})


def test_document_label_and_shared_source_receipt_revision_are_distinct():
    assert DocumentIdentity(document_id="d", document_version="2.1-revised").document_version == "2.1-revised"
    with pytest.raises(ValidationError):
        DocumentIdentity(document_id="d", document_version=2)
    source = SourceReceiptIdentity(run_id="r", source_version=2)
    assert source.domain_revision == 0
    for invalid in (False, 1, "0"):
        with pytest.raises(ValidationError):
            SourceReceiptIdentity(run_id="r", source_version=2, domain_revision=invalid)


@pytest.mark.parametrize("type_,valid,wrong_entity", [
    (RunStatus, "response-plan-ready", "completed"), (CommandStatus, "completed", "complete"),
    (OCRStatus, "complete", "succeeded"), (ScopeStatus, "bound", "requirements"),
    (DispositionStatus, "requirements", "bound"), (ModelRequestStatus, "uncertain", "unresolved"),
    (Task, "extract", "extraction"),
])
def test_entity_status_enums_do_not_accept_each_others_values(type_, valid, wrong_entity):
    validator = TypeAdapter(type_)
    assert validator.validate_json(json.dumps(valid), strict=True) == valid
    with pytest.raises(ValidationError):
        validator.validate_json(json.dumps(wrong_entity), strict=True)


def scope_decision():
    return {"range_id": "context-1", "decision": "bind", "evidence": [{"range_id": "range-1", "quote": "Only lot A"}],
            "rationale": "The named lot is explicit in this clause.", "target_range_ids": ["range-2"]}


def test_scope_and_applicability_preserve_their_distinct_meanings():
    scope = ScopeDecision.model_validate(scope_decision())
    assert ScopeDecision.model_validate_json(scope.model_dump_json()) == scope
    applicability = ApplicabilityDecision(
        requirement_id="requirement-1", applicable=False,
        applicability_basis="Explicit amendment replaces this specific source range.",
        evidence=[Evidence(range_id="replacement-1", quote="Replace 30 days with 45 days")],
        amendment_document_id="amendment", source_document_id="main", source_range_id="range-1",
    )
    restored = ApplicabilityDecision.model_validate_json(applicability.model_dump_json())
    assert restored.source_range_id == "range-1" and restored.requirement_id == "requirement-1"
    assert restored.applicable is False and restored.amendment_document_id == "amendment"
    with pytest.raises(ValidationError):
        ApplicabilityDecision.model_validate({**restored.model_dump(), "basis": "old alias"})


@pytest.mark.parametrize("change", [
    {"target_range_ids": []}, {"target_range_ids": ["r", "r"]},
    {"decision": "local_only"}, {"evidence": []}, {"rationale": "   "},
    {"action": "bind"}, {"reason": "old alias"},
])
def test_scope_decisions_require_evidence_and_the_exact_target_contract(change):
    with pytest.raises(ValidationError):
        ScopeDecision.model_validate({**scope_decision(), **change})


def test_artifact_address_and_character_intervals_are_checked():
    with pytest.raises(ValidationError):
        ArtifactRef.model_validate({**artifact(), "relative_path": "bb/" + "a" * 64})
    with pytest.raises(ValidationError):
        RangeSelector.model_validate({**selector(), "start": 8, "end": 7})
    assert RangeSelector.model_validate({**selector(), "start": 7, "end": 7}).end == 7


def test_old_schema_module_is_absent():
    assert importlib.util.find_spec("docreader.tender.requirements.schema") is None
