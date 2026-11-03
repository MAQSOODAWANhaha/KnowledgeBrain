"""Regression cases for the reviewed retrieval/coverage contract."""

from copy import deepcopy
import json

import pytest

from docreader.tender import authoring_coverage as coverage
from docreader.tests.unit.test_authoring_coverage import (
    outline_for, queries_of, ref, requirement, target, tasks_for,
)


def entry_for(position):
    return coverage.build_ledger(outline_for(position))[0]


def unit_for(*blocks, rows=(), attachments=()):
    return {"task_id": "w1", "target_ids": ["t"], "status": "generated", "blocks": list(blocks),
            "cell_fills": [], "row_fills": list(rows), "attachments": list(attachments)}


def judged(entry, unit, ids, result="supported", round=0):
    return {"coverage_id": entry["coverage_id"], "result": result,
            "content_refs": [{"task_id": "w1", "content_ids": ids}] if ids else [],
            "required_task_ids": ["w1"], "reviewed_task_ids": ["w1"], "task_ids": ["w1"],
            "unit_fps": {"w1": coverage.unit_fingerprint(unit)}, "review_round": round,
            "scope_complete": True, "context_incomplete": False}


def test_document_report_submission_still_searches_but_explicit_external_note_does_not():
    report, external = ref("report"), ref("external")
    position = target("t", [report], kind="attachment")
    note = target("upload", [external])
    outline = outline_for(position, intents=[{"query": "检验报告", "rule_refs": [report]}])
    outline["chapters"].append({"presentation_role": "submission_note", "targets": [note], "children": []})
    requirements = {"rules": [requirement(report, "提交有效检验报告", "submission_requirement"),
                               requirement(external, "平台上传", "submission_requirement")]}
    entries = coverage.build_ledger(outline, requirements)
    plan = coverage.plan_queries(outline, tasks_for(position), entries, requirements)
    assert queries_of(plan) == ["检验报告"]
    assert plan["search"][0]["include_original"] is True
    assert next(e for e in entries if e["target_id"] == "upload")["resolution"] == "submission"


def test_common_rules_keep_target_and_get_their_own_query_alongside_rows():
    common, first, second = ref("C"), ref("R1"), ref("R2")
    excerpts = [{"source_ref": {"unit_id": name}, "selectors": []} for name in ("one", "two")]
    rows = [{"row_id": f"row-{i}", "rule_refs": [rule], "source_excerpts": [source]}
            for i, (rule, source) in enumerate(zip([first, second], excerpts), 1)]
    position = target("t", [common, first, second], response_table={"rows": rows})
    outline = outline_for(position)
    requirements = {"rules": [requirement(common, "共同要求"), requirement(first, "一"), requirement(second, "二")]}
    entries = coverage.build_ledger(outline, requirements)
    by_rule = {e["rule_ref"]["item_ref"]["item_id"]: e for e in entries}
    assert by_rule["C"]["target_id"] == "t" and by_rule["C"]["row_id"] is None
    assert len(entries) == 3
    texts = {json.dumps(source, ensure_ascii=False, sort_keys=True, separators=(",", ":")): str(i)
             for i, source in enumerate(excerpts)}
    planned = coverage.plan_queries(outline, tasks_for(position), entries, requirements, texts)
    assert "共同要求" in queries_of(planned)


def test_fallback_row_query_keeps_shared_conditions_and_exact_reference_deduplication():
    shared = {"source_ref": {"unit_id": "shared"}, "selectors": []}
    own = {"source_ref": {"unit_id": "row"}, "selectors": []}
    row = {"row_id": "row", "rule_refs": [], "source_excerpts": [shared, own]}
    texts = {json.dumps(source, ensure_ascii=False, sort_keys=True, separators=(",", ":")): text
             for source, text in [(shared, "共同型号V2"), (own, "支持离线工作")]}
    position = target("t", [], response_table={"context_source_excerpts": [shared], "rows": [row]})
    outline = outline_for(position)
    planned = coverage.plan_queries(outline, tasks_for(position), coverage.build_ledger(outline), {}, texts)
    assert queries_of(planned) == ["共同型号V2\n支持离线工作"]
    position2 = deepcopy(position)
    other = {"source_ref": {"unit_id": "other"}, "selectors": []}
    position2["response_table"]["context_source_excerpts"] = [other]
    texts[json.dumps(other, ensure_ascii=False, sort_keys=True, separators=(",", ":"))] = "共同型号V3"
    second = coverage.plan_queries(outline_for(position2), tasks_for(position2), coverage.build_ledger(outline_for(position2)), {}, texts)
    assert second["queries"][0]["query_id"] != planned["queries"][0]["query_id"]


