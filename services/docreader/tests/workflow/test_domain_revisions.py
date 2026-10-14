"""Revision, immutable input, and checkpoint fences against real PostgreSQL."""

import asyncio
import copy
import json
from argparse import Namespace

import psycopg
import pytest
from langgraph.checkpoint.base import empty_checkpoint
from psycopg.rows import dict_row

from docreader.integrations.store import Conflict, InvalidConfirmation, StaleExecution
from docreader.workflow.persistence import FencedPostgresSaver, initialize_checkpoints
from test_store import expire, store as store, submitted

pytestmark = pytest.mark.asyncio


async def extracted(store):
    run, command = await submitted(store)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    frozen = store.test_artifacts.put_json({"documents": ["original"], "packs": []})
    requirements = store.test_artifacts.put_json({"packs": [], "counts": {"packs": 0}})
    await store.commit_artifact(lease, "freeze", frozen, input_digest="original-source")
    await store.commit_artifact(lease, "extract", requirements, input_digest="original-requirements")
    corrected = store.test_artifacts.put_json({"packs": [], "correction": "missing condition"})
    return run, command, lease, frozen, requirements, corrected


async def test_domain_correction_preserves_sources_and_invalidates_worker_and_pending_command(store):
    run, command, old, frozen, requirements, corrected = await extracted(store)
    await store.commit_artifact(old, "verify", store.test_artifacts.put_json({"blocking": False}))
    await store.begin_request(old, "old-model", "model-a", "verify")
    await store.finish_request("old-model", "sent")
    revision = await store.correct(run["run_id"], 1, 1, requirements["sha256"], corrected, "correction", "restore condition")
    assert revision["source_version"] == 1
    assert revision["domain_revision"] == 2
    assert revision["checklist_digest"] is None
    assert revision["epoch"] > old.epoch
    assert await store.acquire(run["run_id"], command["command_id"], 1) is None
    with pytest.raises(StaleExecution):
        await store.commit_artifact(old, "plan", corrected)
    current = await store.get_run(run["run_id"])
    assert current["manifest_ref"] == run["manifest_ref"]
    assert [(item["stage"], item["domain_revision"]) for item in current["artifacts"]] == [("freeze", 0)]
    assert {item["stage"] for item in current["history"]["artifacts"]} == {"extract", "verify"}
    assert current["history"]["usage"][0]["status"] == "uncertain"
    assert current["history"]["commands"][0]["status"] == "cancelled"
    assert current["history"]["commands"][0]["receipt"] == {"status": "cancelled", "reason": "domain_corrected"}
    assert current["history"]["commands"][0]["completed_at"] is not None
    new_command = current["commands"][0]
    new = await store.acquire(run["run_id"], new_command["command_id"], 1)
    assert new.domain_revision == 2
    assert (await store.receipt(new, "freeze", input_digest="original-source"))["artifact_ref"] == frozen
    assert await store.receipt(new, "extract") is None
    revision_row = await store.get_revision(new)
    assert revision_row["corrected_requirements_ref"] == corrected
    assert revision_row["upstream_digest"] == requirements["sha256"]
    assert revision_row["execution_spec"] is None
    await store.begin_request(new, "new-model", "model-b", "verify")
    assert (await store.get_run(run["run_id"]))["usage"][0]["domain_revision"] == 2


async def test_correction_is_idempotent_but_rejects_different_or_concurrent_old_baselines(store):
    run, _, _, _, requirements, corrected = await extracted(store)
    arguments = (run["run_id"], 1, 1, requirements["sha256"], corrected, "same-correction", "add omitted requirement")
    one, two = await asyncio.gather(store.correct(*arguments), store.correct(*arguments))
    assert one["submitted_domain_revision"] == two["submitted_domain_revision"] == 2
    assert len((await store.get_run(run["run_id"]))["commands"]) == 1
    with pytest.raises(Conflict):
        await store.correct(*arguments[:-1], "different reason")
    with pytest.raises(Conflict):
        await store.correct(run["run_id"], 1, 1, requirements["sha256"], corrected, "another-key", "stale baseline")


async def test_two_independent_corrections_cannot_both_advance_one_baseline(store):
    run, _, _, _, requirements, corrected = await extracted(store)
    results = await asyncio.gather(*(
        store.correct(run["run_id"], 1, 1, requirements["sha256"], corrected, key, "add missing requirement")
        for key in ("correction-a", "correction-b")
    ), return_exceptions=True)
    assert sum(isinstance(result, Conflict) for result in results) == 1
    assert (await store.get_run(run["run_id"]))["domain_revision"] == 2


