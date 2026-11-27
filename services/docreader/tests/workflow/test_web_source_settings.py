"""Source registration through the project bridge and isolated PostgreSQL."""
import asyncio
import hashlib
from io import BytesIO
from uuid import uuid4

from docx import Document
import pytest

from docreader.integrations.web import WebError, handle_command
from docreader.integrations.authoring import read_authoring_input
from docreader.jobs.settings import Settings
from docreader.tests.acceptance.test_analysis import execute_analysis, result
from docreader.tests.workflow.test_web_analysis import confirmation, correction
from docreader.tests.workflow.test_web_bridge import (
    finish_outline, outline_tree, payload, runtime as runtime,
)


async def upload_source(runtime, title, *, project=None, text=None):
    project = runtime.project if project is None else project
    document = Document()
    document.add_heading(title, level=1)
    document.add_paragraph(text or f"{title}的原文证据。")
    buffer = BytesIO()
    document.save(buffer)
    data = buffer.getvalue()
    document_id = str(uuid4())
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("INSERT INTO tender_documents VALUES(%s,%s,%s,%s,%s,%s)", (
            document_id, project, f"{title}.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            hashlib.sha256(data).hexdigest(), len(data),
        ))
    await handle_command(payload(runtime, "document.put", project_id=project, document_id=document_id), data)
    return document_id


async def settings(runtime):
    value, data = await handle_command(payload(runtime, "sources.status"))
    assert data == b""
    return value["source_settings"]


def entry(document_id, role="main", *, replaces=None, scope=None):
    return {"document_id": document_id, "role": role, "replaces": replaces, "modification_scope": scope}


def save_request(runtime, current, documents, *, expected=(), request_key="sources-save"):
    return payload(runtime, "sources.save", request_key=request_key, expected_version=current["version"],
                   documents=documents, expected_document_ids=list(expected))


async def save(runtime, current, documents, *, expected=(), request_key="sources-save"):
    value, _ = await handle_command(save_request(runtime, current, documents,
                                                 expected=expected, request_key=request_key))
    return value["source_settings"]


@pytest.fixture(autouse=True)
def bridge_settings(runtime, monkeypatch):
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))


async def test_analysis_admission_requires_roles_for_multiple_documents(runtime):
    main = await upload_source(runtime, "招标文件")
    amendment = await upload_source(runtime, "更正公告")
    request = payload(runtime, "analysis.start", request_key="explicit-sources")
    with pytest.raises(WebError) as unconfirmed:
        await handle_command(request)
    assert (unconfirmed.value.code, unconfirmed.value.status) == ("SOURCE_ROLES_REQUIRED", 409)
    assert await runtime.bridge.latest_run(runtime.project) is None
    draft_run = await runtime.bridge.source_run(runtime.project)
    draft = await read_authoring_input(runtime.bridge.store, runtime.bridge.artifacts, run_id=draft_run["run_id"],
                                       source_version=1, domain_revision=1)
    for document_id in (main, amendment):
        assert any(issue["code"] == "source_roles_unconfirmed" and document_id in issue["detail"]
                   for issue in draft["issues"])

    current = await settings(runtime)
    documents = [entry(main), entry(amendment, "amendment", replaces=main, scope="替换第三节报价期限")]
    saved = await save(runtime, current, documents, expected=["报价模板.xlsx"])
    assert saved["version"] != current["version"]
    assert saved["expected_document_ids"] == ["报价模板.xlsx"]
    assert {row["document_id"]: row["file_name"] for row in saved["documents"]} == {
        main: "招标文件.docx", amendment: "更正公告.docx",
    }
    assert await settings(runtime) == saved

    response, _ = await handle_command(request)
    run = await runtime.bridge.store.get_run(response["analysis"]["run_id"])
    registered = runtime.bridge.artifacts.read_json(run["manifest_ref"])
    observed = {row["document_id"]: {key: row[key] for key in documents[0]} for row in registered["documents"]}
    assert observed == {row["document_id"]: row for row in documents}
    assert any(issue["code"] == "missing_expected_document" and issue["document_id"] == "报价模板.xlsx"
               and issue["blocking"] for issue in registered["issues"])
    source_run = await runtime.bridge.source_run(runtime.project)
    frozen_ref = next(row["artifact_ref"] for row in source_run["artifacts"] if row["stage"] == "freeze")
    frozen = runtime.bridge.artifacts.read_json(frozen_ref)
    assert {row["role"] for row in frozen["documents"]} == {"main", "amendment"}
    assert any(issue["code"] == "missing_expected_document" and issue["blocking"] for issue in frozen["issues"])
    draft = await read_authoring_input(runtime.bridge.store, runtime.bridge.artifacts, run_id=source_run["run_id"],
                                       source_version=1, domain_revision=1)
    assert any(issue["code"] == "missing_expected_document" and "报价模板.xlsx" in issue["detail"]
               for issue in draft["issues"])
    assert not run["usage"] and not source_run["usage"]

    bridge = runtime.bridge
    checked = await execute_analysis((bridge.store, bridge.artifacts, runtime.settings, run, []))
    verification = result(bridge.artifacts, checked, "verify")
    assert checked["status"] == "blocked"
    assert any(issue["code"] == "missing_expected_document" for issue in verification["issues"])
    assert all(receipt["metadata"]["external_request"] is False for receipt in checked["usage"])


