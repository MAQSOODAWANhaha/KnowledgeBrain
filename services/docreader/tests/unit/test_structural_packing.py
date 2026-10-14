"""Structural boundaries and full-request admission, using deterministic providers."""
from __future__ import annotations

from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.evidence.planner import make_packs
from docreader.tender.evidence.references import EvidenceRepository, SourceIndex
from docreader.tender.requirements.extract import extract_payload


def source(specs, *, document_id="document"):
    units, identities = [], []
    for number, spec in enumerate(specs):
        key = f"unit:{number}"
        units.append({"unit_id": key, "kind": "table" if spec.get("grid") else "section", "ordinal": number,
                      "text": spec.get("text", ""), "grid": spec.get("grid"),
                      "locator": {"locator_kind": "document", "section_ordinal": number,
                                  "page_ordinal": number, "heading_path": spec.get("heading", "")},
                      "physical_path": f"native:unit:{number}", "header_cells": spec.get("headers", [])})
        identities.append({"unit_id": key, "section_id": spec.get("section", "section"),
                           "parent_section_id": spec.get("parent"), "heading_path": spec.get("heading", ""),
                           "physical_locator": units[-1]["locator"]})
    document = {"document_id": document_id, "document_version": "1", "original_ref": {"sha256": "1" * 64}}
    raw = {"schema_version": 2, "units": units, "source_contract": {"units": identities, "page_manifest": []},
           "images": {}, "ocr": {}}
    return SourceIndex(document, raw)


def budget(capacity=8000):
    return RequestBudget(context_tokens=capacity, output_reserve=0, prompts={"extract": "Read every source range and its support."})


def materialize(index, pack):
    return {**pack, "body": [index.materialize(item) for item in pack["body"]],
            "supporting": [index.materialize(item) for item in pack["supporting"]]}


def assert_coverage(index, packs):
    intervals = defaultdict(list)
    ids = []
    for pack in packs:
        for item in pack["body"]:
            intervals[item["source_range_id"]].append((item["start"], item["end"]))
            ids.append(item["range_id"])
    assert len(ids) == len(set(ids))
    assert set(intervals) == set(index.by_id)
    for identity, spans in intervals.items():
        cursor = 0
        for start, end in sorted(spans):
            assert start == cursor
            cursor = end
        assert cursor == index.by_id[identity]["end"]


def test_complete_chapter_above_old_character_limits_is_not_forced_to_split():
    text = "An ordinary proof is required. " * 1000
    index = source([{"text": text, "heading": "Requirements"}])
    assert len(index.ranges) == 1 and len(text) > 12000
    for capacity in (131072, 262144):
        packs, stats = make_packs(index, budget(capacity))
        assert len(packs) == 1 and stats["probe_count"] == 2
        assert packs[0]["planning_details"]["budget"]["fits"]
        assert_coverage(index, packs)


def test_parent_direct_body_same_named_sections_and_unheaded_sources_keep_identity():
    index = source([
        {"text": "Untitled preface", "section": None},
        {"text": "Parent direct obligations.", "section": "parent", "heading": "Common"},
        {"text": "First child obligations.", "section": "child-one", "heading": "Common > Detail", "parent": "parent"},
        {"text": "Second child obligations.", "section": "child-two", "heading": "Common > Detail", "parent": "parent"},
    ])
    packs, _ = make_packs(index, budget(20000))
    assert len(packs) == 1
    rendered = materialize(index, packs[0])
    assert [item["section_id"] for item in rendered["body"]] == [None, "parent", "child-one", "child-two"]
    assert rendered["supporting"] == []
    assert_coverage(index, packs)


def test_text_split_only_after_full_rejection_preserves_unicode_and_final_links():
    text = "必须提交证明🙂，保留完整来源。\n" * 500
    index = source([{"text": text, "heading": "Proof"}])
    admission = budget(6500)
    packs, _ = make_packs(index, admission)
    assert len(packs) > 1
    assert_coverage(index, packs)
    body = [entry for pack in packs for entry in pack["body"]]
    assert ''.join(index.materialize(entry)["text"] for entry in body) == text
    for ordinal, entry in enumerate(body):
        assert entry.get("previous_fragment_id") == (body[ordinal - 1]["range_id"] if ordinal else None)
        assert entry.get("next_fragment_id") == (body[ordinal + 1]["range_id"] if ordinal + 1 < len(body) else None)
    for pack in packs:
        assert not pack["issues"]
        assert admission.measure("extract", extract_payload(materialize(index, pack)))["fits"]


