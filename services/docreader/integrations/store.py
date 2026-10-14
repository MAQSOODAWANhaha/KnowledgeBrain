"""PostgreSQL business receipts for the independently launched Python workflow.

The run row is the fencing lock, including for the LangGraph checkpointer. Queue
publication is deliberately separate from an execution receipt. No Rust table is
read or written here.
"""

from __future__ import annotations

from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Any, AsyncIterator
from uuid import uuid4

from psycopg import AsyncConnection
from psycopg.rows import dict_row
from psycopg.types.json import Jsonb


class StoreError(RuntimeError):
    pass


class Conflict(StoreError):
    pass


class InvalidConfirmation(Conflict):
    pass


class StaleExecution(StoreError):
    pass


class Cancelled(StaleExecution):
    pass


@dataclass(frozen=True)
class Lease:
    run_id: str
    version: int
    epoch: int
    command_id: str


_SCHEMA = """
CREATE TABLE IF NOT EXISTS tender_runs (
    run_id text PRIMARY KEY,
    request_key text NOT NULL UNIQUE,
    current_version integer NOT NULL DEFAULT 1,
    status text NOT NULL DEFAULT 'pending',
    epoch bigint NOT NULL DEFAULT 0,
    lease_until timestamptz,
    lease_command_id text,
    cancel_requested boolean NOT NULL DEFAULT false,
    checklist_digest text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE IF NOT EXISTS tender_versions (
    run_id text NOT NULL REFERENCES tender_runs(run_id),
    version integer NOT NULL,
    request_key text NOT NULL,
    manifest_ref jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, version),
    UNIQUE (run_id, request_key)
);
CREATE TABLE IF NOT EXISTS tender_commands (
    command_id text PRIMARY KEY,
    run_id text NOT NULL,
    version integer NOT NULL,
    kind text NOT NULL CHECK (kind IN ('start', 'resume')),
    payload jsonb NOT NULL DEFAULT '{}',
    status text NOT NULL DEFAULT 'pending',
    claim_token text,
    claim_until timestamptz,
    published_at timestamptz,
    started_at timestamptz,
    completed_at timestamptz,
    receipt jsonb,
    last_error jsonb,
    delivery_count integer NOT NULL DEFAULT 0,
    execution_count integer NOT NULL DEFAULT 0,
    infrastructure_failures integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (run_id, version) REFERENCES tender_versions(run_id, version)
);
CREATE INDEX IF NOT EXISTS tender_commands_dispatch_idx
    ON tender_commands(status, created_at);
CREATE TABLE IF NOT EXISTS tender_artifacts (
    run_id text NOT NULL,
    version integer NOT NULL,
    stage text NOT NULL,
    scope text NOT NULL DEFAULT '',
    artifact_ref jsonb NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}',
    epoch bigint NOT NULL,
    committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, version, stage, scope),
    FOREIGN KEY (run_id, version) REFERENCES tender_versions(run_id, version)
);
CREATE TABLE IF NOT EXISTS tender_confirmations (
    run_id text NOT NULL,
    version integer NOT NULL,
    checklist_digest text NOT NULL,
    request_key text NOT NULL,
    command_id text NOT NULL REFERENCES tender_commands(command_id),
    confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, version),
    UNIQUE (run_id, request_key),
    FOREIGN KEY (run_id, version) REFERENCES tender_versions(run_id, version)
);
CREATE TABLE IF NOT EXISTS tender_model_requests (
    request_id text PRIMARY KEY,
    run_id text NOT NULL,
    version integer NOT NULL,
    epoch bigint NOT NULL,
    model text NOT NULL,
    kind text NOT NULL,
    status text NOT NULL DEFAULT 'prepared',
    metadata jsonb NOT NULL DEFAULT '{}',
    usage jsonb,
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    sent_at timestamptz,
    finished_at timestamptz,
    FOREIGN KEY (run_id, version) REFERENCES tender_versions(run_id, version)
);
"""


def _safe_metadata(value: Any) -> Any:
    """Remove credential fields; callers must pass error classes, never bodies."""
    if isinstance(value, dict):
        return {
            key: _safe_metadata(item)
            for key, item in value.items()
            if key.lower() not in {"key", "api_key", "authorization", "password", "secret", "access_token"}
        }
    if isinstance(value, list):
        return [_safe_metadata(item) for item in value]
    return value


