"""Structure review sees the complete rules actually assigned to each position."""

from copy import deepcopy
import json
from types import SimpleNamespace

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import authoring_structure
from docreader.tender.authoring_excerpts import whole_source_excerpts
from test_submission_review import outline, source, target


def rule_fixture(artifacts, texts):
    rules = [{"item_id": f"permanent-item-{number}", "kind": "obligation", "text": text,
              "conditions": [f"仅对象{number}适用"], "exceptions": ["原文另有规定时除外"],
              "evidence": [{"range_id": f"permanent-range-{number}", "quote": "共同技术原文。"}]}
             for number, text in enumerate(texts, 1)]
    reference = artifacts.put_json({"items": rules})
    return rules, reference, [{"item_ref": {"artifact_ref": reference, "item_id": rule["item_id"]}}
                              for rule in rules]


def assigned_target(identity, refs, excerpts=()):
    result = target(identity)
    result.update(kind="narrative", manual_fill=False, rule_refs=refs,
                  source_excerpts=list(excerpts), source_refs=[ref["source_ref"] for ref in excerpts])
    return result


def test_structure_binds_each_target_to_deduplicated_complete_rule_semantics(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    excerpts = whole_source_excerpts(artifacts, source(artifacts, text="共同技术原文。"))
    rules, reference, refs = rule_fixture(artifacts, ["保持环境适用条件。", "保持硬件安装条件。"])
    current = outline([assigned_target("environment", [refs[0]], excerpts),
                       assigned_target("hardware", [refs[1]], excerpts),
                       assigned_target("repeated", [refs[0]], excerpts)])
    current["authoring_input"]["extract_refs"] = [reference]
    original = deepcopy(current)

    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)

    targets = payload["chapters"][0]["targets"]
    assert targets[0]["rule_ids"] == targets[2]["rule_ids"] == ["r1"]
    assert targets[1]["rule_ids"] == ["r2"]
    assert payload["target_rules"] == {
        f"r{number}": {key: value for key, value in rule.items() if key not in {"item_id", "evidence"}}
        | {"evidence": [{"quote": "共同技术原文。"}]}
        for number, rule in enumerate(rules, 1)
    }
    encoded = json.dumps(payload, ensure_ascii=False)
    assert not any(value in encoded for value in ("permanent-item", "permanent-range", reference["sha256"],
                                                  "artifact_ref", "source_index_ref"))
    assert payload["rule_dispositions"] == [] and payload["submission_rules"] == []
    assert not gaps and current == original


def test_exact_rule_evidence_reuses_only_its_frozen_support_mapping(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    excerpts = whole_source_excerpts(artifacts, source(artifacts, text="共同技术原文。"))
    _rules, reference, refs = rule_fixture(artifacts, ["保留明确的源范围。"])
    current = outline([assigned_target("actual", refs, excerpts)])
    current["authoring_input"]["extract_refs"] = [reference]
    original_context = authoring_structure._OutlineContext

    def context(*args):
        value = original_context(*args)
        value.excerpts["permanent-range-1"] = deepcopy(excerpts)
        return value

    monkeypatch.setattr(authoring_structure, "_OutlineContext", context)
    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)

    support = payload["chapters"][0]["targets"][0]["source_support_ids"]
    assert payload["target_rules"]["r1"]["evidence"] == [{"support_ids": support}]
    assert payload["supports"][0]["text"] == "共同技术原文。" and not gaps


def test_same_words_in_another_source_do_not_replace_the_rule_quote(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    excerpts = whole_source_excerpts(artifacts, source(artifacts, text="共同技术原文。"))
    alternate = whole_source_excerpts(artifacts, source(artifacts, text="另一个位置：共同技术原文。"))
    _rules, reference, refs = rule_fixture(artifacts, ["完整引文与来源不能按相似度替换。"])
    current = outline([assigned_target("actual", refs, excerpts)])
    current["authoring_input"]["extract_refs"] = [reference]
    original_context = authoring_structure._OutlineContext

    def context(*args):
        value = original_context(*args)
        value.excerpts["permanent-range-1"] = deepcopy(alternate)
        return value

    monkeypatch.setattr(authoring_structure, "_OutlineContext", context)
    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)
    assert payload["target_rules"]["r1"]["evidence"] == [{"quote": "共同技术原文。"}]
    assert not gaps