def grid(rows=16, header_size=8):
    cells = [{"row": row, "column": column, "text": (
        f"Header{row}{column}" * header_size if row < 2 else f"Proof{row}{column}" * 14),
        "row_span": 3 if row == 2 and column == 0 else 1, "col_span": 1}
        for row in range(rows) for column in range(2) if not (row in (3, 4) and column == 0)]
    return {"row_count": rows, "column_count": 2, "cells": cells}


def test_multilevel_headers_vertical_merge_and_row_coverage_survive_budget_split():
    index = source([{"section": "forms", "heading": "Forms", "grid": grid(),
                     "headers": [{"row": row, "column": column, "role": "column_header"}
                                 for row in (0, 1) for column in (0, 1)]}])
    admission = budget(6500)
    packs, _ = make_packs(index, admission)
    assert len(packs) > 1
    assert_coverage(index, packs)
    supported = []
    for pack in packs:
        rendered = materialize(index, pack)
        assert admission.measure("extract", extract_payload(rendered))["fits"]
        all_rows = {item["row_start"] for item in [*rendered["body"], *rendered["supporting"]]}
        assert {0, 1} <= all_rows
        if any(item["row_start"] in (3, 4) for item in rendered["body"]):
            assert 2 in all_rows
        supported.extend(item["reason"] for item in rendered["supporting"])
    assert "header" in supported


def test_minimum_body_and_required_header_overflow_is_explicit_and_bounded():
    native = grid(rows=3, header_size=1000)
    index = source([{"grid": native, "headers": [{"row": 0, "column": 0}, {"row": 1, "column": 0}]}])
    packs, stats = make_packs(index, budget(6000))
    assert any(issue["code"] == "indivisible_source_context_exceeded" for pack in packs for issue in pack["issues"])
    assert len(packs) < 12 and stats["probe_count"] < 40
    assert_coverage(index, packs)


class MeteredBudget(RequestBudget):
    def __init__(self, capacity=10000):
        super().__init__(context_tokens=capacity, output_reserve=0, prompts={"extract": "Read."})
        self.source_characters = 0
        self.measurements = 0

    def measure(self, task, payload, images=()):
        self.source_characters += sum(len(item["text"]) for item in payload["body"])
        self.measurements += 1
        return super().measure(task, payload, images)


def test_long_single_document_preparation_reuses_weight_hint_at_2x_and_4x():
    observations = []
    for count in (32, 64, 128):
        index = source([{"text": "Ordinary requirement. " * 40, "section": f"section-{number}"}
                        for number in range(count)])
        admission = MeteredBudget()
        packs, _ = make_packs(index, admission)
        assert all(pack["planning_details"]["budget"]["fits"] for pack in packs)
        assert_coverage(index, packs)
        observations.append((admission.source_characters, admission.measurements))
    assert observations[2][0] < observations[0][0] * 5
    assert observations[2][1] < observations[0][1] * 5


def test_persistent_pack_is_only_refs_and_parallel_repository_loads_raw_once(tmp_path, monkeypatch):
    index = source([{"text": "Source obligation. " * 500}])
    artifacts = ArtifactStore(tmp_path)
    raw_ref = artifacts.put_json(index.raw)
    source_ref = artifacts.put_json(index.descriptor(raw_ref))
    packs, _ = make_packs(index, budget(6500))
    refs = [artifacts.put_json({**pack, "raw_ref": raw_ref, "source_index_ref": source_ref}) for pack in packs]
    def check(value):
        if isinstance(value, dict):
            assert not {"text", "grid", "cells", "base64"}.intersection(value)
            for item in value.values():
                check(item)
        elif isinstance(value, list):
            for item in value:
                check(item)
    for ref in refs:
        check(artifacts.read_json(ref))
    original = artifacts.read_json
    reads = []
    def read(ref):
        reads.append(ref["sha256"])
        return original(ref)
    monkeypatch.setattr(artifacts, "read_json", read)
    repository = EvidenceRepository(artifacts)
    with ThreadPoolExecutor(max_workers=4) as pool:
        rendered = list(pool.map(repository.load_pack, refs))
    assert reads.count(raw_ref["sha256"]) == 1
    assert reads.count(source_ref["sha256"]) == 1
    assert [pack["body"] for pack in rendered] == [materialize(index, pack)["body"] for pack in packs]
    assert deepcopy(index.raw) == index.raw