async def test_correction_requires_frozen_extracted_current_digest_and_respects_cancel(store):
    run, command = await submitted(store)
    corrected = store.test_artifacts.put_json({"packs": []})
    args = (run["run_id"], 1, 1, corrected["sha256"], corrected, "correct", "add requirement")
    with pytest.raises(Conflict, match="frozen"):
        await store.correct(*args)
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    await store.commit_artifact(lease, "freeze", corrected)
    await store.commit_artifact(lease, "extract", corrected)
    with pytest.raises(Conflict, match="digest"):
        await store.correct(run["run_id"], 1, 1, "wrong-digest", corrected, "wrong", "fix")
    await store.cancel(run["run_id"])
    with pytest.raises(Conflict, match="cancelled"):
        await store.correct(*args)


async def planned_sources(store):
    run, command, lease, frozen, requirements, corrected = await extracted(store)
    plan = store.test_artifacts.put_json({"plan_digest": "current-plan", "packs": []})
    await store.commit_artifact(lease, "pack-plan", plan, metadata={"plan_digest": "current-plan"})
    decisions = store.test_artifacts.put_json({"schema_version": 1, "scope_decisions": [{
        "range_id": "condition-one", "decision": "local_only", "target_range_ids": [],
        "evidence": [{"range_id": "condition-one", "quote": "Synthetic local condition"}],
        "rationale": "The validated clause applies to its own obligation only.",
    }]})
    return run, command, lease, frozen, requirements, corrected, plan, decisions


async def test_scope_review_preserves_sources_invalidates_confirmation_and_fences_old_worker(store):
    run, _, old, frozen, _, _, _, decisions = await planned_sources(store)
    await store.release(old, "waiting-confirmation", checklist_digest="approved-before-review")
    approved = await store.confirm(run["run_id"], 1, "approved-before-review", "approve", domain_revision=1)
    active = await store.acquire(run["run_id"], approved["command_id"], 1)
    await store.begin_request(active, "scope-old-request", "fixture", "plan")
    await store.finish_request("scope-old-request", "sent")
    reviewed = await store.review_scope(run["run_id"], 1, 1, "current-plan", decisions,
                                        "scope-review", "Resolve the validated local-only condition")
    assert reviewed["source_version"] == 1 and reviewed["domain_revision"] == 2
    assert reviewed["submitted_source_version"] == 1 and reviewed["submitted_domain_revision"] == 2
    assert reviewed["checklist_digest"] is None
    with pytest.raises(StaleExecution):
        await store.commit_artifact(active, "plan", frozen)
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, "approved-before-review", "approve", domain_revision=1)
    row = await store.get_run(run["run_id"])
    assert row["manifest_ref"] == run["manifest_ref"]
    assert not row["confirmations"]
    assert row["history"]["confirmations"][0]["domain_revision"] == 1
    assert row["history"]["usage"][0]["status"] == "uncertain"
    old_commands = {item["command_id"]: item for item in row["history"]["commands"]}
    assert old_commands[approved["command_id"]]["receipt"] == {"status": "cancelled", "reason": "scope_reviewed"}
    assert old_commands[approved["command_id"]]["completed_at"]
    fresh = await store.acquire(run["run_id"], row["commands"][0]["command_id"], 1)
    revision = await store.get_revision(fresh)
    assert revision["scope_decisions_ref"] == decisions
    assert revision["upstream_digest"] == "current-plan"
    assert revision["corrected_requirements_ref"] is None
    assert revision["execution_spec"] is None
    assert (await store.receipt(fresh, "freeze", input_digest="original-source"))["artifact_ref"] == frozen
    assert await store.receipt(fresh, "pack-plan") is None
    assert await store.receipt(fresh, "extract") is None
    assert await store.acquire(run["run_id"], approved["command_id"], 1) is None


async def test_scope_review_is_idempotent_and_rejects_competing_or_changed_baselines(store):
    run, _, _, _, _, _, plan, decisions = await planned_sources(store)
    arguments = (run["run_id"], 1, 1, "current-plan", decisions, "same-review", "Clarify validated scope")
    # The semantic plan identity, not the outer file hash, is the baseline.
    with pytest.raises(Conflict, match="digest"):
        await store.review_scope(run["run_id"], 1, 1, plan["sha256"], decisions, "wrong-hash", "Clarify")
    one, two = await asyncio.gather(store.review_scope(*arguments), store.review_scope(*arguments))
    assert one["submitted_domain_revision"] == two["submitted_domain_revision"] == 2
    with pytest.raises(Conflict, match="different"):
        await store.review_scope(*arguments[:-1], "A different review reason")
    with pytest.raises(Conflict, match="different"):
        await store.review_scope(*arguments[:4], plan, *arguments[5:])
    with pytest.raises(Conflict, match="stale"):
        await store.review_scope(*arguments[:5], "new-stale-key", arguments[-1])
    row = await store.get_run(run["run_id"])
    assert row["domain_revision"] == 2 and len(row["commands"]) == 1


