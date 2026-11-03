"""Source-only retrieval follows complete exact selections, including their conditions."""

import json

import pytest

from docreader.tender.authoring_coverage import build_ledger, plan_queries
from docreader.tender.authoring_pipeline import plan_writing_tasks
from docreader.tests.unit.test_authoring_coverage import outline_for, queries_of, target, tasks_for


def excerpt(identity, unit_id="same-native-unit"):
    digest = "b" * 64
    return {"source_ref": {"source_index_ref": {
        "sha256": digest, "relative_path": f"{digest[:2]}/{digest}", "size": 1,
        "media_type": "application/json"}, "unit_id": unit_id},
        "selectors": [{"range_id": identity, "source_range_id": identity, "start": 0, "end": 10}]}


def text_map(*pairs):
    return {json.dumps(ref, ensure_ascii=False, sort_keys=True, separators=(",", ":")): text
            for ref, text in pairs}


def source_position(identity, *excerpts, **extra):
    sources = list({json.dumps(ref["source_ref"], sort_keys=True): ref["source_ref"] for ref in excerpts}.values())
    return target(identity, [], source_refs=sources, source_excerpts=list(excerpts), **extra)


def intent(query, *excerpts):
    return {"query": query, "rule_refs": [],
            "range_ids": [part["range_id"] for ref in excerpts for part in ref["selectors"]],
            "source_refs": [ref["source_ref"] for ref in excerpts]}


def planned_for(*positions, intents=(), texts=None, scope=None):
    outline = outline_for(*positions, intents=intents)
    return plan_queries(outline, tasks_for(*positions), build_ledger(outline), excerpt_texts=texts, scope=scope)


def test_exact_intent_never_flows_to_sibling_selection_in_same_native_unit():
    left, right = excerpt("left"), excerpt("right")
    planned = planned_for(source_position("left", left), source_position("right", right),
        intents=[intent("已有左侧精确查询", left)], texts=text_map((left, "左要求"), (right, "右要求")))
    assert {(row["target_id"], row["query"]) for row in planned["search"]} == {
        ("left", "已有左侧精确查询"), ("right", "右要求")}
    assert planned["gaps"] == []


def test_broader_intent_is_not_reused_for_only_one_of_its_ranges():
    own, sibling = excerpt("own"), excerpt("sibling")
    planned = planned_for(source_position("own", own), intents=[intent("两项综合查询", own, sibling)],
        texts=text_map((own, "本位置完整要求")))
    assert queries_of(planned) == ["本位置完整要求"]


def test_partial_intent_does_not_hide_remaining_ranges_but_complete_partition_is_usable():
    condition, own = excerpt("condition"), excerpt("own")
    position = source_position("whole", condition, own)
    texts = text_map((condition, "适用于版本V2"), (own, "支持离线工作"))
    partial = planned_for(position, intents=[intent("离线能力", own)], texts=texts)
    assert queries_of(partial) == ["适用于版本V2\n支持离线工作"]
    complete = planned_for(position,
        intents=[intent("版本依据", condition), intent("离线能力", own)], texts=texts)
    assert queries_of(complete) == ["版本依据", "离线能力"]


def test_coarse_intent_only_remains_compatible_when_target_also_has_no_exact_selection():
    own = excerpt("own")
    coarse = {"query": "旧范围查询", "rule_refs": [], "range_ids": [], "source_refs": [own["source_ref"]]}
    precise = planned_for(source_position("precise", own), intents=[coarse], texts=text_map((own, "完整精确原文")))
    assert queries_of(precise) == ["完整精确原文"]
    legacy = target("legacy", [], source_refs=[own["source_ref"]])
    compatible = planned_for(legacy, intents=[coarse])
    assert queries_of(compatible) == ["旧范围查询"]


def test_row_intent_also_requires_its_shared_conditions_and_rejects_coarse_source_alias():
    condition, own = excerpt("condition"), excerpt("own")
    position = source_position("table", own, response_table={"context_source_excerpts": [condition], "rows": [
        {"row_id": "row", "rule_refs": [], "source_excerpts": [own]}]})
    texts = text_map((condition, "共同版本V2"), (own, "支持离线工作"))
    precise = planned_for(position, intents=[intent("只有行正文的查询", own)], texts=texts)
    assert queries_of(precise) == ["共同版本V2\n支持离线工作"]
    coarse = {"query": "同单元的其他查询", "rule_refs": [], "range_ids": [], "source_refs": [own["source_ref"]]}
    assert queries_of(planned_for(position, intents=[coarse], texts=texts)) == ["共同版本V2\n支持离线工作"]


