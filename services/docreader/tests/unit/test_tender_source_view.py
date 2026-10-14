"""Sending projection must preserve evidence semantics without storage replicas."""

from copy import deepcopy

import pytest

from docreader.tender.evidence.view import host_reviewed_ranges, source_view


def source(range_id, text="source condition", **extra):
    return {"range_id": range_id, "source_range_id": range_id, "text": text,
            "document_id": "main", "document_version": "v1", "unit_id": "native-" + range_id,
            "kind": "paragraph", "start": 0, "end": len(text), "section_id": "section-1",
            "heading_path": "Chapter one / Delivery", "parent_section_id": None,
            "locator": {"page": 1}, "physical_locator": {"page": 1},
            "physical_path": "/native/source", "unused_debug": {}, **extra}


def pack(body, supporting=(), **extra):
    return {"pack_id": "pack-1", "document_id": "main", "document_version": "v1",
            "body": body, "supporting": list(supporting), **extra}


def blank(range_id, row, **extra):
    return source(range_id, "", kind="table_region", empty=True, non_requirement_reason="empty_table_row",
                  table_id="table-1", row_start=row, row_end=row + 1, column_count=3,
                  cells=[{"column": column, "start": 0, "end": 0} for column in range(3)], **extra)


def test_shared_identity_and_heading_preserve_source_union_without_mutating_canonical():
    first, second, context = source("a"), source("b", "other body"), source("c", "complete condition")
    frozen = pack([first, second], [context, deepcopy(first)])
    before = deepcopy(frozen)
    view = source_view(frozen)
    assert frozen == before
    assert view["document_id"] == "main" and view["document_version"] == "v1"
    assert view["sections"] == [{"section_id": "section-1", "heading_path": "Chapter one / Delivery"}]
    assert [(item["range_id"], item["text"]) for item in view["body"] + view["supporting"]] == [
        ("a", "source condition"), ("b", "other body"), ("c", "complete condition")]
    for item in view["body"] + view["supporting"]:
        assert not {"document_id", "document_version", "unit_id", "heading_path", "locator",
                    "physical_locator", "physical_path", "unused_debug", "source_range_id", "start", "end"} & item.keys()


def test_condition_intervals_and_reviewed_targets_survive_text_deduplication():
    text = "Only Lot A. Exact exception applies."
    binding = {"range_id": "condition", "source_range_id": "a", "start": 0, "end": len(text),
               "scope_binding": {"kind": "reviewed_targets", "status": "bound", "complete_sentence": True,
                                 "heading_path": "Chapter one / Delivery", "section_id": "section-1",
                                 "applies_to_range_ids": ["b"], "reviewed_target_range_ids": ["b"]}}
    view = source_view(pack([source("a", text), source("b")], scope_bindings=[binding]))
    assert view["scope_bindings"][0]["scope_binding"]["applies_to_range_ids"] == ["b"]
    assert view["scope_bindings"][0]["scope_binding"]["reviewed_target_range_ids"] == ["b"]
    assert "text" not in view["scope_bindings"][0]
    assert view["body"][0]["source_range_id"] == "a"
    assert view["body"][0]["start"] == 0 and view["body"][0]["end"] == len(text)
    assert sum(item["text"] == text for item in view["body"] + view["supporting"]) == 1


def test_fragment_cell_offsets_replay_partial_fields_without_duplicated_values():
    fragment = source("fragment", " | 30天", source_range_id="native-row", start=12, end=18,
                      kind="table_region", table_id="table-1", row_start=2, row_end=3,
                      column_count=2, text_boundaries={"start": False, "end": True},
                      cells=[{"column": 0, "start": 12, "end": 12, "row_span": 1, "col_span": 1},
                             {"column": 1, "start": 15, "end": 18, "row_span": 2, "col_span": 1}])
    value = source_view(pack([fragment]))["body"][0]
    assert value["text"] == fragment["text"]
    assert value["source_range_id"] == "native-row"
    assert value["cells"][0] == {"column": 0, "start": 12, "end": 12}
    cell = value["cells"][1]
    assert value["text"][cell["start"] - value["start"]:cell["end"] - value["start"]] == "30天"
    assert cell["row_span"] == 2 and "text" not in cell
    assert value["text_boundaries"] == {"start": False, "end": True}