def test_support_overlap_is_materialized_once_while_retaining_complete_condition():
    from docreader.tender.evidence.packing import SupportingIndex
    from docreader.tender.evidence.references import fragment
    condition = '仅第一标段适用。'
    index = source([{'text': condition + '后续正文。'}, {'text': '必须提交证明。'}])
    first, later = index.ranges
    body = [fragment(first, 4, first['end']), later]
    supporting, _, _, _ = SupportingIndex(index.ranges).select(body)
    spans = sorted((entry['start'], entry['end']) for entry in [*body, *supporting]
                   if entry['source_range_id'] == first['range_id'])
    assert spans[0][0] == 0
    assert all(left[1] == right[0] for left, right in zip(spans, spans[1:]))
    assert spans[-1][1] == first['end']
    assert any(entry.get('scope_binding', {}).get('applies_to_range_ids') == [later['range_id']]
               for entry in supporting)


def test_giant_single_cell_splits_after_rejection_with_header_and_unicode_coverage():
    native_grid = {'row_count': 2, 'column_count': 1, 'cells': [
        {'row': 0, 'column': 0, 'text': 'Required proof', 'row_span': 1, 'col_span': 1},
        {'row': 1, 'column': 0, 'text': '提交🙂证明。' * 1500, 'row_span': 1, 'col_span': 1},
    ]}
    index = source([{'grid': native_grid, 'headers': [{'row': 0, 'column': 0}]}])
    admission = budget(6500)
    packs, _ = make_packs(index, admission)
    assert len(packs) > 1
    assert_coverage(index, packs)
    for pack in packs:
        assert not pack['issues']
        rendered = materialize(index, pack)
        assert any(entry['text'] == 'Required proof' for entry in [*rendered['body'], *rendered['supporting']])
        assert admission.measure('extract', extract_payload(rendered))['fits']


def test_nonmonotone_probe_never_grants_unmeasured_prefix_or_loses_coverage():
    class NonmonotoneBudget(RequestBudget):
        def measure(self, task, payload, images=()):
            result = super().measure(task, payload, images)
            count = len(payload['body'])
            result.update(fits=count in {1, 3}, input_tokens=count * 500, context_tokens=2000, output_reserve=0, safety=0)
            return result
    index = source([{'text': f'Obligation {number}.', 'section': str(number)} for number in range(8)])
    admission = NonmonotoneBudget(context_tokens=2000, output_reserve=0, prompts={'extract': 'Read.'})
    packs, _ = make_packs(index, admission)
    assert_coverage(index, packs)
    assert all(len(pack['body']) in {1, 3} for pack in packs)
    assert all(admission.measure('extract', extract_payload(materialize(index, pack)))['fits'] for pack in packs)


def test_reviewed_binding_survives_body_support_deduplication_and_repartition():
    from docreader.tender.evidence.context import condition_context
    index = source([{'text': '本表仅适用于联合体。'}, {'text': '必须提交证明。' * 900}])
    _, conditions, _, _ = condition_context(index.ranges)
    candidate = next(value for items in conditions.values() for _, value in items)
    target = index.ranges[1]['range_id']
    decision = {'range_id': candidate['range_id'], 'decision': 'bind', 'target_range_ids': [target]}
    for capacity in (131072, 7000):
        admission = budget(capacity)
        packs, _ = make_packs(index, admission, [decision])
        for pack in packs:
            targets = {entry['range_id'] for entry in pack['body'] if entry['source_range_id'] == target}
            if not targets:
                continue
            binding = next(value for value in pack['scope_bindings'] if value['range_id'] == candidate['range_id'])
            assert binding['source_range_id'] == index.ranges[0]['range_id']
            assert set(binding['scope_binding']['applies_to_range_ids']) == targets
            assert binding['scope_binding']['reviewed_target_range_ids'] == [target]
            assert binding['scope_binding']['status'] == 'bound'
            assert 'text' not in binding
            assert pack['planning_details']['budget']['fits']
            if capacity == 131072:
                assert pack['supporting'] == []
        assert_coverage(index, packs)
