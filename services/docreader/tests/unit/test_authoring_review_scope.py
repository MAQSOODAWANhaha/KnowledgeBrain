"""A partial rereview must not erase a finding about several response units."""

import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender import authoring_pipeline as pipeline
from docreader.tests.submission_responses import assemble_independent_candidates


@pytest.mark.parametrize(
    "split_with_failure", [True, False], ids=["partial-failure", "complete-review"]
)
async def test_cross_task_issue_requires_a_complete_successful_rereview(
    tmp_path, monkeypatch, split_with_failure
):
    artifacts = ArtifactStore(tmp_path)
    original_batches = pipeline._batches
    review_rounds, reviews, structure_reviews, writes, identities = [], [], [], [], {}

    def batches(records, budget, stage, make_payload, **options):
        if stage == "authoring_review":
            review_rounds.append(len(records))
            if len(review_rounds) > 1 and split_with_failure:
                for record in records:
                    yield [record]
                return
        yield from original_batches(records, budget, stage, make_payload, **options)

    monkeypatch.setattr(pipeline, "_batches", batches)

    async def execute(stage, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        if stage == "outline":
            if value.get("operation") == "assemble":
                return validate(assemble_independent_candidates(value))
            return validate(
                {
                    "schema_version": 2,
                    "chapters": [
                        {
                            "title": title,
                            "children": [],
                            "targets": [
                                {
                                    "kind": "narrative",
                                    "purpose": "按已有功能资料编写响应",
                                    "rule_ids": [],
                                    "range_ids": [],
                                }
                            ],
                        }
                        for title in ("A部分", "C部分")
                    ],
                    "findings": [],
                }
            )
        if stage == "authoring_basis":
            return validate({})
        if stage == "authoring_write":
            title = value["title_path"][-1]
            revised = "previous_content" in value
            writes.append((title, revised))
            if revised:
                assert title == "A部分"
            return validate(
                {
                    "blocks": [
                        {
                            "kind": "paragraph",
                            "text": title + ("修订正文" if revised else "原正文"),
                            "target_ids": ["t1"],
                            "support_ids": ["k1"],
                        }
                    ]
                }
            )
        if stage == "authoring_structure" or value.get("operation") == "structure":
            structure_reviews.append(value)
            return validate({"findings": []})
        reviews.append(value)
        if len(reviews) == 1:
            identities.update(
                {entry["title_path"][-1]: entry["task_id"] for entry in value["units"]}
            )
            return validate(
                {
                    "findings": [
                        {
                            "code": "refine_a",
                            "detail": "A部分可以依据给定功能细化。",
                            "task_ids": [identities["A部分"]],
                            "content_ids": ["b1"],
                            "suggestion": "细化已有功能。",
                            "can_revise": True,
                        },
                        {
                            "code": "cross_dependency",
                            "detail": "A和C的共同接口条件仍需核对。",
                            "task_ids": [identities["A部分"], identities["C部分"]],
                            "can_revise": False,
                        },
                    ]
                }
            )
        local_ids = {entry["task_id"] for entry in value["units"]}
        previous = next(
            finding
            for finding in value["previous_findings"]
            if finding["code"] == "cross_dependency"
        )
        assert set(previous["task_ids"]) == local_ids
        assert previous["partial_context"] is split_with_failure
        assert "content_ids" not in previous and previous["original_content_ids"] == []
        if split_with_failure and identities["C部分"] in local_ids:
            raise ModelWorkUnitExhausted(stage)
        return validate({"findings": []})

    result = artifacts.read_json(
        await pipeline.generate_document(
            artifacts,
            {
                "schema_version": 2,
                "run_id": "review-scope",
                "source_version": 1,
                "domain_revision": 1,
            },
            RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384),
            execute,
            materials=[
                {
                    "material_id": "method",
                    "title": "功能资料",
                    "text": "A部分和C部分都有已记录的功能和实施方法。",
                }
            ],
        )
    )
    initial = artifacts.read_json(result["initial_review_ref"])["findings"]
    assert {finding["code"] for finding in initial} == {"refine_a", "cross_dependency"}
    final = artifacts.read_json(result["review_ref"])["findings"]
    if split_with_failure:
        assert {finding["code"] for finding in final} == {
            "cross_dependency",
            "review_unavailable",
        }
        retained = next(
            finding for finding in final if finding["code"] == "cross_dependency"
        )
        assert retained == next(
            finding for finding in initial if finding["code"] == "cross_dependency"
        )
        assert {
            gap["task_id"]
            for gap in result["gaps"]
            if gap["code"] == "cross_dependency"
        } == set(identities.values())
        assert len(reviews) == 3
    else:
        assert final == []
        assert not any(gap["code"] == "cross_dependency" for gap in result["gaps"])
        assert len(reviews) == 2
    assert review_rounds == [2, 2] and writes == [
        ("A部分", False),
        ("C部分", False),
        ("A部分", True),
    ]
    assert len(structure_reviews) == 1
    units = {
        unit["task_id"]: unit for unit in map(artifacts.read_json, result["unit_refs"])
    }
    assert units[identities["A部分"]]["blocks"][0]["text"] == "A部分修订正文"
    assert units[identities["C部分"]]["blocks"][0]["text"] == "C部分原正文"
    assert result["docx_ref"] and result["package_ref"]
