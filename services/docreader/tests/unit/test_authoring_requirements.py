"""Keep known tender requirements independently of generated bidder content."""

from copy import deepcopy
from types import SimpleNamespace

from docreader.tender.authoring_requirements import preserve_requirements
from docreader.tender.schema import AuthoringRequirements


def reference(item_id="item-1", **retained):
    return {"item_ref": {"artifact_ref": {"sha256": "a" * 64,
        "relative_path": "aa/" + "a" * 64, "size": 12, "media_type": "application/json"},
        "item_id": item_id}, **retained}


def item(text="应支持角色权限管理。", **optional):
    return {"item_id": "item-1", "kind": "obligation", "text": text,
        "evidence": [{"range_id": "range-1", "quote": text}], **optional}


def context(refs=(), views=(), *, origin="extraction", issues=()):
    return SimpleNamespace(
        input={"schema_version": 2, "run_id": "requirement-preservation",
            "source_version": 1, "domain_revision": 1},
        rules=dict(refs), payload={"rules": list(views)}, rule_origin=origin,
        availability_issues=list(issues),
        sources={"background": [{"unit_id": "review-grid", "text": "背景及评审表，不是应答格式"}]})


def outline(*targets):
    return {"chapters": [{"chapter_id": "chapter-1", "targets": [], "children": [
        {"chapter_id": "chapter-1.1", "children": [], "targets": [
            {"target_id": identity, "rule_refs": refs, "source_refs": []}
            for identity, refs in targets]}]}]}


def test_retained_rules_keep_their_full_identity_without_restoring_deleted_original():
    first = reference(decision_id="amendment-1", rule_index=0)
    second = reference(decision_id="amendment-1", rule_index=1)
    retained = [{"text": text, "scope": scope,
        "evidence": [{"range_id": "amended-range", "quote": text}]}
        for text, scope in (("保留硬件接口要求。", "硬件"), ("保留服务期限要求。", "服务"))]
    value = context([("r1", first), ("r2", second)], [
        {"rule_id": "r1", "item_id": "item-1", "effective_rule": retained[0],
         "item": item("已被补遗删除的旧要求。")},
        {"rule_id": "r2", "item_id": "item-1", "effective_rule": retained[1]}],
        origin="verified_projection")

    result = preserve_requirements(value, outline(("target-1", [first, second])))

    assert [entry["rule_ref"] for entry in result["rules"]] == [first, second]
    assert [entry["rule"] for entry in result["rules"]] == retained
    assert all(entry["target_ids"] == ["target-1"] for entry in result["rules"])
    assert "已被补遗删除" not in str(result)
    assert result["rule_origin"] == "verified_projection"
    AuthoringRequirements.model_validate(result)


def test_all_known_rules_survive_with_unassigned_rows_and_reference_deduplication():
    shared, unassigned = reference(), reference("unassigned")
    value = context([("r1", shared), ("duplicate", deepcopy(shared)), ("r2", unassigned)], [
        {"rule_id": "r1", "item": item()},
        {"rule_id": "duplicate", "item": item()},
        {"rule_id": "r2", "item": {**item("应提交实际业绩证明。"), "item_id": "unassigned"}}])

    result = preserve_requirements(value, outline(
        ("target-1", [shared, deepcopy(shared)]), ("target-2", [shared])))

    assert len(result["rules"]) == 2
    assert result["rules"][0]["target_ids"] == ["target-1", "target-2"]
    assert result["rules"][1]["target_ids"] == []
    assert result["rules"][1]["rule"]["text"] == "应提交实际业绩证明。"


def test_unknown_target_requirement_keeps_its_reference_and_location_without_guessing():
    missing = reference("unreadable")

    result = preserve_requirements(context(), outline(("target-1", [missing])))

    assert result["rules"] == [{"rule_ref": missing, "rule": None, "target_ids": ["target-1"]}]
    assert any(issue["code"] == "authoring_requirement_unavailable" for issue in result["issues"])
    AuthoringRequirements.model_validate(result)