async def test_scope_review_requires_current_frozen_plan_and_respects_cancel(store):
    run, command = await submitted(store)
    decisions = store.test_artifacts.put_json({"schema_version": 1, "scope_decisions": []})
    arguments = (run["run_id"], 1, 1, "current-plan", decisions, "scope-review", "Validated decision")
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    with pytest.raises(Conflict, match="frozen"):
        await store.review_scope(*arguments)
    await store.commit_artifact(lease, "freeze", decisions)
    with pytest.raises(Conflict, match="pack plan"):
        await store.review_scope(*arguments)
    await store.commit_artifact(lease, "pack-plan", decisions, metadata={"plan_digest": "current-plan"})
    with pytest.raises(Conflict, match="stale"):
        await store.review_scope(run["run_id"], 2, 1, "current-plan", decisions, "wrong-source", "Validated")
    await store.cancel(run["run_id"])
    with pytest.raises(Conflict, match="cancelled"):
        await store.review_scope(*arguments)


async def test_requirement_correction_inherits_scope_and_real_source_revision_resets_it(store):
    run, _, _, _, _, corrected, _, decisions = await planned_sources(store)
    await store.review_scope(run["run_id"], 1, 1, "current-plan", decisions, "scope-review", "Validated scope")
    row = await store.get_run(run["run_id"])
    lease = await store.acquire(run["run_id"], row["commands"][0]["command_id"], 1)
    await store.commit_artifact(lease, "extract", corrected)
    latest_requirements = store.test_artifacts.put_json({"packs": [], "correction": "updated obligation"})
    await store.correct(run["run_id"], 1, 2, corrected["sha256"], latest_requirements, "correct-after-scope", "Fix obligation")
    row = await store.get_run(run["run_id"])
    assert row["source_version"] == 1 and row["domain_revision"] == 3
    assert row["revision"]["scope_decisions_ref"] == decisions
    assert row["revision"]["corrected_requirements_ref"] == latest_requirements
    await store.revise(run["run_id"], latest_requirements, "new-source")
    row = await store.get_run(run["run_id"])
    assert row["source_version"] == 2 and row["domain_revision"] == 1
    assert row["revision"]["scope_decisions_ref"] is None
    assert row["revision"]["corrected_requirements_ref"] is None


async def test_confirmation_is_bound_to_domain_revision_and_new_source_resets_revision(store):
    run, _, old, _, requirements, corrected = await extracted(store)
    await store.release(old, "waiting-confirmation", checklist_digest="revision-one")
    previous = await store.confirm(run["run_id"], 1, "revision-one", "approve-old", domain_revision=1)
    await store.correct(run["run_id"], 1, 1, requirements["sha256"], corrected, "correct", "fix applicability")
    current = await store.get_run(run["run_id"])
    assert not current["confirmations"]
    assert current["history"]["confirmations"][0]["domain_revision"] == 1
    assert await store.acquire(run["run_id"], previous["command_id"], 1) is None
    new = await store.acquire(run["run_id"], current["commands"][0]["command_id"], 1)
    await store.release(new, "waiting-confirmation", checklist_digest="revision-two")
    for kwargs in ({"domain_revision": 1}, {}):
        with pytest.raises(InvalidConfirmation):
            await store.confirm(run["run_id"], 1, "revision-one", "approve-old", **kwargs)
    approved = await store.confirm(run["run_id"], 1, "revision-two", "approve-new", domain_revision=2)
    assert approved["domain_revision"] == approved["payload"]["domain_revision"] == 2
    with pytest.raises(Conflict, match="changed manifest"):
        await store.revise(run["run_id"], run["manifest_ref"], "pretend-source-change")
    source = store.test_artifacts.put_json({"documents": ["new attachment"]})
    revised = await store.revise(run["run_id"], source, "source-change")
    assert revised["source_version"] == 2
    assert revised["domain_revision"] == 1
    row = await store.get_run(run["run_id"])
    assert row["artifacts"] == []
    assert row["revision"]["corrected_requirements_ref"] is None
    assert row["revision"]["execution_spec"] is None
    assert len(row["history"]["confirmations"]) == 2
    old_commands = {item["command_id"]: item for item in row["history"]["commands"]}
    assert old_commands[approved["command_id"]]["receipt"] == {"status": "cancelled", "reason": "source_revised"}
    assert old_commands[approved["command_id"]]["completed_at"] is not None
    # The completed start disposition survives both kinds of revision.
    assert old_commands[old.command_id]["receipt"]["status"] == "waiting-confirmation"


