"""Tests for projecting selectable main-document project context sources to the outline workbench."""

from __future__ import annotations

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.web_outline import project_context_sources
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.outline_edit import apply_outline_edit
from docreader.tender.status import DocumentRole


def _build_index(artifacts: ArtifactStore, *, role: DocumentRole = DocumentRole.MAIN, doc_id: str = "main"):
    raw = {
        "schema_version": 2,
        "units": [
            {
                "unit_id": "u-bg",
                "kind": "section",
                "ordinal": 0,
                "text": "本项目建设覆盖云平台、互联网及办公终端。\n   \n",
                "grid": None,
                "locator": {"locator_kind": "pdf", "page_ordinal": 4, "heading_path": "第一章 项目背景"},
            },
            {
                "unit_id": "u-empty",
                "kind": "section",
                "ordinal": 1,
                "text": "   ",
                "grid": None,
                "locator": {"locator_kind": "pdf", "page_ordinal": 5, "heading_path": "空白节"},
            },
        ],
        "images": {},
        "ocr": {},
        "source_contract": {
            "page_manifest": [],
            "units": [
                {
                    "unit_id": "u-bg",
                    "section_id": "section:0",
                    "parent_section_id": None,
                    "heading_path": "第一章 项目背景",
                    "physical_locator": {"locator_kind": "pdf", "page_ordinal": 4},
                },
                {
                    "unit_id": "u-empty",
                    "section_id": "section:1",
                    "parent_section_id": None,
                    "heading_path": "空白节",
                    "physical_locator": {"locator_kind": "pdf", "page_ordinal": 5},
                },
            ],
        },
    }
    raw_ref = artifacts.put_json(raw)
    doc = {
        "document_id": doc_id,
        "document_version": "v1",
        "role": role,
        "original_ref": artifacts.put_bytes(b"synthetic tender"),
    }
    index = SourceIndex(doc, raw)
    index_ref = artifacts.put_json(index.descriptor(raw_ref))
    return {"document_id": doc_id, "document_version": "v1", "role": role, "original_ref": doc["original_ref"], "raw_ref": raw_ref, "source_index_ref": index_ref}


def _outline(frozen_ref: dict | None) -> dict:
    return {
        "schema_version": 2,
        "authoring_input": {
            "schema_version": 2,
            "run_id": "run-1",
            "source_version": 1,
            "domain_revision": 1,
            **({"frozen_ref": frozen_ref} if frozen_ref is not None else {}),
        },
        "chapters": [
            {
                "chapter_id": "chapter-1",
                "title": "项目服务方案",
                "is_volume": False,
                "presentation_role": "body",
                "targets": [
                    {
                        "target_id": "target-1",
                        "kind": "narrative",
                        "purpose": "结合项目现状编制运维方案",
                        "rule_refs": [],
                        "source_refs": [],
                        "template_refs": [],
                        "manual_fill": False,
                        "support_origins": ["knowledge"],
                        "related_target_ids": [],
                    }
                ],
                "children": [],
            }
        ],
        "issues": [],
    }


def test_project_context_sources_projects_valid_main_excerpts_usable_in_outline_save(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    main_doc = _build_index(artifacts, role=DocumentRole.MAIN, doc_id="main")
    frozen_ref = artifacts.put_json({"documents": [main_doc]})
    outline = _outline(frozen_ref)

    projected = project_context_sources(artifacts, outline)
    assert projected["available"] is True
    assert projected["reason"] is None
    assert projected["frozen_sha256"] == frozen_ref["sha256"]
    assert len(projected["options"]) == 1
    opt = projected["options"][0]
    assert opt["page"] == 5
    assert opt["heading_path"] == "第一章 项目背景"
    assert "本项目建设覆盖云平台" in opt["preview"]

    saved = apply_outline_edit(
        outline,
        [
            {
                "chapter_id": "chapter-1",
                "title": "项目服务方案",
                "source_additions": [
                    {"target_id": "target-1", "project_context_excerpts": [opt["excerpt"]]}
                ],
                "children": [],
            }
        ],
        artifacts=artifacts,
    )
    assert saved["chapters"][0]["targets"][0]["project_context_excerpts"] == [opt["excerpt"]]


@pytest.mark.parametrize("extra_role", [DocumentRole.AMENDMENT, DocumentRole.CLARIFICATION])
def test_project_context_sources_blocks_when_amendment_or_clarification_present(tmp_path, extra_role):
    artifacts = ArtifactStore(tmp_path)
    main_doc = _build_index(artifacts, role=DocumentRole.MAIN, doc_id="main")
    extra_doc = _build_index(artifacts, role=extra_role, doc_id="supp")
    frozen_ref = artifacts.put_json({"documents": [main_doc, extra_doc]})

    projected = project_context_sources(artifacts, _outline(frozen_ref))
    assert projected["available"] is False
    assert projected["reason"] == "主件有补遗或澄清，请核对适用范围后再绑定方案背景。"
    assert projected["options"] == []


def test_project_context_sources_handles_missing_frozen_ref(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    projected = project_context_sources(artifacts, _outline(None))
    assert projected["available"] is False
    assert projected["frozen_sha256"] is None
    assert projected["options"] == []


def test_unconfirmed_source_roles_prevent_listing_and_binding_project_context(tmp_path):
    from docreader.integrations.artifacts import ArtifactIntegrityError
    from docreader.tender.source_body import load_source_context

    artifacts = ArtifactStore(tmp_path)
    main = _build_index(artifacts, role=DocumentRole.MAIN, doc_id="main")
    other = _build_index(artifacts, role=DocumentRole.MAIN, doc_id="other")
    main_outline = _outline(artifacts.put_json({"documents": [main]}))
    option, = project_context_sources(artifacts, main_outline)["options"]
    pending = _outline(artifacts.put_json({
        "documents": [main, other],
        "issues": [{"code": "source_roles_unconfirmed", "document_id": "other"}],
    }))
    projected = project_context_sources(artifacts, pending)
    assert not projected["available"] and not projected["options"]
    target = pending["chapters"][0]["targets"][0]
    target["project_context_excerpts"] = [option["excerpt"]]
    with pytest.raises(ArtifactIntegrityError, match="来源角色"):
        load_source_context(artifacts, pending["authoring_input"], target)