def test_missing_semantic_view_keeps_an_unassigned_formal_requirement():
    known = reference()

    result = preserve_requirements(context([("r1", known)]), outline())

    assert result["rules"] == [{"rule_ref": known, "rule": None, "target_ids": []}]
    assert result["issues"]


def test_source_only_keeps_availability_issues_without_inserting_background_or_review_tables():
    issue = {"code": "source_missing", "detail": "部分冻结原文暂不可读。"}
    value = context(origin="source_only", issues=[issue])
    before = deepcopy(value.sources)

    result = preserve_requirements(value, outline(("target-1", [])))

    assert result["rules"] == [] and result["rule_origin"] == "source_only"
    assert issue in result["issues"]
    assert any(entry["code"] == "authoring_requirements_source_only" for entry in result["issues"])
    assert "背景及评审表" not in str(result)
    assert value.sources == before


def test_preserved_requirement_and_input_are_deep_copies():
    ref = reference()
    original = item(conditions=["适用时"], exceptions=["原文明示豁免时除外"], scope="本标段")
    value = context([("r1", ref)], [{"rule_id": "r1", "item": original}])
    planned = outline(("target-1", [ref]))
    before = deepcopy((value.input, value.rules, value.payload, planned))

    result = preserve_requirements(value, planned)
    result["authoring_input"]["run_id"] = "changed"
    result["rules"][0]["rule"]["evidence"][0]["quote"] = "changed"
    result["rules"][0]["rule"]["conditions"].append("changed")
    result["rules"][0]["rule_ref"]["item_ref"]["item_id"] = "changed"
    result["rules"][0]["target_ids"].append("another")

    assert (value.input, value.rules, value.payload, planned) == before


def test_malformed_legacy_semantic_rule_does_not_discard_its_valid_sibling_or_block_generation():
    malformed, valid = reference("legacy"), reference()
    value = context([("old", malformed), ("r1", valid)], [
        {"rule_id": "old", "item": {**item(), "item_id": "legacy", "conditions": []}},
        {"rule_id": "r1", "item": item()}])

    result = preserve_requirements(value, outline())

    assert result["rules"][0] == {"rule_ref": malformed,
        "rule": value.payload["rules"][0]["item"], "target_ids": []}
    assert result["rules"][1]["rule"]["text"] == item()["text"]
    assert result["issues"]
    AuthoringRequirements.model_validate(result)


def test_readable_incomplete_and_extended_legacy_rules_are_preserved_in_full():
    incomplete, extended = reference("incomplete"), reference("extended")
    raw = [{"text": "可读取但还没有完整抽取结构的要求。"},
        {**item(), "item_id": "extended", "legacy_details": {"all": ["原始扩展字段"]}}]
    value = context([("r1", incomplete), ("r2", extended)], [
        {"rule_id": "r1", "item": raw[0]}, {"rule_id": "r2", "item": raw[1]}])

    result = preserve_requirements(value, outline())

    assert [row["rule"] for row in result["rules"]] == raw
    assert any(issue["code"] == "authoring_requirement_contract_unverified" for issue in result["issues"])
    AuthoringRequirements.model_validate(result)


def test_valid_projection_round_trips_without_inserting_forbidden_optional_nulls():
    ref = reference()
    value = context([("r1", ref)], [{"rule_id": "r1", "item": item()}])

    result = preserve_requirements(value, outline(("target-1", [ref])))

    assert "decision_id" not in result["rules"][0]["rule_ref"]
    assert "conditions" not in result["rules"][0]["rule"]
    assert AuthoringRequirements.model_validate(result).model_dump(exclude_unset=True) == result


def test_host_optional_nulls_do_not_change_reference_identity_or_lose_target_binding():
    ref = reference()
    copied = {**deepcopy(ref), "decision_id": None, "rule_index": None}
    value = context([("r1", ref), ("duplicate", copied)], [
        {"rule_id": "r1", "item": item()}, {"rule_id": "duplicate", "item": item()}])

    result = preserve_requirements(value, outline(("target-1", [copied])))

    assert result["rules"] == [{"rule_ref": ref, "rule": item(), "target_ids": ["target-1"]}]
    assert not result["issues"]