def test_same_target_rule_fragments_have_separate_review_ranges_and_real_aliases():
    first, second = ref("R1"), ref("R2")
    positions = [target("t", [first]), target("t", [second])]
    tasks = [{**tasks_for(position)[0], "task_id": f"w{i}"} for i, position in enumerate(positions, 1)]
    entries = coverage.build_ledger(outline_for(target("t", [first, second])))
    record = {"task_id": "w1", "targets": [{"target_id": "actual-t", "rule_ids": ["actual-R"]}],
              "rules": [], "content": {"blocks": []},
              "_coverage_refs": coverage.task_coverage_refs(tasks[0], {
                  "targets": [{"target_id": "actual-t", "rule_ids": ["actual-R"]}]}, {"actual-R": first})}
    items, mapping = coverage.batch_coverage_view(entries, tasks, [record])
    assert len(items) == 1 and items[0]["scope"] == "complete"
    assert items[0]["refs"] == [{"task_id": "w1", "target_id": "actual-t", "rule_ids": ["actual-R"]}]
    assert mapping["c1"]["required_task_ids"] == ["w1"]


def test_empty_negative_assessment_binds_every_reviewed_unit_and_expires_after_revision():
    entry = entry_for(target("t", [ref("R")]))
    unit = unit_for({"kind": "paragraph", "text": "旧稿", "target_ids": ["t"], "material_ids": ["m1"]})
    record = {"task_id": "w1", "content": {"blocks": []}}
    mapping = {"c1": {"coverage_id": entry["coverage_id"], "scope_complete": True,
                      "required_task_ids": ["w1"], "reviewed_task_ids": ["w1"]}}
    accepted = coverage.accept_assessments([{"coverage_id": "c1", "result": "unsupported"}], mapping, [record])
    assert accepted[0]["task_ids"] == ["w1"]
    old = judged(entry, unit, [], "unsupported")
    replacement = deepcopy(unit)
    replacement["blocks"][0]["text"] = "新稿"
    new = judged(entry, replacement, ["b1"], round=1)
    final = coverage.settle_coverage([entry], [replacement], [{"sha256": "new"}], [old, new])[0]
    assert final["assessment"] == "supported"


def test_full_r2_supersedes_r0_even_when_content_fingerprint_is_unchanged():
    entry = entry_for(target("t", [ref("R")]))
    unit = unit_for({"kind": "paragraph", "text": "有据内容", "target_ids": ["t"], "material_ids": ["m1"]})
    old = judged(entry, unit, [], "unsupported")
    new = judged(entry, unit, ["b1"], round=1)
    final = coverage.settle_coverage([entry], [unit], [{"sha256": "same"}], [old, new])[0]
    assert final["assessment"] == "supported"
    conflict = coverage.settle_coverage([entry], [unit], [{"sha256": "same"}], [old, {**new, "review_round": 0}])[0]
    assert conflict["review_status"] == "unreviewed"


def test_partial_r2_cannot_replace_full_r0_and_old_unbound_judgments_stay_unknown():
    entry = entry_for(target("t", [ref("R")]))
    unit = unit_for({"kind": "paragraph", "text": "有据内容", "target_ids": ["t"], "material_ids": ["m1"]})
    complete = judged(entry, unit, ["b1"])
    partial = {**judged(entry, unit, [], "unsupported", round=1), "scope_complete": False,
               "context_incomplete": True, "required_task_ids": ["w1", "w2"]}
    final = coverage.settle_coverage([entry], [unit], [{"sha256": "same"}], [complete, partial])[0]
    assert final["assessment"] == "supported"
    old = {key: value for key, value in complete.items() if key not in {"required_task_ids", "reviewed_task_ids", "review_round"}}
    unknown = coverage.settle_coverage([entry], [unit], [{"sha256": "same"}], [old])[0]
    assert unknown["assessment"] is None and unknown["review_status"] == "unreviewed"
    assert unknown["content_refs"] == [] and "coverage_unfilled" not in unknown["gap_codes"]


