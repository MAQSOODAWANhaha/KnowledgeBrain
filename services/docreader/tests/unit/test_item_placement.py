"""New requirement chain: submission-form gap filling and short-ID item placement."""

import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender import item_placement as placement


ARTIFACT = {"sha256": "0" * 64, "relative_path": "00/" + "0" * 64, "size": 1, "media_type": "application/json"}


def ref(number):
    return {"item_ref": {"artifact_ref": ARTIFACT, "item_id": f"i{number}"}}


def item(number, kind="obligation", documents=None, page=0, quote="原文"):
    semantic: dict = {"kind": kind, "text": f"第{number}项要求"}
    if documents:
        semantic["submission"] = {"documents": documents}
    return {"rule_ref": ref(number), "semantic": semantic,
            "evidence": [{"document_id": "tender", "page": page, "quote": quote}]}


def outline():
    return {"schema_version": 2, "authoring_input": {}, "issues": [], "chapters": [
        {"chapter_id": "c1", "title": "投标书", "children": [], "targets": [
            {"target_id": "a", "kind": "form", "purpose": "致：采购人", "rule_refs": []}]},
        {"chapter_id": "c2", "title": "开标一览表", "children": [], "targets": [
            {"target_id": "b", "kind": "form", "purpose": "开标一览表", "rule_refs": []}]},
    ]}


def test_decode_accepts_one_value_per_item_and_rejects_everything_else():
    positions = placement.placement_positions(outline())
    ids = ["r1", "r2", "r3"]
    good = {"placements": [["r1", "t2"], ["r2", "evaluation"], ["r3", None]]}
    assert placement.decode_placements(good, ids, positions) == {"r1": "t2", "r2": "evaluation", "r3": None}
    for bad in (
        {"placements": good["placements"][:2]},
        {"placements": [*good["placements"], ["r1", "t1"]]},
        {"placements": [["r9", "t1"], *good["placements"][1:]]},
        {"placements": [["r1", "t9"], *good["placements"][1:]]},
        {"placements": [["r1", "scoring"], *good["placements"][1:]]},
        {"placements": good["placements"], "note": "x"},
    ):
        with pytest.raises(ModelContractError):
            placement.decode_placements(bad, ids, positions)


def test_bind_writes_positions_constraints_and_exclusions_and_leaves_pending_out():
    base = outline()
    positions = placement.placement_positions(base)
    items = {f"r{n}": item(n) for n in range(1, 5)}
    bound = placement.bind_placements(base, positions, items,
                                      {"r1": "t2", "r2": "document_wide", "r3": "evaluation", "r4": None})
    assert bound["chapters"][1]["targets"][0]["rule_refs"] == [ref(1)]
    assert bound["constraint_rule_refs"] == [ref(2)]
    excluded, = [issue for issue in bound["issues"] if issue["code"] == "outline_rule_excluded"]
    assert excluded["rule_refs"] == [ref(3)] and "评委" in excluded["detail"]
    text = json.dumps(bound, ensure_ascii=False)
    assert '"i4"' not in text
    assert base["chapters"][1]["targets"][0]["rule_refs"] == []


DOCUMENTS = [{"document_id": "tender", "pages": [
    "须知\n无关联关系承诺函\n致：\n我方承诺……\n承诺人（盖章）：\n",
    "第六章 投标文件格式\n一、投标书\n致：采购人\n",
]}]


def plan():
    return {"nodes": [{"id": "format-1", "name": "投标书", "parent_id": None, "response_kind": "form",
                       "heading": {"document_id": "tender", "page": 1, "line": 1}, "body_windows": []}],
            "composition": {"priority_basis": [], "entries": [{"name": {"document_id": "tender", "page": 1, "line": 1}}]},
            "scopes": [{"items": [{"node_id": "format-1", "entry": 0, "depth": "self",
                                   "presentation": "section", "clause": []}]}],
            "excluded": [], "source_only": []}


def test_named_submission_form_outside_the_format_chapter_becomes_a_chapter_in_source_order():
    items = [item(1, "submission_requirement", ["无关联关系承诺函"], page=0),
             item(2, "submission_requirement", ["投标书"], page=0)]
    result, added_names = placement.add_submission_forms(plan(), DOCUMENTS, items, {("tender", 1)})
    assert added_names == ["无关联关系承诺函"]
    added, = [node for node in result["nodes"] if node["id"].startswith("submission-")]
    assert added["name"] == "无关联关系承诺函"
    assert added["heading"] == {"document_id": "tender", "page": 0, "line": 1}
    assert added["body_windows"] == [{"document_id": "tender", "page": 0, "start_line": 1, "end_line": 4}]
    assert result["composition"]["entries"][0]["name"] == added["heading"]
    order = {entry["node_id"]: entry["entry"] for entry in result["scopes"][0]["items"]}
    assert order == {"submission-1": 0, "format-1": 1}


def test_submission_form_without_a_title_line_is_not_invented():
    items = [item(1, "submission_requirement", ["股权结构说明"], page=0)]
    assert placement.add_submission_forms(plan(), DOCUMENTS, items, {("tender", 1)}) == (plan(), [])


