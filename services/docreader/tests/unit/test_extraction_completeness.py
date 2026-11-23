"""BUG-261: extraction packs use soft output-workload targets and compact reason codes without adding a fill stage."""
from copy import deepcopy

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender.evidence.planner import (
    EXTRACT_PACK_INPUT_TARGET,
    MAX_EXTRACT_BODY_RANGES,
    make_packs,
)
from docreader.tender.prompts import get_prompt
from docreader.tender.requirements import extract_items
from test_analysis_extraction import IDENTITY, pack, result, source
from test_structural_packing import source as native_source


def test_extract_uses_32k_reserve_while_keeping_real_context_admission_ceiling():
    budget = RequestBudget(prompts={"extract": "Read.", "verify": "Read."})
    measured = budget.measure("extract", {"schema_version": 2})
    assert measured["output_reserve"] == 32768
    assert budget.output_reserve_for("extract") == 32768
    assert measured["effective_input_budget_tokens"] == budget.input_budget_for("extract") == 94208
    # Generic base reserve stays 16384 for non-extract tasks.
    assert budget.output_reserve == 16384


def test_extract_packs_stay_within_soft_range_and_input_targets_with_bounded_probes():
    index = native_source([{"text": f"第{number}行。", "section": "one"} for number in range(700)])
    packs, stats = make_packs(index, RequestBudget(prompts={"extract": "Read."}))
    assert MAX_EXTRACT_BODY_RANGES == 200
    assert EXTRACT_PACK_INPUT_TARGET == 55000
    assert len(packs) == 4
    assert all(len(p["body"]) <= MAX_EXTRACT_BODY_RANGES for p in packs)
    assert all(p["planning_details"]["budget"]["input_tokens"] <= EXTRACT_PACK_INPUT_TARGET for p in packs)
    assert sum(len(p["body"]) for p in packs) == 700
    # Range-prefix partitioning avoids linear step-down probes.
    assert stats["probe_count"] < 40


def test_host_reviewed_blank_pages_excluded_from_cap_but_blank_table_rows_still_count():
    from docreader.tender.evidence.references import SourceIndex

    # 50 host-reviewed blank pages + 200 normal ranges fit in one pack (200 model ranges).
    units = [
        {"unit_id": f"unit:{i}", "kind": "section", "ordinal": i, "text": f"条款{i}。",
         "locator": {"locator_kind": "document", "section_ordinal": i, "page_ordinal": i, "heading_path": ""},
         "physical_path": f"native:unit:{i}"}
        for i in range(200)
    ]
    identities = [
        {"unit_id": f"unit:{i}", "section_id": "s1", "parent_section_id": None, "heading_path": "",
         "physical_locator": units[i]["locator"]}
        for i in range(200)
    ]
    blank_pages = [
        {"page_ordinal": 200 + i, "classification": "blank", "unit_ids": []}
        for i in range(50)
    ]
    doc = {"document_id": "document", "document_version": "1", "original_ref": {"sha256": "1" * 64}}
    raw = {"schema_version": 2, "units": units, "source_contract": {"units": identities, "page_manifest": blank_pages},
           "images": {}, "ocr": {}}
    packs, _ = make_packs(SourceIndex(doc, raw), RequestBudget(prompts={"extract": "Read."}))
    assert len(packs) == 1
    assert len(packs[0]["body"]) == 250
    assert len(packs[0]["compacted_blank_ranges"]) == 50

    # 250 blank table rows still require model dispositions, so they must split at 200.
    grid = {"row_count": 251, "column_count": 2,
            "cells": [{"row": 0, "column": 0, "text": "表头", "row_span": 1, "col_span": 1}]}
    table_packs, _ = make_packs(native_source([{"section": "s1", "grid": grid}]), RequestBudget(prompts={"extract": "Read."}))
    assert len(table_packs) == 2
    assert all(len(p["body"]) <= MAX_EXTRACT_BODY_RANGES for p in table_packs)


