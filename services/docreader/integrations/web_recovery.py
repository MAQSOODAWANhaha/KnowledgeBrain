"""Authenticated continuation of failed analysis, retaining its original ledger."""
from __future__ import annotations

import re
from uuid import UUID

from docreader.integrations.store import Conflict, RequestReplayBlocked
from docreader.integrations.web import WebError, _identity, _request_key
from docreader.integrations.web_analysis import _owned_run, _positive


def _view(run, command, blocker, notice):
    return {**_identity(run), "status": run["status"],
            "failed_command_id": str(command["command_id"]) if command else None,
            "can_retry": blocker is None, "blocker": blocker, "notice": notice,
            "saved_responses": sum(row["status"] == "succeeded" and row["response_ref"] is not None for row in run["usage"])}


async def analysis_recovery(bridge, payload):
    run = await _owned_run(bridge, payload, latest=True)
    if run is None:
        return {"recovery": None}
    command = max(run["commands"], key=lambda row: (row["created_at"], str(row["command_id"])), default=None)
    blocker, notice = None, "继续原任务的固定来源和分析版本，复用成功回执；未完成调用仍可能产生费用。"
    if run["domain_revision"] < 1:
        blocker, notice = "unsupported_revision", "此任务的历史版本不能通过当前页面恢复，请管理员核查原任务及已有回执。"
    elif run["status"] != "failed" or run["cancel_requested"]:
        blocker, notice = "not_failed", "当前分析无需失败恢复；已取消的任务不能继续。"
    elif run["lease_live"]:
        blocker, notice = "active_lease", "原执行仍持有任务，请等待其释放后刷新状态。"
    elif (not command or command["status"] != "completed" or (command.get("receipt") or {}).get("status") != "failed"):
        blocker, notice = "failed_command_missing", "尚未取得本次失败的完整记录，请刷新；若持续出现，请管理员核查原任务。"
    else:
        try:
            async with bridge.store.transaction() as conn:
                await bridge.store._assert_analysis_revision(conn, run)
        except Conflict:
            blocker, notice = "unsupported_protocol", "此历史任务不支持当前网页恢复，请管理员核查原执行版本和已有回执。"
        if blocker is None and any(row["status"] in {"sent", "uncertain"} and not (
                isinstance(row["metadata"].get("logical_request_digest"), str)
                and re.fullmatch(r"[a-f0-9]{64}", row["metadata"]["logical_request_digest"])) for row in run["usage"]):
            blocker, notice = "payment_reconciliation", "存在尚待核对的旧发送记录，请管理员核对调用结果和费用后处理原任务。"
        if blocker is None:
            reason = ((command.get("receipt") or {}).get("metadata") or {}).get("reason")
            if reason in {"infrastructure_retries_exhausted", "model_work_unit_retries_exhausted"}:
                notice = "本次执行已用尽重试。请先核对故障原因，再继续原任务；成功回执保留，未完成调用仍可能产生费用。"
    return {"recovery": _view(run, command, blocker, notice)}


async def retry_analysis(bridge, payload):
    from docreader.integrations.web_replay import replay_ended

    try:
        run = await _owned_run(bridge, payload, write=True)
    except WebError as error:
        if error.code != "PROJECT_ENDED":
            raise
        return await replay_ended(bridge, payload)
    allowed = {"action", "project_id", "owner_user_id", "request_key", "run_id", "source_version", "domain_revision",
               "failed_command_id", "reason"}
    if set(payload) - allowed:
        raise WebError("INVALID_ANALYSIS_RECOVERY", "恢复请求包含不支持的字段。")
    source, revision = _positive(payload, "source_version"), _positive(payload, "domain_revision")
    reason = payload.get("reason")
    if not isinstance(reason, str) or not reason.strip() or len(reason) > 4000:
        raise WebError("RECOVERY_REASON_REQUIRED", "请填写恢复理由（最多4000字）。")
    try:
        command_id = str(UUID(payload.get("failed_command_id")))
    except (ValueError, TypeError, AttributeError):
        raise WebError("INVALID_FAILED_COMMAND", "请刷新并核对需要恢复的失败记录。") from None
    try:
        # Project closure and Store's original-command CAS share one transaction.
        async with bridge.store.transaction() as conn:
            if not await bridge.lock_project(conn, payload):
                return await replay_ended(bridge, payload)
            command = await bridge.store.retry(run["run_id"], source, revision, command_id, _request_key(payload), reason,
                                                connection=conn)
    except RequestReplayBlocked:
        raise WebError("ANALYSIS_RECONCILIATION_REQUIRED", "原调用结果与费用尚待核对，请管理员核查原任务后处理。", 409) from None
    except Conflict:
        raise WebError("ANALYSIS_RECOVERY_CONFLICT", "原任务状态、版本或恢复请求已变化，请刷新核对；当前输入仍保留。", 409) from None
    current = await bridge.store.get_run(run["run_id"])
    return {"command_id": str(command["command_id"]), "analysis": {**_identity(current), "status": current["status"]}}