@pytest.mark.parametrize("shape", ["narrative", "row"])
@pytest.mark.parametrize("unreadable", [None, ""])
def test_missing_selected_condition_never_produces_a_partial_raw_query(shape, unreadable):
    condition, own = excerpt("condition"), excerpt("own")
    if shape == "row":
        position = source_position("table", own, response_table={"context_source_excerpts": [condition], "rows": [
            {"row_id": "row", "rule_refs": [], "source_excerpts": [own]}]})
    else:
        position = source_position("narrative", condition, own)
    texts = text_map((own, "只有这段读取成功"))
    if unreadable is not None:
        texts.update(text_map((condition, unreadable)))
    planned = planned_for(position, texts=texts)
    assert planned["search"] == []
    assert {gap["code"] for gap in planned["gaps"]} == {"knowledge_query_missing"}


def test_source_only_raw_query_keeps_original_order_deduplicates_exact_refs_and_has_byte_limit():
    condition, own = excerpt("condition"), excerpt("own")
    position = source_position("body", condition, own, condition)
    planned = planned_for(position, texts=text_map((condition, "  版本V2\n"), (own, "支持离线工作")))
    assert queries_of(planned) == ["  版本V2\n\n支持离线工作"]
    huge = planned_for(position, texts=text_map((condition, "版本V2"), (own, "甲" * 10000)))
    assert huge["search"] == []
    assert {gap["code"] for gap in huge["gaps"]} == {"knowledge_query_too_large"}


def test_same_raw_query_shares_one_operation_but_keeps_both_targets_and_scope_identity():
    own = excerpt("own")
    positions = [source_position(identity, own) for identity in ("first", "second")]
    texts = text_map((own, "同一完整要求"))
    planned = planned_for(*positions, texts=texts, scope={"product_id": "p", "version_id": "v1"})
    assert len(planned["queries"]) == 1
    assert {row["target_id"] for row in planned["search"]} == {"first", "second"}
    changed = planned_for(*positions, texts=texts, scope={"product_id": "p", "version_id": "v2"})
    assert planned["queries"][0]["query_id"] != changed["queries"][0]["query_id"]


def test_pure_navigation_is_skipped_while_content_with_a_reference_searches_its_own_sources():
    own = excerpt("own")
    navigation = target("navigation", [], related_target_ids=["body"])
    body = source_position("body", own, related_target_ids=["navigation"])
    manual = source_position("price", own, manual_fill=True)
    tender = source_position("tender", own, support_origins=["tender"])
    outline = outline_for(navigation, body, manual, tender)
    outline["chapters"][0].update(chapter_id="chapter", title="业务正文")
    tasks = plan_writing_tasks(outline)
    planned = plan_queries(outline, tasks, build_ledger(outline), excerpt_texts=text_map((own, "本位置实际应答要求")))
    assert queries_of(planned) == ["本位置实际应答要求"]
    assert [row["target_id"] for row in planned["search"]] == ["body"]
    assert planned["gaps"] == []


def test_default_knowledge_permission_without_actual_exact_source_does_not_invent_query():
    position = target("empty", [])
    position.pop("support_origins")
    planned = planned_for(position)
    assert planned["search"] == []
    assert {gap["code"] for gap in planned["gaps"]} == {"knowledge_query_missing"}


def test_excerpt_loader_includes_narrative_selections_once_and_preserves_failure(monkeypatch):
    from docreader.tender import authoring_pipeline as pipeline

    own, condition = excerpt("own"), excerpt("condition")
    position = source_position("body", condition, own, condition)
    calls = []

    def load(artifacts, selected):
        calls.append(selected)
        if selected == condition:
            raise ValueError("unreadable selected condition")
        return {"text": "完整正文"}

    monkeypatch.setattr(pipeline, "load_source_excerpt", load)
    texts = pipeline._excerpt_texts(None, outline_for(position))
    assert calls == [condition, own]
    assert texts == text_map((condition, ""), (own, "完整正文"))
