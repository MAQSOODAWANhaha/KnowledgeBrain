"""The final structure review inspects actual instances without rereviewing every blank."""

import asyncio
from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender.authoring_revision import _previous_content
from docreader.tender.evidence.references import SourceIndex


def source(artifacts, text="原格式：投标人____，签字____。"):
    unit = {
        "unit_id": "declaration",
        "kind": "section",
        "ordinal": 0,
        "text": text,
        "locator": {"locator_kind": "document", "section_ordinal": 0},
    }
    raw = {
        "schema_version": 2,
        "units": [unit],
        "images": {},
        "ocr": {},
        "source_contract": {
            "page_manifest": [],
            "units": [
                {
                    "unit_id": "declaration",
                    "section_id": "section-1",
                    "parent_section_id": None,
                    "heading_path": "投标函原格式",
                    "physical_locator": unit["locator"],
                }
            ],
        },
    }
    document = {
        "document_id": "tender",
        "document_version": "1",
        "original_ref": artifacts.put_bytes(b"synthetic original"),
    }
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(index.raw)))
    return {"source_index_ref": index_ref, "unit_id": "declaration"}


def target(identity="form-1", part=None):
    return {
        "target_id": identity,
        "kind": "form",
        "purpose": "按原格式保留投标函待签位置。",
        "manual_fill": True,
        "source_refs": [],
        "rule_refs": [],
        "template_refs": [part] if part else [],
    }


def outline(targets):
    return {
        "schema_version": 2,
        "authoring_input": {
            "schema_version": 2,
            "run_id": "structure",
            "source_version": 1,
            "domain_revision": 1,
        },
        "issues": [],
        "constraint_rule_refs": [],
        "submission_refs": [],
        "chapters": [
            {
                "chapter_id": "business",
                "title": "商务文件",
                "is_volume": True,
                "children": [],
                "targets": targets,
            }
        ],
    }


def unit(identity="w1", target_id="form-1"):
    return {
        "schema_version": 2,
        "task_id": identity,
        "chapter_id": "business",
        "target_ids": [target_id],
        "status": "placeholder",
        "blocks": [],
        "cell_fills": [],
        "attachments": [],
        "gaps": ["企业资料未提供，保留待填。"],
        "error_type": None,
    }


def budget():
    return RequestBudget(model="fixture", context_tokens=131072, prompts={})


def submission_context(monkeypatch):
    from docreader.tender import authoring_pipeline

    def load(_context, refs, submission_refs=()):
        assert refs == []
        return {
            "submission_rules": [],
            "submission_sources": [
                {
                    "source_id": "s1",
                    "text": "商务文件单独提交，投标函按附件一规定格式。",
                    "heading_path": "投标人须知前附表",
                }
            ],
        }

    monkeypatch.setattr(authoring_pipeline, "_constraint_context", load, raising=False)


async def test_structure_review_is_one_call_for_manual_empty_instances_and_shared_template(
    tmp_path, monkeypatch
):
    from docreader.tender.authoring_structure import review_structure

    artifacts = ArtifactStore(tmp_path)
    part = {"source_ref": source(artifacts), "selectors": []}
    current = outline([target(part=part), target("manufacturer", part)])
    originals = deepcopy(current)
    units = [unit(), unit("w2", "manufacturer")]
    submission_context(monkeypatch)
    calls = []

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(payload)
        assert stage == "authoring_structure" and payload["operation"] == "structure"
        assert payload["submission_sources"][0]["text"].startswith("商务文件单独提交")
        targets = payload["chapters"][0]["targets"]
        assert len(targets) == 2 and targets[0]["target_id"] != targets[1]["target_id"]
        assert targets[0]["template_support_ids"] == targets[1]["template_support_ids"]
        assert len(payload["supports"]) == 1
        assert payload["supports"][0]["text"] == "原格式：投标人____，签字____。"
        assert "heading_path" not in payload["supports"][0]
        assert payload["supports"][0]["section_id"] in {
            section["section_id"] for section in payload["sections"]
        }
        assert payload["chapters"][0]["is_volume"]
        assert all(
            not {"blocks", "cell_fills", "source_refs", "artifact_ref"} & item.keys()
            for item in payload["units"]
        )
        return validate(
            {
                "findings": [
                    {
                        "code": "check_subject",
                        "detail": "核对两份实例是否分别适用。",
                        "task_ids": ["w1", "w2"],
                        "can_revise": True,
                    }
                ]
            }
        )

    findings, gaps = await review_structure(
        artifacts, current, units, budget(), execute
    )
    assert len(calls) == 1 and not gaps and current == originals
    assert findings[0]["task_ids"] == ["w1", "w2"]
    assert findings[0]["can_revise"] is False, (
        "Structure feedback must not launch automatic reordering."
    )


