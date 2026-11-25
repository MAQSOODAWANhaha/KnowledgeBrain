"""Explicit instance semantics do not depend on subject-label string equality."""

from typing import TypeVar
from copy import deepcopy
from io import BytesIO
import json

from docx import Document
from pydantic import ValidationError
import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import _OutlineContext
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_templates import load_template_part
from docreader.tender.schema import OutlineAssemblyTarget
from test_authoring_templates import create_context, create_source
from test_complete_format_pipeline import assembled, source_form, source_result
from test_structural_packing import grid

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



CONDITIONS = ["本表仅适用于无委托代理人的情况。", "须附有效原件扫描件并完成规定签章。"]
DECORATION = "源文件固定页首装饰"


def all_targets(chapters):
    return [target for chapter in pipeline._flatten(chapters) for target in chapter["targets"]]


def condition_texts(count):
    return [*CONDITIONS, "其他适用限定及完整填写说明仍须保留。"][:count]


def fixture_context(tmp_path, count, *, decoration=False):
    conditions = condition_texts(count)
    specs = [*([{"text": DECORATION}] if decoration else []),
             {"grid": grid(rows=2)}, *({"text": text} for text in conditions)]
    artifacts, index_ref, native = create_source(tmp_path, specs)
    base = create_context(artifacts, index_ref, native)
    items = [{"item_id": f"condition-{number}", "text": text, "evidence": [{
        "range_id": next(r["range_id"] for r in native.ranges if r["text"] == text),
        "quote": text}]} for number, text in enumerate(conditions)]
    context = _OutlineContext(artifacts, {
        **base.input, "extract_refs": [artifacts.put_json({"items": items})]})
    return artifacts, context, native


def select_source(value, subjects):
    parts = value["template_parts"]
    count = len(subjects)
    decoration, table = parts[:-(count + 1)], parts[-(count + 1)]
    rules = {entry["item"]["text"]: entry["rule_id"] for entry in value["rules"]}
    targets = []
    for number, subject in enumerate(subjects):
        condition = parts[-count + number]
        selected = [*decoration, table, condition]
        targets.append(source_form([part["part_id"] for part in selected], subject=subject,
            range_ids=[identity for part in selected for identity in part["range_ids"]],
            rule_ids=[rules[condition_texts(count)[number]]]))
    return source_result(targets)


async def organize(tmp_path, subjects, selection, *, revision=None):
    artifacts, context, native = fixture_context(tmp_path, len(subjects), decoration=revision is not None)
    before = deepcopy((context.payload, native.raw))
    calls, assembly_calls, reviews = [], [], []

    async def execute(stage, request, validate):
        value = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(stage)
        if stage == "authoring_structure":
            reviews.append(deepcopy(value))
            if len(reviews) > 1:
                return validate({"findings": []})
            target, = all_targets(value["chapters"])
            return validate({"findings": [{"code": "instance_check", "detail": "核对同一填写实例。"}],
                "source_decoration_exclusions": [{"target_id": target["target_id"],
                    "support_ids": target["template_support_ids"][:1],
                    "reason": "该独立部件仅为源文件固定页首装饰。"}]})
        if value.get("operation") == "assemble":
            assembly_calls.append(deepcopy(value))
            if "previous_structure" in value:
                previous, = all_targets(value["previous_structure"])
                assert previous["candidate_target_ids"] == [f"t{number + 1}" for number in range(len(subjects))]
                assert previous.get("same_instance") is True
                assert len(previous["template_part_ids"]) == len(subjects) + 1
                return validate(assembled(_required(_required(revision)(value))))
            return validate(assembled(selection(value)))
        return validate(select_source(value, subjects))

    result = await pipeline._organize(artifacts, context,
        RequestBudget(model="fixture", context_tokens=131072), execute, [],
        structure_results={} if revision is not None else None)
    assert (context.payload, native.raw) == before
    if revision is None:
        assert calls == ["outline", "outline"]
    else:
        assert calls[:4] == ["outline", "outline", "authoring_structure", "outline"]
        assert calls[4:] in [[], ["authoring_structure"]]
    return artifacts, result, assembly_calls


def merged(**selection):
    return {"kind": "form", "purpose": "按完整原格式组织同一份填写实例。",
            "target_ids": ["t1", "t2"], **selection}


