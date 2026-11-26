"""Tests for bounded model-driven proposal sub-outline expansion (Plan A / BUG-337)."""

from __future__ import annotations

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender import authoring_coverage, authoring_pipeline, item_placement
from docreader.tender.proposal_outline import (
    PROPOSAL_OUTLINE_OPERATION,
    PROPOSAL_PENDING_CODE,
    PROPOSAL_TREE_OPERATION,
    decode_proposal_outline,
    decode_proposal_sections,
    expand_proposal_outlines,
)
from docreader.tender.schema import AuthoringInput
from docreader.tender.status import AuthoringTask


_INSTRUCTION = "投标人应当根据“第三章评标办法”、“第五章采购需求”等要求编制相关内容。"


def _proposal_fixture(tmp_path, rule_count=8):
    artifacts = ArtifactStore(tmp_path)
    units = [
        {"unit_id": "u_score", "kind": "section", "text": "第三章 评标办法\n技术部分评分标准\n实施方案与应急响应评审。"},
        {"unit_id": "u_req1", "kind": "section", "text": "第五章 采购需求\n1.1 项目概述\n1.1.1 建设背景\n完成云安全分析。"},
        {"unit_id": "u_req2", "kind": "section", "text": "1.2 服务内容\n1.2.1 驻场安全运维\n提供5x10驻场服务。"},
        {"unit_id": "u_req3", "kind": "section", "text": "1.3 售后与应急保障\n7x24小时响应。"},
    ]
    raw_ref = artifacts.put_json({"units": units})
    index_ref = artifacts.put_json(
        {"ranges": [{"range_id": f"r:{u['unit_id']}", "unit_id": u["unit_id"]} for u in units]}
    )
    frozen_ref = artifacts.put_json(
        {"documents": [{"document_id": "main", "role": "main", "raw_ref": raw_ref, "source_index_ref": index_ref}]}
    )
    items = []
    unit_cycle = ["u_score", "u_req1", "u_req2", "u_req3"]
    for idx in range(rule_count):
        uid = unit_cycle[idx % len(unit_cycle)]
        items.append({
            "item_id": f"i{idx + 1}",
            "kind": "scoring_rule" if uid == "u_score" else "obligation",
            "text": f"评分与服务要求第{idx + 1}项：落实{uid}对应方案。",
            **({"scoring": {"max_score": 10, "method": "综合评分"}} if uid == "u_score" else {}),
            "evidence": [{"range_id": f"r:{uid}", "quote": "要求原文"}],
        })
    extract_ref = artifacts.put_json({"items": items})
    refs = [{"item_ref": {"artifact_ref": extract_ref, "item_id": row["item_id"]}} for row in items]
    outline = {
        "authoring_input": {"frozen_ref": frozen_ref, "extract_refs": [extract_ref]},
        "issues": [],
        "chapters": [{
            "chapter_id": "chapter-11",
            "title": "十一、项目服务方案",
            "presentation_role": "body",
            "is_volume": False,
            "targets": [{
                "target_id": "chapter-11:target-1",
                "kind": "form",
                "purpose": f"十一、项目服务方案\n{_INSTRUCTION}",
                "rule_refs": deepcopy(refs),
                "source_refs": [],
                "template_refs": [],
                "manual_fill": True,
                "support_origins": ["tender"],
            }],
            "children": [{
                "chapter_id": "chapter-11.1",
                "title": "承诺",
                "presentation_role": "body",
                "is_volume": False,
                "targets": [],
                "children": [],
            }],
        }],
    }
    authoring_coverage.attach_w2_proposal_answers(outline)
    return artifacts, outline


def _collect_rule_ids(chapters):
    found = []
    for ch in chapters:
        for target in ch.get("targets") or []:
            found.extend(ref["item_ref"]["item_id"] for ref in target.get("rule_refs") or [])
        found.extend(_collect_rule_ids(ch.get("children") or []))
    return found


@pytest.mark.parametrize(
    "raw",
    [
        {"sections": [{"id": "s1", "title": "甲"}, {"id": "s1", "title": "乙"}]},
        {"sections": [{"id": "s1", "title": "甲"}, {"id": "s2", "parent_id": "missing", "title": "乙"}]},
        {"sections": [{"id": "s1", "parent_id": "s2", "title": "甲"}, {"id": "s2", "parent_id": "s1", "title": "乙"}]},
        {
            "sections": [
                {"id": "s1", "title": "二级"},
                {"id": "s2", "parent_id": "s1", "title": "三级"},
                {"id": "s3", "parent_id": "s2", "title": "四级超限"},
            ]
        },
    ],
)
def test_decode_proposal_sections_rejects_invalid_trees(raw):
    with pytest.raises(ModelContractError):
        decode_proposal_sections(raw)


