"""Independent authoring ownership, paid-call limits and publication use real PostgreSQL."""

import asyncio
from dataclasses import asdict
import hashlib
import os
from uuid import uuid4

import psycopg
from psycopg import sql
from psycopg.conninfo import make_conninfo
import pytest

from docreader.tests.database import initialize_schema
import pytest_asyncio

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import (
    Cancelled,
    Conflict,
    RequestReplayBlocked,
    StaleExecution,
)
from docreader.llm.request import ModelWorkUnitExhausted


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


async def test_unknown_and_failed_calls_share_four_sends_across_lease_takeover(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    for number in range(4):
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
    assert sum(request["sent_at"] is not None for request in requests) == 4


async def test_four_sends_do_not_prevent_reusing_an_existing_success_cache(
    authoring_store,
):
    store = authoring_store
    identity, lease = await submitted(store)
    for number in range(4):
        await store.begin_request(
            lease, f"call-{number}", "fixture", "outline", metadata(f"call-{number}")
        )
        await store.finish_request(f"call-{number}", "sent")
        await store.finish_request(
            f"call-{number}",
            "succeeded" if number == 3 else "failed",
            response_ref=store.test_artifacts.put_json({"response": number}),
        )
    before = (await store.get_execution(identity))["requests"]
    cached = await store.begin_request(
        lease, "cached-no-new-call", "fixture", "outline", metadata("call-3")
    )
    assert cached["request_id"] == "call-3"
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
        activity={"first_data_seconds": 1.2},
    )
    row = (await store.get_execution(identity))["requests"][0]
    assert row["status"] == "succeeded" and row["usage"] == {"total_tokens": 25}
    assert row["response_ref"] == ref and row["error"] is None
    assert row["metadata"]["first_data_seconds"] == 1.2
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
    for number in range(4):
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