def test_fragment_probe_and_materialized_cells_have_the_same_sending_view():
    value = source("fragment", "value", source_range_id="native-row", start=5, end=10,
                   cell_intervals=[{"column": 0, "text": "prefix", "start": 0, "end": 3},
                                   {"column": 1, "text": "prefixvalue", "start": 3, "end": 10},
                                   {"column": 2, "text": "suffix", "start": 12, "end": 18}])
    materialized = {key: item for key, item in value.items() if key != "cell_intervals"}
    materialized["cells"] = [{"column": 1, "start": 5, "end": 10}]
    assert source_view(pack([value])) == source_view(pack([materialized]))


def test_actual_image_field_association_keeps_resolvable_identity_and_anchor():
    row = source("row", " | ", kind="table_region", image_unit_ids=["native-image"])
    anchor = {"sheet_ordinal": 0, "from": {"row": 1, "column": 2}}
    image = source("image", "literal OCR", kind="image_region", unit_id="native-image", image_anchor=anchor,
                   locator={"compound_parent": {"parent_kind": "table_cell", "section_ordinal": 0,
                                                "table_ordinal": 0, "row_ordinal": 1, "cell_ordinal": 2}})
    view = source_view(pack([row, image]))
    assert "unit_id" not in view["body"][0]
    assert view["body"][1]["unit_id"] == view["body"][0]["image_unit_ids"][0]
    assert view["body"][1]["image_anchor"] == anchor
    assert view["body"][1]["compound_parent"]["cell_ordinal"] == 2


def test_blank_template_groups_only_contiguous_rows_and_retains_merges_once():
    rows = [blank("r1", 1), blank("r2", 2), blank("r4", 4)]
    rows[2]["cells"] = [{"column": 0, "col_span": 2, "row_span": 1, "start": 0, "end": 0},
                        {"column": 2, "start": 0, "end": 0}]
    view = source_view(pack(rows))
    assert view["body"] == []
    assert view["blank_table_templates"] == [{"table_id": "table-1", "section_id": "section-1", "column_count": 3,
        "rows": [{"row_start": 1, "row_end": 3, "range_ids": ["r1", "r2"]},
                 {"row_start": 4, "row_end": 5, "range_ids": ["r4"]}],
        "merges": [{"row": 4, "column": 0, "row_span": 1, "col_span": 2}]}]
    assert not host_reviewed_ranges(pack(rows))


@pytest.mark.parametrize("change", [
    {"cells": [{"column": 0, "formula": "=SUM(A1:A2)", "text": ""}]},
    {"cells": [{"column": 0, "image_anchor": {"image_key": "image-1"}, "text": ""}]},
    {"cells": [{"column": 0, "row_span": 2, "text": ""}]},
    {"cells": [{"column": 0, "text": "partially filled"}, {"column": 1, "text": ""}]},
    {"image_unit_ids": ["image-1"]},
])
def test_empty_labels_cannot_hide_formula_image_vertical_merge_or_partial_fields(change):
    value = {**blank("r1", 1), **change}
    view = source_view(pack([value]))
    assert [item["range_id"] for item in view["body"]] == ["r1"]
    assert "blank_table_templates" not in view


def test_unreviewed_images_remain_sources_and_pending_locations_are_not_source_text():
    image = source("image", "", kind="image_region", empty=True, ocr_status="unresolved",
                   non_requirement_reason="reviewed_decorative_image",
                   image_disposition={"status": "unreviewed", "classification": "decorative"})
    pending = {**source("pending"), "reason": "incomplete_condition_sentence",
               "scope_binding": {"status": "unresolved", "complete_sentence": False, "kind": "unresolved",
                                 "applies_to_range_ids": ["image"]}}
    view = source_view(pack([image], pending_scope_context=[pending]))
    assert view["body"][0]["ocr_status"] == "unresolved"
    assert not host_reviewed_ranges(pack([image]))
    metadata = view["pending_scope_context"][0]
    assert metadata["scope_binding"]["applies_to_range_ids"] == ["image"]
    assert not {"text", "locator", "physical_locator", "physical_path", "document_id", "unit_id"} & metadata.keys()
