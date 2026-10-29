"""Saved Word review identifies its original frozen analysis after sources change."""
from docreader.integrations.web import WebError
from docreader.integrations.web_saved_review import review_status, review_view, start_review
from docreader.tests.workflow.test_web_bridge import configured, runtime as runtime, upload
from docreader.tests.workflow.test_web_saved_review import saved


async def test_saved_review_preserves_original_analysis_and_exposes_changed_sources(runtime, monkeypatch):
    configured(monkeypatch)
    request, data = await saved(runtime)
    started = await start_review(runtime.bridge, request, data)
    job = await runtime.bridge.job(runtime.project, started["review"]["execution_id"])
    original = job["payload"]["identity"]
    assert started["review"]["analysis_identity"] == original
    assert started["review"]["sources_current"] is True

    await upload(runtime)
    changed = await review_status(runtime.bridge, request)
    assert changed["review"]["analysis_identity"] == original
    assert changed["review"]["sources_current"] is False
    assert changed["review"]["version_id"] == request["version_id"]
    assert changed["review"]["docx_sha256"] == request["docx_sha256"]
    assert (await start_review(runtime.bridge, request, data))["review"] == changed["review"]


async def test_saved_review_does_not_claim_current_sources_when_comparison_fails(runtime, monkeypatch):
    configured(monkeypatch)
    request, data = await saved(runtime)
    await start_review(runtime.bridge, request, data)

    async def unavailable(*_args):
        raise WebError("TENDER_DOCUMENT_REQUIRED", "项目来源暂不可读取。", 409)
    monkeypatch.setattr(runtime.bridge, "manifest", unavailable)
    value = (await review_status(runtime.bridge, request))["review"]
    assert value["sources_current"] is None
    assert value["source_status_issue"]["code"] == "saved_word_source_status_unavailable"
    assert (await start_review(runtime.bridge, request, data))["review"] == value


async def test_missing_frozen_source_is_unknown_instead_of_a_claim_that_sources_changed(runtime, monkeypatch):
    configured(monkeypatch)
    request, data = await saved(runtime)
    started = await start_review(runtime.bridge, request, data)
    job = await runtime.bridge.job(runtime.project, started["review"]["execution_id"])
    job["payload"]["snapshot"].pop("frozen_ref", None)
    value = (await review_view(runtime.bridge, job))["review"]
    assert value["sources_current"] is None
    assert value["source_status_issue"]["code"] == "saved_word_source_status_unavailable"
