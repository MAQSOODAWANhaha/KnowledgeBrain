"""Structure can revise an automatic outline without pretending prose exists."""

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelContractError, build_request
from docreader.tender import authoring_structure
from docreader.tender.authoring_excerpts import whole_source_excerpts
from test_submission_review import budget, outline, source, submission_context, target, unit
from test_structure_compact_view import PROMPT, payload as large_payload


async def test_outline_phase_has_distinct_target_aliases_no_fake_units_and_revisable_findings(tmp_path, monkeypatch):
    submission_context(monkeypatch)
    current = outline([target("native-template")])
    original = deepcopy(current)

    async def execute(stage, request, validate):
        data = json.loads(request["messages"][1]["content"][0]["text"])
        assert stage == "authoring_structure" and data["phase"] == "outline"
        assert data["units"] == []
        selected = data["chapters"][0]["targets"][0]
        assert selected["target_id"] == "v1" and selected["task_ids"] == []
        return validate({"findings": [{"code": "source_index_as_form", "detail": "v1仅供判断组成。",
            "suggestion": "依据已有组成说明移出v1，仍保留实际投标函。", "can_revise": True}]})

    findings, gaps = await authoring_structure.review_structure(ArtifactStore(tmp_path), current,
        [unit()], budget(), execute, outline_phase=True)
    assert findings[0]["can_revise"] is True
    assert findings[0]["task_ids"] == findings[0]["content_ids"] == []
    assert not gaps and current == original


@pytest.mark.parametrize("references", [{"task_ids": ["w1"]}, {"content_ids": ["b1"]}])
async def test_outline_phase_rejects_task_or_content_id_fabrication(tmp_path, monkeypatch, references):
    submission_context(monkeypatch)

    async def execute(stage, request, validate):
        return validate({"findings": [{"code": "wrong_id", "detail": "无正文可供定位", **references}]})

    with pytest.raises(ModelContractError):
        await authoring_structure.review_structure(ArtifactStore(tmp_path), outline([target()]),
            [unit()], budget(), execute, outline_phase=True)


async def test_structure_projects_exact_response_rows_once_without_persistent_source_metadata(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    original = source(artifacts, text="参数一：完整要求与单位；续文及适用条件。")
    excerpts = whole_source_excerpts(artifacts, original)
    selected = target("technical-response")
    selected.update(kind="narrative", manual_fill=False, source_refs=[original], source_excerpts=excerpts,
                    response_table={"rows": [{"row_id": "persistent-row", "source_excerpts": excerpts, "rule_refs": []}]})
    submission_context(monkeypatch)

    async def execute(stage, request, validate):
        data = json.loads(request["messages"][1]["content"][0]["text"])
        assert len(data["supports"]) == 1
        item = data["chapters"][0]["targets"][0]
        row = item["response_rows"][0]
        assert row["row_id"] == "q1"
        assert row["source_support_ids"] == item["source_support_ids"] == [data["supports"][0]["support_id"]]
        assert data["supports"][0]["text"] == "参数一：完整要求与单位；续文及适用条件。"
        for excluded in ("persistent-row", "source_index_ref", "selectors", "native-template"):
            assert excluded not in request["messages"][1]["content"][0]["text"]
        return validate({"findings": []})

    assert await authoring_structure.review_structure(artifacts, outline([selected]), [], budget(),
                                                    execute, outline_phase=True) == ([], [])


async def test_structure_accepted_original_cache_precedes_compact_replanning(monkeypatch):
    original = large_payload(large=True)
    monkeypatch.setattr(authoring_structure, "_structure_payload", lambda *_args: (original, []))
    admitted = budget()
    admitted.prompts = {"authoring_structure": PROMPT}
    admitted.context_tokens = 100
    expected = build_request(admitted.model, PROMPT, original, task="authoring_structure")
    calls = []

    class Executor:
        async def replay(self, stage, request, validate):
            calls.append(request)
            assert request == expected
            return True, validate({"findings": [{"code": "kept", "detail": "已付费回执有效。"}]})

        async def __call__(self, *_args):
            pytest.fail("Accepted cache must not send or choose a different compact request.")

    findings, gaps = await authoring_structure.review_structure(None, {"chapters": [{}]},
                                                               [], admitted, Executor())
    assert len(calls) == 1 and findings[0]["code"] == "kept" and not gaps


async def test_structure_accepted_compact_cache_precedes_compact_admission(monkeypatch):
    original = large_payload(large=True)
    monkeypatch.setattr(authoring_structure, "_structure_payload", lambda *_args: (original, []))
    admitted = budget()
    admitted.prompts = {"authoring_structure": PROMPT}
    admitted.context_tokens = 100
    calls = []

    class Executor:
        async def replay(self, stage, request, validate):
            data = json.loads(request["messages"][1]["content"][0]["text"])
            calls.append(data)
            if data.get("structure_view") == "compact_v1":
                assert admitted.measure_request(request, stage)["fits"] is False
                return True, validate({"findings": []})
            return False, None

        async def __call__(self, *_args):
            pytest.fail("Already accepted compact responses must not be sent again.")

    assert await authoring_structure.review_structure(None, {"chapters": [{}]}, [], admitted, Executor()) == ([], [])
    assert len(calls) == 2 and "structure_view" not in calls[0] and calls[1]["structure_view"] == "compact_v1"
