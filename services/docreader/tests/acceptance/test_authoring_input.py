"""Confirmed analysis input with real PostgreSQL and explicitly synthetic platform_models."""

import json
from argparse import Namespace
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.cli import run as cli_run
from docreader.integrations.store import InvalidConfirmation
from docreader.tender.authoring import _OutlineContext
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_requirements import preserve_requirements

from test_analysis import analysis_runtime as analysis_runtime, execute_analysis, result


pytestmark = pytest.mark.integration


def expectation(row):
    return {key: row[key] for key in ("run_id", "source_version", "domain_revision", "checklist_digest")}


def root_reference(row, stage):
    return next(entry["artifact_ref"] for entry in row["artifacts"]
                if entry["stage"] == stage and not entry["scope"])


async def test_confirmed_missing_attachment_requires_source_revision_and_preserves_draft_input(
        analysis_runtime, monkeypatch, tmp_path):
    store, artifacts, settings, fixture_run, calls = analysis_runtime
    monkeypatch.setenv("TENDER_DATABASE_URL", settings.dsn)
    monkeypatch.setenv("TENDER_REDIS_URL", settings.redis_url)
    monkeypatch.setenv("TENDER_ARTIFACT_ROOT", str(artifacts.root))
    main, annex = artifacts.read_json(fixture_run["manifest_ref"])["documents"]
    manifest = tmp_path / "manifest.json"

    def write_manifest(documents, expected=()):
        manifest.write_text(json.dumps({"documents": documents, "expected_document_ids": list(expected)}))

    write_manifest([main])
    submitted = await cli_run(Namespace(command="submit", manifest=manifest, request_key="source-main-only"))
    assert submitted is not None
    run_id = submitted["run_id"]
    runtime = store, artifacts, settings, {"run_id": run_id}, calls
    initial = await execute_analysis(runtime)
    assert initial["status"] == "waiting-confirmation"

    write_manifest([main], expected=[annex["document_id"]])
    revised = await cli_run(Namespace(command="revise", run_id=run_id, manifest=manifest, request_key="missing-annex"))
    assert revised is not None
    assert revised["source_version"] == 2
    missing_issue = {"code": "missing_expected_document", "document_id": annex["document_id"],
                     "detail": "", "blocking": True}
    assert revised["issues"] == [missing_issue]
    blocked = await execute_analysis(runtime)
    assert blocked["status"] == "blocked"
    verification = result(artifacts, blocked, "verify")
    issue, = [item for item in verification["issues"] if item["code"] == missing_issue["code"]]
    assert verification["hard_blocking"] is True and issue["issue_id"] not in verification["human_issue_ids"]
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run_id, 2, blocked["checklist_digest"], "must-supply-annex", domain_revision=1)
    pinned_draft = await read_authoring_input(store, artifacts, **expectation(blocked))
    assert pinned_draft["confirmation_ref"] is None
    assert pinned_draft["frozen_ref"] == root_reference(blocked, "freeze")
    assert pinned_draft["verification_ref"] == root_reference(blocked, "verify")
    assert any(item["code"] == "authoring_analysis_issues" for item in pinned_draft["issues"])

    write_manifest([main, annex], expected=[annex["document_id"]])
    restored = await cli_run(Namespace(command="revise", run_id=run_id, manifest=manifest, request_key="supply-annex"))
    assert restored is not None
    assert restored["source_version"] == 3 and restored["issues"] == []
    waiting = await execute_analysis(runtime)
    assert waiting["status"] == "waiting-confirmation"
    assert waiting["checklist_digest"] != blocked["checklist_digest"]
    assert result(artifacts, waiting, "verify")["hard_blocking"] is False
    await store.confirm(run_id, 3, waiting["checklist_digest"], "confirm-complete-source", domain_revision=1)
    ready = await execute_analysis(runtime)
    assert ready["status"] == "analysis-ready"
    new_input = await read_authoring_input(store, artifacts, **expectation(ready))
    assert new_input["confirmation_ref"] == root_reference(ready, "confirm")
    assert pinned_draft["frozen_ref"] != new_input["frozen_ref"]

    def draft_paragraphs(snapshot):
        outline = {"authoring_input": snapshot, "chapters": []}
        requirements = preserve_requirements(_OutlineContext(artifacts, snapshot), outline)
        exported = export_authoring_document(artifacts, outline, [], requirements=requirements)
        document = Document(BytesIO(artifacts.read_bytes(exported["docx_ref"])))
        return [paragraph.text for paragraph in document.paragraphs]

    old_paragraphs = draft_paragraphs(pinned_draft)
    new_paragraphs = draft_paragraphs(new_input)
    assert "负责人须持A证" in old_paragraphs and "服务响应不超过2小时" not in old_paragraphs
    assert new_paragraphs.index("尚未归入响应表") < new_paragraphs.index("服务响应不超过2小时")
    assert all(request["metadata"]["external_request"] is False for request in ready["usage"])


