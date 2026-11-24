"""Independent authoring ownership, paid-call limits and publication use real PostgreSQL."""

import asyncio
from dataclasses import asdict
import hashlib
import json
import os
from uuid import uuid4

import psycopg
import httpx
from psycopg import sql
from psycopg.conninfo import make_conninfo
import pytest

from docreader.tests.database import initialize_schema
import pytest_asyncio

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.authoring_knowledge import KnowledgeResponseError, KnowledgeUnavailable
from docreader.integrations.web_knowledge import build_web_knowledge
from docreader.integrations.store import (
    Cancelled,
    Conflict,
    RequestReplayBlocked,
    StaleExecution,
)
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.document_execution import _FrozenKnowledge, _digest


@pytest_asyncio.fixture
async def authoring_store(tmp_path):
    dsn = os.environ.get("TENDER_TEST_DSN")
    if not dsn:
        pytest.skip("TENDER_TEST_DSN is required for real PostgreSQL tests")
    schema = "authoring_test_" + uuid4().hex
    async with await psycopg.AsyncConnection.connect(dsn, autocommit=True) as conn:
        await conn.execute(sql.SQL("CREATE SCHEMA {}").format(sql.Identifier(schema)))
    store = AuthoringStore(make_conninfo(dsn, options=f"-c search_path={schema}"))
    await initialize_schema(store.dsn, sections=("authoring",))
    await store.setup()
    store.test_artifacts = ArtifactStore(tmp_path)
    try:
        yield store
    finally:
        async with await psycopg.AsyncConnection.connect(dsn, autocommit=True) as conn:
            await conn.execute(
                sql.SQL("DROP SCHEMA {} CASCADE").format(sql.Identifier(schema))
            )


def input_value(store):
    ref = store.test_artifacts.put_json({"synthetic": True})
    return {
        "schema_version": 2,
        "run_id": "parent-analysis",
        "source_version": 1,
        "domain_revision": 2,
        "checklist_digest": "a" * 64,
        "extract_refs": [],
        "issues": [],
        **{
            key: ref
            for key in (
                "frozen_ref",
                "pack_plan_ref",
                "items_ref",
                "verification_ref",
                "confirmation_ref",
            )
        },
    }


async def submitted(store):
    identity = "authoring-" + uuid4().hex
    await store.submit(
        identity, input_value(store), {"protocol": "outline-v1", "model": "fixture"}
    )
    return identity, await store.acquire(identity)


def metadata(actual="original", logical="logical"):
    return {
        "request_digest": hashlib.sha256(actual.encode()).hexdigest(),
        "logical_request_digest": hashlib.sha256(logical.encode()).hexdigest(),
    }


async def expire(store, execution_id):
    async with store.transaction() as conn:
        await conn.execute(
            "UPDATE tender_authoring_executions SET lease_until = clock_timestamp() - interval '1 second' "
            "WHERE execution_id = %s",
            (execution_id,),
        )


async def parent(store):
    # The Store under test creates only authoring tables. This minimal real parent
    # table supplies the existing analysis columns involved in atomic publication.
    async with store.transaction() as conn:
        await conn.execute(
            "CREATE TABLE tender_analysis_runs (run_id text PRIMARY KEY, source_version integer, "
            "domain_revision integer, checklist_digest text, status text, cancel_requested boolean)"
        )
        await conn.execute(
            "INSERT INTO tender_analysis_runs VALUES (%s, 1, 2, %s, 'analysis-ready', false)",
            ("parent-analysis", "a" * 64),
        )