def test_single_indivisible_range_above_soft_target_stays_admitted_within_real_budget():
    # A single ~60KB range exceeds the 55KB multi-range soft target but easily fits 94208.
    index = native_source([{"text": "a" * 60000, "section": "s1"}])
    packs, _ = make_packs(index, RequestBudget(prompts={"extract": "Read."}))
    assert len(packs) == 1
    assert packs[0]["planning_details"]["budget"]["input_tokens"] > EXTRACT_PACK_INPUT_TARGET
    assert packs[0]["planning_details"]["budget"]["fits"] is True
    assert not any(i["code"] == "indivisible_source_context_exceeded" for i in packs[0]["issues"])


class RecordingModel:
    def __init__(self, response):
        self.response, self.calls = response, []

    async def call(self, task, payload, images=()):
        self.calls.append(deepcopy(payload))
        return deepcopy(self.response)


def body():
    return [
        source(),
        source("r2", "第2页"),
        source("r3", "本项目为智慧园区一期建设背景介绍。"),
        source("r4", "评分：商务部分20分。"),
        source("r5", "招标文件编号：ZB-2026-01"),
    ]


@pytest.mark.asyncio
async def test_reason_codes_tolerant_receiver_and_single_pass_omitted_dispositions():
    response = result(dispositions=[
        {"range_id": "r2", "status": "non_item", "reason_code": "page_number"},
        {"range_id": "r3", "status": "non_item", "reason_code": "informational", "reason": "项目一期建设背景介绍。"},
        # Omitted reason_code with explicit reason is tolerated (diagnostic, not blocking).
        {"range_id": "r5", "status": "non_item", "reason": "文件编号元信息。"},
        # r4 intentionally omitted by the model.
    ])
    model = RecordingModel(response)
    extracted = await extract_items(pack(body()), model, identity=IDENTITY)

    # Single pass only: no second fill call.
    assert len(model.calls) == 1
    by_range = {d["range_id"]: d for d in extracted["dispositions"]}
    assert by_range["r2"]["status"] == "non_item" and "页码" in by_range["r2"]["reason"]
    # Explicit reason alongside reason_code is preserved rather than overwritten.
    assert by_range["r3"]["status"] == "non_item" and by_range["r3"]["reason"] == "项目一期建设背景介绍。"
    assert by_range["r5"]["status"] == "non_item" and by_range["r5"]["reason"] == "文件编号元信息。"
    assert any(d["code"] == "disposition_reason_code_missing" and d.get("range_id") == "r5"
               for d in extracted.get("diagnostics", []))
    # Omitted r4 stays unresolved with host issue and diagnostic.
    assert by_range["r4"]["status"] == "unresolved"
    assert any(p["code"] == "extraction_disposition_unresolved" and p.get("range_id") == "r4"
               for p in extracted["issues"])
    assert any(d["code"] == "disposition_missing" and d.get("range_id") == "r4"
               for d in extracted.get("diagnostics", []))


@pytest.mark.asyncio
async def test_other_without_reason_is_rejected_into_pending():
    response = result(dispositions=[
        {"range_id": "r2", "status": "non_item", "reason_code": "other"},
        {"range_id": "r3", "status": "non_item", "reason_code": "informational"},
        {"range_id": "r4", "status": "unresolved", "reason": "评分口径待核。"},
        {"range_id": "r5", "status": "non_item", "reason_code": "informational"},
    ])
    extracted = await extract_items(pack(body()), RecordingModel(response), identity=IDENTITY)
    by_range = {d["range_id"]: d for d in extracted["dispositions"]}
    assert by_range["r2"]["status"] == "unresolved"
    assert any(p["code"] == "extraction_disposition_unresolved" and p.get("range_id") == "r2"
               for p in extracted["issues"])


def test_extract_prompt_has_no_retired_locate_or_association_instructions():
    prompt = get_prompt("extract")
    assert "separate locate operation" not in prompt
    assert "for association and independent review" not in prompt
    assert "informational" in prompt