async def test_structure_sees_adopted_cover_role_and_forward_cross_reference(
    tmp_path, monkeypatch
):
    from docreader.tender.authoring_structure import review_structure

    artifacts = ArtifactStore(tmp_path)
    first, second = target("first"), target("second")
    first["related_target_ids"] = ["second"]
    current = outline([first])
    current["chapters"][0]["presentation_role"] = "cover"
    current["chapters"].append(
        {
            "chapter_id": "later",
            "title": "另一卷",
            "targets": [second],
            "is_volume": True,
            "children": [],
        }
    )
    submission_context(monkeypatch)

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        cover, later = payload["chapters"]
        assert cover["presentation_role"] == "cover"
        assert cover["targets"][0]["related_target_ids"] == [
            later["targets"][0]["target_id"]
        ]
        return validate({"findings": []})

    assert await review_structure(artifacts, current, [], budget(), execute) == ([], [])


async def test_structure_uses_direct_submission_sources_without_any_rules(
    tmp_path, monkeypatch
):
    from docreader.tender.authoring_structure import review_structure
    from docreader.tender import authoring_pipeline

    current = outline([target()])
    current["submission_refs"] = [
        {"source_ref": {"fixture": "direct"}, "selectors": []}
    ]

    def load(_context, refs, submission_refs=()):
        assert refs == [] and submission_refs == current["submission_refs"]
        return {
            "submission_rules": [],
            "submission_sources": [{"source_id": "s1", "text": "技术文件独立提交。"}],
        }

    monkeypatch.setattr(authoring_pipeline, "_constraint_context", load, raising=False)

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        assert payload["submission_sources"][0]["text"] == "技术文件独立提交。"
        return validate({"findings": []})

    assert await review_structure(
        ArtifactStore(tmp_path), current, [unit()], budget(), execute
    ) == ([], [])


@pytest.mark.parametrize(
    "response",
    [
        {
            "findings": [
                {
                    "code": "wrong_volume",
                    "detail": "错误位置",
                    "task_ids": ["unknown-task"],
                }
            ]
        },
        {
            "findings": [
                {
                    "code": "wrong_template",
                    "detail": "未提供正文块",
                    "task_ids": ["w1"],
                    "content_ids": ["b1"],
                }
            ]
        },
    ],
)
async def test_structure_unknown_response_references_use_existing_contract_recovery(
    tmp_path, monkeypatch, response
):
    from docreader.tender.authoring_structure import review_structure

    submission_context(monkeypatch)

    async def execute(stage, request, validate):
        return validate(response)

    with pytest.raises(ModelContractError):
        await review_structure(
            ArtifactStore(tmp_path), outline([target()]), [unit()], budget(), execute
        )


async def test_structure_budget_failure_is_a_gap_and_does_not_send_or_reorder(
    tmp_path, monkeypatch
):
    from docreader.tender.authoring_structure import review_structure

    submission_context(monkeypatch)
    current = outline([target()])
    current["chapters"][0]["targets"][0]["purpose"] = "长" * 50000
    original = deepcopy(current)

    async def execute(*args):
        pytest.fail("Failed admission must not call a model.")

    findings, gaps = await review_structure(
        ArtifactStore(tmp_path), current, [unit()], budget(), execute
    )
    assert findings == [] and [g["code"] for g in gaps] == [
        "structure_review_unavailable"
    ]
    assert current == original


async def test_structure_exhaustion_has_no_outer_retry(tmp_path, monkeypatch):
    from docreader.tender.authoring_structure import review_structure

    submission_context(monkeypatch)
    calls = []

    async def execute(stage, request, validate):
        calls.append(stage)
        raise ModelWorkUnitExhausted(stage)

    findings, gaps = await review_structure(
        ArtifactStore(tmp_path), outline([target()]), [unit()], budget(), execute
    )
    assert calls == ["authoring_structure"] and not findings
    assert gaps[0]["code"] == "structure_review_unavailable"


@pytest.mark.parametrize(
    "error", [asyncio.CancelledError(), OSError("storage unavailable")]
)
async def test_structure_infrastructure_or_cancellation_is_not_semantic_success(
    tmp_path, monkeypatch, error
):
    from docreader.tender.authoring_structure import review_structure

    submission_context(monkeypatch)

    async def execute(*args):
        raise error

    with pytest.raises(type(error)):
        await review_structure(
            ArtifactStore(tmp_path), outline([target()]), [unit()], budget(), execute
        )