@pytest.mark.parametrize("subjects,subject", [
    (["法定代表人（单位负责人）", "无委托代理人的投标人"], "无委托代理人的投标人"),
    (["投标人", None], "投标人；代理经销商按适用条件填写"),
    (["角色描述甲", "角色描述乙"], None),
])
async def test_explicit_same_instance_preserves_one_native_form_and_all_bound_context(
        tmp_path, subjects, subject):
    artifacts, outline, _ = await organize(tmp_path, subjects,
        lambda value: [merged(same_instance=True, **({"subject": subject} if subject else {}))])
    target, = all_targets(outline["chapters"])
    assert target.get("subject") == subject
    assert {ref["item_ref"]["item_id"] for ref in target["rule_refs"]} == {
        "condition-0", "condition-1"}
    assert {ref["unit_id"] for ref in target["source_refs"]} == {"unit:0", "unit:1", "unit:2"}
    assert len(target["template_refs"]) == 3
    loaded = [load_template_part(artifacts, ref) for ref in target["template_refs"]]
    assert next(part["grid"] for part in loaded if part["grid"]) == grid(rows=2)
    assert {part["text"] for part in loaded if part["text"]} == set(CONDITIONS)
    exported = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(exported["docx_ref"])))
    assert len(document.tables) == 1
    for text in CONDITIONS:
        assert [paragraph.text for paragraph in document.paragraphs].count(text) == 1
    assert [[cell.text for cell in row.cells] for row in document.tables[0].rows] == [
        [f"Header{row}{column}" * 8 for column in range(2)] for row in range(2)]


@pytest.mark.parametrize("selection", [{}, {"same_instance": False}])
async def test_default_subject_protection_keeps_distinct_instances(tmp_path, selection):
    artifacts, outline, _ = await organize(tmp_path, ["投标人", "产品原厂"],
        lambda value: [merged(subject="投标人", **selection)])
    targets = all_targets(outline["chapters"])
    assert [target["subject"] for target in targets] == ["投标人", "产品原厂"]
    exported = export_authoring_document(artifacts, outline, [])
    assert len(Document(BytesIO(artifacts.read_bytes(exported["docx_ref"]))).tables) == 2


async def test_explicit_multiple_targets_remain_multiple_instances(tmp_path):
    _, outline, _ = await organize(tmp_path, ["角色甲", "角色乙"], lambda value: [
        {**merged(subject="统一描述", same_instance=True), "target_ids": [identity]}
        for identity in ["t1", "t2"]])
    assert len(all_targets(outline["chapters"])) == 2


def test_same_instance_requires_an_actual_boolean():
    with pytest.raises(ValidationError) as error:
        OutlineAssemblyTarget.model_validate(merged(same_instance="true"))
    assert error.value.errors()[0]["type"] == "bool_type"


@pytest.mark.parametrize("mode,expected", [("omit", 1), ("false", 2)])
async def test_revision_inherits_or_revokes_instance_choice_without_reviving_f0_decoration(
        tmp_path, mode, expected):
    def revise(value):
        return [merged(subject="同份填写主体", **({"same_instance": False} if mode == "false" else {}))]

    artifacts, outline, _ = await organize(tmp_path, ["角色甲", "角色乙"],
        lambda value: [merged(subject="同份填写主体", same_instance=True)], revision=revise)
    targets = all_targets(outline["chapters"])
    assert len(targets) == expected
    exported = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(exported["docx_ref"])))
    assert len(document.tables) == expected
    assert DECORATION not in "\n".join(paragraph.text for paragraph in document.paragraphs)


@pytest.mark.parametrize("change", ["subject", "candidates"])
async def test_changed_subject_or_candidates_do_not_inherit_same_instance(tmp_path, change):
    subjects = ["角色甲", "角色乙"] if change == "subject" else ["角色甲", "角色乙", "角色丙"]

    def revise(value):
        target = merged(subject="不同填写主体" if change == "subject" else "同份填写主体")
        return [target]

    _, outline, _ = await organize(tmp_path, subjects,
        lambda value: [{**merged(subject="同份填写主体", same_instance=True),
                        "target_ids": [f"t{number + 1}" for number in range(len(subjects))]}], revision=revise)
    targets = all_targets(outline["chapters"])
    assert len(targets) == len(subjects)
    assert [target.get("subject") for target in targets] == subjects