def test_decode_proposal_outline_validates_assignments_and_normalizes_string_null():
    valid = {
        "sections": [
            {"id": "s1", "parent_id": "null", "title": "1.1 总体方案"},
            {"id": "s2", "parent_id": "s1", "title": "1.1.1 架构设计"},
        ],
        "assignments": [["r1", "s2"], ["r2", "null"], ["r3", None]],
    }
    sections, assignments = decode_proposal_outline(valid, ["r1", "r2", "r3"])
    assert sections[0]["parent_id"] is None
    assert assignments == {"r1": "s2", "r2": None, "r3": None}

    for bad_assignments in (
        [["r1", "s2"], ["r2", None]],  # missing r3
        [["r1", "s2"], ["r1", "s1"], ["r2", None], ["r3", None]],  # duplicate r1
        [["r1", "s2"], ["r2", "unknown"], ["r3", None]],  # unknown section
        [["r1", "s2"], ["r2", None], ["r3", None], ["r9", None]],  # unknown rule
    ):
        with pytest.raises(ModelContractError):
            decode_proposal_outline({"sections": valid["sections"], "assignments": bad_assignments}, ["r1", "r2", "r3"])


async def test_expand_proposal_outlines_one_call_builds_l2_l3_and_conserves_rules(tmp_path):
    artifacts, outline = _proposal_fixture(tmp_path, rule_count=8)
    sent = []

    async def execute(task, request, validate):
        body = json.loads(request["messages"][1]["content"][0]["text"])
        sent.append((task, body))
        assert body["operation"] == PROPOSAL_OUTLINE_OPERATION
        assert any("第三章 评标办法" in h for h in body["source_headings"])
        assert any("第五章 采购需求" in h for h in body["source_headings"])
        return validate({
            "sections": [
                {"id": "s1", "parent_id": None, "title": "1.1 项目理解与总体设计"},
                {"id": "s1_1", "parent_id": "s1", "title": "1.1.1 云安全分析方案"},
                {"id": "s2", "parent_id": None, "title": "1.2 驻场运维与应急保障"},
            ],
            "assignments": [
                ["r1", "s1"],
                ["r2", "s1_1"],
                ["r3", "s2"],
                ["r4", "s2"],
                ["r5", "s1"],
                ["r6", "s1_1"],
                ["r7", "s2"],
                ["r8", None],
            ],
        })

    budget = RequestBudget(model="unused")
    result = await expand_proposal_outlines(
        outline,
        artifacts,
        budget,
        execute,
        authoring_pipeline._ask,
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.PROPOSAL_OUTLINE, p),
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.ITEM_PLACEMENT, p),
    )
    assert len(sent) == 1 and sent[0][0] == AuthoringTask.PROPOSAL_OUTLINE

    proposal = result["chapters"][0]
    assert [c["title"] for c in proposal["children"]] == [
        "1.1 项目理解与总体设计",
        "1.2 驻场运维与应急保障",
        "承诺",
    ]
    assert [c["title"] for c in proposal["children"][0]["children"]] == ["1.1.1 云安全分析方案"]
    # Strict rule conservation: r8 stayed on parent :answer, r1..r7 distributed across L2/L3 targets.
    assert sorted(_collect_rule_ids([proposal])) == [f"i{n}" for n in range(1, 9)]
    parent_answer = next(t for t in proposal["targets"] if t["target_id"].endswith(":answer"))
    assert [r["item_ref"]["item_id"] for r in parent_answer["rule_refs"]] == ["i8"]

    # Idempotency: a second call must skip already expanded chapters.
    snapshot_after = deepcopy(result)
    again = await expand_proposal_outlines(
        result,
        artifacts,
        budget,
        execute,
        authoring_pipeline._ask,
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.PROPOSAL_OUTLINE, p),
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.ITEM_PLACEMENT, p),
    )
    assert len(sent) == 1 and again == snapshot_after


async def test_expand_proposal_outlines_skips_small_chapters_under_threshold(tmp_path):
    artifacts, outline = _proposal_fixture(tmp_path, rule_count=5)
    called = 0

    async def execute(task, request, validate):
        nonlocal called
        called += 1
        raise AssertionError("should not be called for < 6 rules")

    budget = RequestBudget(model="unused")
    result = await expand_proposal_outlines(
        outline,
        artifacts,
        budget,
        execute,
        authoring_pipeline._ask,
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.PROPOSAL_OUTLINE, p),
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.ITEM_PLACEMENT, p),
    )
    assert called == 0
    assert [c["title"] for c in result["chapters"][0]["children"]] == ["承诺"]


