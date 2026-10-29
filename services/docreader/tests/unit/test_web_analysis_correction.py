"""Structured browser patches retain frozen provenance and full-pack validation."""
from copy import deepcopy
from contextlib import asynccontextmanager
from types import SimpleNamespace

import pytest

from docreader.integrations import web_analysis
from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.web import WebError
from docreader.tender.evidence import EvidenceRepository
from docreader.tender.requirements import validate_item_correction
from test_analysis_extraction import pack, result


@pytest.fixture
def correction_runtime(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    baseline = result()
    baseline["items"][0]["conditions"] = ["原条件"]
    source = pack()
    baseline = validate_item_correction(source, baseline)
    source_ref = artifacts.put_json(source)
    extracted = artifacts.put_json({"schema_version": 2, "packs": [
        {"pack_id": "p1", "artifact_ref": artifacts.put_json(baseline)}]})
    planned = artifacts.put_json({"schema_version": 2, "packs": [{"pack_id": "p1", "artifact_ref": source_ref}]})
    run = {"run_id": "run", "source_version": 1, "domain_revision": 1, "status": "waiting-confirmation",
           "history": {"artifacts": []}, "artifacts": [
               {"stage": stage, "scope": "", "source_version": 1, "domain_revision": 1, "artifact_ref": reference}
               for stage, reference in (("extract", extracted), ("pack-plan", planned))]}
    async def owned(*_args, **_kwargs):
        return run
    submitted = []
    connection = object()
    @asynccontextmanager
    async def transaction():
        yield connection
    async def lock_project(conn, _payload):
        assert conn is connection
        return True
    async def correct(*args, connection=None):
        assert connection is not None
        submitted.append(args)
        return run | {"submitted_source_version": 1, "submitted_domain_revision": 2}
    monkeypatch.setattr(web_analysis, "_owned_run", owned)
    # Only native source hydration is synthetic; the production correction,
    # schema, provenance checks and content-addressed artifacts remain real.
    monkeypatch.setattr(EvidenceRepository, "load_pack", lambda self, reference: self.artifacts.read_json(reference))
    bridge = SimpleNamespace(artifacts=artifacts, lock_project=lock_project,
                             store=SimpleNamespace(correct=correct, transaction=transaction))
    body = {"project_id": "project", "run_id": "run", "source_version": 1, "expected_revision": 1,
            "base_items_digest": extracted["sha256"], "request_key": "structured-action",
            "reason": "对照原文纠正业务字段。", "changes": []}
    return bridge, body, baseline, submitted


async def test_structured_correction_edits_business_fields_and_retains_complete_frozen_pack(correction_runtime):
    bridge, body, baseline, submitted = correction_runtime
    change = {"pack_id": "p1", "item_id": "p1:i1", "text": "在规定范围内交付。", "kind": "scoring_rule",
              "domains": ["技术"], "subject": "投标人", "scope": "本标段", "conditions": None,
              "exceptions": ["原文另有规定时除外"], "numbers": [{"value": 30, "unit": "天", "quote": "30天"}],
              "scoring": {"method": "原文评分办法", "points": 2, "max_points": 5}}
    body["changes"] = [change]
    await web_analysis.correct_analysis(bridge, body)
    reference = submitted[0][4]
    corrected = bridge.artifacts.read_json(bridge.artifacts.read_json(reference)["packs"][0]["artifact_ref"])
    expected = deepcopy(baseline)
    expected["items"][0].update({key: value for key, value in change.items() if key not in {"pack_id", "item_id", "conditions"}})
    expected["items"][0].pop("conditions")
    assert corrected == expected
    await web_analysis.correct_analysis(bridge, body)
    assert submitted[1] == submitted[0]


@pytest.mark.parametrize("details", [
    {"kind": "consequence_rule", "consequence": {"effect": "否决投标", "trigger": "材料缺失"}},
    {"kind": "submission_requirement", "submission": {"fields": ["联系人"], "documents": ["证明材料"]}},
])
async def test_kind_specific_fields_are_supported_without_a_text_edit(correction_runtime, details):
    bridge, body, baseline, submitted = correction_runtime
    body["changes"] = [{"pack_id": "p1", "item_id": "p1:i1", **details}]
    await web_analysis.correct_analysis(bridge, body)
    reference = bridge.artifacts.read_json(submitted[0][4])["packs"][0]["artifact_ref"]
    item = bridge.artifacts.read_json(reference)["items"][0]
    assert item == {**baseline["items"][0], **details}


@pytest.mark.parametrize("patch", [
    {"evidence": [{"range_id": "forged", "quote": "伪造"}]}, {"source_version": 99}, {"relations": []},
    {"numbers": [{"value": 90, "unit": "天", "quote": "不存在的90天"}]},
    {"scoring": {"points": 2}}, {"kind": "other"}, {"conditions": []}, {"text": " "},
    {"scoring": {"evidence": []}}, {"numbers": [{"value": True, "unit": "天", "quote": "30天"}]},
    {"scope": "字" * 20001}, {},
])
async def test_structured_correction_rejects_forged_or_invalid_patches(correction_runtime, patch):
    bridge, body, _, submitted = correction_runtime
    body["changes"] = [{"pack_id": "p1", "item_id": "p1:i1", **patch}]
    with pytest.raises(WebError) as invalid:
        await web_analysis.correct_analysis(bridge, body)
    assert invalid.value.status == 400 and submitted == []
