"""Project-scoped saved Word review jobs; no model runs in the HTTP request."""
import asyncio
import hashlib
import re

from psycopg.types.json import Jsonb

from docreader.integrations.store import Conflict
from docreader.integrations.web import WebError, _identity, _request_key, _uuid
from docreader.llm.config import ModelConfig


async def saved_version(bridge, project_id, version_id):
    version_id = _uuid(version_id)
    async with bridge.store.transaction() as conn:
        version = await (await conn.execute(
            "SELECT id,execution_id,content_sha256,byte_length FROM tender_draft_versions WHERE project_id=%s AND id=%s",
            (project_id, version_id),
        )).fetchone()
    if version is None:
        raise WebError("WORD_VERSION_NOT_FOUND", "已保存 Word 版本不存在或不属于此项目。", 404)
    return version


async def review_view(bridge, job):
    value = await bridge.authoring_view(job)
    value.update({key: job["payload"]["request"][key] for key in ("version_id", "docx_sha256")})
    snapshot = job["payload"]["snapshot"]
    value["analysis_identity"] = _identity(snapshot)
    try:
        if snapshot.get("frozen_ref") is None:
            raise ValueError("saved review has no frozen source index")
        value["sources_current"] = await bridge.outline_sources_current(str(job["project_id"]), {"authoring_input": snapshot})
    except (WebError, ValueError, OSError):
        # Source comparison must not turn an already accepted job into a 4xx
        # response that the browser interprets as an unsubmitted request.
        value["sources_current"] = None
        value["source_status_issue"] = {"code": "saved_word_source_status_unavailable",
                                        "detail": "当前项目来源暂不可核对，不能确认该固定复核依据已覆盖最新要求。请重试读取状态。"}
    if value["status"] == "succeeded":
        execution = await bridge.authoring.get_execution(job["execution_id"])
        value["report"] = await asyncio.to_thread(bridge.artifacts.read_json, execution["result_ref"])
    return {"review": value}


async def review_status(bridge, payload):
    await saved_version(bridge, payload["project_id"], payload.get("version_id"))
    async with bridge.store.transaction() as conn:
        row = await (await conn.execute(
            "SELECT execution_id FROM tender_authoring_jobs WHERE project_id=%s "
            "AND payload->>'operation'='saved_review' AND payload->'request'->>'version_id'=%s "
            "ORDER BY created_at DESC,execution_id DESC LIMIT 1",
            (payload["project_id"], payload["version_id"]),
        )).fetchone()
    return await review_view(bridge, await bridge.job(payload["project_id"], row["execution_id"])) if row else {"review": None}


def review_request(payload):
    reason = payload.get("reason")
    sha = payload.get("docx_sha256")
    if not isinstance(reason, str) or not reason.strip() or len(reason) > 4000:
        raise WebError("REVIEW_REASON_REQUIRED", "请填写不超过 4000 字的保存稿复核说明。")
    if not isinstance(sha, str) or re.fullmatch("[0-9a-f]{64}", sha) is None:
        raise WebError("WORD_IDENTITY_REQUIRED", "请提供明确的已保存 Word 摘要。")
    return {"version_id": _uuid(payload.get("version_id")), "docx_sha256": sha, "reason": reason.strip()}


async def start_review(bridge, payload, blob):
    from docreader.integrations.web_replay import replay_ended

    request = review_request(payload)
    execution_id = "web-saved-review-" + _request_key(payload)
    try:
        existing = await bridge.job(payload["project_id"], execution_id)
    except WebError as error:
        if error.code != "EXECUTION_NOT_FOUND":
            raise
    else:
        if existing["payload"]["request"] != request:
            raise Conflict("request key already identifies another saved Word review")
        return await review_view(bridge, existing)
    version = await saved_version(bridge, payload["project_id"], request["version_id"])
    if (request["docx_sha256"] != version["content_sha256"] or len(blob) != version["byte_length"]
            or hashlib.sha256(blob).hexdigest() != request["docx_sha256"]):
        raise WebError("WORD_IDENTITY_MISMATCH", "保存稿字节与版本摘要不一致，请重新读取版本。", 409)
    document = await bridge.result(payload["project_id"], version["execution_id"])
    source_execution = await bridge.authoring.get_execution(version["execution_id"])
    outline = await asyncio.to_thread(bridge.artifacts.read_json, document["outline_ref"])
    ModelConfig.from_env()  # Configuration admission only; the worker owns model calls.
    reference = await asyncio.to_thread(bridge.artifacts.put_bytes, blob,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    data = {"operation": "saved_review", "owner_user_id": payload["owner_user_id"], "request": request,
            "identity": _identity(outline["authoring_input"]), "snapshot": outline["authoring_input"],
            "document_ref": source_execution["result_ref"], "saved_ref": reference}
    async with bridge.store.transaction() as conn:
        if not await bridge.lock_project(conn, payload):
            return await replay_ended(bridge, payload)
        await conn.execute("INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload) "
                           "VALUES(%s,%s,%s,%s) ON CONFLICT DO NOTHING",
                           (execution_id, payload["project_id"], data["identity"]["run_id"], Jsonb(data)))
    job = await bridge.job(payload["project_id"], execution_id)
    if job["payload"]["request"] != request:
        raise Conflict("request key already identifies another saved Word review")
    return await review_view(bridge, job)
