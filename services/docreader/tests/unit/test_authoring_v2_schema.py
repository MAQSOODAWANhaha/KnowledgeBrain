"""Wire support choices stay separate from host-owned persisted references."""

import pytest
from pydantic import ValidationError

from docreader.tender.schema import (
    AuthoringBasisContent, AuthoringBasisResponse, AuthoringBlock, OutlineResponse, OutlineTargetResponse,
    WritingResponse,
)


def target(**values):
    return {"kind": "narrative", "purpose": "本项目背景", "rule_ids": [], "range_ids": [], **values}


def test_outline_accepts_bounded_search_intents_and_allowed_support_origins():
    result = OutlineResponse.model_validate({"schema_version": 2, "chapters": [{"title": "项目说明",
        "targets": [target(support_origins=["tender", "knowledge"])], "children": []}],
        "findings": [], "search_intents": [{"query": "角色权限管理", "for_ids": ["e1", "r1"]}]}).model_dump()
    assert result["search_intents"] == [{"query": "角色权限管理", "for_ids": ["e1", "r1"]}]
    assert result["chapters"][0]["targets"][0]["support_origins"] == ["tender", "knowledge"]


@pytest.mark.parametrize("intent", [
    {"query": " ", "for_ids": ["e1"]}, {"query": "甲" * 257, "for_ids": ["e1"]},
    {"query": "角色权限管理", "for_ids": []},
])
def test_invalid_search_intents_cannot_masquerade_as_a_valid_search_plan(intent):
    with pytest.raises(ValidationError):
        OutlineResponse.model_validate({"schema_version": 2, "chapters": [], "findings": [],
                                        "search_intents": [intent]})


def test_support_origins_are_defaulted_and_deduplicated_without_a_new_business_state():
    assert OutlineTargetResponse.model_validate(target()).support_origins == ["knowledge"]
    assert OutlineTargetResponse.model_validate(target(support_origins=["tender", "tender"])).support_origins == ["tender"]
    for invalid in ([], ["unknown"], ["tender", "knowledge", "tender"]):
        with pytest.raises(ValidationError):
            OutlineTargetResponse.model_validate(target(support_origins=invalid))


def test_writer_only_selects_support_refs_while_persisted_blocks_keep_formal_refs():
    response = WritingResponse.model_validate({"blocks": [{"kind": "paragraph", "text": "项目位于甲园区。",
        "target_ids": ["t1"], "support_ids": ["s1"]}], "attachment_ids": ["k1"]}).model_dump()
    assert response["blocks"][0]["support_ids"] == ["s1"]
    assert "material_ids" not in response["blocks"][0] and "source_refs" not in response["blocks"][0]
    assert response["attachment_ids"] == ["k1"]
    old = AuthoringBlock.model_validate({"kind": "paragraph", "text": "旧稿"}).model_dump()
    assert old["source_refs"] == [] and old["material_ids"] == []
    with pytest.raises(ValidationError):
        WritingResponse.model_validate({"blocks": [{"kind": "paragraph", "material_ids": ["permanent-id"]}]})


def test_shared_arrangement_wire_refs_and_persisted_material_refs_are_distinct():
    arrangement = {"topic": "访问控制", "text": "按角色分配权限。", "nature": "provided"}
    wire = AuthoringBasisResponse.model_validate({"shared_arrangements": [
        {**arrangement, "support_ids": ["k1"]}]}).model_dump()
    persisted = AuthoringBasisContent.model_validate({"shared_arrangements": [
        {**arrangement, "material_ids": ["permissions-document"]}]}).model_dump()
    assert wire["shared_arrangements"][0]["support_ids"] == ["k1"]
    assert persisted["shared_arrangements"][0]["material_ids"] == ["permissions-document"]
    with pytest.raises(ValidationError):
        AuthoringBasisResponse.model_validate(persisted)
