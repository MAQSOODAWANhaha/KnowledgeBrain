"""A revision keeps exact tender support and maps answers to frozen response rows."""

from copy import deepcopy
import json

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring_pipeline import bind_writing_response
from docreader.tender.authoring_revision import _has_content, _previous_content, revise_units


def reference():
    return {"source_index_ref": {"sha256": "a" * 64, "relative_path": "aa/" + "a" * 64,
                                 "size": 0, "media_type": "application/json"}, "unit_id": "same-unit"}


def excerpt(start, end):
    return {"source_ref": reference(), "selectors": [{"range_id": f"fragment-{start}", "source_range_id": "native",
                                                        "start": start, "end": end}]}


def test_previous_content_retains_distinct_same_unit_excerpt_supports():
    first, second = excerpt(0, 8), excerpt(8, 16)
    target = {"target_id": "actual-target", "source_refs": [reference()], "kind": "narrative"}
    task = {"targets": [target]}
    payload = {"targets": [{"target_id": "t1", "support_ids": ["s1", "s2"]}], "supports": [
        {"support_id": "s1", "origin": "tender"}, {"support_id": "s2", "origin": "tender"}]}
    bindings = {"s1": {"origin": "tender", "source_ref": reference(), "source_excerpt": first},
                "s2": {"origin": "tender", "source_ref": reference(), "source_excerpt": second}}
    unit = {"blocks": [{"kind": "paragraph", "text": text, "target_ids": ["actual-target"],
                        "material_ids": [], "source_refs": [reference()], "source_excerpts": [source]}
                       for text, source in (("甲段项目事实。", first), ("乙段条件。", second))]}
    before = deepcopy(unit)
    previous = _previous_content(task, unit, payload, {"s1": first, "s2": second}, bindings)
    assert previous is not None
    assert [block["support_ids"] for block in previous["blocks"]] == [["s1"], ["s2"]]
    assert all(not {"source_refs", "source_excerpts", "material_ids"} & block.keys()
               for block in previous["blocks"])
    assert unit == before


def test_excerpt_support_cannot_be_replaced_by_another_slice_or_whole_unit():
    first, second = excerpt(0, 8), excerpt(8, 16)
    task = {"targets": [{"target_id": "actual-target"}]}
    payload = {"targets": [{"target_id": "t1"}], "supports": [{"support_id": "s1", "origin": "tender"}]}
    unit = {"blocks": [{"kind": "paragraph", "text": "已有事实。", "target_ids": ["actual-target"],
                        "source_refs": [reference()], "source_excerpts": [first]}]}
    for selected in (second, reference()):
        binding = {"origin": "tender", "source_ref": reference()}
        if selected is second:
            binding["source_excerpt"] = second
        assert _previous_content(task, unit, payload, {"s1": selected}, {"s1": binding}) is None


def test_previous_row_answer_uses_current_short_row_reference_without_recopying_target_identity():
    task = {"targets": [{"target_id": "actual-target", "response_table": {"rows": [
        {"row_id": "stable-a"}, {"row_id": "stable-b"}]}}]}
    payload = {"targets": [{"target_id": "t1"}], "response_rows": [{"row_id": "q1"}, {"row_id": "q2"}],
               "supports": [{"support_id": "k4", "origin": "knowledge"}]}
    bindings = {"k4": {"origin": "knowledge", "material_id": "knowledge-version"}}
    unit = {"row_fills": [{"row_id": "stable-b", "text": "资料支持的实际功能。", "material_ids": ["knowledge-version"],
                            "source_refs": [], "source_excerpts": []}]}
    previous = _previous_content(task, unit, payload, {}, bindings)
    assert previous["row_fills"] == [{"row_id": "q2", "text": "资料支持的实际功能。",
                                      "support_ids": ["k4"], "content_id": "r1"}]
    assert _has_content(unit)


def test_unbound_previous_row_stops_revision_without_erasing_existing_answer():
    task = {"targets": [{"target_id": "actual-target", "response_table": {"rows": [{"row_id": "new-row"}]}}]}
    payload = {"targets": [{"target_id": "t1"}], "response_rows": [{"row_id": "q1"}], "supports": []}
    unit = {"row_fills": [{"row_id": "old-row", "text": "保留的原稿应答。", "material_ids": [], "source_refs": []}]}
    assert _previous_content(task, unit, payload, {}, {}) is None


async def test_row_only_revision_is_adoptable_and_accepted_cache_skips_tighter_admission(tmp_path):
    task = {"task_id": "writing", "chapter_id": "chapter", "title_path": ["技术响应"], "targets": [
        {"target_id": "actual-target", "kind": "narrative", "purpose": "按要求逐项应答", "rule_refs": [],
         "source_refs": [], "response_table": {"rows": [{"row_id": "stable-a"}]}}]}
    material = {"material_id": "knowledge", "title": "产品说明", "text": "支持双机热备并可自动切换。",
                "usage": "reference", "attachment": False, "original_ref": None, "locator": {}}
    payload = {"targets": [{"target_id": "t1", "support_ids": ["k1"]}],
               "response_rows": [{"row_id": "q1", "target_id": "t1", "support_ids": []}],
               "supports": [{"support_id": "k1", "origin": "knowledge", "text": material["text"], "target_ids": ["t1"]}],
               "long_source": "来源" * 1000}
    old = {"schema_version": 2, "task_id": "writing", "chapter_id": "chapter", "target_ids": ["actual-target"],
           "status": "generated", "blocks": [], "cell_fills": [], "row_fills": [
               {"row_id": "stable-a", "text": "支持双机热备。", "material_ids": ["knowledge"],
                "source_refs": [], "source_excerpts": []}], "attachments": [], "gaps": []}
    calls = []

    class CachedExecutor:
        async def replay(self, operation, request, validate):
            calls.append(operation)
            content = json.loads(request["messages"][1]["content"][0]["text"])
            assert content["previous_content"]["row_fills"][0]["row_id"] == "q1"
            return True, validate({"blocks": [], "row_fills": [
                {"row_id": "q1", "text": material["text"], "support_ids": ["k1"]}]})

        async def __call__(self, *_args):
            raise AssertionError("accepted revision must not be sent again")

    revised, old_refs, diagnostics = await revise_units(
        ArtifactStore(tmp_path), RequestBudget(context_tokens=128, model="fixture", prompts={}), CachedExecutor(),
        [task], [old], [{"task_ids": ["writing"], "can_revise": True, "code": "imprecise", "detail": "补充切换功能。"}],
        lambda _: (payload, {}, {"k1": {"origin": "knowledge", "material_id": "knowledge"}}),
        lambda selected, raw: bind_writing_response(selected, raw, {}, [material], support_views=payload["supports"]))
    assert calls == ["authoring_write"] and not diagnostics and len(old_refs) == 1
    assert revised[0]["status"] == "generated"
    assert revised[0]["row_fills"][0]["row_id"] == "stable-a"
    assert revised[0]["row_fills"][0]["text"] == material["text"]
    assert old["row_fills"][0]["text"] == "支持双机热备。"
