"""Exact authoring text selections keep native identities separate from grids."""
from copy import deepcopy

import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring import validate_outline_references
from docreader.tender.authoring_excerpts import (
    bind_source_excerpts, load_source_excerpt, response_row_id, source_excerpt_key,
    whole_source_excerpts,
)
from docreader.tender.authoring_templates import prepare_template_parts
from docreader.tender.evidence.references import fragment, selector
from docreader.tender.schema import ResponseTarget, SourceExcerptRef, WritingResponse, WritingUnit
from test_authoring_templates import create_context, create_source, make_part, outline
from test_structural_packing import grid


def test_source_binder_and_schema_keep_distinct_slices_of_one_native_unit(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "参数甲🙂\n仅适用于标段乙"}])
    context = create_context(artifacts, index_ref, native)
    records, _ = prepare_template_parts(artifacts, context)
    ids = [record["source"]["range_id"] for record in records]
    first, second = [bind_source_excerpts(context, [identity])[0] for identity in ids]
    assert first["source_ref"] == second["source_ref"]
    assert source_excerpt_key(first) != source_excerpt_key(second)
    assert load_source_excerpt(artifacts, first)["text"] == "参数甲🙂"
    assert load_source_excerpt(artifacts, second)["text"] == "仅适用于标段乙"
    value = context.validate(outline([{"kind": "narrative", "purpose": "说明参数及适用条件", "rule_ids": [],
        "range_ids": ids, "response_table": {"rows": [{"range_ids": ids, "rule_ids": []}]}}]))
    target = value["chapters"][0]["targets"][0]
    assert len(target["source_excerpts"]) == 2
    row = target["response_table"]["rows"][0]
    assert row["source_excerpts"] == target["source_excerpts"]
    assert row["row_id"] == response_row_id(target["target_id"], row["source_excerpts"], [])
    assert ResponseTarget.model_validate(target).model_dump(exclude_unset=True) == target
    assert native.raw["units"][0]["text"] == "参数甲🙂\n仅适用于标段乙"


def test_excerpt_reads_native_grid_row_without_cropping_or_returning_the_grid(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"grid": grid(rows=3)}])
    selected = make_part(index_ref, "unit:0", [selector(native.ranges[1])])
    loaded = load_source_excerpt(artifacts, selected)
    assert loaded["text"] == native.ranges[1]["text"]
    assert loaded["grid"] is None and "grid" not in loaded["unit"]
    assert loaded["ranges"][0]["unit_id"] == "unit:0"
    assert len(whole_source_excerpts(artifacts, selected["source_ref"])[0]["selectors"]) == 3


@pytest.mark.parametrize("corruption", ["wrong_unit", "outside", "forged_id", "empty_selectors"])
def test_excerpt_does_not_fall_back_to_whole_source_on_invalid_selection(tmp_path, corruption):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "参数甲"}, {"text": "条件乙"}])
    selected = make_part(index_ref, "unit:0", [selector(fragment(native.ranges[0], 0, 2))])
    if corruption == "wrong_unit":
        selected["source_ref"]["unit_id"] = "unit:1"
    elif corruption == "outside":
        selected["selectors"][0]["end"] = 100
    elif corruption == "forged_id":
        selected["selectors"][0]["range_id"] = "invented"
    else:
        selected["selectors"] = []
    with pytest.raises((ArtifactIntegrityError, ValueError)):
        load_source_excerpt(artifacts, selected)


def test_response_rows_bind_only_current_short_references_and_do_not_rewrite_source(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "跨页要求前半"}, {"text": "后半及独立证明"}])
    context = create_context(artifacts, index_ref, native)
    ids = [row["range_id"] for row in native.ranges]
    raw = outline([{"kind": "narrative", "purpose": "逐项响应", "rule_ids": [], "range_ids": [],
                    "response_table": {"rows": [{"range_ids": ids}]}}])
    value = context.validate(raw)
    row = value["chapters"][0]["targets"][0]["response_table"]["rows"][0]
    assert [part["source_ref"]["unit_id"] for part in row["source_excerpts"]] == ["unit:0", "unit:1"]
    assert "text" not in row
    parsed = deepcopy(raw)
    parsed["constraint_rule_ids"], parsed["constraint_range_ids"], parsed["search_intents"] = [], [], []
    parsed["chapters"][0]["targets"][0]["template_part_ids"] = []
    validate_outline_references(parsed, rule_ids=[], source_ids=ids, template_part_ids=[])
    parsed["chapters"][0]["targets"][0]["response_table"]["rows"][0]["range_ids"] = ["absent"]
    from docreader.llm.request import ModelContractError
    with pytest.raises(ModelContractError):
        validate_outline_references(parsed, rule_ids=[], source_ids=ids, template_part_ids=[])


def test_row_identity_includes_selected_rules_and_fill_contract_is_not_native_grid(tmp_path):
    _, index_ref, native = create_source(tmp_path, [{"text": "技术参数"}])
    excerpt = make_part(index_ref, "unit:0", [selector(native.ranges[0])])
    assert response_row_id("target", [excerpt], []) != response_row_id(
        "target", [excerpt], [{"item_ref": {"artifact_ref": index_ref, "item_id": "rule"}}])
    response = WritingResponse.model_validate({"blocks": [], "row_fills": [
        {"row_id": "q1", "text": "有据响应", "support_ids": ["k1"]}]})
    assert response.row_fills[0].row_id == "q1"
    saved = WritingUnit.model_validate({"task_id": "w1", "chapter_id": "c1", "target_ids": ["t1"],
        "status": "generated", "row_fills": [{"row_id": "row-1", "text": "有据响应",
            "material_ids": ["material"], "source_excerpts": [excerpt]}]})
    assert saved.row_fills[0].source_excerpts[0].source_ref.unit_id == "unit:0"
    with pytest.raises(ValidationError):
        SourceExcerptRef.model_validate({"source_ref": excerpt["source_ref"], "selectors": []})