async def test_lost_save_cannot_reapply_after_an_intentional_revert(runtime):
    main = await upload_source(runtime, "招标文件")
    original = await save(runtime, await settings(runtime), [entry(main)])
    request = save_request(runtime, original, [entry(main, scope="新范围")], request_key="unknown-result")
    result, _ = await handle_command(request)
    reverted = await save(runtime, result["source_settings"], [entry(main)], request_key="revert")
    assert reverted["version"] != original["version"]
    with pytest.raises(WebError) as stale:
        await handle_command(request)
    assert stale.value.code == "SOURCE_SETTINGS_CONFLICT"
    assert await settings(runtime) == reverted


async def test_concurrent_source_save_preserves_one_project_version(runtime):
    main = await upload_source(runtime, "招标文件")
    current = await settings(runtime)
    requests = [save_request(runtime, current, [entry(main, scope=f"适用范围 {i}")], request_key=f"save-{i}")
                for i in range(2)]
    results = await asyncio.gather(*(handle_command(request) for request in requests), return_exceptions=True)
    success, = [index for index, result in enumerate(results) if isinstance(result, tuple)]
    conflict, = [result for result in results if isinstance(result, WebError)]
    assert (conflict.code, conflict.status) == ("SOURCE_SETTINGS_CONFLICT", 409)
    assert await handle_command(requests[success]) == results[success]
    assert (await settings(runtime))["documents"][0]["modification_scope"] == f"适用范围 {success}"


async def test_upload_invalidates_open_source_settings_and_requires_new_role(runtime):
    main = await upload_source(runtime, "招标文件")
    amendment = await upload_source(runtime, "更正公告")
    documents = [entry(main), entry(amendment, "amendment", scope="适用第三节")]
    saved = await save(runtime, await settings(runtime), documents)
    attachment = await upload_source(runtime, "报价附件")

    with pytest.raises(WebError) as stale:
        await save(runtime, saved, documents, request_key="sources-from-old-tab")
    assert (stale.value.code, stale.value.status) == ("SOURCE_SETTINGS_CONFLICT", 409)
    with pytest.raises(WebError) as unconfirmed:
        await handle_command(payload(runtime, "analysis.start", request_key="new-upload"))
    assert unconfirmed.value.code == "SOURCE_ROLES_REQUIRED"

    current = await settings(runtime)
    assert current["version"] != saved["version"]
    assert next(row for row in current["documents"] if row["document_id"] == amendment)["role"] == "amendment"
    await save(runtime, current, [*documents, entry(attachment, "attachment")], request_key="include-new-upload")
    response, _ = await handle_command(payload(runtime, "analysis.start", request_key="new-upload"))
    run = await runtime.bridge.store.get_run(response["analysis"]["run_id"])
    assert {row["document_id"]: row["role"] for row in runtime.bridge.artifacts.read_json(run["manifest_ref"])["documents"]} == {
        main: "main", amendment: "amendment", attachment: "attachment",
    }


