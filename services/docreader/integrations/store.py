"""PostgreSQL business receipts for the independently launched Python workflow.

The run row is the fencing lock, including for the LangGraph checkpointer. Queue
publication is deliberately separate from an execution receipt. No Rust table is
read or written here.
"""

from __future__ import annotations

from contextlib import asynccontextmanager
from dataclasses import dataclass
import re
from typing import Any, AsyncIterator
from uuid import uuid4

from psycopg import AsyncConnection
from psycopg.rows import dict_row
from psycopg.types.json import Jsonb

from docreader.tender.schema import Task, validate_issue_resolutions
from docreader.workflow.state import WORKFLOW_PROTOCOL


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


class RequestReplayBlocked(StoreError):
    """The provider may already have executed this immutable logical request."""

    def __init__(self, request_id: str, status: str):
        self.request_id, self.status = request_id, status
        super().__init__("logical model request requires explicit reconciliation before replay")


@dataclass(frozen=True)
class Lease:
    run_id: str
    source_version: int
    epoch: int
    command_id: str
    domain_revision: int = 1


SOURCE_STAGES = frozenset({"register", "freeze", "freeze-document", "ocr", "source-spec"})


def _revision_view(row: dict[str, Any]) -> dict[str, Any]:
    """Expose the current item name while retaining the historical SQL column."""
    result = dict(row)
    result["corrected_items_ref"] = result.pop("corrected_requirements_ref")
    return result


def _assert_analysis_protocol(revision: dict[str, Any]) -> None:
    spec = revision.get("execution_spec")
    if spec is not None and spec.get("workflow_protocol") != WORKFLOW_PROTOCOL:
        raise Conflict("legacy six-stage revisions are read-only; submit a new analysis run")


_SCHEMA = """
CREATE TABLE IF NOT EXISTS tender_runs (
    run_id text PRIMARY KEY,
    request_key text NOT NULL UNIQUE,
    source_version integer NOT NULL DEFAULT 1,
    domain_revision integer NOT NULL DEFAULT 1,
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
    source_version integer NOT NULL,
    request_key text NOT NULL,
    manifest_ref jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version),
    UNIQUE (run_id, request_key)
);
CREATE TABLE IF NOT EXISTS tender_revisions (
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    request_key text NOT NULL,
    corrected_requirements_ref jsonb,
    scope_decisions_ref jsonb,
    upstream_digest text NOT NULL DEFAULT '',
    reason text NOT NULL DEFAULT '',
    execution_spec jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version, domain_revision),
    UNIQUE (run_id, request_key),
    FOREIGN KEY (run_id, source_version) REFERENCES tender_versions(run_id, source_version)
);
CREATE TABLE IF NOT EXISTS tender_commands (
    command_id text PRIMARY KEY,
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
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
    FOREIGN KEY (run_id, source_version, domain_revision) REFERENCES tender_revisions(run_id, source_version, domain_revision)
);
CREATE INDEX IF NOT EXISTS tender_commands_dispatch_idx
    ON tender_commands(status, created_at);
CREATE TABLE IF NOT EXISTS tender_artifacts (
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    stage text NOT NULL,
    scope text NOT NULL DEFAULT '',
    artifact_ref jsonb NOT NULL,
    input_digest text NOT NULL DEFAULT '',
    metadata jsonb NOT NULL DEFAULT '{}',
    epoch bigint NOT NULL,
    committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version, domain_revision, stage, scope),
    FOREIGN KEY (run_id, source_version) REFERENCES tender_versions(run_id, source_version)
);
CREATE TABLE IF NOT EXISTS tender_confirmations (
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    checklist_digest text NOT NULL,
    request_key text NOT NULL,
    command_id text NOT NULL REFERENCES tender_commands(command_id),
    issue_resolutions jsonb,
    confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (run_id, source_version, domain_revision),
    UNIQUE (run_id, request_key),
    FOREIGN KEY (run_id, source_version, domain_revision) REFERENCES tender_revisions(run_id, source_version, domain_revision)
);
ALTER TABLE tender_confirmations ADD COLUMN IF NOT EXISTS issue_resolutions jsonb;
CREATE TABLE IF NOT EXISTS tender_model_requests (
    request_id text PRIMARY KEY,
    run_id text NOT NULL,
    source_version integer NOT NULL,
    domain_revision integer NOT NULL,
    epoch bigint NOT NULL,
    model text NOT NULL,
    task text NOT NULL,
    status text NOT NULL DEFAULT 'prepared',
    metadata jsonb NOT NULL DEFAULT '{}',
    usage jsonb,
    response_ref jsonb,
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    sent_at timestamptz,
    finished_at timestamptz,
    FOREIGN KEY (run_id, source_version, domain_revision) REFERENCES tender_revisions(run_id, source_version, domain_revision)
);
CREATE INDEX IF NOT EXISTS tender_model_requests_replay_idx
    ON tender_model_requests(run_id, source_version, domain_revision, task, (metadata->>'request_digest'))
    WHERE status IN ('sent', 'uncertain', 'succeeded');
CREATE INDEX IF NOT EXISTS tender_model_requests_source_reuse_idx
    ON tender_model_requests(run_id, source_version, task, (metadata->>'request_digest'))
    WHERE status = 'succeeded';
"""


