"""Independent authoring leases and model receipts; analysis rows remain unchanged."""

from __future__ import annotations

from contextlib import asynccontextmanager
from dataclasses import dataclass
import math
import re

from psycopg import AsyncConnection
from psycopg.rows import dict_row
from psycopg.types.json import Jsonb

from docreader.integrations.store import (
    Cancelled, Conflict, RequestReplayBlocked, StaleExecution,
    StoreError, _recorded_first_data, _safe_metadata,
)
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.schema import ArtifactRef, AuthoringInput
from docreader.tender.status import AuthoringExecutionStatus, ModelRequestStatus, RunStatus, status_row


@dataclass(frozen=True)
class AuthoringLease:
    execution_id: str
    epoch: int



def _duration(value):
    if type(value) not in {int, float} or not math.isfinite(value) or value <= 0:
        raise ValueError("authoring lease duration must be finite and positive")


def _spec(value):
    if not isinstance(value, dict) or _safe_metadata(value) != value:
        raise ValueError("authoring execution specification must contain only non-secret configuration")
    return value


class AuthoringStore:
    def __init__(self, dsn: str):
        self.dsn = dsn

    @asynccontextmanager
    async def transaction(self):
        async with await AsyncConnection.connect(self.dsn, row_factory=dict_row) as conn:
            async with conn.transaction():
                yield conn

    async def setup(self):
        async with self.transaction() as conn:
            await conn.execute("SELECT execution_id FROM tender_authoring_executions LIMIT 0")
            await conn.execute("SELECT request_id FROM tender_authoring_model_requests LIMIT 0")

    async def _execution(self, conn, execution_id, *, lock=False, missing_ok=False):
        query = ("SELECT *, lease_until > clock_timestamp() AS lease_live "
                 "FROM tender_authoring_executions WHERE execution_id = %s")
        row = await (await conn.execute(query + (" FOR UPDATE" if lock else ""), (execution_id,))).fetchone()
        if row is None and not missing_ok:
            raise StoreError("authoring execution not found")
        return status_row(row, AuthoringExecutionStatus)

    async def submit(self, execution_id, authoring_input, execution_spec):
        if not isinstance(execution_id, str) or not execution_id.strip():
            raise ValueError("authoring execution_id is required")
        value = AuthoringInput.model_validate(authoring_input).model_dump()
        spec = _spec(execution_spec)
        async with self.transaction() as conn:
            await conn.execute(
                "INSERT INTO tender_authoring_executions(execution_id, authoring_input, execution_spec) "
                "VALUES (%s, %s, %s) ON CONFLICT (execution_id) DO NOTHING",
                (execution_id, Jsonb(value), Jsonb(spec)),
            )
            row = await self._execution(conn, execution_id, lock=True)
            if (AuthoringInput.model_validate(row["authoring_input"]).model_dump() != value
                    or row["execution_spec"] != spec):
                raise Conflict("execution_id already identifies different authoring input or configuration")
            return row

    async def get_execution(self, execution_id, *, missing_ok=False):
        async with self.transaction() as conn:
            row = await self._execution(conn, execution_id, lock=True, missing_ok=missing_ok)
            if row is None:
                return None
            row["requests"] = [status_row(entry, ModelRequestStatus) for entry in await (await conn.execute(
                "SELECT * FROM tender_authoring_model_requests WHERE execution_id = %s ORDER BY created_at, request_id",
                (execution_id,),
            )).fetchall()]
            return row

    async def _abandon(self, conn, execution_id, error_type, *, before_epoch=None):
        await conn.execute(
            "UPDATE tender_authoring_model_requests SET status = CASE WHEN status = 'sent' THEN 'uncertain' "
            "ELSE 'failed' END, finished_at = clock_timestamp(), error = %s "
            "WHERE execution_id = %s AND status IN ('prepared', 'sent') "
            "AND (%s::bigint IS NULL OR epoch < %s)",
            (Jsonb({"type": error_type}), execution_id, before_epoch, before_epoch),
        )

    async def acquire(self, execution_id, lease_seconds=60):
        _duration(lease_seconds)
        async with self.transaction() as conn:
            row = await self._execution(conn, execution_id, lock=True)
            if row["cancel_requested"] or row["status"] in {AuthoringExecutionStatus.SUCCEEDED, AuthoringExecutionStatus.CANCELLED} or row["lease_live"]:
                return None
            epoch = row["epoch"] + 1
            await self._abandon(conn, execution_id, "executor_lost", before_epoch=epoch)
            await conn.execute(
                "UPDATE tender_authoring_executions SET status = 'running', epoch = %s, "
                "lease_until = clock_timestamp() + %s * interval '1 second', updated_at = clock_timestamp() "
                "WHERE execution_id = %s", (epoch, lease_seconds, execution_id),
            )
            return AuthoringLease(execution_id, epoch)

    async def assert_fence(self, lease, conn=None):
        if conn is None:
            async with self.transaction() as connection:
                return await self.assert_fence(lease, connection)
        row = await self._execution(conn, lease.execution_id, lock=True)
        if row["cancel_requested"]:
            raise Cancelled("authoring cancellation requested")
        if row["epoch"] != lease.epoch or row["status"] != AuthoringExecutionStatus.RUNNING or not row["lease_live"]:
            raise StaleExecution("authoring execution lease expired or was replaced")
        return row

    async def heartbeat(self, lease, lease_seconds=60):
        _duration(lease_seconds)
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            await conn.execute(
                "UPDATE tender_authoring_executions SET lease_until = clock_timestamp() + %s * interval '1 second', "
                "updated_at = clock_timestamp() WHERE execution_id = %s", (lease_seconds, lease.execution_id),
            )

    async def release(self, lease, status: AuthoringExecutionStatus, result_ref=None, error=None):
        status = AuthoringExecutionStatus(status)
        if status not in {AuthoringExecutionStatus.SUCCEEDED, AuthoringExecutionStatus.FAILED}:
            raise ValueError("invalid authoring execution result")
        if status == AuthoringExecutionStatus.SUCCEEDED:
            result_ref = ArtifactRef.model_validate(result_ref).model_dump()
        async with self.transaction() as conn:
            row = await self.assert_fence(lease, conn)
            if status == AuthoringExecutionStatus.SUCCEEDED:
                expected = row["authoring_input"]
                parent = status_row(await (await conn.execute(
                    "SELECT * FROM tender_analysis_runs WHERE run_id = %s FOR UPDATE", (expected["run_id"],),
                )).fetchone(), RunStatus)
                if parent is None:
                    raise StoreError("authoring parent analysis not found")
                if parent["cancel_requested"] or parent["status"] == RunStatus.CANCELLED:
                    raise Cancelled("parent analysis cancellation requested")
            await self._abandon(conn, lease.execution_id, "execution_finished")
            await conn.execute(
                "UPDATE tender_authoring_executions SET status = %s, result_ref = %s, error = %s, "
                "lease_until = NULL, updated_at = clock_timestamp() WHERE execution_id = %s",
                (status, Jsonb(result_ref) if status == AuthoringExecutionStatus.SUCCEEDED else None,
                 Jsonb(_safe_metadata(error)), lease.execution_id),
            )
            return await self._execution(conn, lease.execution_id)

    async def cancel(self, execution_id):
        async with self.transaction() as conn:
            row = await self._execution(conn, execution_id, lock=True)
            if row["status"] == AuthoringExecutionStatus.SUCCEEDED:
                return row
            await conn.execute(
                "UPDATE tender_authoring_executions SET status = 'cancelled', cancel_requested = true, "
                "lease_until = NULL, updated_at = clock_timestamp() WHERE execution_id = %s", (execution_id,),
            )
            await self._abandon(conn, execution_id, "execution_cancelled")
            return await self._execution(conn, execution_id)

    async def pin_execution_spec(self, lease, spec):
        spec = _spec(spec)
        async with self.transaction() as conn:
            row = await self.assert_fence(lease, conn)
            if row["execution_spec"] != spec:
                raise Conflict("authoring execution configuration changed")
            return row["execution_spec"]

    async def receipt(self, lease, stage, scope="", *, input_digest=None):
        async with self.transaction() as conn:
            row = await self.assert_fence(lease, conn)
            receipt = next((r for r in row["receipts"] if r["stage"] == stage and r["scope"] == scope), None)
            if receipt is not None and input_digest is not None and receipt["input_digest"] != input_digest:
                raise Conflict("authoring artifact inputs changed within an immutable stage identity")
            return receipt

    async def commit_artifact(self, lease, stage, artifact_ref, scope="", *, input_digest="", metadata=None):
        reference = ArtifactRef.model_validate(artifact_ref).model_dump()
        async with self.transaction() as conn:
            row = await self.assert_fence(lease, conn)
            prior = next((r for r in row["receipts"] if r["stage"] == stage and r["scope"] == scope), None)
            if prior is not None:
                if prior["artifact_ref"] != reference or prior["input_digest"] != input_digest:
                    raise Conflict("authoring artifact already committed different content or inputs")
                return prior
            receipt = {"stage": stage, "scope": scope, "input_digest": input_digest,
                       "artifact_ref": reference, "epoch": lease.epoch, "metadata": _safe_metadata(metadata or {})}
            await conn.execute("UPDATE tender_authoring_executions SET receipts = %s, updated_at = clock_timestamp() "
                               "WHERE execution_id = %s", (Jsonb([*row["receipts"], receipt]), lease.execution_id))
            return receipt

    async def begin_request(self, lease, request_id, model, task, metadata):
        if task not in {"outline", "authoring_basis", "authoring_write", "authoring_review"}:
            raise ValueError("unsupported authoring task")
        metadata = _safe_metadata(metadata)
        for key in ("request_digest", "logical_request_digest"):
            value = metadata.get(key)
            if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None:
                raise ValueError(f"authoring model audit requires a lowercase SHA-256 {key}")
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            rows = [status_row(entry, ModelRequestStatus) for entry in await (await conn.execute(
                "SELECT * FROM tender_authoring_model_requests WHERE execution_id = %s ORDER BY created_at, request_id",
                (lease.execution_id,),
            )).fetchall()]
            matching = [r for r in rows if r["task"] == task and
                        r["metadata"]["logical_request_digest"] == metadata["logical_request_digest"]]
            cached = next((r for r in matching if r["status"] == ModelRequestStatus.SUCCEEDED
                           and r["metadata"]["request_digest"] == metadata["request_digest"]), None)
            if cached is not None:
                return cached
            active = next((r for r in matching if r["status"] == ModelRequestStatus.SENT), None)
            if active is not None:
                raise RequestReplayBlocked(active["request_id"], active["status"])
            if sum(r["sent_at"] is not None for r in matching) >= 4:
                raise ModelWorkUnitExhausted(task)
            return status_row(await (await conn.execute(
                "INSERT INTO tender_authoring_model_requests(request_id, execution_id, epoch, model, task, metadata) "
                "VALUES (%s, %s, %s, %s, %s, %s) RETURNING *",
                (request_id, lease.execution_id, lease.epoch, model, task, Jsonb(metadata)),
            )).fetchone(), ModelRequestStatus)

    async def finish_request(self, request_id, status: ModelRequestStatus, usage=None, error=None, *, response_ref=None, activity=None):
        status = ModelRequestStatus(status)
        if status not in {ModelRequestStatus.SENT, ModelRequestStatus.SUCCEEDED, ModelRequestStatus.FAILED, ModelRequestStatus.UNCERTAIN}:
            raise ValueError("invalid authoring model request status")
        if response_ref is not None:
            response_ref = ArtifactRef.model_validate(response_ref).model_dump()
        async with self.transaction() as conn:
            request = await (await conn.execute(
                "SELECT execution_id FROM tender_authoring_model_requests WHERE request_id = %s", (request_id,),
            )).fetchone()
            if request is None:
                raise StoreError("authoring model request not registered")
            execution = await self._execution(conn, request["execution_id"], lock=True)
            row = status_row(await (await conn.execute(
                "SELECT * FROM tender_authoring_model_requests WHERE request_id = %s FOR UPDATE", (request_id,),
            )).fetchone(), ModelRequestStatus)
            if status == ModelRequestStatus.SENT:
                await self.assert_fence(AuthoringLease(row["execution_id"], row["epoch"]), conn)
                if row["status"] != ModelRequestStatus.PREPARED:
                    raise RequestReplayBlocked(request_id, row["status"])
                attempts = [status_row(entry, ModelRequestStatus) for entry in await (await conn.execute(
                    "SELECT * FROM tender_authoring_model_requests WHERE execution_id = %s AND sent_at IS NOT NULL",
                    (execution["execution_id"],),
                )).fetchall()]
                matching = [r for r in attempts if r["task"] == row["task"] and
                            r["metadata"]["logical_request_digest"] == row["metadata"]["logical_request_digest"]]
                active = next((r for r in matching if r["status"] == ModelRequestStatus.SENT), None)
                if active is not None:
                    raise RequestReplayBlocked(active["request_id"], active["status"])
                cached = next((r for r in matching if r["status"] == ModelRequestStatus.SUCCEEDED and
                               r["metadata"]["request_digest"] == row["metadata"]["request_digest"]), None)
                if cached is not None:
                    raise RequestReplayBlocked(cached["request_id"], cached["status"])
                if len(matching) >= 4:
                    raise ModelWorkUnitExhausted(row["task"])
            if row["response_ref"] is not None and response_ref is not None and row["response_ref"] != response_ref:
                raise Conflict("authoring model response cannot replace immutable received content")
            terminal = row["status"] in {ModelRequestStatus.SUCCEEDED, ModelRequestStatus.FAILED}
            await conn.execute(
                "UPDATE tender_authoring_model_requests SET status = %s, usage = COALESCE(usage, %s), "
                "response_ref = COALESCE(response_ref, %s), error = %s, metadata = metadata || %s, "
                "sent_at = CASE WHEN %s = 'sent' THEN COALESCE(sent_at, clock_timestamp()) ELSE sent_at END, "
                "finished_at = CASE WHEN %s != 'sent' THEN COALESCE(finished_at, clock_timestamp()) "
                "ELSE finished_at END WHERE request_id = %s",
                (row["status"] if terminal else status, Jsonb(usage) if usage is not None else None,
                 Jsonb(response_ref) if response_ref is not None else None,
                 Jsonb(row["error"] if terminal else _safe_metadata(error)),
                 Jsonb(_recorded_first_data(activity) or {}), status, status, request_id),
            )
