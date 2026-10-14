"""Dependency identities follow source intervals and current applicability."""

from copy import deepcopy

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.evidence.downstream import ResponsePlanner, _range_requirements, plan_downstream
from docreader.tender.evidence.planner import StructuralPlanner
from docreader.tender.evidence.references import SourceIndex, fragment, selector
from test_structural_packing import source


def requirement(identity, evidence, cross_references=()):
    return {"requirement_id": identity, "text": "Supplier must provide the cited evidence.", "subject": "Supplier",
            "evidence": [{"range_id": item["range_id"], "quote": item["text"]} for item in evidence],
            "conditions": [], "exceptions": [], "numbers": [], "scoring": {}, "veto_risk": False,
            "cross_references": list(cross_references), "applicable": True,
            "applicability_basis": "An explicit instruction applies to this response."}


def extraction(requirements, body):
    dispositions = []
    for item in body:
        ids = [req["requirement_id"] for req in requirements
               if any(evidence["range_id"] == item["range_id"] for evidence in req["evidence"])]
        dispositions.append({"range_id": item["range_id"], "requirement_ids": ids,
                             "status": "requirements" if ids else "non_requirement",
                             "reason": "This source provides the declared obligations or their shared context."})
    return {"requirements": requirements, "dispositions": dispositions}


@pytest.mark.parametrize("body_form", ["native", "fragment"])
def test_explicit_support_alias_connects_the_complete_overlapping_body_obligation(body_form):
    native = source([{"text": "Supplier must provide insurance.", "section": "left"},
                     {"text": "Supplier must provide a certified licence and renewal evidence.", "section": "right"}])
    left, right = native.ranges
    body_right = right if body_form == "native" else fragment(right, 0, 45)
    excerpt = fragment(right, 9, 30)
    body = [left, body_right]
    original = {"body": body, "supporting": [excerpt]}
    requirements = [requirement("left", [left], [excerpt["range_id"]]), requirement("right", [body_right])]
    outputs = extraction(requirements, body)
    owned = {"left": {left["range_id"]}, "right": {body_right["range_id"]}}
    ranges = _range_requirements(native, original, outputs, owned)
    assert ranges[excerpt["range_id"]] == {"right"}
    planner = ResponsePlanner(native, RequestBudget(context_tokens=131072), original, requirements,
                              {}, [], {}, ranges)
    assert [{item["requirement_id"] for item in group} for group in planner.components] == [{"left", "right"}]


def test_adjacent_nonoverlapping_fragment_and_shared_support_do_not_join_dependencies():
    native = source([{"text": "Supplier must provide insurance.", "section": "left"},
                     {"text": "Supplier must provide a certified licence and renewal evidence.", "section": "right"}])
    left, right = native.ranges
    first, second = fragment(right, 0, 30), fragment(right, 30, right["end"])
    excerpt = fragment(right, 9, 30)
    body = [left, first, second]
    original = {"body": body, "supporting": [excerpt]}
    requirements = [requirement("left", [left], [excerpt["range_id"]]),
                    requirement("first", [first]), requirement("second", [second, excerpt])]
    owned = {"left": {left["range_id"]}, "first": {first["range_id"]}, "second": {second["range_id"]}}
    ranges = _range_requirements(native, original, extraction(requirements, body), owned)
    assert ranges[excerpt["range_id"]] == {"first"}
    planner = ResponsePlanner(native, RequestBudget(context_tokens=131072), original, requirements,
                              {}, [], {}, ranges)
    assert [{item["requirement_id"] for item in group} for group in planner.components] == [
        {"left", "first"}, {"second"}]


def test_shared_supporting_condition_without_explicit_reference_does_not_join_obligations():
    native = source([{"text": "Only the selected lot applies.", "section": "condition"},
                     {"text": "Supplier must provide insurance.", "section": "left"},
                     {"text": "Supplier must provide a certified licence.", "section": "right"}])
    condition, left, right = native.ranges
    excerpt = fragment(condition, 0, 12)
    original = {"body": [condition, left, right], "supporting": [excerpt]}
    requirements = [requirement("left", [left, excerpt]), requirement("right", [right, excerpt])]
    owned = {"left": {left["range_id"]}, "right": {right["range_id"]}}
    ranges = _range_requirements(native, original, extraction(requirements, original["body"]), owned)
    assert not ranges.get(excerpt["range_id"])
    planner = ResponsePlanner(native, RequestBudget(context_tokens=131072), original, requirements,
                              {}, [], {}, ranges)
    assert [{item["requirement_id"] for item in group} for group in planner.components] == [{"left"}, {"right"}]


