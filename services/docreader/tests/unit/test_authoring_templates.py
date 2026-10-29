"""Native source boundaries and template identities stay with the host."""
from copy import deepcopy

import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.llm.request import ModelContractError
from docreader.tender.authoring import _OutlineContext
from docreader.tender.evidence.references import SourceIndex, fragment, selector
from docreader.tender.schema import AuthoringOutline, OutlineAssemblyTarget, ResponseTarget, TemplatePartRef
from docreader.tender.authoring_templates import (
    load_template_part, prepare_template_parts, template_part_key,
)

from test_structural_packing import grid, source


def create_source(tmp_path, units):
    artifacts = ArtifactStore(tmp_path)
    native = source(units)
    native = SourceIndex({**native.document, "original_ref": artifacts.put_bytes(b"native-fixture")}, native.raw)
    index_ref = artifacts.put_json(native.descriptor(artifacts.put_json(native.raw)))
    return artifacts, index_ref, native


def create_context(artifacts, index_ref, native):
    frozen = artifacts.put_json({"documents": [{**native.document, "source_index_ref": index_ref,
                                              "role": "main"}]})
    return _OutlineContext(artifacts, {"schema_version": 2, "run_id": "templates",
        "source_version": 1, "domain_revision": 1, "frozen_ref": frozen})


def make_part(index_ref, unit_id, selections=()):
    return {"source_ref": {"source_index_ref": index_ref, "unit_id": unit_id},
            "selectors": list(selections)}


def form(*, ranges=(), parts=()):
    return {"kind": "form", "purpose": "填写并提交此份声明", "rule_ids": [],
            "range_ids": list(ranges), "template_part_ids": list(parts)}


def outline(targets=(), **extra):
    return {"schema_version": 2, "chapters": [{"title": "声明", "children": [], "targets": list(targets)}],
            "findings": [], **extra}


def test_historical_targets_do_not_upgrade_context_references_into_templates(tmp_path):
    _, index_ref, _ = create_source(tmp_path, [{"text": "评审时检查声明。"}])
    value = {"target_id": "t1", "kind": "form", "purpose": "提交声明", "rule_refs": [],
             "source_refs": [{"source_index_ref": index_ref, "unit_id": "unit:0"}]}
    assert ResponseTarget.model_validate(value).template_refs == []
    assert OutlineAssemblyTarget.model_validate({"kind": "form", "purpose": "提交声明",
                                                 "target_ids": ["t1"]}).template_part_ids == []
    with pytest.raises(ValidationError):
        TemplatePartRef.model_validate({"source_ref": value["source_refs"][0]})


def test_exact_text_part_preserves_unicode_and_does_not_include_another_declaration(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "声明甲🙂：____\n声明乙：____"}])
    base = native.ranges[0]
    selected = selector(fragment(base, base["text"].index("声明乙"), base["end"]))
    loaded = load_template_part(artifacts, make_part(index_ref, "unit:0", [selected]))
    assert loaded["text"] == loaded["unit"]["text"] == "声明乙：____"
    assert loaded["grid"] is None
    assert native.raw["units"][0]["text"] == "声明甲🙂：____\n声明乙：____"


@pytest.mark.parametrize("corruption", ["wrong_unit", "outside", "forged_id", "empty"])
def test_invalid_selector_never_falls_back_to_a_whole_unit(tmp_path, corruption):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "甲声明"}, {"text": "乙声明"}])
    selected = selector(fragment(native.ranges[0], 0, 2))
    unit_id = "unit:0"
    if corruption == "wrong_unit":
        unit_id = "unit:1"
    elif corruption == "outside":
        selected["end"] = native.ranges[0]["end"] + 1
    elif corruption == "forged_id":
        selected["range_id"] = "fragment-invented"
    else:
        selected["end"] = selected["start"]
    with pytest.raises((ArtifactIntegrityError, ValueError)):
        load_template_part(artifacts, make_part(index_ref, unit_id, [selected]))


def test_explicit_whole_grid_has_original_coordinates_without_duplicate_flattened_text(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "表格内容副本", "grid": grid(rows=5)}])
    part = make_part(index_ref, "unit:0")
    loaded = load_template_part(artifacts, part)
    assert loaded["text"] == loaded["unit"]["text"] == ""
    assert loaded["grid"] == native.raw["units"][0]["grid"]
    assert loaded["unit"]["locator"] == native.raw["units"][0]["locator"]
    with pytest.raises((ArtifactIntegrityError, ValueError)):
        load_template_part(artifacts, make_part(index_ref, "unit:0", [selector(native.ranges[0])]))