async def test_expand_proposal_outlines_uses_two_step_fallback_when_output_budget_tight(tmp_path):
    artifacts, outline = _proposal_fixture(tmp_path, rule_count=10)
    sent = []

    async def execute(task, request, validate):
        body = json.loads(request["messages"][1]["content"][0]["text"])
        sent.append((task, body.get("operation")))
        if task == AuthoringTask.PROPOSAL_OUTLINE:
            assert body["operation"] == PROPOSAL_TREE_OPERATION
            return validate({
                "sections": [
                    {"id": "s1", "parent_id": None, "title": "1.1 技术方案"},
                    {"id": "s2", "parent_id": None, "title": "1.2 售后保障"},
                ]
            })
        assert task == AuthoringTask.ITEM_PLACEMENT
        pos_ids = [row["id"] for row in body["positions"]]
        return validate({
            "placements": [
                [row["id"], pos_ids[idx % len(pos_ids)]]
                for idx, row in enumerate(body["requirements"])
            ]
        })

    # Tight output_reserve forces 2-step fallback (tree-only + item_placement batches).
    budget = RequestBudget(model="unused", output_reserve=800)
    result = await expand_proposal_outlines(
        outline,
        artifacts,
        budget,
        execute,
        authoring_pipeline._ask,
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.PROPOSAL_OUTLINE, p),
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.ITEM_PLACEMENT, p),
    )
    assert sent[0] == (AuthoringTask.PROPOSAL_OUTLINE, PROPOSAL_TREE_OPERATION)
    assert any(task == AuthoringTask.ITEM_PLACEMENT for task, _op in sent[1:])
    proposal = result["chapters"][0]
    assert [c["title"] for c in proposal["children"][:2]] == ["1.1 技术方案", "1.2 售后保障"]
    assert sorted(_collect_rule_ids([proposal])) == sorted(f"i{n}" for n in range(1, 11))
    # All rules moved to sub-chapters, so parent :answer is pruned.
    assert not any(t["target_id"].endswith(":answer") for t in proposal["targets"])


async def test_expand_proposal_outlines_degrades_gracefully_on_model_failure(tmp_path):
    artifacts, outline = _proposal_fixture(tmp_path, rule_count=6)

    async def execute(task, request, validate):
        raise ModelWorkUnitExhausted(task)

    budget = RequestBudget(model="unused")
    result = await expand_proposal_outlines(
        outline,
        artifacts,
        budget,
        execute,
        authoring_pipeline._ask,
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.PROPOSAL_OUTLINE, p),
        lambda p: authoring_pipeline._fits(budget, AuthoringTask.ITEM_PLACEMENT, p),
    )
    proposal = result["chapters"][0]
    assert [c["title"] for c in proposal["children"]] == ["承诺"]
    assert sorted(_collect_rule_ids([proposal])) == [f"i{n}" for n in range(1, 7)]
    assert any(issue["code"] == PROPOSAL_PENDING_CODE for issue in result["issues"])


async def test_generate_document_runs_proposal_outline_in_requirement_chain(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    pages = [
        f"第六章 投标文件格式\n一、投标书\n致：招标人\n十一、项目服务方案\n{_INSTRUCTION}\n",
    ]
    extract_ref = artifacts.put_json({"items": []})
    ledger = [
        {
            "rule_ref": {"item_ref": {"artifact_ref": extract_ref, "item_id": f"i{n}"}},
            "semantic": {"kind": "obligation", "text": f"服务方案要求{n}"},
            "evidence": [{"document_id": "tender", "page": 0, "quote": "要求"}],
        }
        for n in range(1, 7)
    ]
    monkeypatch.setattr(item_placement, "_items", lambda _context: ledger)
    sent = []

    async def execute(task, request, validate):
        body = json.loads(request["messages"][1]["content"][0]["text"])
        sent.append(task)
        if task == AuthoringTask.DIRECTORY_PLAN:
            found = {text.strip(): ref for ref, text in body["lines"]}
            return validate({"chapters": [
                {"heading_refs": [found["一、投标书"]], "label": "一、投标书", "name": "投标书"},
                {"heading_refs": [found["十一、项目服务方案"]], "label": "十一、项目服务方案", "name": "项目服务方案"},
            ]})
        if task == AuthoringTask.ITEM_PLACEMENT:
            proposal_pos = next(row["id"] for row in body["positions"] if "项目服务方案" in row["title_path"][0])
            return validate({"placements": [[row["id"], proposal_pos] for row in body["requirements"]]})
        assert task == AuthoringTask.PROPOSAL_OUTLINE
        return validate({
            "sections": [
                {"id": "s1", "parent_id": None, "title": "1.1 总体实施方案"},
                {"id": "s2", "parent_id": None, "title": "1.2 售后保障方案"},
            ],
            "assignments": [[row["id"], "s1" if idx < 3 else "s2"] for idx, row in enumerate(body["requirements"])],
        })

    snapshot = AuthoringInput.model_validate({
        "schema_version": 2, "run_id": "proposal-chain", "source_version": 1, "domain_revision": 1
    }).model_dump()
    reference = await authoring_pipeline.generate_document(
        artifacts, snapshot, RequestBudget(model="unused"), execute,
        directory_documents=[{"document_id": "tender", "role": "main", "pages": pages, "line_units": [[]]}],
        outline_only=True,
        directory_contract="bid_format_source_outline_v1",
        requirement_chain_contract=item_placement.REQUIREMENT_CHAIN_CONTRACT,
    )
    outline = artifacts.read_json(reference)
    assert sent == [AuthoringTask.DIRECTORY_PLAN, AuthoringTask.ITEM_PLACEMENT, AuthoringTask.PROPOSAL_OUTLINE]
    proposal = next(ch for ch in outline["chapters"] if ch["title"] == "十一、项目服务方案")
    assert [child["title"] for child in proposal["children"]] == ["1.1 总体实施方案", "1.2 售后保障方案"]
    assert sorted(_collect_rule_ids([proposal])) == [f"i{n}" for n in range(1, 7)]
