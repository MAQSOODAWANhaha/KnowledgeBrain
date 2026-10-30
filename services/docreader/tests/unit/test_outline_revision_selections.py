"""A purpose-only revision preserves accepted additional reference selections."""
from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender import authoring_structure
from docreader.tender.authoring_excerpts import load_source_excerpt
from docreader.tender.evidence.references import fragment, selector

from test_authoring_templates import create_context, create_source


TEXTS = ["来源原有依据。", "首次新增规则的完整原文。", "独立选择的理解原文。",
         "替换规则的完整原文。", "替换选择的理解原文。"]


async def revise(tmp_path, monkeypatch, *, rule_mode="omit", source_mode="omit", match="same",
                 independently_select_rule_source=False, shared_unit=False,
                 source_manual=False, assembly_manual=False, revision_manual=None,
                 related_mode=None, referenced_layout=None):
    artifacts, index_ref, native = create_source(tmp_path, [{"text": "\n".join(TEXTS)}] if shared_unit else
                                                [{"text": text} for text in TEXTS])
    context = create_context(artifacts, index_ref, native)
    evidence_ids = [entry["range_id"] for entry in native.ranges]
    if shared_unit:
        evidence_ids, offset = [], 0
        for text in TEXTS:
            piece = fragment(native.ranges[0], offset, offset + len(text))
            evidence_ids.append(piece["range_id"])
            context.sources[piece["range_id"]] = deepcopy(context.sources[native.ranges[0]["range_id"]])
            context.excerpts[piece["range_id"]] = [{"source_ref": context.sources[piece["range_id"]][0],
                                                    "selectors": [selector(piece)]}]
            offset += len(text) + 1
    items = [{"item_id": f"item-{i}", "text": f"规则{i}", "evidence": [
        {"range_id": evidence_ids[unit], "quote": TEXTS[unit]}]}
        for i, unit in enumerate((0, 1, 3, 2) if shared_unit else (0, 1, 3))]
    item_ref = artifacts.put_json({"items": items})
    references = [{"item_ref": {"artifact_ref": item_ref, "item_id": item["item_id"]}} for item in items]
    context.rules = {f"rule-{i}": ref for i, ref in enumerate(references)}
    context.payload["rules"] = [{"rule_id": identity, "item": item}
                                for identity, item in zip(context.rules, items)]
    reviewed, previous, calls = [], [], []

    async def review(_artifacts, outline, *_args, **_kwargs):
        reviewed.append(deepcopy(outline))
        return ([{"code": "purpose_detail", "detail": "只完善用途说明。", "suggestion": "", "can_revise": True}]
                if len(reviewed) == 1 else []), []

    monkeypatch.setattr(authoring_structure, "review_structure", review)

    async def execute(operation, request, validate):
        data = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(data.get("operation", operation))
        if data.get("operation") != "assemble":
            sources = {entry["text"]: entry["range_id"] for entry in data["sources"]}
            rules = {entry["item"]["text"]: entry["rule_id"] for entry in data["rules"]}
            target = {"kind": "narrative", "purpose": "保留来源原有依据", "subject": "申请主体",
                      "manual_fill": source_manual,
                      "rule_ids": [rules["规则0"]], "range_ids": [sources[TEXTS[0]]]}
            targets = [target, {**target, "purpose": "另一候选"}]
            if referenced_layout == "merged":
                targets.append({**target, "purpose": "第三候选"})
            return validate({"schema_version": 2, "findings": [], "chapters": [{"title": "原始候选",
                "targets": targets}],
                "constraint_range_ids": [sources[TEXTS[i]] for i in (1, 2, 4)],
                "constraint_rule_ids": [rules["规则3"]] if shared_unit else []})
        rules = {entry["rule"]["text"]: entry["rule_id"] for entry in data["unassigned_rules"]}
        sources = {entry["text"]: entry["source_id"] for entry in data["submission_sources"]}
        target = {"kind": "narrative", "purpose": "接受新增依据", "subject": "申请主体", "target_ids": ["t1"]}
        old_source = TEXTS[1] if independently_select_rule_source else TEXTS[2]
        if not data.get("previous_structure"):
            target.update(rule_ids=[rules["规则1"]], source_ids=[sources[old_source]])
            if assembly_manual:
                target["manual_fill"] = True
            if related_mode is not None:
                target["related_target_ids"] = ["t2"]
            targets = [target]
            if match == "ambiguous":
                targets.append({**target, "purpose": "相同血缘的另一位置", "rule_ids": [rules["规则2"]]})
        else:
            previous.extend(deepcopy(data["previous_structure"]))
            target["purpose"] = "只完善后的用途"
            if match == "subject":
                target["subject"] = "另一主体"
            if match == "lineage":
                target["target_ids"] = ["t2"]
            if rule_mode != "omit":
                target["rule_ids"] = [rules["规则2"]] if rule_mode == "replace" else []
            if source_mode != "omit":
                target["source_ids"] = [sources[TEXTS[4]]] if source_mode == "replace" else []
            if revision_manual is not None:
                target["manual_fill"] = revision_manual
            if related_mode == "clear":
                target["related_target_ids"] = []
            elif related_mode == "replace":
                target["related_target_ids"] = ["t2"]
            for field in ("rule_ids", "source_ids", "related_target_ids"):
                invalid = {"chapters": [{"title": "最终位置", "targets": [{**target, field: ["absent"]}]}]}
                with pytest.raises(ModelContractError):
                    validate(invalid)
            if match == "same" and rule_mode == "omit":
                invalid = {"chapters": [{"title": "最终位置", "targets": [target]}],
                           "excluded_rules": [{"rule_ids": [rules["规则1"]], "reason": "不能既继承又排除。"}]}
                with pytest.raises(ModelContractError) as error:
                    validate(invalid)
                assert error.value.errors[0]["code"] == "outline_conflicting_disposition"
            targets = [target]
            if referenced_layout:
                referred = {"kind": "narrative", "purpose": "重组后的被引用位置", "target_ids": ["t2"]}
                if referenced_layout == "split":
                    targets.extend([referred, {**referred, "purpose": "被引用候选的另一实例"}])
                else:
                    targets.append({**referred, "target_ids": ["t2", "t3"]})
        return validate({"chapters": [{"title": "最终位置", "targets": targets}]})

    result = await pipeline._organize(artifacts, context, RequestBudget(
        model="fixture", context_tokens=131072, output_reserve=16384), execute, [], structure_results={})
    assert calls == ["outline", "assemble", "assemble"] and len(reviewed) == 2
    return artifacts, result, references, previous, reviewed


