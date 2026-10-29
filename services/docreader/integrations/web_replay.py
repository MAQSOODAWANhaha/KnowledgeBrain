"""Read existing receipts after project closure, without resubmitting work."""
import asyncio
import hashlib
import json
from uuid import UUID

from docreader.integrations.store import Conflict
from docreader.integrations.web import (
    WebError, _analysis_request, _authoring_request, _digest, _identity, _outline_request, _request_key,
)


def _ended():
    raise WebError("PROJECT_ENDED", "项目已结束，仅可确认已有请求或读取已有文件。", 409)


def _same(actual, expected):
    if actual != expected:
        raise Conflict("request key already identifies a different submitted request")


class _ReadOnlyArtifacts:
    """Reuse correction validation while computing references entirely in memory."""
    def __init__(self, artifacts):
        self.read_json = artifacts.read_json

    def put_json(self, value):
        data = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
        digest = hashlib.sha256(data).hexdigest()
        return {"sha256": digest, "relative_path": f"{digest[:2]}/{digest}", "size": len(data), "media_type": "application/json"}


async def replay_ended(bridge, payload):
    action, project = payload["action"], payload["project_id"]
    if action not in {"authoring.start", "outline.start", "docx.review.start", "authoring.resume", "outline.save",
                      "analysis.start", "analysis.confirm", "analysis.correct", "analysis.retry"}:
        _ended()
    key = _request_key(payload)
    if action in {"authoring.start", "outline.start", "docx.review.start", "authoring.resume"}:
        prefix = {"authoring.start": "web-document-", "outline.start": "web-outline-", "docx.review.start": "web-saved-review-"}
        execution_id = payload.get("execution_id") if action == "authoring.resume" else prefix[action] + key
        try:
            job = await bridge.job(project, execution_id)
        except WebError as error:
            if error.code != "EXECUTION_NOT_FOUND":
                raise
            _ended()
        if action == "authoring.resume":
            if key not in job["resume_keys"]:
                _ended()
            return {"authoring": await bridge.authoring_view(job)}
        if action == "docx.review.start":
            from docreader.integrations.web_saved_review import review_request, review_view
            _same(job["payload"]["request"], review_request(payload))
            return await review_view(bridge, job)
        _same(job["payload"]["request"], _authoring_request(payload, outline_only=action == "outline.start"))
        return {"generation" if action == "outline.start" else "authoring": await bridge.authoring_view(job)}
    if action == "outline.save":
        async with bridge.store.transaction() as conn:
            saved = await (await conn.execute("SELECT * FROM tender_outline_revisions WHERE project_id=%s AND request_key=%s",
                                              (project, key))).fetchone()
        if saved is None:
            _ended()
        _same(saved["request_digest"], _digest(_outline_request(payload)))
        return {"draft": await bridge.outline_draft(saved)}
    if action == "analysis.start":
        _analysis_request(payload)
        async with bridge.store.transaction() as conn:
            saved = await (await conn.execute("SELECT run_id FROM tender_project_analyses WHERE project_id=%s "
                "AND request_key=%s AND NOT source_only", (project, key))).fetchone()
        if saved is None:
            _ended()
        return {"analysis": await bridge.analysis_view(await bridge.store.get_run(saved["run_id"]))}
    if action in {"analysis.confirm", "analysis.correct", "analysis.retry"}:
        from docreader.integrations.web_analysis import (
            _corrected_ref, _digest as analysis_digest, _fields, _owned_run, _positive, correction_request,
        )
        from docreader.tender.schema import validate_issue_resolutions

        run = await _owned_run(bridge, payload)
        async with bridge.store.transaction() as conn:
            if action == "analysis.confirm":
                saved = await (await conn.execute("SELECT * FROM tender_analysis_confirmations WHERE run_id=%s AND request_key=%s",
                                                  (run["run_id"], key))).fetchone()
            elif action == "analysis.correct":
                saved = await (await conn.execute("SELECT * FROM tender_analysis_revisions WHERE run_id=%s AND request_key=%s",
                                                  (run["run_id"], key))).fetchone()
            else:
                saved = await (await conn.execute("SELECT * FROM tender_analysis_commands WHERE run_id=%s "
                    "AND payload->'retry'->>'request_key'=%s", (run["run_id"], key))).fetchone()
        if saved is None:
            _ended()
        if action == "analysis.correct":
            source, revision, digest, _, reason, changes = correction_request(payload)
            reference = await asyncio.to_thread(_corrected_ref, _ReadOnlyArtifacts(bridge.artifacts), run, source, revision, digest, changes)
            _same((saved["source_version"], saved["domain_revision"], saved["upstream_digest"], saved["reason"], saved["corrected_items_ref"]),
                  (source, revision + 1, digest, reason, reference))
            return {"analysis": {**_identity(run), "status": run["status"]},
                    "submitted_source_version": source, "submitted_domain_revision": saved["domain_revision"]}
        source, revision = _positive(payload, "source_version"), _positive(payload, "domain_revision")
        _same((saved["source_version"], saved["domain_revision"]), (source, revision))
        if action == "analysis.confirm":
            _fields(payload, {"run_id", "source_version", "domain_revision", "checklist_digest", "issue_resolutions"})
            resolutions = saved["issue_resolutions"]
            pending = [item["issue_id"] for item in (resolutions or {}).get("resolutions", [])]
            _same((saved["checklist_digest"], resolutions),
                  (analysis_digest(payload, "checklist_digest"), validate_issue_resolutions(pending, payload.get("issue_resolutions"))))
        else:
            _fields(payload, {"run_id", "source_version", "domain_revision", "failed_command_id", "reason"})
            try:
                failed = str(UUID(payload.get("failed_command_id")))
            except (ValueError, TypeError, AttributeError):
                raise WebError("INVALID_FAILED_COMMAND", "请刷新并核对需要恢复的失败记录。") from None
            _same(saved["payload"]["retry"], {"failed_command_id": failed, "request_key": key, "reason": payload.get("reason")})
        return {"command_id": str(saved["command_id"]), "analysis": {**_identity(run), "status": run["status"]}}
    _ended()