def test_missing_assigned_rule_keeps_the_target_and_reports_a_local_gap(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    missing = {"item_ref": {"artifact_ref": artifacts.put_json({"items": []}), "item_id": "missing-private-item"}}
    current = outline([assigned_target("actual-private-target", [missing])])
    original = deepcopy(current)

    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)

    assert payload["chapters"][0]["targets"][0]["rule_ids"] == []
    assert payload["target_rules"] == {}
    assert gaps == [{"code": "structure_target_rule_unavailable", "detail": "本位置部分规则的当前语义或引文无法读取，保留规则引用及结构待核。",
                     "chapter_id": "business", "target_id": "actual-private-target"}]
    assert any(issue["code"] == "structure_target_rule_unavailable" and issue["target_id"] == "v1"
               for issue in payload["input_issues"])
    assert "missing-private-item" not in json.dumps(payload) and "actual-private-target" not in json.dumps(payload)
    assert current == original


def test_missing_evidence_keeps_readable_rule_semantics_and_marks_it_unverified(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    rules, _reference, _refs = rule_fixture(artifacts, ["引文未取得，不能伪装已经核对。"])
    rules[0]["evidence"] = []
    reference = artifacts.put_json({"items": rules})
    ref = {"item_ref": {"artifact_ref": reference, "item_id": rules[0]["item_id"]}}
    current = outline([assigned_target("actual", [ref])])
    current["authoring_input"]["extract_refs"] = [reference]

    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)

    assert payload["target_rules"]["r1"]["text"] == rules[0]["text"]
    assert payload["target_rules"]["r1"]["evidence"] == []
    assert [gap["code"] for gap in gaps] == ["structure_target_rule_unavailable"]


def test_response_row_rules_keep_their_specific_mapping(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    excerpts = whole_source_excerpts(artifacts, source(artifacts, text="共同技术原文。"))
    _rules, reference, refs = rule_fixture(artifacts, ["第一行专用条件。", "第二行专用条件。"])
    selected = assigned_target("actual", [refs[0]], excerpts)
    selected["response_table"] = {"rows": [
        {"row_id": "private-row-1", "source_excerpts": excerpts, "rule_refs": [refs[0]]},
        {"row_id": "private-row-2", "source_excerpts": excerpts, "rule_refs": [refs[1]]},
    ]}
    current = outline([selected])
    current["authoring_input"]["extract_refs"] = [reference]

    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)

    item = payload["chapters"][0]["targets"][0]
    assert item["rule_ids"] == ["r1"]
    assert [row["rule_ids"] for row in item["response_rows"]] == [["r1"], ["r2"]]
    assert set(payload["target_rules"]) == {"r1", "r2"} and not gaps


def test_assigned_rule_uses_the_current_effective_semantics_not_the_old_item(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    reference = {"item_ref": {"item_id": "private-old-item", "artifact_ref": artifacts.put_json({"items": []})},
                 "decision_id": "private-retained-decision", "rule_index": 0}
    current_rule = {"text": "仅保留制造商维护职责。", "scope": "制造商",
                    "evidence": [{"range_id": "private-range", "quote": "制造商须维护。"}]}
    context = SimpleNamespace(
        rules={"rule-1": reference},
        payload={"rules": [{"rule_id": "rule-1", "item": {"text": "已被替代的全部义务。"},
                             "effective_rule": current_rule}]},
        availability_issues=[],
    )
    monkeypatch.setattr(authoring_structure, "_OutlineContext", lambda *_args: context)
    current = outline([assigned_target("actual", [reference])])

    payload, gaps = authoring_structure._structure_payload(artifacts, current, [], outline_phase=True)

    assert payload["target_rules"] == {"r1": {"text": current_rule["text"], "scope": "制造商",
                                            "evidence": [{"quote": "制造商须维护。"}]}}
    assert "已被替代" not in json.dumps(payload, ensure_ascii=False) and not gaps