class Store:
    def __init__(self, dsn: str):
        self.dsn = dsn

    @asynccontextmanager
    async def transaction(self) -> AsyncIterator[AsyncConnection]:
        async with await AsyncConnection.connect(self.dsn, row_factory=dict_row) as conn:
            async with conn.transaction():
                yield conn

    async def setup(self) -> None:
        async with self.transaction() as conn:
            await conn.execute(_SCHEMA)

    async def _run(self, conn: AsyncConnection, run_id: str, *, lock: bool = False) -> dict[str, Any]:
        query = "SELECT *, lease_until > clock_timestamp() AS lease_live FROM tender_runs WHERE run_id = %s"
        if lock:
            query += " FOR UPDATE"
        row = await (await conn.execute(query, (run_id,))).fetchone()
        if row is None:
            raise StoreError("run not found")
        return row

    async def _command(
        self, conn: AsyncConnection, run_id: str, version: int, kind: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        return await (await conn.execute(
            "INSERT INTO tender_commands(command_id, run_id, version, kind, payload) "
            "VALUES (%s, %s, %s, %s, %s) RETURNING *",
            (str(uuid4()), run_id, version, kind, Jsonb(payload or {})),
        )).fetchone()

    async def submit(self, manifest_ref: dict[str, Any], request_key: str) -> dict[str, Any]:
        if not request_key:
            raise ValueError("request_key is required")
        async with self.transaction() as conn:
            run_id = str(uuid4())
            inserted = await (await conn.execute(
                "INSERT INTO tender_runs(run_id, request_key) VALUES (%s, %s) "
                "ON CONFLICT (request_key) DO NOTHING RETURNING *", (run_id, request_key),
            )).fetchone()
            if inserted is None:
                existing = await (await conn.execute(
                    "SELECT r.*, v.manifest_ref FROM tender_runs r JOIN tender_versions v "
                    "ON r.run_id = v.run_id AND v.version = 1 WHERE r.request_key = %s", (request_key,),
                )).fetchone()
                if existing["manifest_ref"] != manifest_ref:
                    raise Conflict("request_key already identifies a different submission")
                return existing
            await conn.execute(
                "INSERT INTO tender_versions(run_id, version, request_key, manifest_ref) VALUES (%s, 1, %s, %s)",
                (run_id, request_key, Jsonb(manifest_ref)),
            )
            await self._command(conn, run_id, 1, "start")
            return {**inserted, "manifest_ref": manifest_ref}

    async def get_run(self, run_id: str) -> dict[str, Any]:
        async with self.transaction() as conn:
            run = await self._run(conn, run_id)
            version = run["current_version"]
            run["manifest_ref"] = (await (await conn.execute(
                "SELECT manifest_ref FROM tender_versions WHERE run_id = %s AND version = %s", (run_id, version),
            )).fetchone())["manifest_ref"]
            for key, query in (
                ("artifacts", "SELECT * FROM tender_artifacts WHERE run_id = %s AND version = %s ORDER BY stage, scope"),
                ("commands", "SELECT * FROM tender_commands WHERE run_id = %s AND version = %s ORDER BY created_at"),
                ("usage", "SELECT * FROM tender_model_requests WHERE run_id = %s AND version = %s ORDER BY created_at"),
                ("confirmations", "SELECT * FROM tender_confirmations WHERE run_id = %s AND version = %s"),
            ):
                run[key] = await (await conn.execute(query, (run_id, version))).fetchall()
            return run

    async def get_manifest(self, run_id: str, version: int) -> dict[str, Any]:
        async with self.transaction() as conn:
            row = await (await conn.execute(
                "SELECT manifest_ref FROM tender_versions WHERE run_id = %s AND version = %s", (run_id, version),
            )).fetchone()
            if row is None:
                raise StoreError("source version not found")
            return row["manifest_ref"]

    async def get_command(self, command_id: str) -> dict[str, Any]:
        async with self.transaction() as conn:
            row = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE command_id = %s", (command_id,),
            )).fetchone()
            if row is None:
                raise StoreError("command not found")
            return row

    async def claim_commands(
        self, limit: int = 20, claim_seconds: float = 30, redeliver_seconds: float = 60,
    ) -> list[dict[str, Any]]:
        if limit < 1 or claim_seconds <= 0 or redeliver_seconds <= 0:
            raise ValueError("dispatch limits and intervals must be positive")
        async with self.transaction() as conn:
            rows = await (await conn.execute(
                "SELECT c.command_id FROM tender_commands c JOIN tender_runs r ON r.run_id = c.run_id "
                "WHERE c.version = r.current_version AND NOT r.cancel_requested "
                "AND (r.lease_until IS NULL OR r.lease_until <= clock_timestamp()) AND ("
                "c.status = 'pending' OR (c.status = 'claimed' AND c.claim_until <= clock_timestamp()) OR "
                "(c.status = 'published' AND c.published_at <= clock_timestamp() - %s * interval '1 second') OR "
                "c.status = 'running') ORDER BY c.created_at LIMIT %s FOR UPDATE OF c SKIP LOCKED",
                (redeliver_seconds, limit),
            )).fetchall()
            claimed = []
            for row in rows:
                claimed.append(await (await conn.execute(
                    "UPDATE tender_commands SET status = 'claimed', claim_token = %s, "
                    "claim_until = clock_timestamp() + %s * interval '1 second', "
                    "delivery_count = delivery_count + 1 WHERE command_id = %s RETURNING *",
                    (str(uuid4()), claim_seconds, row["command_id"]),
                )).fetchone())
            return claimed

    async def mark_published(self, command_id: str, claim_token: str) -> bool:
        async with self.transaction() as conn:
            row = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE command_id = %s FOR UPDATE", (command_id,),
            )).fetchone()
            if not row or row["claim_token"] != claim_token:
                return False
            # The worker may receive and finish before the publisher records its ack.
            if row["status"] in {"published", "running", "completed"}:
                return True
            result = await conn.execute(
                "UPDATE tender_commands SET status = 'published', published_at = clock_timestamp() "
                "WHERE command_id = %s AND status = 'claimed' AND claim_until > clock_timestamp()",
                (command_id,),
            )
            return result.rowcount == 1

    async def publish_failed(
        self, command_id: str, claim_token: str, error: Any = None, *,
        retryable: bool = True, max_infrastructure_failures: int = 4,
    ) -> bool:
        if max_infrastructure_failures < 1:
            raise ValueError("max_infrastructure_failures must be positive")
        async with self.transaction() as conn:
            identity = await (await conn.execute(
                "SELECT run_id FROM tender_commands WHERE command_id = %s", (command_id,),
            )).fetchone()
            if identity is None:
                return False
            # acquire/cancel/revise also lock run before command. A stale publisher
            # must neither consume the new claim's budget nor overwrite its state.
            run = await self._run(conn, identity["run_id"], lock=True)
            command = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE command_id = %s FOR UPDATE", (command_id,),
            )).fetchone()
            if command["status"] != "claimed" or command["claim_token"] != claim_token:
                return False
            failures = command["infrastructure_failures"] + int(retryable)
            terminal = not retryable or failures >= max_infrastructure_failures
            receipt = None
            if terminal:
                receipt = {
                    "status": "failed", "phase": "publish",
                    "metadata": {"reason": "infrastructure_retries_exhausted" if retryable else "publish_rejected"},
                }
            await conn.execute(
                "UPDATE tender_commands SET status = %s, claim_token = NULL, claim_until = NULL, "
                "last_error = %s, infrastructure_failures = %s, receipt = %s, "
                "completed_at = CASE WHEN %s THEN clock_timestamp() ELSE NULL END WHERE command_id = %s",
                ("completed" if terminal else "pending", Jsonb(_safe_metadata(error)), failures,
                 Jsonb(receipt) if receipt is not None else None, terminal, command_id),
            )
            if terminal:
                await conn.execute(
                    "UPDATE tender_runs SET status = 'failed', updated_at = clock_timestamp() WHERE run_id = %s",
                    (run["run_id"],),
                )
            return True

    async def acquire(
        self, run_id: str, command_id: str, version: int, lease_seconds: float = 30,
    ) -> Lease | None:
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be positive")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            if run["current_version"] != version or run["cancel_requested"] or run["lease_live"]:
                return None
            command = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE command_id = %s FOR UPDATE", (command_id,),
            )).fetchone()
            if (not command or command["run_id"] != run_id or command["version"] != version
                    or command["status"] in {"completed", "cancelled"}):
                return None
            epoch = run["epoch"] + 1
            await conn.execute(
                "UPDATE tender_runs SET status = 'running', epoch = %s, lease_command_id = %s, "
                "lease_until = clock_timestamp() + %s * interval '1 second', updated_at = clock_timestamp() "
                "WHERE run_id = %s", (epoch, command_id, lease_seconds, run_id),
            )
            await conn.execute(
                "UPDATE tender_commands SET status = 'running', started_at = clock_timestamp(), "
                "execution_count = execution_count + 1 WHERE command_id = %s", (command_id,),
            )
            # A terminated predecessor cannot tell whether an already sent model call
            # was billed. Preserve that uncertainty before a new executor can retry.
            await conn.execute(
                "UPDATE tender_model_requests SET status = CASE WHEN status = 'sent' THEN 'uncertain' ELSE 'failed' END, "
                "finished_at = clock_timestamp(), error = '{\"type\":\"executor_lost\"}'::jsonb "
                "WHERE run_id = %s AND version = %s AND epoch < %s AND status IN ('prepared', 'sent')",
                (run_id, version, epoch),
            )
            return Lease(run_id, version, epoch, command_id)

    async def assert_fence(self, lease: Lease, conn: AsyncConnection | None = None) -> dict[str, Any]:
        if conn is None:
            async with self.transaction() as connection:
                return await self.assert_fence(lease, connection)
        run = await self._run(conn, lease.run_id, lock=True)
        if run["cancel_requested"]:
            raise Cancelled("run cancellation requested")
        if (run["current_version"] != lease.version or run["epoch"] != lease.epoch
                or run["lease_command_id"] != lease.command_id or not run["lease_live"]):
            raise StaleExecution("execution lease expired or was replaced")
        return run

    async def heartbeat(self, lease: Lease, lease_seconds: float = 30) -> None:
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be positive")
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            await conn.execute(
                "UPDATE tender_runs SET lease_until = clock_timestamp() + %s * interval '1 second', "
                "updated_at = clock_timestamp() WHERE run_id = %s", (lease_seconds, lease.run_id),
            )

    async def receipt(self, lease: Lease, stage: str, scope: str = "") -> dict[str, Any] | None:
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            return await (await conn.execute(
                "SELECT * FROM tender_artifacts WHERE run_id = %s AND version = %s AND stage = %s AND scope = %s",
                (lease.run_id, lease.version, stage, scope),
            )).fetchone()

    async def commit_artifact(
        self, lease: Lease, stage: str, artifact_ref: dict[str, Any], scope: str = "",
        metadata: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            prior = await (await conn.execute(
                "SELECT * FROM tender_artifacts WHERE run_id = %s AND version = %s AND stage = %s AND scope = %s",
                (lease.run_id, lease.version, stage, scope),
            )).fetchone()
            if prior:
                if prior["artifact_ref"] != artifact_ref:
                    raise Conflict("stage already committed a different immutable artifact")
                return prior
            return await (await conn.execute(
                "INSERT INTO tender_artifacts(run_id, version, stage, scope, artifact_ref, metadata, epoch) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s) RETURNING *",
                (lease.run_id, lease.version, stage, scope, Jsonb(artifact_ref), Jsonb(_safe_metadata(metadata or {})), lease.epoch),
            )).fetchone()

    async def release(
        self, lease: Lease, status: str, checklist_digest: str | None = None,
        error: Any = None, metadata: dict[str, Any] | None = None, max_infrastructure_failures: int = 4,
    ) -> dict[str, Any]:
        if status not in {"retry", "waiting-confirmation", "response-plan-ready", "blocked", "failed"}:
            raise ValueError("invalid execution result")
        if max_infrastructure_failures < 1:
            raise ValueError("max_infrastructure_failures must be positive")
        if status == "waiting-confirmation" and (not checklist_digest or (metadata or {}).get("blocking_issues", 0)):
            raise InvalidConfirmation("only an unblocked checklist may wait for confirmation")
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            if status == "retry":
                command = await (await conn.execute(
                    "UPDATE tender_commands SET infrastructure_failures = infrastructure_failures + 1 "
                    "WHERE command_id = %s RETURNING infrastructure_failures", (lease.command_id,),
                )).fetchone()
                if command["infrastructure_failures"] >= max_infrastructure_failures:
                    status = "failed"
                    metadata = {**(metadata or {}), "reason": "infrastructure_retries_exhausted"}
            if status == "retry":
                await conn.execute(
                    "UPDATE tender_commands SET status = 'pending', claim_token = NULL, claim_until = NULL, "
                    "published_at = NULL, last_error = %s WHERE command_id = %s",
                    (Jsonb(_safe_metadata(error)), lease.command_id),
                )
            else:
                receipt = {"status": status, "epoch": lease.epoch, "metadata": _safe_metadata(metadata or {})}
                await conn.execute(
                    "UPDATE tender_commands SET status = 'completed', completed_at = clock_timestamp(), "
                    "receipt = %s, last_error = %s WHERE command_id = %s",
                    (Jsonb(receipt), Jsonb(_safe_metadata(error)), lease.command_id),
                )
            await conn.execute(
                "UPDATE tender_runs SET status = %s, lease_until = NULL, lease_command_id = NULL, "
                "checklist_digest = COALESCE(%s, checklist_digest), updated_at = clock_timestamp() WHERE run_id = %s",
                ("pending" if status == "retry" else status, checklist_digest, lease.run_id),
            )
            return await self._run(conn, lease.run_id)

    async def confirm(
        self, run_id: str, version: int, checklist_digest: str, idempotency_key: str,
    ) -> dict[str, Any]:
        if not idempotency_key:
            raise ValueError("idempotency_key is required")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            if run["current_version"] != version or run["cancel_requested"]:
                raise InvalidConfirmation("confirmation refers to a cancelled or replaced source version")
            prior = await (await conn.execute(
                "SELECT * FROM tender_confirmations WHERE run_id = %s AND (version = %s OR request_key = %s)",
                (run_id, version, idempotency_key),
            )).fetchall()
            for item in prior:
                if item["version"] != version or item["checklist_digest"] != checklist_digest:
                    raise InvalidConfirmation("confirmation key or checklist is stale")
            if prior:
                return await (await conn.execute(
                    "SELECT * FROM tender_commands WHERE command_id = %s", (prior[0]["command_id"],),
                )).fetchone()
            if run["status"] != "waiting-confirmation" or run["checklist_digest"] != checklist_digest:
                raise InvalidConfirmation("run is not waiting for this checklist")
            command = await self._command(
                conn, run_id, version, "resume", {"version": version, "checklist_digest": checklist_digest},
            )
            await conn.execute(
                "INSERT INTO tender_confirmations(run_id, version, checklist_digest, request_key, command_id) "
                "VALUES (%s, %s, %s, %s, %s)", (run_id, version, checklist_digest, idempotency_key, command["command_id"]),
            )
            await conn.execute(
                "UPDATE tender_runs SET status = 'pending', updated_at = clock_timestamp() WHERE run_id = %s", (run_id,),
            )
            return command

    async def cancel(self, run_id: str) -> dict[str, Any]:
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            if run["cancel_requested"]:
                return run
            await conn.execute(
                "UPDATE tender_runs SET cancel_requested = true, status = 'cancelled', epoch = epoch + 1, "
                "lease_until = NULL, lease_command_id = NULL, checklist_digest = NULL, "
                "updated_at = clock_timestamp() WHERE run_id = %s", (run_id,),
            )
            await conn.execute(
                "UPDATE tender_commands SET status = 'cancelled' WHERE run_id = %s AND status NOT IN ('completed', 'cancelled')",
                (run_id,),
            )
            await self._abandon_requests(conn, run_id)
            return await self._run(conn, run_id)

    async def revise(self, run_id: str, manifest_ref: dict[str, Any], request_key: str) -> dict[str, Any]:
        if not request_key:
            raise ValueError("request_key is required")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            prior = await (await conn.execute(
                "SELECT * FROM tender_versions WHERE run_id = %s AND request_key = %s", (run_id, request_key),
            )).fetchone()
            if prior:
                if prior["manifest_ref"] != manifest_ref:
                    raise Conflict("request_key already identifies a different source revision")
                return {**run, "manifest_ref": prior["manifest_ref"], "submitted_version": prior["version"]}
            version = run["current_version"] + 1
            await conn.execute(
                "UPDATE tender_runs SET current_version = %s, status = 'pending', epoch = epoch + 1, "
                "lease_until = NULL, lease_command_id = NULL, cancel_requested = false, checklist_digest = NULL, "
                "updated_at = clock_timestamp() WHERE run_id = %s", (version, run_id),
            )
            await conn.execute(
                "UPDATE tender_commands SET status = 'cancelled' WHERE run_id = %s AND status NOT IN ('completed', 'cancelled')",
                (run_id,),
            )
            await self._abandon_requests(conn, run_id)
            await conn.execute(
                "INSERT INTO tender_versions(run_id, version, request_key, manifest_ref) VALUES (%s, %s, %s, %s)",
                (run_id, version, request_key, Jsonb(manifest_ref)),
            )
            await self._command(conn, run_id, version, "start")
            return {**(await self._run(conn, run_id)), "manifest_ref": manifest_ref, "submitted_version": version}

    async def _abandon_requests(self, conn: AsyncConnection, run_id: str) -> None:
        await conn.execute(
            "UPDATE tender_model_requests SET status = CASE WHEN status = 'sent' THEN 'uncertain' ELSE 'failed' END, "
            "finished_at = clock_timestamp(), error = '{\"type\":\"run_invalidated\"}'::jsonb "
            "WHERE run_id = %s AND status IN ('prepared', 'sent')", (run_id,),
        )

    async def begin_request(
        self, lease: Lease, request_id: str, model: str, kind: str, metadata: dict[str, Any] | None = None,
    ) -> None:
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            await conn.execute(
                "INSERT INTO tender_model_requests(request_id, run_id, version, epoch, model, kind, metadata) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s)",
                (request_id, lease.run_id, lease.version, lease.epoch, model, kind, Jsonb(_safe_metadata(metadata or {}))),
            )

    async def finish_request(
        self, request_id: str, status: str, usage: dict[str, Any] | None = None, error: Any = None,
    ) -> None:
        if status not in {"sent", "succeeded", "failed", "uncertain"}:
            raise ValueError("invalid model request status")
        async with self.transaction() as conn:
            if status == "sent":
                request = await (await conn.execute(
                    "SELECT * FROM tender_model_requests WHERE request_id = %s", (request_id,),
                )).fetchone()
                if not request:
                    raise StoreError("model request not registered")
                # Follow the same run -> request lock order as acquire/cancel.
                run = await self._run(conn, request["run_id"], lock=True)
                if run["cancel_requested"]:
                    raise Cancelled("run cancellation requested before model send")
                if (run["current_version"] != request["version"] or run["epoch"] != request["epoch"]
                        or not run["lease_live"]):
                    raise StaleExecution("model request belongs to a stale executor")
            row = await (await conn.execute(
                "SELECT * FROM tender_model_requests WHERE request_id = %s FOR UPDATE", (request_id,),
            )).fetchone()
            if not row:
                raise StoreError("model request not registered")
            if row["status"] in {"succeeded", "failed"}:
                if row["status"] != status:
                    raise Conflict("model request already finished")
                return
            if row["status"] == "uncertain" and status == "sent":
                raise Conflict("an uncertain request cannot be sent again with the same request_id")
            await conn.execute(
                "UPDATE tender_model_requests SET status = %s, usage = COALESCE(%s, usage), error = %s, "
                "sent_at = CASE WHEN %s = 'sent' THEN COALESCE(sent_at, clock_timestamp()) ELSE sent_at END, "
                "finished_at = CASE WHEN %s != 'sent' THEN clock_timestamp() ELSE finished_at END WHERE request_id = %s",
                (status, Jsonb(usage) if usage is not None else None, Jsonb(_safe_metadata(error)), status, status, request_id),
            )