def test_headers_rows_and_real_attachment_are_content_but_title_is_not():
    entry = entry_for(target("t", [ref("R")]))
    material_ref = ref("unused")["item_ref"]["artifact_ref"]
    unit = unit_for({"kind": "table", "headers": ["参数"], "rows": [["支持"]],
                     "target_ids": ["t"], "material_ids": ["m1"]},
                    attachments=[{"material_id": "m2", "title": "检验报告", "artifact_ref": material_ref}])
    record = {"task_id": "w1", "supports": [{"support_id": "k2", "origin": "knowledge", "attachment": True,
                                           "text": "有效证明", "target_ids": ["t1"]}],
              "content": {"blocks": [{"content_id": "b1", "kind": "table", "headers": ["参数"], "rows": [["支持"]]}],
                          "attachments": [{"content_id": "a1", "attachment_id": "k2", "title": "检验报告"}]}}
    mapping = {"c1": {"coverage_id": entry["coverage_id"], "scope_complete": True,
                       "required_task_ids": ["w1"], "reviewed_task_ids": ["w1"]}}
    accepted = coverage.accept_assessments([{"coverage_id": "c1", "result": "supported", "content_refs": [
        {"task_id": "w1", "content_ids": ["b1", "a1"]}]}], mapping, [record])
    assert len(accepted) == 1
    final = coverage.settle_coverage([entry], [unit], [{"sha256": "new"}], [judged(entry, unit, ["b1", "a1"])])[0]
    assert {r["content_id"] for r in final["content_refs"]} == {"b1", "a1"}
    assert final["material_ids"] == ["m1", "m2"]
    record["supports"] = []
    assert coverage.accept_assessments([{"coverage_id": "c1", "result": "supported", "content_refs": [
        {"task_id": "w1", "content_ids": ["a1"]}]}], mapping, [record]) == []


def test_final_bindings_follow_assessment_selection_not_all_content_at_target():
    first, second = ref("R1"), ref("R2")
    entries = coverage.build_ledger(outline_for(target("t", [first, second])))
    unit = unit_for({"kind": "paragraph", "text": "规则一", "target_ids": ["t"], "material_ids": ["m1"]},
                    {"kind": "paragraph", "text": "规则二", "target_ids": ["t"], "material_ids": ["m2"]})
    settled = coverage.settle_coverage(entries, [unit], [{"sha256": "same"}], [judged(entries[0], unit, ["b1"])])
    assert [r["content_id"] for r in settled[0]["content_refs"]] == ["b1"]
    assert settled[0]["material_ids"] == ["m1"]
    assert settled[1]["content_refs"] == [] and settled[1]["material_ids"] == []
    assert "coverage_unassessed" in settled[1]["gap_codes"]
    assert "coverage_unfilled" not in settled[1]["gap_codes"]


def test_shared_rule_spans_all_its_real_fragments_but_each_row_has_its_own_scope():
    common, first, second = ref("C"), ref("R1"), ref("R2")
    positions = [target("t", [common, rule], response_table={"rows": [
        {"row_id": f"row-{i}", "rule_refs": [rule], "source_excerpts": []}]})
        for i, rule in enumerate([first, second], 1)]
    tasks = [{**tasks_for(position)[0], "task_id": f"w{i}"} for i, position in enumerate(positions, 1)]
    records = []
    for i, task in enumerate(tasks, 1):
        payload = {"targets": [{"target_id": "t1", "rule_ids": ["common", "own"]}],
                   "response_rows": [{"row_id": "q1", "target_id": "t1", "rule_ids": ["own"]}]}
        records.append({"task_id": task["task_id"], **payload, "content": {"row_fills": []},
                        "_coverage_refs": coverage.task_coverage_refs(task, payload,
                            {"common": common, "own": [first, second][i - 1]})})
    all_position = target("t", [common, first, second], response_table={"rows": [
        *positions[0]["response_table"]["rows"], *positions[1]["response_table"]["rows"]]})
    entries = coverage.build_ledger(outline_for(all_position))
    _, partial = coverage.batch_coverage_view(entries, tasks, records[:1])
    by_id = {meta["coverage_id"]: meta for meta in partial.values()}
    common_id = coverage.coverage_identity(common, "t", None)
    first_id = coverage.coverage_identity(first, "t", "row-1")
    assert by_id[common_id]["scope_complete"] is False
    assert by_id[first_id]["scope_complete"] is True
    _, complete = coverage.batch_coverage_view(entries, tasks, records)
    assert all(meta["scope_complete"] for meta in complete.values())
    assert next(meta for meta in complete.values() if meta["coverage_id"] == common_id)["required_task_ids"] == ["w1", "w2"]


def test_another_subject_or_another_response_row_is_not_an_allowed_selection():
    position = target("t", [ref("R")], response_table={"rows": [
        {"row_id": "row", "rule_refs": [ref("R")], "source_excerpts": []}]})
    task = tasks_for(position)[0]
    payload = {"targets": [{"target_id": "t1", "rule_ids": ["R"]}],
               "response_rows": [{"row_id": "q1", "target_id": "t1", "rule_ids": ["R"]}]}
    record = {"task_id": "w1", **payload,
              "_coverage_refs": coverage.task_coverage_refs(task, payload, {"R": ref("R")}),
              "content": {"row_fills": [
                  {"content_id": "r1", "row_id": "q1", "text": "本行"},
                  {"content_id": "r2", "row_id": "q2", "text": "其他主体或行"}]}}
    _, mapping = coverage.batch_coverage_view(coverage.build_ledger(outline_for(position)), [task], [record])
    raw = [{"coverage_id": "c1", "result": "supported", "content_refs": [
        {"task_id": "w1", "content_ids": [identity]}]} for identity in ["r2", "r1"]]
    accepted = coverage.accept_assessments(raw, mapping, [record])
    assert [ref["content_ids"] for item in accepted for ref in item["content_refs"]] == [["r1"]]


