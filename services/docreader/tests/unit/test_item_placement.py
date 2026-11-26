"""New requirement chain: submission-form gap filling and short-ID item placement."""

import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender import item_placement as placement
from docreader.tender.authoring_coverage import build_ledger
from docreader.tender.authoring_export import _submission_notes


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
    # BUG-340: quoted string "null" is normalized to None rather than rejecting the whole batch.
    quoted_null = {"placements": [["r1", "t2"], ["r2", "evaluation"], ["r3", "null"]]}
    assert placement.decode_placements(quoted_null, ids, positions) == {"r1": "t2", "r2": "evaluation", "r3": None}
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
    items = {f"r{n}": item(n) for n in range(1, 6)}
    bound = placement.bind_placements(base, positions, items, {"r1": "t2", "r2": "document_wide",
        "r3": "evaluation", "r4": None, "r5": "performance"})
    assert bound["chapters"][1]["targets"][0]["rule_refs"] == [ref(1)]
    assert bound["constraint_rule_refs"] == [ref(2)]
    excluded, = [issue for issue in bound["issues"] if issue["code"] == "outline_rule_excluded"]
    assert excluded["rule_refs"] == [ref(5)] and "签约后" in excluded["detail"]
    pending, = [issue for issue in bound["issues"] if issue["code"] == "outline_rule_unassigned"]
    assert pending["rule_refs"] == [ref(3)]
    text = json.dumps(bound, ensure_ascii=False)
    assert '"i4"' not in text
    assert base["chapters"][1]["targets"][0]["rule_refs"] == []


async def test_evaluation_proposals_remain_pending_for_every_item_kind():
    items = [item(1, "scoring_rule", quote="每份得一分。"),
             item(2, "submission_requirement", ["说明文件"]), item(3), item(4, "consequence_rule"),
             item(5, "scoring_rule"), item(6, "obligation"), item(7)]
    items[0]["semantic"]["text"] = "每份得一分。需附加盖公章的证明文件。"
    items[2]["semantic"].pop("kind")
    items[2]["rule_ref"].update(decision_id="decision-3", rule_index=0)

    async def ask(_execute, _budget, task, payload, validate):
        assert task == "item_placement"
        assert len(payload["requirements"]) == len(items)
        return validate({"placements": [["r1", "evaluation"], ["r2", "evaluation"],
                                        ["r3", "evaluation"], ["r4", "evaluation"],
                                        ["r5", "not_applicable"], ["r6", "evaluation"], ["r7", "t2"]]})

    bound, stats = await placement.place_items(outline(), items, RequestBudget(), None, ask, lambda _payload: True)
    assert stats == {"batches": 1, "failed": 0}
    pending, = [issue for issue in bound["issues"] if issue["code"] == "outline_rule_unassigned"]
    assert pending["rule_refs"] == [items[i]["rule_ref"] for i in (0, 1, 2, 3, 5)]
    assert "评标" in pending["detail"] and "核定" in pending["detail"]
    assert [issue["rule_refs"] for issue in bound["issues"] if issue["code"] == "outline_rule_excluded"] == [[ref(5)]]

    requirements = {"rules": [{"rule_ref": row["rule_ref"], "rule": row["semantic"]} for row in items]}
    ledger = build_ledger(bound, requirements)
    formal_entries = [entry for entry in ledger if entry["rule_ref"] is not None]
    assert len(formal_entries) == len(items)
    assert {entry["rule_ref"]["item_ref"]["item_id"]: (entry["resolution"], entry["target_id"])
            for entry in formal_entries} == {
        "i1": ("pending", None), "i2": ("pending", None), "i3": ("pending", None),
        "i4": ("pending", None), "i5": ("excluded", None), "i6": ("pending", None), "i7": ("content", "b"),
    }
    notes = _submission_notes(bound, requirements, [], None, ledger)
    before, after = notes.split("## 尚未安排的要求", 1)
    for row in (items[i] for i in (0, 1, 2, 3, 5)):
        assert row["semantic"]["text"] not in before
        assert row["semantic"]["text"] in after


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


def test_named_submission_form_outside_the_format_chapter_stays_after_bid_format_chapters():
    items = [item(1, "submission_requirement", ["无关联关系承诺函"], page=0),
             item(2, "submission_requirement", ["投标书"], page=0)]
    result, added_names = placement.add_submission_forms(plan(), DOCUMENTS, items, {("tender", 1)})
    assert added_names == ["无关联关系承诺函"]
    added, = [node for node in result["nodes"] if node["id"].startswith("submission-")]
    assert added["name"] == "无关联关系承诺函"
    assert added["heading"] == {"document_id": "tender", "page": 0, "line": 1}
    assert added["body_windows"] == [{"document_id": "tender", "page": 0, "start_line": 1, "end_line": 4}]
    # BUG-339: outside-format forms are appended after bid-format entries with entry=None so
    # they never jump ahead of the cover or Chapter 1 when their PDF page number is smaller.
    assert [entry["node_id"] for entry in result["scopes"][0]["items"]] == ["format-1", "submission-1"]
    assert result["scopes"][0]["items"][1]["entry"] is None


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