@pytest.mark.parametrize("rule_mode", ["omit", "clear", "replace"])
@pytest.mark.parametrize("source_mode", ["omit", "clear", "replace"])
async def test_revision_omission_inherits_each_selection_and_explicit_arrays_replace_it(
        tmp_path, monkeypatch, rule_mode, source_mode):
    artifacts, result, references, previous, reviewed = await revise(
        tmp_path, monkeypatch, rule_mode=rule_mode, source_mode=source_mode)
    target = result["chapters"][0]["targets"][0]
    expected_rule = {"omit": 1, "replace": 2}.get(rule_mode)
    assert target["rule_refs"] == [references[0], *([references[expected_rule]] if expected_rule else [])]
    expected_units = {0}
    if expected_rule:
        expected_units.add({1: 1, 2: 3}[expected_rule])
    if source_mode != "clear":
        expected_units.add(4 if source_mode == "replace" else 2)
    assert {ref["unit_id"] for ref in target["source_refs"]} == {f"unit:{i}" for i in expected_units}
    assert {load_source_excerpt(artifacts, ref)["text"] for ref in target["source_excerpts"]} == {
        TEXTS[i] for i in expected_units}
    assert reviewed[0]["chapters"][0]["targets"][0]["rule_refs"] == references[:2]
    old = previous[0]["targets"][0]
    assert old["rule_ids"] == ["u1"] and len(old["source_ids"]) == 1
    assert not any(key.startswith("_") for chapter in pipeline._flatten(result["chapters"])
                   for entry in chapter["targets"] for key in entry)


@pytest.mark.parametrize("match", ["subject", "lineage", "ambiguous"])
async def test_revision_never_inherits_additions_from_another_or_ambiguous_position(tmp_path, monkeypatch, match):
    _, result, references, _, _ = await revise(tmp_path, monkeypatch, match=match)
    target = result["chapters"][0]["targets"][0]
    assert target["rule_refs"] == references[:1]
    assert [ref["unit_id"] for ref in target["source_refs"]] == ["unit:0"]