def test_excerpt_loader_reads_shared_conditions_before_row_sources(monkeypatch):
    from docreader.tender import authoring_pipeline as pipeline
    shared, own = {"fixture": "shared"}, {"fixture": "own"}
    position = target("t", [], response_table={"context_source_excerpts": [shared], "rows": [
        {"row_id": "row", "source_excerpts": [own, shared]}]})
    calls = []
    def load(artifacts, source):
        calls.append(source)
        return {"text": source["fixture"]}
    monkeypatch.setattr(pipeline, "load_source_excerpt", load)
    texts = pipeline._excerpt_texts(None, outline_for(position))
    assert list(texts.values()) == ["shared", "own"] and calls == [shared, own]


@pytest.mark.parametrize("same_content", [False, True])
async def test_actual_pipeline_binds_empty_negative_and_r2_to_the_real_reviewed_rows(tmp_path, same_content):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.llm.budget import RequestBudget
    from docreader.tender.authoring_pipeline import generate_document
    from test_authoring_pipeline_boundaries import snapshot
    from test_response_table_pipeline import SOURCES, SUPPORTED, assembly_response, request_payload, source_response

    artifacts = ArtifactStore(tmp_path)
    frozen = snapshot(artifacts, [{"heading": "技术要求", "text": text} for text in SOURCES])
    reviews = []
    async def execute(operation, request, validate):
        value = request_payload(request)
        stage = value.get("operation", operation)
        if stage == "outline":
            return validate(source_response(value))
        if stage == "assemble":
            return validate(assembly_response(value, explicit=True))
        if stage == "structure":
            return validate({"findings": []})
        if operation == "authoring_basis":
            return validate({})
        if operation == "authoring_write":
            revision = "previous_content" in value
            return validate({"row_fills": [{"row_id": row["row_id"],
                "text": SUPPORTED + ("（修订）" if revision and not same_content else ""), "support_ids": ["k1"]}
                for row in value["response_rows"]], "gaps": ["重新核对过表达"] if revision else []})
        reviews.append(value)
        assert all("_coverage_refs" not in unit for unit in value["units"])
        assessments = []
        for item in value["coverage"]:
            reference, = item["refs"]
            unit, = [unit for unit in value["units"] if unit["task_id"] == reference["task_id"]]
            content = [fill["content_id"] for fill in unit["content"]["row_fills"]
                       if fill["row_id"] == reference["row_id"]]
            assessments.append({"coverage_id": item["coverage_id"],
                "result": "unsupported" if len(reviews) == 1 else "supported",
                "content_refs": [] if len(reviews) == 1 else [{"task_id": unit["task_id"], "content_ids": content}]})
        return validate({"assessments": assessments, "findings": [{"code": "refine_expression", "detail": "按资料核对表达。",
            "task_ids": [value["units"][0]["task_id"]], "can_revise": True, "suggestion": "核对后保留有据表述。"}]
            if len(reviews) == 1 else []})

    result = artifacts.read_json(await generate_document(artifacts, frozen,
        RequestBudget(model="fixture", context_tokens=131072, output_reserve=16384), execute,
        materials=[{"material_id": "guide", "title": "产品资料", "text": SUPPORTED}]))
    final = artifacts.read_json(result["coverage_ref"])
    assert len(reviews) == 2
    assert all(entry["assessment"] == "supported" and entry["review_status"] == "reviewed" for entry in final["entries"])
    assert all(len(entry["content_refs"]) == 1 and entry["material_ids"] == ["guide"] for entry in final["entries"])
    initial = artifacts.read_json(result["initial_review_ref"])["coverage_assessments"]
    assert initial and all(item["result"] == "unsupported" and item["content_refs"] == [] for item in initial)
    for item in initial:
        assert item["required_task_ids"] == item["reviewed_task_ids"] == list(item["reviewed_unit_refs"])
        assert item["review_round"] == 0
        for identity, reference in item["reviewed_unit_refs"].items():
            assert coverage.unit_fingerprint(artifacts.read_json(reference)) == item["unit_fps"][identity]
    recorded = artifacts.read_json(result["review_ref"])["coverage_assessments"]
    assert {item["review_round"] for item in recorded} == {0, 1}