def test_cover_keeps_the_same_item_placement_aliases_and_is_still_carried():
    base = outline()
    expected = placement.placement_positions(base)
    cover = {'chapter_id': 'cover', 'title': '封面', 'presentation_role': 'cover', 'children': [], 'targets': [
        {'target_id': 'cover-grid', 'kind': 'form', 'purpose': '投标人字段', 'rule_refs': []}]}
    base['chapters'].insert(0, cover)
    positions = placement.placement_positions(base)
    assert positions == expected
    bound = placement.bind_placements(base, positions, {'r1': item(1)}, {'r1': 't1'})
    assert bound['chapters'][0] == cover
    assert bound['chapters'][1]['targets'][0]['rule_refs'] == [ref(1)]


def test_batches_follow_the_window_and_keep_source_order():
    positions = placement.placement_positions(outline())
    items = {f"r{n}": item(n) for n in range(1, 6)}
    batches = placement.plan_batches(RequestBudget(), positions, items,
                                     lambda payload: len(payload["requirements"]) <= 2)
    assert batches == ([["r1", "r2"], ["r3", "r4"], ["r5"]], [])


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


def test_batches_stop_at_half_the_input_budget_and_still_send_a_single_item():
    budget = RequestBudget(context_tokens=8192, output_reserve=512, tokenizer="o200k_base")
    positions = placement.placement_positions(outline())
    items = {f"r{n}": item(n, quote="事项" * 500) for n in range(1, 8)}

    def fits(payload):
        from docreader.tender.authoring_pipeline import _fits
        return _fits(budget, "item_placement", payload)

    batches, unsent = placement.plan_batches(budget, positions, items, fits)
    assert unsent == [] and len(batches) > 1
    limit = budget.effective_input_budget_tokens // 2
    for batch in batches:
        payload = placement.placement_payload(positions, items, batch)
        from docreader.tender.authoring_pipeline import _request
        measured = budget.require_request(_request(budget, "item_placement", payload), "item_placement")
        if len(batch) > 1:
            assert measured["input_tokens"] <= limit
    alone = placement.placement_payload(positions, items, ["r1"])
    from docreader.tender.authoring_pipeline import _request
    one = budget.require_request(_request(budget, "item_placement", alone), "item_placement")
    assert one["fits"] is True
    assert any(batch[0] == "r1" for batch in batches)


def test_batches_never_exceed_the_soft_item_cap(monkeypatch):
    monkeypatch.setattr(placement, "PLACEMENT_BATCH_ITEMS", 3)
    positions = placement.placement_positions(outline())
    items = {f"r{n}": item(n) for n in range(1, 8)}
    batches, unsent = placement.plan_batches(RequestBudget(), positions, items, lambda payload: True)
    assert [len(batch) for batch in batches] == [3, 3, 1] and unsent == []


async def test_bug333_items_that_cannot_fit_with_the_catalogue_stay_pending_without_aborting():
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        ids = [row["id"] for row in payload["requirements"]]
        calls.append(ids)
        return validate({"placements": [[identity, "t1"] for identity in ids]})

    def fits(payload):
        # r1's evidence is too long to ever share a request with the position catalogue.
        return all(row["id"] != "r1" for row in payload["requirements"])

    bound, stats = await placement.place_items(
        outline(), [item(n) for n in range(1, 4)], RequestBudget(), None, ask, fits)
    assert calls == [["r2", "r3"]] and stats == {"batches": 1, "failed": 0}
    failed, = [issue for issue in bound["issues"] if issue["code"] == "outline_item_placement_failed"]
    assert failed["rule_refs"] == [ref(1)]

    calls.clear()
    bound, stats = await placement.place_items(
        outline(), [item(n) for n in range(1, 4)], RequestBudget(), None, ask, lambda _payload: False)
    assert calls == [] and stats == {"batches": 0, "failed": 0}
    failed, = [issue for issue in bound["issues"] if issue["code"] == "outline_item_placement_failed"]
    assert failed["rule_refs"] == [ref(1), ref(2), ref(3)]


CHAIN_PAGES = [
    '第四章 合同格式\n无关联关系承诺函\n致：采购人\n承诺人（盖章）：\n',
    '第六章 投标文件格式\n一、投标书\n致：招标人\n投标人名称：________\n二、开标一览表\n总价：\n',
]