async def test_source_settings_reject_foreign_owner_and_replacement(runtime):
    main = await upload_source(runtime, "招标文件")
    amendment = await upload_source(runtime, "更正公告")
    foreign = await upload_source(runtime, "其他项目", project=runtime.other)
    current = await settings(runtime)
    request = save_request(runtime, current, [entry(main), entry(amendment, "amendment", replaces=foreign)])
    with pytest.raises(WebError) as wrong_owner:
        await handle_command({**request, "owner_user_id": str(uuid4())})
    assert wrong_owner.value.status == 404
    with pytest.raises(WebError) as wrong_target:
        await handle_command(request)
    assert wrong_target.value.status == 400
    assert await settings(runtime) == current


@pytest.mark.parametrize("change", ["modification_scope", "replaces", "expected_document_ids"])
async def test_source_metadata_change_invalidates_analysis_review_and_saved_outline(runtime, monkeypatch, change):
    main = await upload_source(runtime, "招标文件")
    amendment = await upload_source(runtime, "更正公告")
    documents = [entry(main), entry(amendment, "amendment", scope="原范围")]
    original = await save(runtime, await settings(runtime), documents)
    response, _ = await handle_command(payload(runtime, "analysis.start", request_key="analysis-with-sources"))
    run_id = response["analysis"]["run_id"]
    execution_id, _, outline = await finish_outline(runtime, monkeypatch)
    saved = (await runtime.bridge.save_outline(payload(runtime, request_key="pin-source-outline",
        expected_revision_id=None, source_execution_id=execution_id, chapters=outline_tree(outline))))["draft"]
    review = payload(runtime, "analysis.review")
    assert (await handle_command(review))[0]["review"]["sources_current"] is True
    assert saved["sources_current"] is True

    changed = list(documents)
    expected = []
    if change == "modification_scope":
        changed[1] = {**changed[1], "modification_scope": "新范围"}
    elif change == "replaces":
        changed[1] = {**changed[1], "replaces": main}
    else:
        expected = ["缺少的附件.xlsx"]
    updated = await save(runtime, original, changed, expected=expected, request_key="metadata-only-change")
    assert updated["version"] != original["version"]
    assert (await handle_command(review))[0]["review"]["sources_current"] is False
    assert (await runtime.bridge.outline_view(runtime.project))["draft"]["sources_current"] is False
    with pytest.raises(WebError) as stale:
        await handle_command(payload(runtime, "authoring.start", request_key="stale-draft",
                                     outline_revision_id=saved["revision_id"], outline_sha256=saved["sha256"]))
    assert stale.value.code == "SOURCE_CHANGED_REQUIRES_NEW_OUTLINE"

    checklist_digest = "c" * 64
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_analysis_runs SET status='waiting-confirmation',checklist_digest=%s WHERE run_id=%s",
                           (checklist_digest, run_id))
    with pytest.raises(WebError) as stale_confirmation:
        await handle_command(payload(runtime, "analysis.confirm", request_key="stale-confirmation", run_id=run_id,
                                     source_version=1, domain_revision=1, checklist_digest=checklist_digest))
    assert stale_confirmation.value.code == "ANALYSIS_SOURCE_CHANGED"
    assert not (await runtime.bridge.store.get_run(run_id))["confirmations"]


@pytest.mark.parametrize(("action", "body"), [("analysis.confirm", confirmation), ("analysis.correct", correction)])
async def test_source_change_keeps_existing_decision_receipt_without_accepting_new_writes(runtime, action, body):
    main = await upload_source(runtime, "招标文件", text="项目负责人须持A证。")
    started, _ = await handle_command(payload(runtime, "analysis.start", request_key="source-review"))
    run_id = started["analysis"]["run_id"]
    bridge = runtime.bridge
    run = await bridge.store.get_run(run_id)
    waiting = await execute_analysis((bridge.store, bridge.artifacts, runtime.settings, run, []))
    assert waiting["status"] == "waiting-confirmation"
    response, _ = await handle_command(payload(runtime, "analysis.review"))
    decision = payload(runtime, action, request_key="recorded-decision", **body(response["review"]))
    accepted = await handle_command(decision)

    await save(runtime, await settings(runtime), [entry(main, scope="复核后的范围")])
    after_source_change = await bridge.store.get_run(run_id)
    assert await handle_command(decision) == accepted
    with pytest.raises(WebError) as stale:
        await handle_command({**decision, "request_key": "unsubmitted-new-key"})
    assert stale.value.code == "ANALYSIS_SOURCE_CHANGED"
    after_replay = await bridge.store.get_run(run_id)
    assert after_replay["commands"] == after_source_change["commands"]
