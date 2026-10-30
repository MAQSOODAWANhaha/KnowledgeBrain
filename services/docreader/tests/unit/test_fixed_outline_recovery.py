"""Fixed-outline recovery rebuilds the same host-owned source context."""

from copy import deepcopy

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.evidence.references import EvidenceRepository, fragment

from test_authoring_pipeline import outline as empty_outline
from test_authoring_pipeline_boundaries import budget, ordinary_response, payload, snapshot
from docreader.tests.submission_responses import assemble_independent_candidates


async def test_fixed_outline_recovery_preserves_complete_writing_request(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    first, second = "须提交实施方案。", "实施方案须覆盖安装与验收。"
    fixed = snapshot(artifacts, [{"text": first + "\n" + second}])
    frozen = artifacts.read_json(fixed["frozen_ref"])
    source = EvidenceRepository(artifacts).source_index(frozen["documents"][0]["source_index_ref"]).ranges[0]
    continuation = fragment(source, len(first) + 1, source["end"])
    item_ref = artifacts.put_json({"items": [
        {"item_id": identity, "kind": "obligation", "text": text,
         "evidence": [{"range_id": source["range_id"], "quote": text}]}
        for identity, text in (("implementation", first), ("acceptance", second))]})
    relation = {"relation_id": "implementation-acceptance", "from_item_id": "implementation",
                "to_item_id": "acceptance", "relation": "supplements",
                "evidence": [{"range_id": continuation["range_id"], "quote": second}]}
    issue = {"issue_id": "acceptance-scope", "code": "scope_unclear", "detail": "验收边界待核。",
             "blocking": False, "range_id": continuation["range_id"]}
    fixed["verification_ref"] = artifacts.put_json({
        "item_refs": [{"item_id": identity, "artifact_ref": item_ref}
                      for identity in ("implementation", "acceptance")],
        "decisions_ref": artifacts.put_json([]), "accepted_decision_ids": [],
        "relations_ref": artifacts.put_json([relation]), "issues": [issue]})
    calls, writes = [], []

    async def execute(task, request, validate):
        value = payload(request)
        calls.append((task, value.get("operation")))
        if task == "outline":
            if value.get("operation") == "assemble":
                return validate(assemble_independent_candidates(value))
            return validate({"schema_version": 2, "chapters": [{"title": "实施方案", "targets": [{
                "kind": "narrative", "purpose": "按原要求编写实施方法。",
                "rule_ids": [rule["rule_id"] for rule in value["rules"]],
                "range_ids": [value["sources"][0]["range_id"]], "support_origins": ["tender"]}]}],
                "findings": []})
        if task == "authoring_write":
            writes.append(deepcopy(request))
            return validate({"blocks": []})
        return validate(ordinary_response(task, value))

    result = artifacts.read_json(await generate_document(artifacts, fixed, budget(), execute))
    tree = artifacts.read_json(result["outline_ref"])
    before = deepcopy(tree)
    first_write = payload(writes[0])
    assert first_write["relations"] == [relation]
    assert issue in first_write["analysis_issues"]
    assert second in [support["text"] for support in first_write["supports"]]
    calls.clear()
    recovered = artifacts.read_json(await generate_document(
        artifacts, fixed, budget(), execute, outline=tree))

    assert len(writes) == 2
    assert writes[1] == writes[0]
    assert all(task != "outline" for task, _ in calls)
    assert artifacts.read_json(recovered["outline_ref"]) == tree == before


@pytest.mark.parametrize("frozen_value", ["missing", [], None, "not an object"],
                         ids=["missing", "array", "null", "string"])
async def test_fixed_outline_with_unavailable_source_still_exports(tmp_path, frozen_value):
    artifacts = ArtifactStore(tmp_path)
    tree = empty_outline()
    frozen_ref = artifacts.put_json({"documents": []} if frozen_value == "missing" else frozen_value)
    if frozen_value == "missing":
        artifacts.path(frozen_ref).unlink()
    tree["authoring_input"]["frozen_ref"] = frozen_ref
    before = deepcopy(tree)
    calls = []

    async def execute(task, request, validate):
        calls.append(task)
        return validate({"findings": []})

    result = artifacts.read_json(await generate_document(
        artifacts, tree["authoring_input"], budget(), execute, outline=tree))

    assert tree == before and "outline" not in calls
    assert artifacts.read_bytes(result["docx_ref"]).startswith(b"PK")
    assert result["gaps"]