async def test_clearing_a_rule_keeps_its_source_when_also_selected_independently(tmp_path, monkeypatch):
    artifacts, result, references, _, _ = await revise(
        tmp_path, monkeypatch, rule_mode="clear", independently_select_rule_source=True)
    target = result["chapters"][0]["targets"][0]
    assert target["rule_refs"] == references[:1]
    assert {ref["unit_id"] for ref in target["source_refs"]} == {"unit:0", "unit:1"}
    assert {load_source_excerpt(artifacts, ref)["text"] for ref in target["source_excerpts"]} == set(TEXTS[:2])


async def test_clearing_rule_removes_only_its_exact_excerpt_when_sources_share_a_unit(tmp_path, monkeypatch):
    artifacts, result, references, _, _ = await revise(
        tmp_path, monkeypatch, rule_mode="clear", shared_unit=True)
    target = result["chapters"][0]["targets"][0]
    assert target["rule_refs"] == references[:1]
    assert [ref["unit_id"] for ref in target["source_refs"]] == ["unit:0"]
    assert {load_source_excerpt(artifacts, ref)["text"] for ref in target["source_excerpts"]} == {
        TEXTS[0], TEXTS[2]}


@pytest.mark.parametrize("source_manual,assembly_manual,revision_manual,expected", [
    (False, True, None, True), (False, True, False, False),
    (False, True, True, True), (True, False, False, True),
])
async def test_revision_preserves_manual_boundary_unless_explicitly_replaced(
        tmp_path, monkeypatch, source_manual, assembly_manual, revision_manual, expected):
    _, result, _, previous, _ = await revise(tmp_path, monkeypatch, source_manual=source_manual,
        assembly_manual=assembly_manual, revision_manual=revision_manual)
    assert result["chapters"][0]["targets"][0]["manual_fill"] is expected
    assert previous[0]["targets"][0]["manual_fill"] is (source_manual or assembly_manual)


@pytest.mark.parametrize("match", ["subject", "lineage", "ambiguous"])
async def test_revision_does_not_guess_a_manual_boundary_from_another_position(tmp_path, monkeypatch, match):
    _, result, _, _, _ = await revise(tmp_path, monkeypatch, match=match, assembly_manual=True)
    assert result["chapters"][0]["targets"][0]["manual_fill"] is False


@pytest.mark.parametrize("related_mode", ["omit", "clear", "replace"])
async def test_revision_related_selection_is_retained_or_explicitly_replaced(tmp_path, monkeypatch, related_mode):
    _, result, _, previous, reviewed = await revise(tmp_path, monkeypatch, related_mode=related_mode)
    target = result["chapters"][0]["targets"][0]
    assert reviewed[0]["chapters"][0]["targets"][0]["related_target_ids"]
    if related_mode == "clear":
        assert target["related_target_ids"] == []
    else:
        referred = next(entry for chapter in pipeline._flatten(result["chapters"])
                        for entry in chapter["targets"] if entry["purpose"] == "另一候选")
        assert target["related_target_ids"] == [referred["target_id"]]
    assert previous[0]["targets"][0]["related_target_ids"] == ["t2"]


@pytest.mark.parametrize("match", ["subject", "lineage", "ambiguous"])
async def test_revision_does_not_guess_related_targets_from_another_position(tmp_path, monkeypatch, match):
    _, result, _, _, _ = await revise(tmp_path, monkeypatch, match=match, related_mode="omit")
    assert result["chapters"][0]["targets"][0]["related_target_ids"] == []


@pytest.mark.parametrize("referenced_layout", ["split", "merged"])
async def test_inherited_relation_resolves_against_revised_candidate_owners(tmp_path, monkeypatch, referenced_layout):
    _, result, _, _, _ = await revise(tmp_path, monkeypatch, related_mode="omit", referenced_layout=referenced_layout)
    target = result["chapters"][0]["targets"][0]
    if referenced_layout == "split":
        assert target["related_target_ids"] == []
        assert any(issue["code"] == "outline_reference_ambiguous" for issue in result["issues"])
    else:
        assert target["related_target_ids"] == [result["chapters"][0]["targets"][1]["target_id"]]