def test_part_key_uses_exact_native_selection_without_duplicating_locator_metadata(tmp_path):
    _, index_ref, native = create_source(tmp_path, [{"text": "声明甲\n声明乙"}])
    selected = selector(fragment(native.ranges[0], 0, 3))
    first = make_part(index_ref, "unit:0", [selected])
    second = deepcopy(first)
    second["selectors"][0]["reason"] = "same frozen selection"
    assert template_part_key(first) == template_part_key(second)
    assert template_part_key(first) != template_part_key(make_part(index_ref, "unit:0"))


def test_prepare_exposes_paragraph_parts_without_resending_the_whole_unit(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "声明甲：____\n声明乙：____"}])
    ctx = create_context(artifacts, index_ref, native)
    records, views = prepare_template_parts(artifacts, ctx)
    sources = [record["source"] for record in records]
    assert [s["text"] for s in sources] == ["声明甲：____", "声明乙：____"]
    assert len(views) == 2 and len(ctx.template_parts) == 2
    assert [load_template_part(artifacts, ctx.template_parts[v["part_id"]])["text"] for v in views] == [
        "声明甲：____", "声明乙：____"]
    assert all(v["range_ids"] == [s["range_id"]] for v, s in zip(views, sources))
    assert all(s["range_id"] in ctx.sources for s in sources)
    assert all(ctx.range_roots[s["range_id"]] == native.ranges[0]["range_id"] for s in sources)


def test_complete_grid_records_are_indivisible_and_do_not_repeat_any_source_row(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"grid": grid(rows=5)}, {"text": "正文说明"}])
    ctx = create_context(artifacts, index_ref, native)
    original_records = [{"source": {"range_id": row["range_id"], "text": row["text"],
        "kind": row["kind"], "role": "main"}, "rules": [{"rule_id": "e1"}]} for row in native.ranges]
    records, views = prepare_template_parts(artifacts, ctx, original_records)
    assert len(records) == 2
    assert [s["range_id"] for s in records[0]["sources"]] == [r["range_id"] for r in native.ranges[:-1]]
    assert records[0]["rules"] == [{"rule_id": "e1"}]
    table = next(v for v in views if not ctx.template_parts[v["part_id"]]["selectors"])
    assert table["range_ids"] == [row["range_id"] for row in native.ranges[:-1]]


def test_paragraph_preparation_keeps_rules_after_leading_blanks_and_handles_sources_records(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "\n声明甲\n声明乙"}, {"text": "\n\n"}])
    ctx = create_context(artifacts, index_ref, native)
    original = [{"sources": [{"range_id": row["range_id"], "text": row["text"], "kind": "section"}],
                 "rules": [{"rule_id": f"e{number}"}]} for number, row in enumerate(native.ranges)]
    records, views = prepare_template_parts(artifacts, ctx, original)
    assert len(views) == 2
    assert [record["source"]["text"] for record in records[:2]] == ["声明甲", "声明乙"]
    assert all("sources" not in record for record in records[:2])
    assert records[0]["rules"] == [{"rule_id": "e0"}]
    assert records[-1] == original[-1]


def test_context_binds_only_selected_parts_and_leaves_context_only_forms_pending(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "评审检查说明"}, {"text": "声明甲\n声明乙"}])
    ctx = create_context(artifacts, index_ref, native)
    _, views = prepare_template_parts(artifacts, ctx)
    selected = views[-1]["part_id"]
    value = ctx.validate(outline([form(ranges=[native.ranges[0]["range_id"]], parts=[selected, selected]),
                                   form(ranges=[native.ranges[0]["range_id"]])]))
    first, second = value["chapters"][0]["targets"]
    assert first["template_refs"] == [ctx.template_parts[selected]]
    assert second["template_refs"] == []
    assert first["source_refs"] == second["source_refs"]
    assert any(issue["code"] == "outline_template_missing" and issue["target_id"] == second["target_id"]
               for issue in value["issues"])
    assert AuthoringOutline.model_validate(value).model_dump(exclude_unset=True) == value
    with pytest.raises(ModelContractError):
        ctx.validate(outline([form(parts=["part-not-provided"])]))


def test_source_only_submission_constraints_remain_read_only_references(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "分为商务、技术及报价三卷。"}])
    ctx = create_context(artifacts, index_ref, native)
    value = ctx.validate(outline(constraint_range_ids=[native.ranges[0]["range_id"]] * 2))
    assert value["submission_refs"] == [{"source_index_ref": index_ref, "unit_id": "unit:0"}]
    assert value["constraint_rule_refs"] == []
    with pytest.raises(ModelContractError):
        ctx.validate(outline(constraint_range_ids=["not-provided"]))