async def test_artifact_input_identity_and_execution_configuration_are_immutable(store):
    run, command, lease, _, requirements, corrected = await extracted(store)
    with pytest.raises(Conflict, match="inputs"):
        await store.receipt(lease, "extract", input_digest="different-input")
    with pytest.raises(Conflict, match="inputs"):
        await store.commit_artifact(lease, "extract", requirements, input_digest="different-input")
    spec = {"chat": {"endpoint": "https://model.invalid/v1/chat/completions", "model": "chat-a", "context": 131072},
            "prompts": {"extract": "sha-of-prompt"}}
    assert await store.pin_execution_spec(lease, spec) == spec
    await store.release(lease, "retry")
    restored = await store.acquire(run["run_id"], command["command_id"], 1)
    assert await store.pin_execution_spec(restored, spec) == spec
    with pytest.raises(Conflict, match="configuration"):
        await store.pin_execution_spec(restored, {**spec, "prompts": {"extract": "changed-prompt"}})
    with pytest.raises(ValueError, match="credentials"):
        await store.pin_execution_spec(restored, {"api_key": "must-never-persist"})
    await store.correct(run["run_id"], 1, 1, requirements["sha256"], corrected, "correct", "new verified configuration")
    row = await store.get_run(run["run_id"])
    newest = await store.acquire(run["run_id"], row["commands"][0]["command_id"], 1)
    assert (await store.get_revision(newest))["execution_spec"] is None
    changed = {**spec, "prompts": {"extract": "changed-prompt"}}
    assert await store.pin_execution_spec(newest, changed) == changed
    with pytest.raises(StaleExecution):
        await store.pin_execution_spec(restored, changed)


async def test_expired_worker_cannot_aput_or_aput_writes_after_new_executor_takes_over(store):
    await initialize_checkpoints(store.dsn)
    run, command = await submitted(store)
    old_lease = await store.acquire(run["run_id"], command["command_id"], 1)
    lookup = {"configurable": {"thread_id": f"{run['run_id']}:1:1", "checkpoint_ns": ""}}
    async with await psycopg.AsyncConnection.connect(store.dsn, autocommit=True, row_factory=dict_row) as old_conn:
        old = FencedPostgresSaver(old_conn, store, old_lease)
        initial = empty_checkpoint()
        initial["channel_values"] = {"probe": "old executor"}
        initial["channel_versions"] = {"probe": "1"}
        saved = await old.aput(lookup, initial, {"source": "input", "step": 0, "parents": {}}, {"probe": "1"})
        await old.aput_writes(saved, [("probe", "before pause")], "old-task")
        # Keep the old connection alive, exactly as a paused worker would; it has
        # not called release and cannot authorize its own later writes.
        await expire(store, run["run_id"])
        replacement = await store.acquire(run["run_id"], command["command_id"], 1)
        assert replacement.epoch > old_lease.epoch
        async with await psycopg.AsyncConnection.connect(store.dsn, autocommit=True, row_factory=dict_row) as new_conn:
            new = FencedPostgresSaver(new_conn, store, replacement)
            fresh = empty_checkpoint()
            fresh["channel_values"] = {"probe": "new executor"}
            fresh["channel_versions"] = {"probe": "2"}
            latest = await new.aput(saved, fresh, {"source": "loop", "step": 1, "parents": {}}, {"probe": "2"})
            with pytest.raises(StaleExecution):
                await old.aput(latest, empty_checkpoint(), {"source": "loop", "step": 2, "parents": {}}, {})
            with pytest.raises(StaleExecution):
                await old.aput_writes(latest, [("probe", "stale overwrite")], "resumed-old-task")
            readback = await new.aget_tuple(lookup)
            assert readback.checkpoint["id"] == fresh["id"]
            assert readback.checkpoint["channel_values"]["probe"] == "new executor"
            assert not any(value == "stale overwrite" for _, _, value in readback.pending_writes)


