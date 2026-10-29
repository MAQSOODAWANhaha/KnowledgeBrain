"""Project-scoped human decisions and provenance-preserving business corrections."""
from __future__ import annotations

import asyncio
import copy
import json
import re

from docreader.integrations.cli import _correction_ref
from docreader.integrations.store import Conflict, InvalidConfirmation
from docreader.integrations.web import WebError, _identity, _request_key, _source_identities
from docreader.tender.schema import validate_issue_resolutions


_CONTEXT = {"action", "project_id", "owner_user_id", "request_key"}
_EDITABLE = {"text", "kind", "domains", "subject", "scope", "conditions", "exceptions",
             "numbers", "scoring", "consequence", "submission"}


def _fields(payload, allowed):
    if set(payload) - _CONTEXT - set(allowed):
        raise WebError("INVALID_ANALYSIS_REVIEW", "请求包含此操作不支持的字段。")


def _positive(payload, field):
    value = payload.get(field)
    if type(value) is not int or value < 1:
        raise WebError("INVALID_ANALYSIS_VERSION", "请提供有效的分析版本。")
    return value


def _digest(payload, field):
    value = payload.get(field)
    if not isinstance(value, str) or not re.fullmatch(r"[a-f0-9]{64}", value):
        raise WebError("INVALID_ANALYSIS_DIGEST", "分析基准不正确，请刷新后核对。")
    return value


async def _owned_run(bridge, payload, *, write=False, latest=False):
    project = await bridge.authorize(payload, write=write)
    run_id = payload.get("run_id")
    if not latest and (not isinstance(run_id, str) or not run_id or len(run_id) > 256):
        raise WebError("ANALYSIS_NOT_FOUND", "分析不存在或不属于本项目。", 404)
    async with bridge.store.transaction() as conn:
        if latest:
            row = await (await conn.execute(
                "SELECT run_id FROM tender_project_analyses WHERE project_id=%s AND NOT source_only "
                "ORDER BY created_at DESC,run_id DESC LIMIT 1", (project,),
            )).fetchone()
        else:
            row = await (await conn.execute(
                "SELECT run_id FROM tender_project_analyses WHERE project_id=%s AND run_id=%s AND NOT source_only",
                (project, run_id),
            )).fetchone()
    if row is None:
        if latest:
            return None
        raise WebError("ANALYSIS_NOT_FOUND", "分析不存在或不属于本项目。", 404)
    return await bridge.store.get_run(row["run_id"])


def _receipt(run, stage, source_version, revision):
    return next((entry for entry in [*run["artifacts"], *run["history"]["artifacts"]]
                 if entry["stage"] == stage and not entry["scope"]
                 and entry["source_version"] == source_version and entry["domain_revision"] == revision), None)


def _pending(run):
    waiting = next((command for command in reversed(run["commands"])
                    if command["status"] == "completed" and (command.get("receipt") or {}).get("status") == "waiting-confirmation"), None)
    return ((waiting or {}).get("receipt") or {}).get("metadata", {}).get("human_issue_ids", [])


def _baseline(artifacts, run, source_version, revision, digest=None):
    receipt = _receipt(run, "extract", source_version, revision)
    plan = _receipt(run, "pack-plan", source_version, revision)
    if not receipt or not plan or digest is not None and receipt["artifact_ref"]["sha256"] != digest:
        raise Conflict("correction baseline does not identify frozen extracted items")
    index = artifacts.read_json(receipt["artifact_ref"])
    if index.get("schema_version") != 2:
        raise WebError("ANALYSIS_READ_ONLY", "此历史分析仅支持读取，请更新来源后重新分析。", 409)
    packs = [{"pack_id": entry["pack_id"], **artifacts.read_json(entry["artifact_ref"])} for entry in index["packs"]]
    return receipt["artifact_ref"]["sha256"], packs


async def _sources_current(bridge, project, run):
    submitted = await asyncio.to_thread(bridge.artifacts.read_json, run["manifest_ref"])
    try:
        current = await bridge.manifest(project)
    except WebError as error:
        if error.code == "TENDER_DOCUMENT_REQUIRED":
            return False
        raise
    return _source_identities(submitted["documents"]) == _source_identities(current["documents"])


async def analysis_review(bridge, payload):
    run = await _owned_run(bridge, payload, latest=True)
    if run is None:
        return {"review": None}
    receipt = _receipt(run, "verify", run["source_version"], run["domain_revision"])
    verified = await asyncio.to_thread(bridge.artifacts.read_json, receipt["artifact_ref"]) if receipt else {}
    correction = None
    if (_receipt(run, "extract", run["source_version"], run["domain_revision"])
            and _receipt(run, "pack-plan", run["source_version"], run["domain_revision"])):
        try:
            digest, packs = await asyncio.to_thread(_baseline, bridge.artifacts, run, run["source_version"], run["domain_revision"])
            correction = {"base_items_digest": digest,
                          "items": [{"pack_id": pack["pack_id"], "item": item} for pack in packs for item in pack["items"]]}
        except WebError as error:
            if error.code != "ANALYSIS_READ_ONLY":
                raise
    return {"review": {**_identity(run), "status": run["status"], "checklist_digest": run["checklist_digest"],
                       "sources_current": await _sources_current(bridge, payload["project_id"], run),
                       "human_issue_ids": _pending(run), "issues": verified.get("issues", []),
                       "hard_blocking": verified.get("hard_blocking", False), "correction": correction}}