async def test_structure_broken_template_keeps_other_instances_and_reports_source_gap(
    tmp_path, monkeypatch
):
    from docreader.tender.authoring_structure import review_structure

    artifacts = ArtifactStore(tmp_path)
    good = {"source_ref": source(artifacts), "selectors": []}
    broken = {
        "source_ref": {**good["source_ref"], "unit_id": "missing"},
        "selectors": [],
    }
    current = outline([target(part=broken), target("valid", good)])
    submission_context(monkeypatch)

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        assert len(payload["supports"]) == 1
        assert payload["chapters"][0]["targets"][0]["template_support_ids"] == []
        assert payload["chapters"][0]["targets"][1]["template_support_ids"]
        return validate({"findings": []})

    findings, gaps = await review_structure(
        artifacts, current, [unit(), unit("w2", "valid")], budget(), execute
    )
    assert not findings and gaps[0]["code"] == "structure_template_unavailable"
    assert gaps[0]["target_id"] == "form-1"


async def test_no_structure_means_no_model_call(tmp_path):
    from docreader.tender.authoring_structure import review_structure

    async def execute(*args):
        pytest.fail("No candidate or final structure to review.")

    current = outline([])
    current["chapters"] = []
    assert await review_structure(
        ArtifactStore(tmp_path), current, [], budget(), execute
    ) == ([], [])


def test_revision_does_not_project_an_old_fill_onto_context_or_another_form(tmp_path):
    reference = source(ArtifactStore(tmp_path))
    task = {"targets": [{"target_id": "actual-form", "kind": "form"}]}
    current = {
        "blocks": [],
        "cell_fills": [
            {
                "source_ref": reference,
                "target_ids": ["actual-form"],
                "material_ids": [],
                "source_refs": [],
                "row": 0,
                "column": 0,
                "text": "已有填写",
            }
        ],
        "attachments": [],
        "gaps": [],
    }
    payload = {
        "targets": [
            {"target_id": "t1", "template_support_ids": [], "support_ids": ["s1"]}
        ],
        "supports": [
            {
                "support_id": "s1",
                "origin": "tender",
                "target_ids": ["t1"],
                "text": "理解依据",
            }
        ],
    }
    assert (
        _previous_content(
            task,
            current,
            payload,
            {"s1": reference},
            {"s1": {"origin": "tender", "source_ref": reference}},
        )
        is None
    )


def test_revision_selects_the_current_template_alias_not_a_same_unit_context_alias(
    tmp_path,
):
    reference = source(ArtifactStore(tmp_path))
    task = {"targets": [{"target_id": "actual-form", "kind": "form"}]}
    current = {
        "blocks": [],
        "cell_fills": [
            {
                "source_ref": reference,
                "target_ids": ["actual-form"],
                "material_ids": [],
                "source_refs": [],
                "row": 0,
                "column": 0,
                "text": "已有填写",
            }
        ],
        "attachments": [],
        "gaps": [],
    }
    payload = {
        "targets": [
            {"target_id": "t1", "template_support_ids": ["s1"], "support_ids": []}
        ],
        "supports": [
            {
                "support_id": identity,
                "origin": "tender",
                "target_ids": ["t1"],
                "text": "固定格式",
            }
            for identity in ("s1", "s2")
        ],
    }
    sources = {"s1": reference, "s2": reference}
    bindings = {
        identity: {"origin": "tender", "source_ref": reference} for identity in sources
    }
    previous = _previous_content(task, current, payload, sources, bindings)
    assert previous["cell_fills"][0]["source_id"] == "s1"
    current["cell_fills"][0]["target_ids"] = []
    assert _previous_content(task, current, payload, sources, bindings) is None