async def correction_input(store):
    from docx import Document
    from docreader.tender.evidence import EvidenceRepository, freeze_sources, plan_sources, register_manifest
    from docreader.tender.requirements import validate_correction

    documents = []
    for number in range(2):
        path = store.test_artifacts.root / f"correction-source-{number}.docx"
        document = Document()
        document.add_paragraph(f"Project name: administrative building {number}.")
        document.save(path)
        documents.append({"document_id": f"document-{number}", "document_version": "submitted",
                          "role": "main" if number == 0 else "attachment", "path": str(path)})
    registered = register_manifest({"documents": documents}, store.test_artifacts)
    run = await store.submit(store.test_artifacts.put_json(registered), "cli-sources")
    command = (await store.get_run(run["run_id"]))["commands"][0]
    lease = await store.acquire(run["run_id"], command["command_id"], 1)
    frozen = await freeze_sources(registered, store.test_artifacts, plan=False)
    plan_ref = await plan_sources(frozen, store.test_artifacts)
    plan = store.test_artifacts.read_json(plan_ref)
    repository = EvidenceRepository(store.test_artifacts)
    extractions, inline = [], []
    for entry in plan["packs"]:
        pack = repository.load_pack(entry["artifact_ref"])
        result = {"requirements": [], "dispositions": [{"range_id": source["range_id"], "status": "non_requirement",
                  "reason": "The source identifies the project name and contains no supplier obligation.",
                  "requirement_ids": []} for source in pack["body"]], "issues": []}
        extractions.append({"pack_id": entry["pack_id"], "artifact_ref": store.test_artifacts.put_json(validate_correction(pack, result))})
        result["dispositions"][0]["reason"] = "Administrative building is the project name, not an obligation on a bidder."
        inline.append({"pack_id": entry["pack_id"], **result})
    requirements = store.test_artifacts.put_json({"packs": extractions, "counts": {"packs": len(extractions)}})
    await store.commit_artifact(lease, "freeze", frozen)
    await store.commit_artifact(lease, "pack-plan", plan_ref, metadata={"plan_digest": plan["plan_digest"]})
    await store.commit_artifact(lease, "extract", requirements)
    await store.release(lease, "blocked")
    return run, requirements, {"packs": inline}


async def test_cli_correction_validates_complete_inline_packs_and_replays_idempotently(store, tmp_path, monkeypatch):
    from docreader.integrations.cli import run as cli_run
    from docreader.jobs.settings import Settings

    run, baseline, inline = await correction_input(store)
    path = tmp_path / "corrected.json"
    path.write_text(json.dumps(inline))
    monkeypatch.setattr(Settings, "from_env", lambda: Settings(store.dsn, "redis://unused.invalid", store.test_artifacts.root))
    arguments = Namespace(command="correct", run_id=run["run_id"], requirements=path, source_version=1,
                          expected_revision=1, base_requirements_digest=baseline["sha256"],
                          reason="Clarify why descriptive ranges are not requirements", request_key="cli-correction")
    result = await cli_run(arguments)
    assert result["source_version"] == 1 and result["domain_revision"] == 2
    assert result == await cli_run(arguments)
    latest = await store.get_run(run["run_id"])
    index = store.test_artifacts.read_json(latest["revision"]["corrected_requirements_ref"])
    assert len(index["packs"]) == 2
    for item in index["packs"]:
        corrected = store.test_artifacts.read_json(item["artifact_ref"])
        assert corrected["pack_id"] == item["pack_id"]
        assert len(corrected["source_ranges"]) == 1  # Rebuilt from frozen evidence.
        assert corrected["dispositions"][0]["reason"].startswith("Administrative building")
    arguments.request_key = "stale-second-request"
    with pytest.raises(Conflict):
        await cli_run(arguments)
    assert (await store.get_run(run["run_id"]))["domain_revision"] == 2


async def test_cli_rejects_missing_duplicate_or_forged_correction_packs(store):
    from docreader.integrations.cli import _correction_ref

    run, baseline, inline = await correction_input(store)
    row = await store.get_run(run["run_id"])
    args = (store.test_artifacts, row, 1, 1, baseline["sha256"])
    with pytest.raises(ValueError, match="every frozen"):
        _correction_ref(*args, {"packs": inline["packs"][:1]})
    with pytest.raises(ValueError, match="unique"):
        _correction_ref(*args, {"packs": [inline["packs"][0], inline["packs"][0]]})
    forged = copy.deepcopy(inline)
    forged["packs"][0]["source_ranges"] = {"invented": {"document_id": "fake"}}
    with pytest.raises(ValueError):
        _correction_ref(*args, forged)
    incomplete = copy.deepcopy(inline)
    incomplete["packs"][0]["dispositions"] = []
    with pytest.raises(ValueError):
        _correction_ref(*args, incomplete)
    assert (await store.get_run(run["run_id"]))["domain_revision"] == 1