async def test_authoring_reads_available_current_analysis_without_model_or_state_changes(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime)
    assert waiting["status"] == "waiting-confirmation"
    before_confirm = len(calls)
    pending_input = await read_authoring_input(store, artifacts, **expectation(waiting))
    assert pending_input["confirmation_ref"] is None and pending_input["issues"]

    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "authoring-input", domain_revision=1)
    ready = await execute_analysis(analysis_runtime)
    assert ready["status"] == "analysis-ready" and len(calls) == before_confirm
    files = set(artifacts.root.rglob("*"))
    value = await read_authoring_input(store, artifacts, **expectation(ready))
    assert value == {
        "schema_version": 2, "extract_refs": [], "issues": [],
        **expectation(ready),
        "frozen_ref": root_reference(ready, "freeze"),
        "pack_plan_ref": root_reference(ready, "pack-plan"),
        "items_ref": root_reference(ready, "extract"),
        "verification_ref": root_reference(ready, "verify"),
        "confirmation_ref": root_reference(ready, "confirm"),
    }
    assert await read_authoring_input(store, artifacts, **expectation(ready)) == value
    assert await store.get_run(run["run_id"]) == ready
    assert set(artifacts.root.rglob("*")) == files
    assert len(calls) == before_confirm
    assert all(request["metadata"]["external_request"] is False for request in ready["usage"])


async def test_acknowledged_semantic_issues_remain_visible_to_authoring(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime, blocked=True)
    verification = result(artifacts, waiting, "verify")
    assert verification["blocking"] is True and verification["hard_blocking"] is False
    pending = verification["human_issue_ids"]
    assert pending
    resolutions = {"schema_version": 1, "resolutions": [
        {"issue_id": issue_id, "verdict": "acknowledged", "reason": "合成验收：人工知悉并保留原问题。"}
        for issue_id in sorted(pending)]}
    before = len(calls)
    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "authoring-retained-issues",
                        domain_revision=1, issue_resolutions=resolutions)
    ready = await execute_analysis(analysis_runtime)
    assert ready["status"] == "analysis-ready"

    value = await read_authoring_input(store, artifacts, **expectation(ready))
    retained = artifacts.read_json(value["verification_ref"])
    assert retained == verification
    assert retained["blocking"] is True
    assert retained["human_issue_ids"] == pending
    assert artifacts.read_json(value["confirmation_ref"])["issue_resolutions"] == resolutions
    assert len(calls) == before
    assert (await store.get_run(run["run_id"]))["usage"] == ready["usage"]


async def test_authoring_rejects_old_expected_revision_after_correction_and_reconfirmation(analysis_runtime):
    store, artifacts, _, run, calls = analysis_runtime
    waiting = await execute_analysis(analysis_runtime)
    await store.confirm(run["run_id"], 1, waiting["checklist_digest"], "authoring-old", domain_revision=1)
    old = await execute_analysis(analysis_runtime)
    snapshot = await read_authoring_input(store, artifacts, **expectation(old))
    baseline = snapshot["items_ref"]

    # The existing valid index is a complete unchanged correction; the node
    # revalidates every pack before creating the new revision's analysis.
    await store.correct(run["run_id"], 1, 1, baseline["sha256"], baseline,
                        "authoring-correction", "Reconfirm the unchanged synthetic source interpretation")
    with pytest.raises(InvalidConfirmation):
        await read_authoring_input(store, artifacts, **expectation(old))
    revised = await execute_analysis(analysis_runtime)
    assert revised["domain_revision"] == 2 and revised["status"] == "waiting-confirmation"
    assert revised["history"]["confirmations"] and not revised["confirmations"]
    await store.confirm(run["run_id"], 1, revised["checklist_digest"], "authoring-new", domain_revision=2)
    current = await execute_analysis(analysis_runtime)
    assert current["status"] == "analysis-ready"
    before = len(calls)

    with pytest.raises(InvalidConfirmation):
        await read_authoring_input(store, artifacts, **expectation(old))
    for stale in ({"source_version": 2}, {"checklist_digest": old["checklist_digest"]}):
        with pytest.raises(InvalidConfirmation):
            await read_authoring_input(store, artifacts, **{**expectation(current), **stale})
    value = await read_authoring_input(store, artifacts, **expectation(current))
    assert value["domain_revision"] == 2
    assert value["checklist_digest"] != snapshot["checklist_digest"]
    assert value["frozen_ref"] == snapshot["frozen_ref"]
    assert value["verification_ref"] == root_reference(current, "verify")
    assert value["confirmation_ref"] == root_reference(current, "confirm")
    assert len(calls) == before