async def test_two_tables_submission_identity_and_configuration_are_independent(
    authoring_store,
):
    store = authoring_store
    value, spec = input_value(store), {"protocol": "outline-v1", "model": "fixture"}
    first, second = await asyncio.gather(
        store.submit("same", value, spec), store.submit("same", value, spec)
    )
    assert first["execution_id"] == second["execution_id"] == "same"
    assert first["authoring_input"] == value and first["execution_spec"] == spec
    for changed_value, changed_spec in [
        ({**value, "domain_revision": 3}, spec),
        (value, {**spec, "model": "changed"}),
    ]:
        with pytest.raises(Conflict):
            await store.submit("same", changed_value, changed_spec)
    async with store.transaction() as conn:
        tables = await (
            await conn.execute(
                "SELECT tablename FROM pg_tables WHERE schemaname = current_schema() "
                "ORDER BY tablename"
            )
        ).fetchall()
    assert [row["tablename"] for row in tables] == [
        "tender_authoring_executions",
        "tender_authoring_model_requests",
    ]
    leases = await asyncio.gather(*(store.acquire("same") for _ in range(4)))
    assert sum(lease is not None for lease in leases) == 1
    lease = next(lease for lease in leases if lease is not None)
    assert asdict(lease) == {"execution_id": "same", "epoch": 1}
    assert await store.pin_execution_spec(lease, spec) == spec
    with pytest.raises(Conflict):
        await store.pin_execution_spec(lease, {**spec, "model": "changed"})


async def test_initialized_schema_is_safe_when_independent_executors_start_concurrently():
    dsn = os.environ.get("TENDER_TEST_DSN")
    if not dsn:
        pytest.skip("TENDER_TEST_DSN is required for real PostgreSQL tests")
    schema = "authoring_setup_" + uuid4().hex
    async with await psycopg.AsyncConnection.connect(dsn, autocommit=True) as conn:
        await conn.execute(sql.SQL("CREATE SCHEMA {}").format(sql.Identifier(schema)))
    isolated = make_conninfo(dsn, options=f"-c search_path={schema}")
    try:
        await initialize_schema(isolated, sections=("authoring",))
        results = await asyncio.gather(
            *(AuthoringStore(isolated).setup() for _ in range(4)),
            return_exceptions=True,
        )
        assert all(result is None for result in results), [
            type(result).__name__ for result in results
        ]
    finally:
        async with await psycopg.AsyncConnection.connect(dsn, autocommit=True) as conn:
            await conn.execute(
                sql.SQL("DROP SCHEMA {} CASCADE").format(sql.Identifier(schema))
            )


async def test_setup_does_not_recreate_missing_business_tables(authoring_store):
    async with authoring_store.transaction() as conn:
        await conn.execute("DROP TABLE tender_authoring_model_requests")
    with pytest.raises(psycopg.errors.UndefinedTable):
        await authoring_store.setup()


