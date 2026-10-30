"""Actual v2 extraction envelopes preserve source text without a repeated global catalog."""

from functools import partial

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.evidence.freeze import plan_sources
from docreader.tender.evidence.planner import make_packs
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.checks.analysis import _Analysis
from docreader.tender.requirements import item_payload
from test_analysis_checks import IDENTITY, scenario
from test_structural_packing import assert_coverage, materialize, source


def test_v2_extraction_excludes_global_navigation_without_losing_source_coverage():
    index = source([{"text": "Source obligation. " * 25, "section": f"s{number}"} for number in range(6)])
    catalog = [{"document_id": "document", "document_version": "a" * 64, "role": "main", "sections": [
        {"section_id": f"s{number}", "heading_path": "Readable source heading " * 28}
        for number in range(262)]}]
    identity = {"source_version": 1, "domain_revision": 2}
    problems = [{"issue_id": "source-1", "code": "scope_pending", "detail": "Scope still needs review.", "blocking": True}]
    builder = partial(item_payload, identity=identity, issues=problems)
    # Include the static generation contract; the repeated catalog must still
    # exceed this small synthetic window while complete source packs fit.
    admission = RequestBudget(context_tokens=11000, output_reserve=0, prompts={"extract": "Read every source."})
    packs, _ = make_packs(index, admission, payload_builder=builder)
    assert_coverage(index, packs)
    for pack in packs:
        payload = builder(materialize(index, pack))
        measured = admission.measure("extract", payload)
        assert measured["fits"]
        assert measured == pack["planning_details"]["budget"]
        assert "catalog" not in payload and payload["issues"] == problems
        assert "source_version" not in payload and "domain_revision" not in payload
        assert not admission.measure("extract", {**payload, "catalog": catalog})["fits"]


@pytest.mark.asyncio
async def test_payload_content_not_revision_binds_plan_when_measured_size_matches(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    index = source([{"text": "The delivery obligation is explicit."}])
    original_ref = artifacts.put_bytes(b"native source", "text/plain")
    index = SourceIndex({**index.document, "original_ref": original_ref}, index.raw)
    raw_ref = artifacts.put_json(index.raw)
    source_ref = artifacts.put_json(index.descriptor(raw_ref))
    frozen = {"schema_version": 2, "documents": [{
        **index.document, "role": "main", "ordinal": 0, "file_name": "source.txt", "issues": [],
        "raw_ref": raw_ref, "source_index_ref": source_ref, "status": "complete", "native_range_count": 1,
    }], "issues": [], "native_range_count": 1,
        "stats": {"document_count": 1, "native_range_count": 1, "image_count": 0,
                  "ocr_request_count": 0, "duration_seconds": 0.0}}
    admission = RequestBudget(context_tokens=20000, output_reserve=0, prompts={"extract": "Read."})
    plans = []
    for revision in (1, 2):
        builder = partial(item_payload, identity={"source_version": 1, "domain_revision": revision})
        reference = await plan_sources(frozen, artifacts, budget=admission, payload_builder=builder)
        plans.append(artifacts.read_json(reference))
    assert plans[0]["frozen_ref"] == plans[1]["frozen_ref"]
    assert plans[0]["input_digest"] == plans[1]["input_digest"]
    assert plans[0]["plan_digest"] == plans[1]["plan_digest"]
    first = artifacts.read_json(plans[0]["packs"][0]["artifact_ref"])
    second = artifacts.read_json(plans[1]["packs"][0]["artifact_ref"])
    assert first["body"] == second["body"]
    assert first["planning_details"]["budget"] == second["planning_details"]["budget"]


@pytest.mark.asyncio
async def test_token_accurate_review_keeps_request_granularity_without_losing_targets(tmp_path, monkeypatch):
    data = await scenario(tmp_path)
    prompts = {"extract": "Analyze source data.", "verify": "Review source data independently."}
    plain = RequestBudget(context_tokens=131072, prompts=prompts)
    tokenized = RequestBudget(context_tokens=131072, prompts=prompts, tokenizer="o200k_base")

    def reviews(admission):
        return _Analysis(data[1], data[2], data[0], IDENTITY, admission).review_requests()

    def targets(requests):
        return {d["range_id"] for payload in requests for d in payload["dispositions"]}

    whole, plain_before = reviews(tokenized), reviews(plain)
    monkeypatch.setattr(_Analysis, "REVIEW_INPUT_TOKENS", 1)
    split = reviews(tokenized)
    assert len(split) > len(whole)
    assert all(len(payload["dispositions"]) == 1 for payload in split)
    assert targets(split) == targets(whole)
    assert reviews(plain) == plain_before


@pytest.mark.parametrize("wording", ["本表适用于本地字段。", "本章所有条款仅适用于第一标段。"])
def test_analysis_scope_words_do_not_create_host_semantic_binding_or_blockers(wording):
    index = source([{"text": wording, "heading": "技术要求"},
                    {"text": "应提交技术字段证明。" * 400, "heading": "技术要求"}])
    admission = RequestBudget(context_tokens=7000, output_reserve=0, prompts={"extract": "Read source data."})
    legacy, _ = make_packs(index, admission)
    assert any(pack["scope_bindings"] or any(problem["code"] == "condition_scope_unresolved"
                                            for problem in pack["issues"]) for pack in legacy)
    packs, _ = make_packs(index, admission, analysis=True)
    assert_coverage(index, packs)
    assert all(not pack["scope_bindings"] for pack in packs)
    assert not any(problem["code"] == "condition_scope_unresolved" for pack in packs for problem in pack["issues"])
    assert any(wording in entry["text"] for pack in packs for entry in materialize(index, pack)["body"])
    assert any(wording in entry["text"] for pack in packs for entry in materialize(index, pack)["supporting"])


def test_explicit_reviewed_scope_binding_survives_analysis_repartition():
    from docreader.tender.evidence.context import condition_context

    index = source([{"text": "本表适用于本地字段。", "heading": "技术要求"},
                    {"text": "应提交技术字段证明。" * 400, "heading": "技术要求"}])
    _, conditions, _, _ = condition_context(index.ranges)
    candidate = next(value for candidates in conditions.values() for _, value in candidates)
    target = index.ranges[1]["range_id"]
    decision = {"range_id": candidate["range_id"], "decision": "bind", "target_range_ids": [target]}
    admission = RequestBudget(context_tokens=7000, output_reserve=0, prompts={"extract": "Read source data."})
    packs, _ = make_packs(index, admission, [decision], analysis=True)
    assert_coverage(index, packs)
    for pack in packs:
        targets = {entry["range_id"] for entry in pack["body"] if entry["source_range_id"] == target}
        if targets:
            binding = next(binding for binding in pack["scope_bindings"] if binding["range_id"] == candidate["range_id"])
            assert binding["scope_binding"]["status"] == "bound"
            assert set(binding["scope_binding"]["applies_to_range_ids"]) == targets