def _recorded_first_data(activity: dict[str, Any] | None) -> dict[str, float] | None:
    value = activity.get("first_data_seconds") if isinstance(activity, dict) else None
    if type(value) not in {int, float} or isinstance(value, bool) or value < 0:
        return None
    return {"first_data_seconds": value}


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
        self, conn: AsyncConnection, run_id: str, source_version: int, kind: str,
        payload: dict[str, Any] | None = None, *, domain_revision: int = 1,
    ) -> dict[str, Any]:
        return await (await conn.execute(
            "INSERT INTO tender_commands(command_id, run_id, source_version, domain_revision, kind, payload) "
            "VALUES (%s, %s, %s, %s, %s, %s) RETURNING *",
            (str(uuid4()), run_id, source_version, domain_revision, kind, Jsonb(payload or {})),
        )).fetchone()

    async def _assert_analysis_revision(self, conn: AsyncConnection, run: dict[str, Any]) -> None:
        revision = await (await conn.execute(
            "SELECT execution_spec FROM tender_revisions WHERE run_id = %s AND source_version = %s AND domain_revision = %s",
            (run["run_id"], run["source_version"], run["domain_revision"]),
        )).fetchone()
        _assert_analysis_protocol(revision)
        if revision["execution_spec"] is None:
            # Legacy workers wrote source-spec before pinning a model config.
            # New v2 workers pin first; same-source corrections inherit that
            # explicit protocol while reusing immutable source receipts.
            legacy = await (await conn.execute(
                "SELECT 1 FROM tender_artifacts a WHERE a.run_id = %s AND a.source_version = %s "
                "AND a.stage = 'source-spec' AND NOT EXISTS (SELECT 1 FROM tender_revisions d "
                "WHERE d.run_id = a.run_id AND d.source_version = a.source_version "
                "AND d.execution_spec->'workflow_protocol' = %s)",
                (run["run_id"], run["source_version"], Jsonb(WORKFLOW_PROTOCOL)),
            )).fetchone()
            if legacy:
                raise Conflict("legacy six-stage revisions are read-only; submit a new analysis run")

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
                    "ON r.run_id = v.run_id AND v.source_version = 1 WHERE r.request_key = %s", (request_key,),
                )).fetchone()
                if existing["manifest_ref"] != manifest_ref:
                    raise Conflict("request_key already identifies a different submission")
                return {**existing, "submitted_source_version": 1, "submitted_domain_revision": 1}
            await conn.execute(
                "INSERT INTO tender_versions(run_id, source_version, request_key, manifest_ref) VALUES (%s, 1, %s, %s)",
                (run_id, request_key, Jsonb(manifest_ref)),
            )
            await conn.execute(
                "INSERT INTO tender_revisions(run_id, source_version, domain_revision, request_key) VALUES (%s, 1, 1, %s)",
                (run_id, request_key),
            )
            await self._command(conn, run_id, 1, "start")
            return {**inserted, "manifest_ref": manifest_ref,
                    "submitted_source_version": 1, "submitted_domain_revision": 1}

    async def get_run(self, run_id: str) -> dict[str, Any]:
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            source_version, revision = run["source_version"], run["domain_revision"]
            run["manifest_ref"] = (await (await conn.execute(
                "SELECT manifest_ref FROM tender_versions WHERE run_id = %s AND source_version = %s", (run_id, source_version),
            )).fetchone())["manifest_ref"]
            run["revision"] = await (await conn.execute(
                "SELECT * FROM tender_revisions WHERE run_id = %s AND source_version = %s AND domain_revision = %s",
                (run_id, source_version, revision),
            )).fetchone()
            run["revision"] = _revision_view(run["revision"])
            for key, query in (
                ("artifacts", "SELECT * FROM tender_artifacts WHERE run_id = %s AND source_version = %s AND domain_revision IN (0, %s) ORDER BY stage, scope"),
                ("commands", "SELECT * FROM tender_commands WHERE run_id = %s AND source_version = %s AND domain_revision = %s ORDER BY created_at"),
                ("usage", "SELECT * FROM tender_model_requests WHERE run_id = %s AND source_version = %s AND domain_revision = %s ORDER BY created_at"),
                ("confirmations", "SELECT * FROM tender_confirmations WHERE run_id = %s AND source_version = %s AND domain_revision = %s"),
            ):
                run[key] = await (await conn.execute(query, (run_id, source_version, revision))).fetchall()
            run["history"] = {}
            for key, query in (
                ("artifacts", "SELECT * FROM tender_artifacts WHERE run_id = %s AND NOT (source_version = %s AND domain_revision IN (0, %s)) ORDER BY committed_at"),
                ("commands", "SELECT * FROM tender_commands WHERE run_id = %s AND NOT (source_version = %s AND domain_revision = %s) ORDER BY created_at"),
                ("usage", "SELECT * FROM tender_model_requests WHERE run_id = %s AND NOT (source_version = %s AND domain_revision = %s) ORDER BY created_at"),
                ("confirmations", "SELECT * FROM tender_confirmations WHERE run_id = %s AND NOT (source_version = %s AND domain_revision = %s) ORDER BY confirmed_at"),
                ("revisions", "SELECT * FROM tender_revisions WHERE run_id = %s AND NOT (source_version = %s AND domain_revision = %s) ORDER BY created_at"),
            ):
                run["history"][key] = await (await conn.execute(query, (run_id, source_version, revision))).fetchall()
            run["history"]["revisions"] = [_revision_view(item) for item in run["history"]["revisions"]]
            return run

    async def get_revision(self, lease: Lease) -> dict[str, Any]:
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            row = await (await conn.execute(
                "SELECT * FROM tender_revisions WHERE run_id = %s AND source_version = %s AND domain_revision = %s",
                (lease.run_id, lease.source_version, lease.domain_revision),
            )).fetchone()
            return _revision_view(row)

    async def pin_execution_spec(self, lease: Lease, spec: dict[str, Any]) -> dict[str, Any]:
        if _safe_metadata(spec) != spec:
            raise ValueError("execution specification must not contain credentials")
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            row = await (await conn.execute(
                "SELECT execution_spec FROM tender_revisions WHERE run_id = %s AND source_version = %s AND domain_revision = %s",
                (lease.run_id, lease.source_version, lease.domain_revision),
            )).fetchone()
            if row["execution_spec"] is not None:
                if row["execution_spec"] != spec:
                    raise Conflict("execution configuration changed within a domain revision")
                return row["execution_spec"]
            await conn.execute(
                "UPDATE tender_revisions SET execution_spec = %s WHERE run_id = %s AND source_version = %s AND domain_revision = %s",
                (Jsonb(spec), lease.run_id, lease.source_version, lease.domain_revision),
            )
            return spec

    async def get_manifest(self, run_id: str, source_version: int) -> dict[str, Any]:
        async with self.transaction() as conn:
            row = await (await conn.execute(
                "SELECT manifest_ref FROM tender_versions WHERE run_id = %s AND source_version = %s", (run_id, source_version),
            )).fetchone()
            if row is None:
                raise StoreError("source snapshot not found")
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
                "JOIN tender_revisions d ON d.run_id = r.run_id AND d.source_version = r.source_version "
                "AND d.domain_revision = r.domain_revision "
                "WHERE c.source_version = r.source_version AND c.domain_revision = r.domain_revision AND NOT r.cancel_requested "
                "AND (d.execution_spec->'workflow_protocol' = %s OR (d.execution_spec IS NULL AND ("
                "NOT EXISTS (SELECT 1 FROM tender_artifacts a WHERE a.run_id = r.run_id "
                "AND a.source_version = r.source_version AND a.stage = 'source-spec') OR "
                "EXISTS (SELECT 1 FROM tender_revisions h WHERE h.run_id = r.run_id "
                "AND h.source_version = r.source_version AND h.execution_spec->'workflow_protocol' = %s)))) "
                "AND (r.lease_until IS NULL OR r.lease_until <= clock_timestamp()) AND ("
                "c.status = 'pending' OR (c.status = 'claimed' AND c.claim_until <= clock_timestamp()) OR "
                "(c.status = 'published' AND c.published_at <= clock_timestamp() - %s * interval '1 second') OR "
                "c.status = 'running') ORDER BY c.created_at LIMIT %s FOR UPDATE OF c SKIP LOCKED",
                (Jsonb(WORKFLOW_PROTOCOL), Jsonb(WORKFLOW_PROTOCOL), redeliver_seconds, limit),
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
        self, run_id: str, command_id: str, source_version: int, lease_seconds: float = 30,
    ) -> Lease | None:
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be positive")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            if run["source_version"] != source_version or run["cancel_requested"] or run["lease_live"]:
                return None
            command = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE command_id = %s FOR UPDATE", (command_id,),
            )).fetchone()
            if (not command or command["run_id"] != run_id or command["source_version"] != source_version
                    or command["domain_revision"] != run["domain_revision"]
                    or command["status"] in {"completed", "cancelled"}):
                return None
            await self._assert_analysis_revision(conn, run)
            epoch = run["epoch"] + 1
            # A terminated predecessor cannot tell whether an already sent model call
            # was billed. Preserve that uncertainty even if its retry budget is spent.
            await conn.execute(
                "UPDATE tender_model_requests SET status = CASE WHEN status = 'sent' THEN 'uncertain' ELSE 'failed' END, "
                "finished_at = clock_timestamp(), error = '{\"type\":\"executor_lost\"}'::jsonb "
                "WHERE run_id = %s AND source_version = %s AND epoch < %s AND status IN ('prepared', 'sent')",
                (run_id, source_version, epoch),
            )
            if run["lease_command_id"] == command_id:
                # Dispatch can change running to claimed/published before takeover.
                # The expired lease identifies the lost executor, not command status.
                # Old workers did not count losses; execution_count preserves that floor.
                failures = max(command["infrastructure_failures"] + 1, command["execution_count"])
                await conn.execute(
                    "UPDATE tender_commands SET infrastructure_failures = %s, last_error = %s WHERE command_id = %s",
                    (failures, Jsonb({"type": "executor_lost"}), command_id),
                )
                if failures >= 4:
                    receipt = {"status": "failed", "epoch": run["epoch"],
                               "metadata": {"reason": "infrastructure_retries_exhausted"}}
                    await conn.execute(
                        "UPDATE tender_commands SET status = 'completed', completed_at = clock_timestamp(), "
                        "receipt = %s WHERE command_id = %s", (Jsonb(receipt), command_id),
                    )
                    await conn.execute(
                        "UPDATE tender_runs SET status = 'failed', lease_until = NULL, lease_command_id = NULL, "
                        "updated_at = clock_timestamp() WHERE run_id = %s", (run_id,),
                    )
                    return None
            await conn.execute(
                "UPDATE tender_runs SET status = 'running', epoch = %s, lease_command_id = %s, "
                "lease_until = clock_timestamp() + %s * interval '1 second', updated_at = clock_timestamp() "
                "WHERE run_id = %s", (epoch, command_id, lease_seconds, run_id),
            )
            await conn.execute(
                "UPDATE tender_commands SET status = 'running', started_at = clock_timestamp(), "
                "execution_count = execution_count + 1 WHERE command_id = %s", (command_id,),
            )
            return Lease(run_id, source_version, epoch, command_id, command["domain_revision"])

    async def assert_fence(self, lease: Lease, conn: AsyncConnection | None = None) -> dict[str, Any]:
        if conn is None:
            async with self.transaction() as connection:
                return await self.assert_fence(lease, connection)
        run = await self._run(conn, lease.run_id, lock=True)
        if run["cancel_requested"]:
            raise Cancelled("run cancellation requested")
        if (run["source_version"] != lease.source_version or run["domain_revision"] != lease.domain_revision
                or run["epoch"] != lease.epoch
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

    async def receipt(
        self, lease: Lease, stage: str, scope: str = "", *, input_digest: str = "",
    ) -> dict[str, Any] | None:
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            row = await (await conn.execute(
                "SELECT * FROM tender_artifacts WHERE run_id = %s AND source_version = %s AND domain_revision = %s AND stage = %s AND scope = %s",
                (lease.run_id, lease.source_version, 0 if stage in SOURCE_STAGES else lease.domain_revision, stage, scope),
            )).fetchone()
            if row and row["input_digest"] != input_digest:
                raise Conflict("artifact inputs changed within an immutable stage identity")
            return row

    async def commit_artifact(
        self, lease: Lease, stage: str, artifact_ref: dict[str, Any], scope: str = "",
        metadata: dict[str, Any] | None = None, *, input_digest: str = "",
    ) -> dict[str, Any]:
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            prior = await (await conn.execute(
                "SELECT * FROM tender_artifacts WHERE run_id = %s AND source_version = %s AND domain_revision = %s AND stage = %s AND scope = %s",
                (lease.run_id, lease.source_version, 0 if stage in SOURCE_STAGES else lease.domain_revision, stage, scope),
            )).fetchone()
            if prior:
                if prior["artifact_ref"] != artifact_ref or prior["input_digest"] != input_digest:
                    raise Conflict("stage already committed different immutable content or inputs")
                return prior
            return await (await conn.execute(
                "INSERT INTO tender_artifacts(run_id, source_version, domain_revision, stage, scope, artifact_ref, input_digest, metadata, epoch) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s) RETURNING *",
                (lease.run_id, lease.source_version, 0 if stage in SOURCE_STAGES else lease.domain_revision, stage, scope,
                 Jsonb(artifact_ref), input_digest, Jsonb(_safe_metadata(metadata or {})), lease.epoch),
            )).fetchone()

    async def release(
        self, lease: Lease, status: str, checklist_digest: str | None = None,
        error: Any = None, metadata: dict[str, Any] | None = None, max_infrastructure_failures: int = 4,
    ) -> dict[str, Any]:
        if status not in {"retry", "waiting-confirmation", "analysis-ready", "response-plan-ready", "blocked", "failed"}:
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

    async def retry(
        self, run_id: str, source_version: int, domain_revision: int, failed_command_id: str,
        request_key: str, reason: str,
    ) -> dict[str, Any]:
        """Explicitly continue a failed execution, retaining all paid receipts."""
        if not request_key.strip() or not reason.strip() or source_version < 1 or domain_revision < 1:
            raise ValueError("retry requires a request key, reason, and positive source/domain versions")
        audit = {"failed_command_id": failed_command_id, "request_key": request_key, "reason": reason}
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            if (run["source_version"] != source_version or run["domain_revision"] != domain_revision
                    or run["cancel_requested"]):
                raise Conflict("retry baseline is stale or the run is cancelled")
            await self._assert_analysis_revision(conn, run)
            prior = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE run_id = %s AND payload->'retry'->>'request_key' = %s",
                (run_id, request_key),
            )).fetchone()
            if prior:
                if (prior["source_version"] != source_version or prior["domain_revision"] != domain_revision
                        or prior["payload"]["retry"] != audit):
                    raise Conflict("request_key already identifies a different retry")
                return prior
            if run["status"] != "failed" or run["lease_live"]:
                raise Conflict("only a failed run without an active lease can be retried")
            failed = await (await conn.execute(
                "SELECT * FROM tender_commands WHERE run_id = %s AND source_version = %s AND domain_revision = %s "
                "ORDER BY created_at DESC, command_id DESC LIMIT 1 FOR UPDATE",
                (run_id, source_version, domain_revision),
            )).fetchone()
            if (not failed or failed["command_id"] != failed_command_id or failed["status"] != "completed"
                    or (failed["receipt"] or {}).get("status") != "failed"):
                raise Conflict("retry must identify the current latest failed command")
            uncertain = await (await conn.execute(
                "SELECT request_id, status FROM tender_model_requests WHERE run_id = %s AND source_version = %s "
                "AND domain_revision = %s AND status IN ('sent', 'uncertain') ORDER BY created_at, request_id LIMIT 1",
                (run_id, source_version, domain_revision),
            )).fetchone()
            if uncertain:
                raise RequestReplayBlocked(uncertain["request_id"], uncertain["status"])
            payload = {key: value for key, value in failed["payload"].items() if key != "retry"}
            command = await self._command(conn, run_id, source_version, failed["kind"],
                                          {**payload, "retry": audit}, domain_revision=domain_revision)
            await conn.execute(
                "UPDATE tender_runs SET status = 'pending', updated_at = clock_timestamp() WHERE run_id = %s",
                (run_id,),
            )
            return command

    async def confirm(
        self, run_id: str, source_version: int, checklist_digest: str, idempotency_key: str,
        *, domain_revision: int | None = None, issue_resolutions: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        if not idempotency_key:
            raise ValueError("idempotency_key is required")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            revision = run["domain_revision"] if domain_revision is None else domain_revision
            if (run["source_version"] != source_version or revision != run["domain_revision"]
                    or run["cancel_requested"]):
                raise InvalidConfirmation("confirmation refers to a cancelled or replaced source/domain revision")
            await self._assert_analysis_revision(conn, run)
            # Validate before persisting: a stored confirmation is idempotent and
            # an incomplete one would otherwise block every later correct attempt.
            waiting = await (await conn.execute(
                "SELECT receipt FROM tender_commands WHERE run_id = %s AND source_version = %s AND domain_revision = %s "
                "AND status = 'completed' AND receipt->>'status' = 'waiting-confirmation' "
                "ORDER BY completed_at DESC, command_id DESC LIMIT 1",
                (run_id, source_version, revision),
            )).fetchone()
            pending = ((waiting or {}).get("receipt") or {}).get("metadata", {}).get("human_issue_ids", [])
            try:
                resolutions = validate_issue_resolutions(pending, issue_resolutions)
            except ValueError:
                raise InvalidConfirmation("confirmation must resolve exactly the pending semantic issues") from None
            prior = await (await conn.execute(
                "SELECT * FROM tender_confirmations WHERE run_id = %s AND ((source_version = %s AND domain_revision = %s) OR request_key = %s)",
                (run_id, source_version, revision, idempotency_key),
            )).fetchall()
            for item in prior:
                if (item["source_version"] != source_version or item["domain_revision"] != revision
                        or item["checklist_digest"] != checklist_digest or item["issue_resolutions"] != resolutions):
                    raise InvalidConfirmation("confirmation key or checklist is stale")
            if prior:
                return await (await conn.execute(
                    "SELECT * FROM tender_commands WHERE command_id = %s", (prior[0]["command_id"],),
                )).fetchone()
            if run["status"] != "waiting-confirmation" or run["checklist_digest"] != checklist_digest:
                raise InvalidConfirmation("run is not waiting for this checklist")
            command = await self._command(
                conn, run_id, source_version, "resume",
                {"source_version": source_version, "domain_revision": revision, "checklist_digest": checklist_digest,
                 **({"issue_resolutions": resolutions} if resolutions is not None else {})},
                domain_revision=revision,
            )
            await conn.execute(
                "INSERT INTO tender_confirmations(run_id, source_version, domain_revision, checklist_digest, request_key, "
                "command_id, issue_resolutions) VALUES (%s, %s, %s, %s, %s, %s, %s)",
                (run_id, source_version, revision, checklist_digest, idempotency_key, command["command_id"],
                 Jsonb(resolutions) if resolutions is not None else None),
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
            await self._cancel_commands(conn, run_id, "cancel_requested")
            await self._abandon_requests(conn, run_id)
            return await self._run(conn, run_id)

    async def revise(self, run_id: str, manifest_ref: dict[str, Any], request_key: str) -> dict[str, Any]:
        if not request_key:
            raise ValueError("request_key is required")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            await self._assert_analysis_revision(conn, run)
            prior = await (await conn.execute(
                "SELECT * FROM tender_versions WHERE run_id = %s AND request_key = %s", (run_id, request_key),
            )).fetchone()
            if prior:
                if prior["manifest_ref"] != manifest_ref:
                    raise Conflict("request_key already identifies a different source revision")
                return {**run, "manifest_ref": prior["manifest_ref"],
                        "submitted_source_version": prior["source_version"], "submitted_domain_revision": 1}
            current = await (await conn.execute(
                "SELECT manifest_ref FROM tender_versions WHERE run_id = %s AND source_version = %s",
                (run_id, run["source_version"]),
            )).fetchone()
            if current["manifest_ref"] == manifest_ref:
                raise Conflict("source revision requires a changed manifest; use correct for requirement corrections")
            source_version = run["source_version"] + 1
            reused = await (await conn.execute(
                "SELECT 1 FROM tender_revisions WHERE run_id = %s AND request_key = %s", (run_id, request_key),
            )).fetchone()
            if reused:
                raise Conflict("request_key already identifies a domain correction")
            await conn.execute(
                "UPDATE tender_runs SET source_version = %s, domain_revision = 1, status = 'pending', epoch = epoch + 1, "
                "lease_until = NULL, lease_command_id = NULL, cancel_requested = false, checklist_digest = NULL, "
                "updated_at = clock_timestamp() WHERE run_id = %s", (source_version, run_id),
            )
            await self._cancel_commands(conn, run_id, "source_revised")
            await self._abandon_requests(conn, run_id)
            await conn.execute(
                "INSERT INTO tender_versions(run_id, source_version, request_key, manifest_ref) VALUES (%s, %s, %s, %s)",
                (run_id, source_version, request_key, Jsonb(manifest_ref)),
            )
            await conn.execute(
                "INSERT INTO tender_revisions(run_id, source_version, domain_revision, request_key) VALUES (%s, %s, 1, %s)",
                (run_id, source_version, request_key),
            )
            await self._command(conn, run_id, source_version, "start")
            return {**(await self._run(conn, run_id)), "manifest_ref": manifest_ref,
                    "submitted_source_version": source_version, "submitted_domain_revision": 1}

    async def correct(
        self, run_id: str, source_version: int, expected_revision: int, base_items_digest: str,
        corrected_items_ref: dict[str, Any], request_key: str, reason: str,
    ) -> dict[str, Any]:
        if not request_key or not reason.strip() or expected_revision < 1:
            raise ValueError("correction requires a request key, reason, and positive expected revision")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            await self._assert_analysis_revision(conn, run)
            prior = await (await conn.execute(
                "SELECT * FROM tender_revisions WHERE run_id = %s AND request_key = %s", (run_id, request_key),
            )).fetchone()
            if prior:
                if (prior["source_version"] != source_version or prior["domain_revision"] != expected_revision + 1
                        or prior["upstream_digest"] != base_items_digest
                        or prior["corrected_requirements_ref"] != corrected_items_ref or prior["reason"] != reason):
                    raise Conflict("request_key already identifies a different correction")
                return {**run, "submitted_source_version": source_version, "submitted_domain_revision": prior["domain_revision"]}
            if (run["source_version"] != source_version or run["domain_revision"] != expected_revision
                    or run["cancel_requested"]):
                raise Conflict("correction baseline is stale or the run is cancelled")
            frozen = await (await conn.execute(
                "SELECT 1 FROM tender_artifacts WHERE run_id = %s AND source_version = %s AND domain_revision = 0 "
                "AND stage = 'freeze' AND scope = ''", (run_id, source_version),
            )).fetchone()
            extracted = await (await conn.execute(
                "SELECT artifact_ref FROM tender_artifacts WHERE run_id = %s AND source_version = %s AND domain_revision = %s "
                "AND stage = 'extract' AND scope = ''", (run_id, source_version, expected_revision),
            )).fetchone()
            if not frozen or not extracted:
                raise Conflict("sources must be frozen and requirements extracted before correction")
            if extracted["artifact_ref"]["sha256"] != base_items_digest:
                raise Conflict("items baseline digest does not match")
            previous_revision = await (await conn.execute(
                "SELECT scope_decisions_ref, execution_spec FROM tender_revisions WHERE run_id = %s AND source_version = %s AND domain_revision = %s",
                (run_id, source_version, expected_revision),
            )).fetchone()
            revision = expected_revision + 1
            await conn.execute(
                "INSERT INTO tender_revisions(run_id, source_version, domain_revision, request_key, corrected_requirements_ref, scope_decisions_ref, upstream_digest, reason) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s, %s)",
                (run_id, source_version, revision, request_key, Jsonb(corrected_items_ref),
                 Jsonb(previous_revision["scope_decisions_ref"]) if previous_revision["scope_decisions_ref"] is not None else None,
                 base_items_digest, reason),
            )
            await conn.execute(
                "UPDATE tender_runs SET domain_revision = %s, status = 'pending', epoch = epoch + 1, "
                "lease_until = NULL, lease_command_id = NULL, checklist_digest = NULL, updated_at = clock_timestamp() "
                "WHERE run_id = %s", (revision, run_id),
            )
            await self._cancel_commands(conn, run_id, "domain_corrected")
            await self._abandon_requests(conn, run_id)
            await self._command(conn, run_id, source_version, "start", domain_revision=revision)
            return {**(await self._run(conn, run_id)), "submitted_source_version": source_version, "submitted_domain_revision": revision}

    async def review_scope(
        self, run_id: str, source_version: int, expected_revision: int, base_plan_digest: str,
        scope_decisions_ref: dict[str, Any], request_key: str, reason: str,
    ) -> dict[str, Any]:
        """Advance a validated, durable scope decision without changing native sources.

        The domain/CLI boundary validates decision evidence and target ranges.
        This transaction binds those decisions to the still-current plan receipt.
        """
        if not request_key or not reason.strip() or expected_revision < 1 or not scope_decisions_ref:
            raise ValueError("scope review requires decisions, a request key, reason, and positive expected revision")
        async with self.transaction() as conn:
            run = await self._run(conn, run_id, lock=True)
            await self._assert_analysis_revision(conn, run)
            prior = await (await conn.execute(
                "SELECT * FROM tender_revisions WHERE run_id = %s AND request_key = %s", (run_id, request_key),
            )).fetchone()
            if prior:
                if (prior["source_version"] != source_version or prior["domain_revision"] != expected_revision + 1
                        or prior["upstream_digest"] != base_plan_digest or prior["scope_decisions_ref"] != scope_decisions_ref
                        or prior["corrected_requirements_ref"] is not None or prior["reason"] != reason):
                    raise Conflict("request_key already identifies a different scope review")
                return {**run, "submitted_source_version": source_version,
                        "submitted_domain_revision": prior["domain_revision"]}
            if (run["source_version"] != source_version or run["domain_revision"] != expected_revision
                    or run["cancel_requested"]):
                raise Conflict("scope review baseline is stale or the run is cancelled")
            frozen = await (await conn.execute(
                "SELECT 1 FROM tender_artifacts WHERE run_id = %s AND source_version = %s AND domain_revision = 0 "
                "AND stage = 'freeze' AND scope = ''", (run_id, source_version),
            )).fetchone()
            plan = await (await conn.execute(
                "SELECT metadata->>'plan_digest' AS plan_digest FROM tender_artifacts "
                "WHERE run_id = %s AND source_version = %s AND domain_revision = %s AND stage = 'pack-plan' AND scope = ''",
                (run_id, source_version, expected_revision),
            )).fetchone()
            if not frozen or not plan:
                raise Conflict("native sources must be frozen and the current pack plan committed before scope review")
            if not base_plan_digest or plan["plan_digest"] != base_plan_digest:
                raise Conflict("pack plan baseline digest does not match")
            revision = expected_revision + 1
            await conn.execute(
                "INSERT INTO tender_revisions(run_id, source_version, domain_revision, request_key, scope_decisions_ref, upstream_digest, reason) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s)",
                (run_id, source_version, revision, request_key, Jsonb(scope_decisions_ref), base_plan_digest, reason),
            )
            await conn.execute(
                "UPDATE tender_runs SET domain_revision = %s, status = 'pending', epoch = epoch + 1, "
                "lease_until = NULL, lease_command_id = NULL, checklist_digest = NULL, updated_at = clock_timestamp() "
                "WHERE run_id = %s", (revision, run_id),
            )
            await self._cancel_commands(conn, run_id, "scope_reviewed")
            await self._abandon_requests(conn, run_id)
            await self._command(conn, run_id, source_version, "start", domain_revision=revision)
            return {**(await self._run(conn, run_id)), "submitted_source_version": source_version,
                    "submitted_domain_revision": revision}

    async def _cancel_commands(self, conn: AsyncConnection, run_id: str, reason: str) -> None:
        await conn.execute(
            "UPDATE tender_commands SET status = 'cancelled', completed_at = clock_timestamp(), receipt = %s "
            "WHERE run_id = %s AND status NOT IN ('completed', 'cancelled')",
            (Jsonb({"status": "cancelled", "reason": reason}), run_id),
        )

    async def _abandon_requests(self, conn: AsyncConnection, run_id: str) -> None:
        await conn.execute(
            "UPDATE tender_model_requests SET status = CASE WHEN status = 'sent' THEN 'uncertain' ELSE 'failed' END, "
            "finished_at = clock_timestamp(), error = '{\"type\":\"run_invalidated\"}'::jsonb "
            "WHERE run_id = %s AND status IN ('prepared', 'sent')", (run_id,),
        )

    async def begin_request(
        self, lease: Lease, request_id: str, model: str, task: Task, metadata: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        digest = (metadata or {}).get("request_digest")
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            raise ValueError("model audit requires a lowercase SHA-256 request_digest")
        async with self.transaction() as conn:
            await self.assert_fence(lease, conn)
            prior = await self._matching_model_requests(
                conn, lease.run_id, lease.source_version, lease.domain_revision, task, digest,
            )
            for row in prior:
                if row["status"] in {"sent", "uncertain"} or row["response_ref"] is None:
                    raise RequestReplayBlocked(row["request_id"], row["status"])
            if prior:
                # The completed provider response can reconstruct the domain result
                # after a worker died between model audit and artifact/checkpoint.
                # No new audit row is created and the caller must not contact the provider.
                return prior[0]
            reused = await self._reusable_source_requests(
                conn, lease.run_id, lease.source_version, task, digest,
            )
            if reused:
                # Same source, same complete request: reuse the paid success. sent/uncertain
                # still fence only the current domain_revision.
                return reused[0]
            return await (await conn.execute(
                "INSERT INTO tender_model_requests(request_id, run_id, source_version, domain_revision, epoch, model, task, metadata) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s, %s) RETURNING *",
                (request_id, lease.run_id, lease.source_version, lease.domain_revision, lease.epoch, model, task, Jsonb(_safe_metadata(metadata or {}))),
            )).fetchone()

    async def _matching_model_requests(
        self, conn: AsyncConnection, run_id: str, source_version: int, domain_revision: int, task: Task, digest: str,
    ) -> list[dict[str, Any]]:
        # Caller holds the run lock, shared by begin, send, cancel and lease takeover.
        # Prepared requests have not been sent; explicit failures may be retried.
        return await (await conn.execute(
            "SELECT * FROM tender_model_requests WHERE run_id = %s AND source_version = %s AND domain_revision = %s "
            "AND task = %s AND metadata->>'request_digest' = %s AND status IN ('sent', 'uncertain', 'succeeded') "
            "ORDER BY created_at, request_id FOR UPDATE",
            (run_id, source_version, domain_revision, task, digest),
        )).fetchall()

    async def _reusable_source_requests(
        self, conn: AsyncConnection, run_id: str, source_version: int, task: Task, digest: str,
    ) -> list[dict[str, Any]]:
        return await (await conn.execute(
            "SELECT * FROM tender_model_requests WHERE run_id = %s AND source_version = %s "
            "AND task = %s AND metadata->>'request_digest' = %s AND status = 'succeeded' "
            "AND response_ref IS NOT NULL ORDER BY created_at, request_id FOR UPDATE",
            (run_id, source_version, task, digest),
        )).fetchall()

    async def finish_request(
        self, request_id: str, status: str, usage: dict[str, Any] | None = None, error: Any = None,
        *, response_ref: dict[str, Any] | None = None, activity: dict[str, Any] | None = None,
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
                if (run["source_version"] != request["source_version"] or run["domain_revision"] != request["domain_revision"]
                        or run["epoch"] != request["epoch"]
                        or not run["lease_live"]):
                    raise StaleExecution("model request belongs to a stale executor")
            row = await (await conn.execute(
                "SELECT * FROM tender_model_requests WHERE request_id = %s FOR UPDATE", (request_id,),
            )).fetchone()
            if not row:
                raise StoreError("model request not registered")
            if status == "sent":
                if row["status"] != "prepared":
                    raise RequestReplayBlocked(row["request_id"], row["status"])
                prior = await self._matching_model_requests(
                    conn, row["run_id"], row["source_version"], row["domain_revision"], row["task"],
                    row["metadata"]["request_digest"],
                )
                if prior:
                    raise RequestReplayBlocked(prior[0]["request_id"], prior[0]["status"])
            if response_ref is not None and row["response_ref"] is not None and row["response_ref"] != response_ref:
                raise Conflict("model request already identifies a different immutable response")
            if row["status"] in {"succeeded", "failed"}:
                if row["status"] != status:
                    raise Conflict("model request already finished")
                # A delayed audit can attach the first known response, but cannot
                # replace the original response or alter an existing terminal result.
                if response_ref is not None and row["response_ref"] is None:
                    await conn.execute(
                        "UPDATE tender_model_requests SET response_ref = %s WHERE request_id = %s",
                        (Jsonb(response_ref), request_id),
                    )
                return
            observed = _recorded_first_data(activity)
            await conn.execute(
                "UPDATE tender_model_requests SET status = %s, usage = COALESCE(%s, usage), error = %s, "
                "response_ref = COALESCE(%s, response_ref), "
                "metadata = CASE WHEN %s IS NULL THEN metadata ELSE metadata || %s END, "
                "sent_at = CASE WHEN %s = 'sent' THEN COALESCE(sent_at, clock_timestamp()) ELSE sent_at END, "
                "finished_at = CASE WHEN %s != 'sent' THEN clock_timestamp() ELSE finished_at END WHERE request_id = %s",
                (status, Jsonb(usage) if usage is not None else None, Jsonb(_safe_metadata(error)),
                 Jsonb(response_ref) if response_ref is not None else None,
                 Jsonb(observed) if observed is not None else None,
                 Jsonb(observed) if observed is not None else None,
                 status, status, request_id),
            )