async def test_generate_document_runs_the_requirement_chain(tmp_path, monkeypatch):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender import authoring_pipeline
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
        assert [row.get("origin") for row in body["positions"]][-1] == "outside_format"
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
    assert [chapter["title"] for chapter in outline["chapters"]] == ["投标书", "开标一览表", "无关联关系承诺函"]
    resolution = {row["rule_ref"]["item_ref"]["item_id"]: row["resolution"]
                  for row in build_ledger(outline, {"rules": [{"rule_ref": row["rule_ref"]} for row in ledger]})
                  if row["rule_ref"]}
    assert resolution["i3"] == resolution["i4"] == "pending"
    assert resolution["i1"] != "pending" and resolution["i2"] != "pending"


def test_bug332_an_amendment_restating_a_listed_form_is_kept_beside_it_as_a_conflict():
    documents = [*DOCUMENTS, {"document_id": "addendum", "role": "amendment", "pages": [
        "补遗一\n原第六章投标书格式修改如下：\n投标书\n致：采购人（修改后）\n"]}]
    amended = item(1, "submission_requirement", ["投标书"], page=0)
    amended["evidence"] = [{"document_id": "addendum", "page": 0, "quote": "投标书"}]
    result, added_names = placement.add_submission_forms(plan(), documents, [amended], {("tender", 1)})
    assert added_names == []
    added, = [node for node in result["nodes"] if node["id"].startswith("submission-")]
    assert added["name"] == "投标书" and added["heading"] == {"document_id": "addendum", "page": 0, "line": 2}
    conflict, = result["skipped_item_issues"]
    assert conflict["code"] == "outline_directory_format_conflict"
    assert "第 2 页" in conflict["detail"] and "补遗（文档 addendum）第 1 页" in conflict["detail"]

    main_only = item(2, "submission_requirement", ["投标书"], page=0)
    assert placement.add_submission_forms(plan(), documents, [main_only], {("tender", 1)}) == (plan(), [])


async def test_bug332_the_requirement_chain_keeps_an_inline_amendment_form_beside_the_original(tmp_path, monkeypatch):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender import authoring_pipeline
    from docreader.tender.schema import AuthoringInput

    amended = item(1, "submission_requirement", ["投标书"], page=0)
    amended["evidence"] = [{"document_id": "addendum", "page": 0, "quote": "投标书"}]
    monkeypatch.setattr(placement, "_items", lambda _context: [amended, item(2)])

    async def execute(task, request, validate):
        body = json.loads(request["messages"][1]["content"][0]["text"])
        if task == "directory_plan":
            found = {text.strip(): ref for ref, text in body["lines"]}
            return validate({"chapters": [{"heading_refs": [found["一、投标书"]], "name": "投标书"},
                                          {"heading_refs": [found["二、开标一览表"]], "name": "开标一览表"}]})
        assert not [row for row in body["positions"] if row.get("origin")]
        return validate({"placements": [["r1", "document_wide"], ["r2", None]]})

    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput.model_validate({"schema_version": 2, "run_id": "chain", "source_version": 1,
                                              "domain_revision": 1}).model_dump()
    reference = await authoring_pipeline.generate_document(
        artifacts, snapshot, RequestBudget(model="unused"), execute,
        directory_documents=[{"document_id": "tender", "role": "main", "pages": CHAIN_PAGES,
                              "line_units": [[] for _ in CHAIN_PAGES]},
                             {"document_id": "addendum", "role": "amendment",
                              "pages": ["补遗一\n原投标书格式修改如下：\n投标书\n致：采购人（修改后）\n"], "line_units": [[]]}],
        outline_only=True, directory_contract="bid_format_source_outline_v1",
        requirement_chain_contract=placement.REQUIREMENT_CHAIN_CONTRACT)
    outline = artifacts.read_json(reference)
    assert [chapter["title"] for chapter in outline["chapters"]] == ["投标书", "开标一览表", "投标书"]
    codes = [issue["code"] for issue in outline["issues"]]
    assert "outline_directory_format_conflict" in codes
    assert "outline_directory_revision_unreviewed" not in codes and "outline_directory_rejected" not in codes


def test_bug340_decode_placements_normalizes_string_null():
    positions = {"t1": {"title_path": ["投标书"]}}
    decoded = placement.decode_placements(
        {"placements": [["r1", "null"], ["r2", " NULL "], ["r3", "t1"]]},
        ["r1", "r2", "r3"],
        positions,
    )
    assert decoded == {"r1": None, "r2": None, "r3": "t1"}