async def test_structure_receives_only_unassigned_and_excluded_rule_semantics(tmp_path):
    from docreader.tender.authoring_structure import review_structure

    artifacts = ArtifactStore(tmp_path)
    rules = [
        {
            "item_id": identity,
            "kind": "obligation",
            "text": text,
            "evidence": [{"range_id": "permanent-" + identity, "quote": text}],
        }
        for identity, text in (
            ("assigned", "已安排的要求不重复发送。"),
            ("pending", "履约阶段仍须提供保证。"),
            ("excluded", "仅接受联合体时提交协议。"),
        )
    ]
    rules[1].update(
        scope="中标后",
        conditions=["签订合同时"],
        exceptions=["原文另有规定除外"],
        numbers=[{"value": "0", "unit": "元", "quote": "0元"}],
    )
    reference = artifacts.put_json({"items": rules})
    refs = [
        {"item_ref": {"item_id": rule["item_id"], "artifact_ref": reference}}
        for rule in rules
    ]
    current = outline([target()])
    current["authoring_input"]["extract_refs"] = [reference]
    current["chapters"][0]["targets"][0]["rule_refs"] = [refs[0]]
    current["issues"] = [
        {
            "code": "outline_rule_unassigned",
            "detail": "尚未安排履约提醒。",
            "rule_refs": [refs[1], refs[1]],
        },
        {
            "code": "outline_rule_excluded",
            "detail": "本项目明确不接受联合体。",
            "rule_refs": [refs[2]],
        },
        {
            "code": "another_observation",
            "detail": "其他来源观察不复制。",
            "rule_refs": [refs[0]],
        },
    ]
    original = deepcopy(current)

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        dispositions = payload["rule_dispositions"]
        assert [(entry["code"], entry["reason"]) for entry in dispositions] == [
            ("outline_rule_unassigned", "尚未安排履约提醒。"),
            ("outline_rule_excluded", "本项目明确不接受联合体。"),
        ]
        assert dispositions[0]["rules"] == [
            {
                key: value
                for key, value in rules[1].items()
                if key not in {"item_id", "evidence"}
            }
            | {"evidence": [{"quote": rules[1]["text"]}]}
        ]
        assert dispositions[1]["rules"][0]["text"] == rules[2]["text"]
        encoded = json.dumps(dispositions, ensure_ascii=False)
        assert rules[0]["text"] not in encoded and reference["sha256"] not in encoded
        assert not any(
            field in encoded
            for field in ("item_id", "range_id", "artifact_ref", "relative_path")
        )
        return validate(
            {
                "findings": [
                    {
                        "code": "disposition_needs_review",
                        "detail": "履约保证仍应作为提醒，不因缺少企业资料排除。",
                        "task_ids": [],
                        "can_revise": True,
                    }
                ]
            }
        )

    findings, gaps = await review_structure(
        artifacts, current, [unit()], budget(), execute
    )
    assert (
        findings[0]["code"] == "disposition_needs_review"
        and findings[0]["can_revise"] is False
    )
    assert (
        not gaps
        and current == original
        and artifacts.read_json(reference) == {"items": rules}
    )


async def test_structure_uses_retained_rule_instead_of_original_amended_item(
    tmp_path, monkeypatch
):
    from types import SimpleNamespace
    from docreader.tender import authoring_structure

    artifacts = ArtifactStore(tmp_path)
    ref = {
        "item_ref": {
            "item_id": "original-item",
            "artifact_ref": artifacts.put_json({"old": "已删除义务"}),
        },
        "decision_id": "permanent-decision",
        "rule_index": 0,
    }
    semantic = {
        "text": "仅保留制造商的维护义务。",
        "scope": "制造商",
        "evidence": [{"range_id": "permanent-source", "quote": "制造商须提供维护。"}],
    }
    context = SimpleNamespace(
        rules={"rule-1": ref},
        payload={
            "rules": [
                {
                    "rule_id": "rule-1",
                    "item_id": "original-item",
                    "effective_rule": semantic,
                }
            ]
        },
        availability_issues=[],
    )
    monkeypatch.setattr(authoring_structure, "_OutlineContext", lambda *_args: context)
    current = outline([target()])
    current["issues"] = [
        {"code": "outline_rule_unassigned", "detail": "仍待安排。", "rule_refs": [ref]}
    ]

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        assert payload["rule_dispositions"] == [
            {
                "code": "outline_rule_unassigned",
                "reason": "仍待安排。",
                "rules": [
                    {
                        "text": semantic["text"],
                        "scope": "制造商",
                        "evidence": [{"quote": "制造商须提供维护。"}],
                    }
                ],
            }
        ]
        assert "已删除义务" not in json.dumps(payload, ensure_ascii=False)
        return validate({"findings": []})

    assert await authoring_structure.review_structure(
        artifacts, current, [], budget(), execute
    ) == ([], [])


async def test_structure_unreadable_disposition_keeps_reference_and_reports_review_gap(
    tmp_path,
):
    from docreader.tender.authoring_structure import review_structure

    artifacts = ArtifactStore(tmp_path)
    reference = {
        "item_ref": {
            "item_id": "unavailable-item",
            "artifact_ref": artifacts.put_json({"items": []}),
        }
    }
    current = outline([target()])
    current["issues"] = [
        {
            "code": "outline_rule_excluded",
            "detail": "排除理由待核。",
            "rule_refs": [reference],
        }
    ]
    original = deepcopy(current)

    async def execute(stage, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        assert payload["rule_dispositions"] == [
            {"code": "outline_rule_excluded", "reason": "排除理由待核。", "rules": []}
        ]
        assert any(
            issue["code"] == "structure_rule_disposition_unavailable"
            for issue in payload["input_issues"]
        )
        assert "unavailable-item" not in json.dumps(payload)
        return validate({"findings": []})

    findings, gaps = await review_structure(
        artifacts, current, [unit()], budget(), execute
    )
    assert not findings and [gap["code"] for gap in gaps] == [
        "structure_rule_disposition_unavailable"
    ]
    assert current == original
