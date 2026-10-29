"""Saved Word reviews bind real bytes, project ownership and a fixed generation."""
import hashlib
from uuid import uuid4

import pytest

from docreader.integrations.store import Conflict
from docreader.integrations.web import WebError, execute_authoring_job, handle_command
from docreader.integrations.web_saved_review import review_status, start_review
from docreader.jobs.settings import Settings
from docreader.tests.unit.test_saved_word_review import saved_bytes
from docreader.tests.workflow.test_web_bridge import (
    completed_job, configured, payload, runtime as runtime, upload,
)


async def saved(runtime):
    await upload(runtime)
    execution = await completed_job(runtime)
    version, data = str(uuid4()), saved_bytes()
    sha = hashlib.sha256(data).hexdigest()
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("CREATE TABLE tender_draft_versions(id uuid PRIMARY KEY,project_id uuid,execution_id text,content_sha256 text,byte_length bigint)")
        await conn.execute("INSERT INTO tender_draft_versions VALUES(%s,%s,%s,%s,%s)",
                           (version, runtime.project, execution, sha, len(data)))
    return payload(runtime, "docx.review.start", version_id=version, docx_sha256=sha,
                   reason="核对人工修改的工期", request_key="review-save"), data


async def test_saved_review_pins_actual_human_file_and_original_generation(runtime, monkeypatch):
    configured(monkeypatch)
    request, data = await saved(runtime)
    started = await start_review(runtime.bridge, request, data)
    job = await runtime.bridge.job(runtime.project, started["review"]["execution_id"])
    assert job["payload"]["operation"] == "saved_review"
    assert runtime.bridge.artifacts.read_bytes(job["payload"]["saved_ref"]) == data
    assert started["review"]["version_id"] == request["version_id"]
    assert started["review"]["docx_sha256"] == request["docx_sha256"]
    assert await start_review(runtime.bridge, request, data) == started
    assert await review_status(runtime.bridge, request) == started
    assert (await runtime.bridge.workbench(runtime.project))["authoring"]["execution_id"] != job["execution_id"]


async def test_saved_review_rejects_changed_same_key_and_wrong_project(runtime, monkeypatch):
    configured(monkeypatch)
    request, data = await saved(runtime)
    await start_review(runtime.bridge, request, data)
    with pytest.raises(Conflict):
        await start_review(runtime.bridge, {**request, "reason": "different"}, data)
    with pytest.raises(WebError, match="不属于此项目"):
        await start_review(runtime.bridge, {**request, "project_id": runtime.other}, data)


@pytest.mark.parametrize("change", ["bytes", "sha", "reason"])
async def test_saved_review_rejects_unconfirmed_inputs_without_a_job(runtime, monkeypatch, change):
    configured(monkeypatch)
    request, data = await saved(runtime)
    if change == "bytes":
        data += b"changed"
    elif change == "sha":
        request["docx_sha256"] = "0" * 64
    else:
        request["reason"] = " "
    with pytest.raises(WebError):
        await start_review(runtime.bridge, request, data)
    assert await review_status(runtime.bridge, request) == {"review": None}


async def test_saved_review_worker_receives_actual_saved_ref_without_generating_word(runtime, monkeypatch):
    from docreader.tender import saved_word_review

    configured(monkeypatch)
    request, data = await saved(runtime)
    started = await start_review(runtime.bridge, request, data)
    captured = {}
    async def capture(*args, **kwargs):
        captured.update(kwargs)
        return runtime.bridge.artifacts.put_json({"findings": []})
    monkeypatch.setattr(saved_word_review, "execute_saved_review", capture)
    monkeypatch.setattr("docreader.tender.document_execution.execute_document",
                        lambda *args, **kwargs: pytest.fail("review must never compile a new Word"))
    monkeypatch.setattr("docreader.integrations.web.ModelConfig", object(), raising=False)
    from docreader.llm.config import ModelConfig
    monkeypatch.setattr(ModelConfig, "from_env", classmethod(lambda cls: type("Config", (), {"chat": type("Chat", (), {"context_tokens": 131072, "model": "test"})()})()))
    await execute_authoring_job(started["review"]["execution_id"], runtime.settings)
    assert captured["version_id"] == request["version_id"] and captured["reason"] == request["reason"]
    assert runtime.bridge.artifacts.read_bytes(captured["saved_ref"]) == data


async def test_saved_review_authorization_precedes_saved_bytes_or_source_read(runtime, monkeypatch):
    request, data = await saved(runtime)
    monkeypatch.setattr(Settings, "from_env", classmethod(lambda cls: runtime.settings))
    with pytest.raises(WebError, match="无权访问"):
        await handle_command({**request, "owner_user_id": str(uuid4())}, data)
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_projects SET status='ended' WHERE id=%s", (runtime.project,))
    with pytest.raises(WebError, match="项目已结束"):
        await handle_command(request, data)