async def test_recovery_preserves_immutable_cas_receipts_and_fences_old_owner(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    ref = store.test_artifacts.put_json({"rejected": True})
    receipt = await store.commit_artifact(
        lease, "model-rejection", ref, scope="one", input_digest="input"
    )
    assert (
        await store.receipt(lease, "model-rejection", "one", input_digest="input")
        == receipt
    )
    with pytest.raises(Conflict):
        await store.receipt(lease, "model-rejection", "one", input_digest="changed")
    await expire(store, identity)
    current = await store.acquire(identity)
    assert current.epoch == lease.epoch + 1
    for action in (
        store.heartbeat(lease),
        store.commit_artifact(lease, "other", ref),
        store.release(lease, "failed"),
    ):
        with pytest.raises(StaleExecution):
            await action
    assert (
        await store.commit_artifact(
            current, "model-rejection", ref, scope="one", input_digest="input"
        )
        == receipt
    )
    with pytest.raises(Conflict):
        await store.commit_artifact(
            current,
            "model-rejection",
            store.test_artifacts.put_json({"different": True}),
            scope="one",
            input_digest="input",
        )
    assert (await store.get_execution(identity))["receipts"] == [receipt]


async def test_success_cache_does_not_create_new_request_or_consume_paid_attempt(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    await store.begin_request(lease, "paid", "fixture", "outline", metadata())
    await store.finish_request("paid", "sent")
    ref = store.test_artifacts.put_json({"complete": True})
    await store.finish_request(
        "paid", "succeeded", usage={"total_tokens": 12}, response_ref=ref
    )
    cached = await store.begin_request(
        lease, "must-not-exist", "fixture", "outline", metadata()
    )
    assert cached["request_id"] == "paid" and cached["response_ref"] == ref
    assert len((await store.get_execution(identity))["requests"]) == 1


async def test_prepared_duplicate_cannot_send_after_another_owner_received_success(
    authoring_store,
):
    store = authoring_store
    _, lease = await submitted(store)
    await store.begin_request(lease, "original", "fixture", "outline", metadata())
    await store.begin_request(lease, "duplicate", "fixture", "outline", metadata())
    await store.finish_request("original", "sent")
    await store.finish_request(
        "original",
        "succeeded",
        response_ref=store.test_artifacts.put_json({"success": True}),
    )
    with pytest.raises(RequestReplayBlocked):
        await store.finish_request("duplicate", "sent")


async def test_sent_fence_covers_feedback_digests_and_concurrent_preparations(
    authoring_store,
):
    store = authoring_store
    _, lease = await submitted(store)
    await store.begin_request(lease, "one", "fixture", "outline", metadata("one"))
    await store.begin_request(
        lease, "two", "fixture", "outline", metadata("feedback-two")
    )
    results = await asyncio.gather(
        store.finish_request("one", "sent"),
        store.finish_request("two", "sent"),
        return_exceptions=True,
    )
    assert sum(result is None for result in results) == 1
    assert sum(isinstance(result, RequestReplayBlocked) for result in results) == 1
    with pytest.raises(RequestReplayBlocked):
        await store.begin_request(
            lease, "three", "fixture", "outline", metadata("feedback-three")
        )


async def test_independent_logical_tasks_can_send_concurrently_with_separate_retry_budgets(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    for task, logical in [
        ("outline", "outline"),
        ("authoring_basis", "basis"),
        ("authoring_write", "chapter-one"),
        ("authoring_write", "chapter-two"),
        ("authoring_review", "review"),
        ("authoring_structure", "structure"),
    ]:
        await store.begin_request(
            lease, logical, "fixture", task, metadata(logical, logical)
        )
        await store.finish_request(logical, "sent")
    assert len((await store.get_execution(identity))["requests"]) == 6
    with pytest.raises(RequestReplayBlocked):
        await store.begin_request(
            lease,
            "same-chapter",
            "fixture",
            "authoring_write",
            metadata("feedback", "chapter-one"),
        )
    with pytest.raises(ValueError):
        await store.begin_request(lease, "wrong-task", "fixture", "extract", metadata())


async def test_unknown_and_failed_corrections_stay_bounded_across_lease_takeover(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    for number in range(3):
        request_id = f"attempt-{number}"
        await store.begin_request(
            lease, request_id, "fixture", "outline", metadata(f"feedback-{number}")
        )
        await store.finish_request(request_id, "sent")
        await store.finish_request(
            request_id, "uncertain" if number % 2 == 0 else "failed"
        )
        await expire(store, identity)
        lease = await store.acquire(identity)
    before = (await store.get_execution(identity))["requests"]
    with pytest.raises(ModelWorkUnitExhausted):
        await store.begin_request(
            lease, "fifth", "fixture", "outline", metadata("feedback-five")
        )
        await store.finish_request("fifth", "sent")
    requests = (await store.get_execution(identity))["requests"]
    assert requests == before
    assert sum(request["sent_at"] is not None for request in requests) == 3


async def test_exhausted_sends_do_not_prevent_reusing_an_existing_success_cache(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    for number in range(3):
        await store.begin_request(
            lease, f"call-{number}", "fixture", "outline", metadata(f"call-{number}")
        )
        await store.finish_request(f"call-{number}", "sent")
        await store.finish_request(
            f"call-{number}",
            "succeeded" if number == 2 else "failed",
            response_ref=store.test_artifacts.put_json({"response": number}),
        )
    before = (await store.get_execution(identity))["requests"]
    cached = await store.begin_request(
        lease, "cached-no-new-call", "fixture", "outline", metadata("call-2")
    )
    assert cached["request_id"] == "call-2"
    assert (await store.get_execution(identity))["requests"] == before


async def test_late_usage_audit_retains_terminal_outcome_and_immutable_response(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    await store.begin_request(lease, "audit", "fixture", "outline", metadata())
    await store.finish_request("audit", "sent")
    ref = store.test_artifacts.put_json({"complete": True})
    await store.finish_request("audit", "succeeded", response_ref=ref)
    await store.finish_request(
        "audit",
        "failed",
        usage={"total_tokens": 25},
        error={"type": "late-disconnect"},
        activity={"first_data_seconds": 1.2, "first_raw_seconds": 0.8,
                  "first_content_seconds": 2.4, "max_raw_idle_seconds": 3,
                  "semaphore_wait_seconds": 0.5, "raw_body": "private"},
    )
    row = (await store.get_execution(identity))["requests"][0]
    assert row["status"] == "succeeded" and row["usage"] == {"total_tokens": 25}
    assert row["response_ref"] == ref and row["error"] is None
    assert row["metadata"]["first_data_seconds"] == 1.2
    assert row["metadata"]["first_raw_seconds"] == 0.8
    assert row["metadata"]["first_content_seconds"] == 2.4
    assert row["metadata"]["max_raw_idle_seconds"] == 3
    assert row["metadata"]["semaphore_wait_seconds"] == 0.5
    assert "raw_body" not in row["metadata"]
    with pytest.raises(Conflict):
        await store.finish_request(
            "audit",
            "succeeded",
            response_ref=store.test_artifacts.put_json({"other": True}),
        )


async def test_takeover_records_unknown_sends_and_does_not_count_prepared_calls(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    for number in range(5):
        await store.begin_request(
            lease,
            f"prepared-{number}",
            "fixture",
            "outline",
            metadata(f"prepared-{number}"),
        )
    await store.finish_request("prepared-0", "sent")
    await expire(store, identity)
    lease = await store.acquire(identity)
    rows = (await store.get_execution(identity))["requests"]
    assert rows[0]["status"] == "uncertain" and all(
        row["status"] == "failed" for row in rows[1:]
    )
    assert sum(row["sent_at"] is not None for row in rows) == 1
    await store.begin_request(
        lease, "new-owner", "fixture", "outline", metadata("new-owner")
    )
    await store.finish_request("new-owner", "sent")


async def test_cancellation_stops_sending_and_publication_but_allows_late_audit(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    await store.begin_request(lease, "sent", "fixture", "outline", metadata())
    await store.finish_request("sent", "sent")
    await store.cancel(identity)
    ref = store.test_artifacts.put_json({"late_response": True})
    await store.finish_request(
        "sent", "succeeded", usage={"total_tokens": 19}, response_ref=ref
    )
    row = await store.get_execution(identity)
    assert row["status"] == "cancelled" and row["result_ref"] is None
    assert row["requests"][0]["response_ref"] == ref and row["requests"][0][
        "usage"
    ] == {"total_tokens": 19}
    assert await store.acquire(identity) is None
    for action in (
        store.heartbeat(lease),
        store.release(lease, "succeeded", result_ref=ref),
        store.begin_request(
            lease, "cancelled-send", "fixture", "outline", metadata("cancelled")
        ),
    ):
        with pytest.raises(Cancelled):
            await action


async def test_cancelled_prepared_request_cannot_be_sent(authoring_store):
    store = authoring_store
    identity, lease = await submitted(store)
    await store.begin_request(lease, "prepared", "fixture", "outline", metadata())
    await store.cancel(identity)
    with pytest.raises(Cancelled):
        await store.finish_request("prepared", "sent")
    assert (await store.get_execution(identity))["requests"][0]["sent_at"] is None


@pytest.mark.parametrize("field", ["request_digest", "logical_request_digest"])
@pytest.mark.parametrize("value", [None, "invalid", "A" * 64])
async def test_audit_requires_both_lowercase_sha256_digests(
    authoring_store, field, value
):
    _, lease = await submitted(authoring_store)
    with pytest.raises(ValueError):
        await authoring_store.begin_request(
            lease, "invalid", "fixture", "outline", {**metadata(), field: value}
        )


async def test_successful_publication_is_terminal_and_bound_to_current_parent(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    await parent(store)
    ref = store.test_artifacts.put_json({"chapters": []})
    assert (await store.release(lease, "succeeded", result_ref=ref))[
        "status"
    ] == "succeeded"
    assert await store.acquire(identity) is None
    assert (await store.get_execution(identity))["result_ref"] == ref
    async with store.transaction() as conn:
        unchanged = await (
            await conn.execute("SELECT * FROM tender_analysis_runs")
        ).fetchone()
    assert unchanged["status"] == "analysis-ready" and unchanged["domain_revision"] == 2


@pytest.mark.parametrize("mutation", ["revision", "digest", "status", "cancelled"])
async def test_snapshot_publication_survives_analysis_changes_but_respects_cancellation(
    authoring_store, mutation
):
    store = authoring_store
    identity, lease = await submitted(store)
    await parent(store)
    statements = {
        "revision": "domain_revision = 3",
        "digest": "checklist_digest = 'changed'",
        "status": "status = 'waiting-confirmation'",
        "cancelled": "cancel_requested = true",
    }
    async with store.transaction() as conn:
        await conn.execute("UPDATE tender_analysis_runs SET " + statements[mutation])
    reference = store.test_artifacts.put_json({"old": True})
    if mutation == "cancelled":
        with pytest.raises(Cancelled):
            await store.release(lease, "succeeded", result_ref=reference)
        assert (await store.get_execution(identity))["status"] == "running"
    else:
        row = await store.release(lease, "succeeded", result_ref=reference)
        assert (
            row["status"] == "succeeded"
            and row["authoring_input"]["domain_revision"] == 2
        )


async def test_exhausted_chapter_does_not_spend_another_chapters_retry_budget(
    authoring_store,
):
    store = authoring_store
    _, lease = await submitted(store)
    for number in range(3):
        await store.begin_request(
            lease,
            f"failed-{number}",
            "fixture",
            "authoring_write",
            metadata(f"feedback-{number}", "first-chapter"),
        )
        await store.finish_request(f"failed-{number}", "sent")
        await store.finish_request(f"failed-{number}", "failed")
    with pytest.raises(ModelWorkUnitExhausted):
        await store.begin_request(
            lease,
            "fifth",
            "fixture",
            "authoring_write",
            metadata("fifth", "first-chapter"),
        )
    await store.begin_request(
        lease,
        "second",
        "fixture",
        "authoring_write",
        metadata("second", "second-chapter"),
    )
    await store.finish_request("second", "sent")


async def test_same_execution_survives_kill_and_rejects_the_expired_worker(authoring_store, monkeypatch):
    """Lease takeover stays on one execution. It is not access to another user's project."""
    async def no_wait(_seconds):
        return None

    monkeypatch.setattr("docreader.tender.document_execution.asyncio.sleep", no_wait)
    store, artifacts = authoring_store, authoring_store.test_artifacts
    identity, lease = await submitted(store)
    spec = {"provider": "fixture", "scope": "collection"}

    class Provider:
        def __init__(self):
            self.calls = 0
            self.crash = True

        async def search(self, queries, _artifacts, *, run_operation):
            async def send_once():
                self.calls += 1
                if self.crash and self.calls == 2:
                    self.crash = False
                    raise RuntimeError("killed after the second send")
                raise KnowledgeResponseError("synthetic failed response")
            key = _digest({"query": queries[0]["query"], "provider": "fixture"})
            try:
                return await run_operation("search", key, send_once)
            except KnowledgeUnavailable:
                return {"materials": [], "issues": [{
                    "code": "knowledge_unavailable", "detail": "失败", "target_id": "query"}]}

    provider = Provider()
    first = _FrozenKnowledge(provider, spec, store, artifacts, lease)
    with pytest.raises(RuntimeError, match="killed"):
        await first.search([{"query": "权限", "target_id": "target"}], artifacts)
    resumed = _FrozenKnowledge(provider, spec, store, artifacts, lease)
    failed = await resumed.search([{"query": "权限", "target_id": "target"}], artifacts)
    assert provider.calls == 4
    assert failed["issues"][0]["code"] == "knowledge_unavailable"
    await resumed.search([{"query": "权限", "target_id": "other"}], artifacts)
    assert provider.calls == 4

    unit_ref = artifacts.put_json({"task_id": "w1", "text": "已保存章节"})
    await store.commit_artifact(
        lease, "document-unit", unit_ref, scope="w1", input_digest=_digest("w1"))
    await expire(store, identity)
    replacement = await store.acquire(identity)
    assert replacement is not None and replacement.execution_id == identity
    assert replacement.epoch == lease.epoch + 1
    with pytest.raises(StaleExecution):
        await _FrozenKnowledge(provider, spec, store, artifacts, lease).search(
            [{"query": "权限", "target_id": "late"}], artifacts)
    with pytest.raises(StaleExecution):
        await store.commit_artifact(
            lease, "document-unit", artifacts.put_json({"task_id": "w2"}),
            scope="w2", input_digest=_digest("w2"))
    assert provider.calls == 4
    receipt = await store.receipt(
        replacement, "document-unit", "w1", input_digest=_digest("w1"))
    assert artifacts.read_json(receipt["artifact_ref"])["text"] == "已保存章节"
    quiet = _FrozenKnowledge(provider, spec, store, artifacts, replacement)
    replayed = await quiet.search([{"query": "权限", "target_id": "replacement"}], artifacts)
    assert provider.calls == 4
    assert replayed["issues"][0]["code"] == "knowledge_unavailable"


def web_knowledge(store, artifacts, lease, handler):
    provider = build_web_knowledge(execution_id=lease.execution_id,
        selections=[{"product_id": "4c78bf65-9ee2-4788-bc0c-f14769bb153f",
                     "version_id": "37f9257d-f030-46cb-ae5b-e34dfebcb14b"}],
        base_url="https://knowledge.invalid", service_secret="synthetic-test-secret",
        transport=httpx.MockTransport(handler))
    return _FrozenKnowledge(provider, provider.spec(), store, artifacts, lease)


async def test_real_web_operation_counts_are_persisted_without_nested_retries(authoring_store, monkeypatch):
    async def no_wait(_seconds):
        return None
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", no_wait)
    store, artifacts = authoring_store, authoring_store.test_artifacts
    identity, lease = await submitted(store)
    sent = []
    async def handler(request):
        sent.append(json.loads(request.content))
        return httpx.Response(503)
    reader = web_knowledge(store, artifacts, lease, handler)
    result = await reader.search([{"query": "完整要求", "target_id": "target"}], artifacts)
    assert len(sent) == 4
    assert result["issues"][0]["code"] == "knowledge_unavailable"
    stored = await store.get_execution(identity)
    receipts = [receipt for receipt in stored["receipts"] if receipt["stage"] == "knowledge-operation"]
    assert sum(receipt["scope"].endswith(":start") for receipt in receipts) == 4
    assert sum(receipt["scope"].endswith(":outcome") for receipt in receipts) == 4


@pytest.mark.parametrize("empty", [False, True])
async def test_real_pg_operation_result_resumes_before_snapshot_commit(authoring_store, monkeypatch, empty):
    store, artifacts = authoring_store, authoring_store.test_artifacts
    _, lease = await submitted(store)
    sent = []
    async def handler(request):
        command = json.loads(request.content)
        sent.append(command["operation"])
        if command["operation"] == "document":
            return httpx.Response(200, json={"id": command["document_id"], "product_version_id": command["version_id"],
                                           "file_name": "资料.pdf", "object_ref": "objects/" + "0" * 64})
        return httpx.Response(200, json={"hits": [] if empty else [{
            "id": "chunk", "content": "测试资料", "document_id": "b78d5270-5938-48c2-acb7-e9538910fea7",
            "product_id": command["product_id"], "version_id": command["version_id"]}]})
    commit = store.commit_artifact
    async def fail_snapshot(current, stage, reference, scope="", *, input_digest="", metadata=None):
        if stage == "knowledge-snapshot":
            raise OSError("injected snapshot failure")
        return await commit(current, stage, reference, scope, input_digest=input_digest, metadata=metadata)
    with monkeypatch.context() as patch:
        patch.setattr(store, "commit_artifact", fail_snapshot)
        with pytest.raises(OSError, match="snapshot failure"):
            await web_knowledge(store, artifacts, lease, handler).search(
                [{"query": "完整要求", "target_id": "before"}], artifacts)
    result = await web_knowledge(store, artifacts, lease, handler).search(
        [{"query": "完整要求", "target_id": "after"}], artifacts)
    assert sent == (["search"] if empty else ["search", "document"])
    assert bool(result["materials"]) is not empty
    if not empty:
        assert result["materials"][0]["locator"]["target_ids"] == ["after"]


async def test_real_web_unknown_attempts_keep_budget_across_lease_takeover(authoring_store, monkeypatch):
    async def no_wait(_seconds):
        return None
    monkeypatch.setattr("docreader.integrations.authoring_knowledge.asyncio.sleep", no_wait)
    store, artifacts = authoring_store, authoring_store.test_artifacts
    identity, lease = await submitted(store)
    sent = []
    async def handler(request):
        sent.append(request)
        if len(sent) == 2:
            raise RuntimeError("interrupted after second HTTP send")
        return httpx.Response(503)
    with pytest.raises(RuntimeError, match="second HTTP"):
        await web_knowledge(store, artifacts, lease, handler).search(
            [{"query": "完整要求", "target_id": "before"}], artifacts)
    await expire(store, identity)
    replacement = await store.acquire(identity)
    assert replacement.epoch == lease.epoch + 1
    result = await web_knowledge(store, artifacts, replacement, handler).search(
        [{"query": "完整要求", "target_id": "after"}], artifacts)
    assert len(sent) == 4
    assert result["issues"][0]["code"] == "knowledge_unavailable"
    with pytest.raises(StaleExecution):
        await web_knowledge(store, artifacts, lease, handler).search(
            [{"query": "完整要求", "target_id": "old-owner"}], artifacts)
    assert len(sent) == 4


async def limited(store):
    from docreader.llm.request import RUN_LIMIT_MULTIPLIER, RUN_LIMIT_POLICY

    identity = "authoring-" + uuid4().hex
    await store.submit(identity, input_value(store), {
        "protocol": "outline-v1", "model": "fixture",
        "run_limit_policy": RUN_LIMIT_POLICY, "run_limit_multiplier": RUN_LIMIT_MULTIPLIER})
    lease = await store.acquire(identity)
    return identity, lease


def sized(actual, logical, input_tokens=100, output_reserve=100):
    return {**metadata(actual, logical), "input_tokens": input_tokens, "output_reserve": output_reserve}


async def test_run_limit_caps_new_logical_requests_at_twice_the_task_plan(authoring_store):
    from docreader.llm.request import ExecutionBudgetExhausted

    store = authoring_store
    identity, lease = await limited(store)
    with pytest.raises(ExecutionBudgetExhausted):
        await store.begin_request(lease, "unplanned", "fixture", "authoring_write", sized("w", "w"))
    plan = store.test_artifacts.put_json({"planned_requests": 1})
    await store.register_plan(lease, "outline", "batches", 1, plan)
    await store.register_plan(lease, "outline", "batches", 1, plan)  # replay is idempotent
    for logical in ("first", "second"):
        await store.begin_request(lease, logical, "fixture", "outline", sized(logical, logical))
        await store.finish_request(logical, "sent")
        await store.finish_request(logical, "succeeded", usage={"total_tokens": 150},
                                   response_ref=store.test_artifacts.put_json({"ok": logical}))
    with pytest.raises(ExecutionBudgetExhausted):
        await store.begin_request(lease, "third", "fixture", "outline", sized("third", "third"))
    cached = await store.begin_request(lease, "cache", "fixture", "outline", sized("first", "first"))
    assert cached["request_id"] == "first"
    assert len((await store.get_execution(identity))["requests"]) == 2


async def test_run_limit_stops_retries_once_tokens_reach_twice_the_plan(authoring_store):
    from docreader.llm.request import ExecutionBudgetExhausted

    store = authoring_store
    _, lease = await limited(store)
    await store.register_plan(lease, "outline", "batches", 1, store.test_artifacts.put_json({"planned_requests": 1}))
    # Plan: 100 input + 100 output reserve = 200; the limit is 400 tokens.
    await store.begin_request(lease, "first", "fixture", "outline", sized("first", "unit"))
    await store.finish_request("first", "sent")
    await store.finish_request("first", "failed", usage={"total_tokens": 150}, error={"type": "http_500"})
    await store.begin_request(lease, "resend", "fixture", "outline", sized("first", "unit"))
    await store.finish_request("resend", "sent")
    await store.finish_request("resend", "failed", error={"type": "http_500"})
    # Used 150 + 200 (unknown usage counts as its plan); another 200 would exceed 400.
    with pytest.raises(ExecutionBudgetExhausted):
        await store.begin_request(lease, "third", "fixture", "outline", sized("first", "unit"))


async def test_executions_without_run_limit_policy_are_not_capped(authoring_store):
    store = authoring_store
    _, lease = await submitted(store)
    await store.begin_request(lease, "unplanned", "fixture", "authoring_write", sized("w", "w"))