def persisted_inputs(tmp_path, *, cross_document=False):
    """Real immutable source/output artifacts; no model or database dependency."""
    artifacts = ArtifactStore(tmp_path)
    budget = RequestBudget(context_tokens=131072)
    entries, outputs, sources = [], [], []
    for document_id in (["main", "other"] if cross_document else ["main"]):
        draft = source([{"text": "Supplier must provide insurance.", "section": "left"},
                        {"text": "Supplier must provide a certified licence.", "section": "right"}],
                       document_id=document_id)
        document = {**draft.document, "original_ref": artifacts.put_bytes(document_id.encode())}
        native = SourceIndex(document, draft.raw)
        sources.append(native)
        raw_ref = artifacts.put_json(native.raw)
        source_ref = artifacts.put_json(native.descriptor(raw_ref))
        pack = StructuralPlanner(native, budget).candidate(native.ranges, "complete_adjacent_sections")
        excerpt = fragment(native.ranges[-1], 9, 30)
        pack["supporting"].append(selector(excerpt))
        pack.update(raw_ref=raw_ref, source_index_ref=source_ref,
                    document_id=document_id, document_version="1", task="extract",
                    input_digest="1" * 64, budget_digest="2" * 64, plan_digest="3" * 64)
        entries.append({"pack_id": pack["pack_id"], "document_id": document_id,
                        "artifact_ref": artifacts.put_json(pack), "range_count": len(native.ranges)})
        requirements = [requirement(f"{document_id}-left", [native.ranges[0]]),
                        requirement(f"{document_id}-right", [native.ranges[1]])]
        outputs.append({"pack_id": pack["pack_id"], **extraction(requirements, native.ranges)})
    index = {"schema_version": 2, "task": "extract", "input_digest": "1" * 64, "budget_digest": "2" * 64,
             "plan_digest": "3" * 64, "frozen_ref": artifacts.put_json({"frozen": True}),
             "budget_spec_ref": artifacts.put_json(budget.spec()), "documents": [], "packs": entries,
             "range_count": 2 * len(sources), "native_range_count": 2 * len(sources), "issues": [],
             "scope_decisions": [], "stats": {"document_count": len(sources), "pack_count": len(entries),
             "native_range_count": 2 * len(sources), "probe_count": 0},
             "run_identity": {"run_id": "dependency-fixture", "source_version": 1, "domain_revision": 2}}
    return artifacts, budget, index, outputs, sources


def plan_outputs(artifacts, budget, index, outputs, *, task, decisions=None):
    requirements_ref = artifacts.put_json({"packs": [
        {"pack_id": item["pack_id"], "artifact_ref": artifacts.put_json(item)} for item in outputs]})
    verification_ref = artifacts.put_json({"applicability_decisions": decisions or {}}) if task == "plan" else None
    result = artifacts.read_json(plan_downstream(index, requirements_ref, artifacts, budget,
                                                task=task, verification_ref=verification_ref))
    return result, [artifacts.read_json(item["artifact_ref"]) for item in result["packs"]]


@pytest.mark.parametrize("reference_form", ["native", "excerpt"])
def test_source_reference_to_inactive_obligation_blocks_current_response_plan(tmp_path, reference_form):
    artifacts, budget, index, outputs, sources = persisted_inputs(tmp_path)
    left, right = outputs[0]["requirements"]
    target = sources[0].ranges[-1]
    if reference_form == "excerpt":
        target = fragment(target, 9, 30)
    left["cross_references"] = [target["range_id"]]
    decisions = {right["requirement_id"]: {"requirement_id": right["requirement_id"], "applicable": False,
                 "applicability_basis": "An explicit current amendment replaces the original obligation."}}
    result, packs = plan_outputs(artifacts, budget, index, outputs, task="plan", decisions=decisions)
    assert "inactive_requirement_dependency" in {item["code"] for item in result["issues"]}
    affected = next(pack for pack in packs if left["requirement_id"] in {
        ref["requirement_id"] for ref in pack["requirement_refs"]})
    assert any(item["code"] == "inactive_requirement_dependency" for item in affected["issues"])
    assert right["requirement_id"] not in {ref["requirement_id"] for pack in packs for ref in pack["requirement_refs"]}


@pytest.mark.parametrize("task", ["verify", "plan"])
@pytest.mark.parametrize("cross_document", [False, True])
def test_identity_in_both_requirement_and_source_namespaces_is_blocked(tmp_path, task, cross_document):
    artifacts, budget, index, outputs, sources = persisted_inputs(tmp_path, cross_document=cross_document)
    left, right = outputs[0]["requirements"]
    collision = sources[-1].ranges[-1]["range_id"]
    old_id = right["requirement_id"]
    right["requirement_id"] = collision
    for disposition in outputs[0]["dispositions"]:
        disposition["requirement_ids"] = [collision if value == old_id else value
                                          for value in disposition["requirement_ids"]]
    left["cross_references"] = [collision]
    original = deepcopy(outputs)
    result, packs = plan_outputs(artifacts, budget, index, outputs, task=task)
    assert "dependency_reference_unresolved" in {item["code"] for item in result["issues"]}
    affected = next(pack for pack in packs if left["requirement_id"] in {
        ref["requirement_id"] for ref in pack["requirement_refs"]})
    assert any(item["code"] == "dependency_reference_unresolved" for item in affected["issues"])
    assert outputs == original