async def confirm_analysis(bridge, payload):
    from docreader.integrations.web_replay import replay_ended

    try:
        run = await _owned_run(bridge, payload, write=True)
    except WebError as error:
        if error.code != "PROJECT_ENDED":
            raise
        return await replay_ended(bridge, payload)
    _fields(payload, {"run_id", "source_version", "domain_revision", "checklist_digest", "issue_resolutions"})
    source, revision = _positive(payload, "source_version"), _positive(payload, "domain_revision")
    digest = _digest(payload, "checklist_digest")
    if source != run["source_version"] or revision != run["domain_revision"]:
        raise WebError("ANALYSIS_VERSION_CONFLICT", "分析已有新版本，当前人工意见已保留，请刷新并核对。", 409)
    try:
        resolutions = validate_issue_resolutions(_pending(run), payload.get("issue_resolutions"))
    except ValueError:
        raise WebError("ISSUE_RESOLUTIONS_REQUIRED", "请逐条处理全部待确认问题，选择判定并填写人工理由。") from None
    try:
        async with bridge.store.transaction() as conn:
            if not await bridge.lock_project(conn, payload):
                return await replay_ended(bridge, payload)
            command = await bridge.store.confirm(run["run_id"], source, digest, _request_key(payload),
                                                 domain_revision=revision, issue_resolutions=resolutions,
                                                 connection=conn)
    except InvalidConfirmation:
        raise WebError("ANALYSIS_VERSION_CONFLICT", "确认清单或请求已有变化，当前人工意见已保留，请刷新并核对。", 409) from None
    current = await bridge.store.get_run(run["run_id"])
    return {"command_id": command["command_id"], "analysis": {**_identity(current), "status": current["status"]}}


def _corrected_ref(artifacts, run, source, revision, digest, changes):
    _, packs = _baseline(artifacts, run, source, revision, digest)
    by_id = {(pack["pack_id"], item["item_id"]): item for pack in packs for item in pack["items"]}
    for change in changes:
        item = by_id.get((change["pack_id"], change["item_id"]))
        if item is None:
            raise WebError("INVALID_CORRECTION_ITEM", "纠正事项不属于当前分析基准，请核对后重试。")
        for field in _EDITABLE & change.keys():
            if change[field] is None:
                item.pop(field, None)
            else:
                item[field] = copy.deepcopy(change[field])
    complete = {"schema_version": 2, "packs": [{key: copy.deepcopy(pack[key]) for key in
                ("pack_id", "schema_version", "items", "dispositions", "relations", "issues")} for pack in packs]}
    return _correction_ref(artifacts, run, source, revision, digest, complete)


def correction_request(payload):
    _fields(payload, {"run_id", "source_version", "expected_revision", "base_items_digest", "reason", "changes"})
    source, revision = _positive(payload, "source_version"), _positive(payload, "expected_revision")
    digest, key = _digest(payload, "base_items_digest"), _request_key(payload)
    reason, changes = payload.get("reason"), payload.get("changes")
    if not isinstance(reason, str) or not reason.strip() or len(reason) > 4000:
        raise WebError("CORRECTION_REASON_REQUIRED", "请填写纠正理由（最多 4000 字）。")
    if not isinstance(changes, list) or not changes:
        raise WebError("CORRECTION_ITEMS_REQUIRED", "请至少修改一条事项。")
    seen, characters = set(), 0
    for change in changes:
        if (not isinstance(change, dict) or set(change) - {"pack_id", "item_id"} - _EDITABLE
                or not set(change) & _EDITABLE
                or not all(isinstance(change.get(field), str) and change[field].strip() for field in ("pack_id", "item_id"))):
            raise WebError("INVALID_CORRECTION_ITEM", "仅可修改当前事项的业务字段，来源和证据由服务端保留。")
        identity = change["pack_id"], change["item_id"]
        try:
            characters += len(json.dumps(change, ensure_ascii=False, allow_nan=False))
        except (TypeError, ValueError, RecursionError):
            raise WebError("INVALID_CORRECTION_ITEM", "事项业务字段格式不正确。") from None
        values = list(change.values())
        while values:
            value = values.pop()
            if isinstance(value, str) and len(value) > 20000:
                raise WebError("INVALID_CORRECTION_ITEM", "单个业务字段最多 20000 字。")
            if isinstance(value, dict):
                values.extend(value.values())
            elif isinstance(value, list):
                values.extend(value)
        if identity in seen or characters > 200000:
            raise WebError("INVALID_CORRECTION_ITEM", "事项不可重复，单次纠正内容合计最多 200000 字。")
        seen.add(identity)
    return source, revision, digest, key, reason, changes


async def correct_analysis(bridge, payload):
    from docreader.integrations.web_replay import replay_ended

    try:
        run = await _owned_run(bridge, payload, write=True)
    except WebError as error:
        if error.code != "PROJECT_ENDED":
            raise
        return await replay_ended(bridge, payload)
    source, revision, digest, key, reason, changes = correction_request(payload)
    try:
        reference = await asyncio.to_thread(_corrected_ref, bridge.artifacts, run, source, revision, digest, changes)
    except (WebError, Conflict):
        raise
    except ValueError:
        raise WebError("INVALID_CORRECTION", "纠正事项未通过现有分析校验，请核对类型、字段和数值原文片段。") from None
    async with bridge.store.transaction() as conn:
        if not await bridge.lock_project(conn, payload):
            return await replay_ended(bridge, payload)
        submitted = await bridge.store.correct(run["run_id"], source, revision, digest, reference, key, reason,
                                                connection=conn)
    return {"analysis": {**_identity(submitted), "status": submitted["status"]},
            "submitted_source_version": submitted["submitted_source_version"],
            "submitted_domain_revision": submitted["submitted_domain_revision"]}