def test_a_candidate_form_stays_only_when_an_item_is_placed_in_it():
    base = outline()
    base["chapters"].insert(0, {"chapter_id": "c0", "title": "廉洁履约承诺书", "children": [], "targets": [
        {"target_id": "z", "kind": "form", "purpose": "廉洁履约承诺书", "rule_refs": []}]})
    positions = placement.placement_positions(base, {"廉洁履约承诺书"})
    payload = placement.placement_payload(positions, {"r1": item(1)}, ["r1"])
    assert [row.get("origin") for row in payload["positions"]] == ["outside_format", None, None]
    items = {"r1": item(1)}
    dropped = placement.bind_placements(base, positions, items, {"r1": "performance"})
    assert [chapter["title"] for chapter in dropped["chapters"]] == ["投标书", "开标一览表"]
    kept = placement.bind_placements(base, positions, items, {"r1": "t1"})
    assert kept["chapters"][0]["title"] == "廉洁履约承诺书"


def test_batches_follow_the_window_and_keep_source_order():
    positions = placement.placement_positions(outline())
    items = {f"r{n}": item(n) for n in range(1, 6)}
    batches = placement.plan_batches(RequestBudget(), positions, items,
                                     lambda payload: len(payload["requirements"]) <= 2)
    assert batches == [["r1", "r2"], ["r3", "r4"], ["r5"]]


async def test_a_failed_batch_leaves_its_items_pending_and_named():
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        assert task == "item_placement" and payload["operation"] == "place_items_v1"
        ids = [row["id"] for row in payload["requirements"]]
        calls.append(ids)
        if "r3" in ids:
            raise ModelWorkUnitExhausted(task)
        return validate({"placements": [[identity, "t1"] for identity in ids]})

    bound, stats = await placement.place_items(
        outline(), [item(n) for n in range(1, 5)], RequestBudget(), None, ask,
        lambda payload: len(payload["requirements"]) <= 2)
    assert calls == [["r1", "r2"], ["r3", "r4"]] and stats == {"batches": 2, "failed": 1}
    assert bound["chapters"][0]["targets"][0]["rule_refs"] == [ref(1), ref(2)]
    failed, = [issue for issue in bound["issues"] if issue["code"] == "outline_item_placement_failed"]
    assert failed["rule_refs"] == [ref(3), ref(4)]


def test_batches_never_exceed_the_soft_item_cap(monkeypatch):
    monkeypatch.setattr(placement, "PLACEMENT_BATCH_ITEMS", 3)
    positions = placement.placement_positions(outline())
    items = {f"r{n}": item(n) for n in range(1, 8)}
    batches = placement.plan_batches(RequestBudget(), positions, items, lambda payload: True)
    assert [len(batch) for batch in batches] == [3, 3, 1]


CHAIN_PAGES = [
    '第四章 合同格式\n无关联关系承诺函\n致：采购人\n承诺人（盖章）：\n',
    '第六章 投标文件格式\n一、投标书\n致：招标人\n投标人名称：________\n二、开标一览表\n总价：\n',
]


async def test_generate_document_runs_the_requirement_chain(tmp_path, monkeypatch):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender import authoring_pipeline
    from docreader.tender.authoring_coverage import build_ledger
    from docreader.tender.schema import AuthoringInput

    ledger = [item(1, "submission_requirement", ["无关联关系承诺函"], page=0),
              item(2, "submission_requirement", ["开标一览表"], page=1),
              item(3, "scoring_rule"), item(4)]
    monkeypatch.setattr(placement, "_items", lambda _context: ledger)
    sent = []

    async def execute(task, request, validate):
        body = json.loads(request["messages"][1]["content"][0]["text"])
        sent.append(task)
        if task == "directory_plan":
            found = {text.strip(): ref for ref, text in body["lines"]}
            return validate({"chapters": [{"heading_refs": [found["一、投标书"]], "name": "投标书"},
                                          {"heading_refs": [found["二、开标一览表"]], "name": "开标一览表"}]})
        ids = {row["title_path"][0]: row["id"] for row in body["positions"]}
        assert [row.get("origin") for row in body["positions"]][0] == "outside_format"
        return validate({"placements": [["r1", ids["无关联关系承诺函"]], ["r2", ids["开标一览表"]],
                                        ["r3", "evaluation"], ["r4", None]]})

    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput.model_validate({"schema_version": 2, "run_id": "chain", "source_version": 1,
                                              "domain_revision": 1}).model_dump()
    reference = await authoring_pipeline.generate_document(
        artifacts, snapshot, RequestBudget(model="unused"), execute,
        directory_documents=[{"document_id": "tender", "role": "main", "pages": CHAIN_PAGES,
                              "line_units": [[] for _ in CHAIN_PAGES]}],
        outline_only=True, directory_contract="bid_format_source_outline_v1",
        requirement_chain_contract=placement.REQUIREMENT_CHAIN_CONTRACT)
    outline = artifacts.read_json(reference)
    assert sent == ["directory_plan", "item_placement"]
    assert [chapter["title"] for chapter in outline["chapters"]] == ["无关联关系承诺函", "投标书", "开标一览表"]
    resolution = {row["rule_ref"]["item_ref"]["item_id"]: row["resolution"]
                  for row in build_ledger(outline, {"rules": [{"rule_ref": row["rule_ref"]} for row in ledger]})
                  if row["rule_ref"]}
    assert resolution["i3"] == "excluded" and resolution["i4"] == "pending"
    assert resolution["i1"] != "pending" and resolution["i2"] != "pending"
