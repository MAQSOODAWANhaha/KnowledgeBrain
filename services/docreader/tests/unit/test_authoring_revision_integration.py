"""Revision preserves related gaps while runtime-failed blanks skip content review."""

from collections import Counter
from io import BytesIO
import json

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.authoring_pipeline import generate_document
from docreader.tests.submission_responses import assemble_independent_candidates


async def test_runtime_failed_blank_skips_review_and_related_gap_survives_revision(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    budget = RequestBudget(model="fixture", context_tokens=131072, prompts={}, output_reserve=16384)
    snapshot = {"schema_version": 2, "run_id": "revision-integration", "source_version": 1, "domain_revision": 1}
    calls, reviews, structure_reviews, task_ids = [], [], [], {}

    async def execute(stage, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(stage)
        if stage == "outline":
            if value.get("operation") == "assemble":
                return validate(assemble_independent_candidates(value))
            return validate({"schema_version": 2, "chapters": [
                {"title": name, "children": [], "targets": [{"kind": "narrative",
                 "purpose": "编写本部分已有依据支持的具体方案", "rule_ids": [], "range_ids": []}]}
                for name in ("A部分", "B部分", "C部分")], "findings": []})
        if stage == "authoring_basis":
            return validate({"approach": "保持各部分安排一致，未取得的信息继续留空。"})
        if stage == "authoring_write":
            title = value["title_path"][-1]
            if "previous_content" in value:
                assert title == "A部分", "Only the task with an executable revision finding may be rewritten."
                assert value["previous_content"]["blocks"][0]["text"] == "A部分原成功正文"
                return validate({"blocks": [{"kind": "paragraph", "text": "A部分修订后成功正文",
                    "target_ids": [value["targets"][0]["target_id"]], "support_ids": ["k1"]}]})
            if title == "B部分":
                raise ModelWorkUnitExhausted(stage)
            return validate({"blocks": [{"kind": "paragraph", "text": title + "原成功正文",
                "target_ids": [value["targets"][0]["target_id"]], "support_ids": ["k1"]}]})
        assert stage == "authoring_review"
        if value.get("operation") == "structure":
            structure_reviews.append(value)
            return validate({"findings": []})
        reviews.append(value)
        if len(reviews) == 1:
            task_ids.update({entry["title_path"][-1]: entry["task_id"] for entry in value["units"]})
            assert set(task_ids) == {"A部分", "C部分"}
            assert [entry["content"]["status"] for entry in value["units"]] == ["generated", "generated"]
            return validate({"findings": [
                {"code": "a_revision", "detail": "A的既有正文可按给定依据细化。",
                 "task_ids": [task_ids["A部分"]], "suggestion": "修订A的具体步骤。", "can_revise": True},
                {"code": "a_c_dependency", "detail": "A与C所依赖的共同材料尚未提供。",
                 "task_ids": [task_ids["A部分"], task_ids["C部分"]], "suggestion": "补充共同材料后核对A和C。", "can_revise": False},
            ]})
        assert len(reviews) == 2
        assert {entry["task_id"] for entry in value["units"]} == {task_ids["A部分"], task_ids["C部分"]}
        assert {finding["code"] for finding in value["previous_findings"]} == {"a_revision", "a_c_dependency"}
        return validate({"findings": [{"code": "a_c_dependency", "detail": "A与C所依赖的共同材料仍待补。",
                         "task_ids": [task_ids["A部分"], task_ids["C部分"]], "can_revise": False}]})

    result = artifacts.read_json(await generate_document(artifacts, snapshot, budget, execute, materials=[
        {"material_id": "project-method", "title": "给定方案依据",
         "text": "A部分原成功正文；A部分修订后成功正文；B部分计划；C部分原成功正文。"}]))
    initial = artifacts.read_json(result["initial_review_ref"])
    assert {finding["code"] for finding in initial["findings"]} == {"a_revision", "a_c_dependency"}
    final = artifacts.read_json(result["review_ref"])
    assert [finding["code"] for finding in final["findings"]] == ["a_c_dependency"]
    assert final["findings"][0]["task_ids"] == [task_ids["A部分"], task_ids["C部分"]]
    assert {gap["task_id"] for gap in result["gaps"] if gap["code"] == "a_c_dependency"} == {
        task_ids["A部分"], task_ids["C部分"]}
    assert len(result["superseded_unit_refs"]) == 1
    old_unit = artifacts.read_json(result["superseded_unit_refs"][0])
    assert old_unit["task_id"] == task_ids["A部分"] and old_unit["blocks"][0]["text"] == "A部分原成功正文"
    units = {unit["task_id"]: unit for unit in map(artifacts.read_json, result["unit_refs"])}
    assert units[task_ids["A部分"]]["blocks"][0]["text"] == "A部分修订后成功正文"
    assert next(value for value in units.values() if value["error_type"])["status"] == "placeholder"
    assert units[task_ids["C部分"]]["blocks"][0]["text"] == "C部分原成功正文"
    document = Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))
    paragraphs = [paragraph.text for paragraph in document.paragraphs]
    assert "A部分修订后成功正文" in paragraphs and "C部分原成功正文" in paragraphs
    assert "A部分原成功正文" not in paragraphs and any("待补" in text for text in paragraphs)
    assert Counter(calls)["authoring_write"] == 4 and Counter(calls)["authoring_review"] == 3
    assert len(reviews) == 2 and len(structure_reviews) == 1
