"""Full-input downstream planning through the real workflow and PostgreSQL store.

All model responses are deterministic contract doubles; no external requests run.
"""
from collections import Counter
from copy import deepcopy
from uuid import uuid4

from docx import Document
import pytest

from docreader.integrations.store import InvalidConfirmation
from docreader.llm import ModelInfrastructureError, RequestBudget
from docreader.tender.evidence import EvidenceRepository, register_manifest
from docreader.tender.checks.verify import verify_payload
from docreader.workflow.nodes import TenderNodes
from docreader.workflow.runner import execute_command

from fixture_model import FixtureModel, fixture_request_digest
from test_phase1 import output, plan_items, runtime as runtime

pytestmark = pytest.mark.integration


async def _submit(runtime, *, ballast=False, document_ids=("complete-input",), suffixes="abcdefgh", condition=False):
    store, artifacts, settings, _ = runtime
    documents = []
    for document_id in document_ids:
        path = settings.artifact_root.parent / f"{document_id}.docx"
        document = Document()
        if condition:
            document.add_heading("第一章 适用范围", level=1)
            document.add_paragraph("仅第一标段适用。")
        for suffix in suffixes:
            if ballast:
                document.add_heading(f"Background package {suffix}", level=2 if condition else 1)
                document.add_paragraph("Background explanation of the delivery workflow. " * 70)
            document.add_heading(f"Delivery package {suffix}", level=2 if condition else 1)
            document.add_paragraph(f"The supplier must provide a signed commitment for package {suffix} in {document_id}.")
        document.save(path)
        documents.append({"document_id": document_id, "document_version": "one",
                          "role": "main", "path": str(path)})
    registered = register_manifest({"documents": documents}, artifacts)
    return await store.submit(artifacts.put_json(registered), uuid4().hex)


def _executor(runtime, run, *, basis_repeats=60, fail_task=None, include_conditions=False):
    store, _, settings, _ = runtime
    budget = RequestBudget(context_tokens=32768)
    calls = []
    attempts = Counter()
    failed = False

    class CompleteInputModel(FixtureModel):
        def request_budget(self):
            return budget

        async def call(self, task, payload, images=()):
            nonlocal failed
            budget.require(task, payload, images)
            attempts[task] += 1
            if task == fail_task and attempts[task] == 2 and not failed:
                failed = True
                request_id = uuid4().hex
                await self.store.begin_request(self.lease, request_id, "deterministic-contract-fixture", task, {
                    "request_digest": fixture_request_digest(task, payload, images),
                    "test_double": True, "external_request": False,
                })
                await self.store.finish_request(request_id, "failed", error={"type": "injected_confirmed_service_rejection"})
                raise ModelInfrastructureError("confirmed fixture rejection; successful sibling receipts remain reusable")
            calls.append((self.lease.domain_revision, task, deepcopy(payload)))
            result = await super().call(task, payload, images)
            if task == "extract":
                for requirement in result["requirements"]:
                    requirement["applicability_basis"] = "The unconditional supplier instruction applies to this response. " * basis_repeats
                    if include_conditions:
                        for source in payload["supporting"]:
                            if source.get("reason") == "condition":
                                requirement["conditions"].append(source["text"])
                                requirement["evidence"].append({"range_id": source["range_id"], "quote": source["text"]})
                for disposition in result["dispositions"]:
                    disposition["reason"] += " The complete cited source supports this disposition." * 12
            if task == "plan" and include_conditions:
                by_id = {item["requirement_id"]: item for item in payload["requirements"]}
                for item in result["items"]:
                    item["action"] += " " + " ".join(by_id[item["requirement_id"]]["conditions"])
            return result

    async def execute():
        command = (await store.get_run(run["run_id"]))["commands"][-1]
        await execute_command(run["run_id"], command["command_id"], command["source_version"], settings,
                              lambda s, a, lease: TenderNodes(s, a, lease, CompleteInputModel(s, lease)))
        return await store.get_run(run["run_id"])

    return execute, calls, budget


def _extractions(artifacts, row):
    index = output(artifacts, row, "extract")
    return index, [artifacts.read_json(entry["artifact_ref"]) for entry in index["packs"]]


async def _correct(runtime, row, edited):
    store, artifacts, _, _ = runtime
    index, _ = _extractions(artifacts, row)
    for entry, extraction in zip(index["packs"], edited, strict=True):
        entry["artifact_ref"] = artifacts.put_json(extraction)
    original = next(item["artifact_ref"] for item in row["artifacts"] if item["stage"] == "extract" and not item["scope"])
    await store.correct(row["run_id"], row["source_version"], row["domain_revision"], original["sha256"],
                        artifacts.put_json(index), uuid4().hex, "Review complete downstream dependencies against frozen evidence")


def _task_calls(calls, task, revision=1):
    return [payload for current, called_task, payload in calls if called_task == task and current == revision]


def _assert_exact_coverage(extractions, payloads):
    assert Counter(item["requirement_id"] for payload in payloads for item in payload["requirements"]) == Counter(
        item["requirement_id"] for extraction in extractions for item in extraction["requirements"])
    assert Counter(item["range_id"] for payload in payloads for item in payload["dispositions"]) == Counter(
        item["range_id"] for extraction in extractions for item in extraction["dispositions"])


@pytest.mark.parametrize("reference_kind", ["requirement", "range"])
async def test_cross_extract_dependencies_are_reviewed_and_planned_together(runtime, reference_kind):
    store, artifacts, _, _ = runtime
    run = await _submit(runtime, ballast=True)
    execute, calls, budget = _executor(runtime, run, basis_repeats=50)
    first = await execute()
    assert first["status"] == "waiting-confirmation", output(artifacts, first, "verify")["issues"]
    original = output(artifacts, first, "pack-plan")
    index, extractions = _extractions(artifacts, first)
    assert len(original["packs"]) == len(extractions) > 1
    with_requirements = [extraction for extraction in extractions if extraction["requirements"]]
    left = with_requirements[0]["requirements"][0]
    right = with_requirements[-1]["requirements"][-1]
    left["cross_references"] = [right["requirement_id"] if reference_kind == "requirement" else right["evidence"][0]["range_id"]]
    # Keep a second same-range claim: the disposition must accompany both complete obligations.
    companion = deepcopy(left)
    companion["requirement_id"] += "-companion"
    companion["applicability_basis"] = "The same source also supports this independently reviewed companion obligation."
    companion["cross_references"] = []
    with_requirements[0]["requirements"].append(companion)
    next(item for item in with_requirements[0]["dispositions"]
         if item["range_id"] == left["evidence"][0]["range_id"])["requirement_ids"].append(companion["requirement_id"])
    await _correct(runtime, first, extractions)
    second = await execute()
    checked = output(artifacts, second, "verify")
    assert second["status"] == "waiting-confirmation", checked["issues"]
    verification_calls = _task_calls(calls, "verify", 2)
    _assert_exact_coverage(extractions, verification_calls)
    dependency_ids = {left["requirement_id"], right["requirement_id"], companion["requirement_id"]}
    connected_calls = [payload for payload in verification_calls
                       if left["requirement_id"] in {item["requirement_id"] for item in payload["requirements"]}]
    assert len(connected_calls) == 1
    assert dependency_ids <= {item["requirement_id"] for item in connected_calls[0]["requirements"]}
    assert {left["evidence"][0]["range_id"], right["evidence"][0]["range_id"]} <= {
        item["range_id"] for item in connected_calls[0]["body"]}
    assert all(budget.fits("verify", payload) for payload in verification_calls)
    # The pre-replan extraction envelope fits extract but not complete verification.
    repo = EvidenceRepository(artifacts)
    assert any(not budget.fits("verify", verify_payload(repo.load_pack(entry), extraction))
               for entry, extraction in zip(original["packs"], extractions, strict=True))
    verification_plan = artifacts.read_json(checked["pack_plan_ref"])
    descriptors = [artifacts.read_json(entry["artifact_ref"]) for entry in verification_plan["packs"]]
    connected_descriptor = next(pack for pack in descriptors
                                if left["requirement_id"] in {ref["requirement_id"] for ref in pack["requirement_refs"]})
    assert len({ref["artifact_ref"]["sha256"] for ref in connected_descriptor["requirement_refs"]}) >= 2
    assert all("text" not in selected for pack in descriptors for selected in [*pack["body"], *pack["supporting"]])
    assert all("requirements" not in pack and "dispositions" not in pack for pack in descriptors)
    assert Counter(ref["range_id"] for pack in descriptors for ref in pack["disposition_refs"]) == Counter(
        item["range_id"] for extraction in extractions for item in extraction["dispositions"])
    assert len(index["packs"]) == len(original["packs"])
    await store.confirm(run["run_id"], 1, second["checklist_digest"], "cross-dependency-confirm", domain_revision=2)
    ready = await execute()
    plan = output(artifacts, ready, "plan")
    assert ready["status"] == "response-plan-ready", plan
    plan_calls = _task_calls(calls, "plan", 2)
    assert len(plan_calls) > 1
    assert any({left["requirement_id"], right["requirement_id"]} <= {
        item["requirement_id"] for item in payload["requirements"]} for payload in plan_calls)
    assert Counter(item["requirement_id"] for payload in plan_calls for item in payload["requirements"]) == Counter(
        item["requirement_id"] for extraction in extractions for item in extraction["requirements"])
    response_descriptors = [artifacts.read_json(entry["artifact_ref"])
                            for entry in artifacts.read_json(plan["pack_plan_ref"])["packs"]]
    assert any(len({ref["artifact_ref"]["sha256"] for ref in pack["requirement_refs"]}) >= 2
               for pack in response_descriptors)
    expected_refs = {item["requirement_id"]: entry["artifact_ref"]
                     for entry in artifacts.read_json(checked["requirements_ref"])["packs"]
                     for item in artifacts.read_json(entry["artifact_ref"])["requirements"]}
    assert all(item["requirement_ref"]["artifact_ref"] == expected_refs[item["requirement_id"]]
               for item in plan_items(artifacts, plan))


async def test_indivisible_complete_verify_input_blocks_before_sending(runtime):
    store, artifacts, _, _ = runtime
    run = await _submit(runtime, suffixes="a")
    execute, calls, budget = _executor(runtime, run, basis_repeats=400)
    row = await execute()
    checked = output(artifacts, row, "verify")
    assert row["status"] == "blocked"
    assert _task_calls(calls, "extract")
    assert not _task_calls(calls, "verify")
    assert not _task_calls(calls, "plan")
    codes = {item["code"] for item in checked["issues"]}
    assert "indivisible_verify_input_exceeded" in codes
    assert "verification_budget_exceeded" in codes
    assert "verification_semantic_failure" not in codes
    assert all(request["task"] == "extract" for request in row["usage"])
    index, extractions = _extractions(artifacts, row)
    assert len(index["packs"]) == 1 and len(extractions[0]["requirements"]) == 1
    assert not budget.fits("verify", verify_payload(
        EvidenceRepository(artifacts).load_pack(output(artifacts, row, "pack-plan")["packs"][0]), extractions[0]))
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, row["checklist_digest"], "cannot-force-indivisible")
    before = len(calls)
    await execute()
    assert len(calls) == before


@pytest.mark.parametrize("failed_task", ["verify", "plan"])
async def test_partial_downstream_success_reuses_durable_subpack_receipts(runtime, failed_task):
    store, artifacts, _, _ = runtime
    run = await _submit(runtime)
    execute, calls, _ = _executor(runtime, run, fail_task=failed_task)
    if failed_task == "plan":
        checked = await execute()
        assert checked["status"] == "waiting-confirmation"
        await store.confirm(run["run_id"], 1, checked["checklist_digest"], "partial-plan-confirm")
    with pytest.raises(ModelInfrastructureError):
        await execute()
    failed = await store.get_run(run["run_id"])
    assert failed["status"] == "pending"
    saved = [item for item in failed["artifacts"] if item["stage"] == f"{failed_task}-pack"]
    assert saved, "at least one independent downstream child completed before recovery"
    succeeded = {request["metadata"]["request_digest"] for request in failed["usage"]
                 if request["task"] == failed_task and request["status"] == "succeeded"}
    assert succeeded
    before_calls = Counter(fixture_request_digest(task, payload) for _, task, payload in calls if task == failed_task)
    recovered = await execute()
    assert recovered["status"] == ("waiting-confirmation" if failed_task == "verify" else "response-plan-ready")
    current = {(item["stage"], item["scope"]): item for item in recovered["artifacts"]}
    for receipt in saved:
        preserved = current[(receipt["stage"], receipt["scope"])]
        assert preserved["artifact_ref"] == receipt["artifact_ref"]
        assert preserved["epoch"] == receipt["epoch"]
        assert preserved["input_digest"] == receipt["input_digest"]
    after_calls = Counter(fixture_request_digest(task, payload) for _, task, payload in calls if task == failed_task)
    assert all(after_calls[digest] == before_calls[digest] == 1 for digest in succeeded)
    plan_stage = "verify-plan" if failed_task == "verify" else "response-pack-plan"
    stage_plan = output(artifacts, recovered, plan_stage)
    assert all(receipt["scope"].startswith(stage_plan["plan_digest"] + ":") for receipt in saved)
    if failed_task == "verify":
        await store.confirm(run["run_id"], 1, recovered["checklist_digest"], "recovered-verify-confirm")
        ready = await execute()
    else:
        ready = recovered
    assert ready["status"] == "response-plan-ready"
    assert len(plan_items(artifacts, output(artifacts, ready, "plan"))) == 8
    final_count = len(calls)
    await execute()
    assert len(calls) == final_count


async def test_same_source_revision_invalidates_downstream_plans_and_receipts(runtime):
    store, artifacts, _, _ = runtime
    run = await _submit(runtime)
    execute, calls, _ = _executor(runtime, run)
    first = await execute()
    await store.confirm(run["run_id"], 1, first["checklist_digest"], "revision-confirm-one")
    first_ready = await execute()
    assert first_ready["status"] == "response-plan-ready"
    original_frozen = next(item["artifact_ref"] for item in first_ready["artifacts"] if item["stage"] == "freeze")
    old_plan_digests = {stage: output(artifacts, first_ready, stage)["plan_digest"]
                        for stage in ("verify-plan", "response-pack-plan")}
    _, extractions = _extractions(artifacts, first_ready)
    changed = extractions[0]["requirements"][0]
    changed["veto_risk"] = True
    await _correct(runtime, first_ready, extractions)
    pending = await store.get_run(run["run_id"])
    assert pending["source_version"] == 1 and pending["domain_revision"] == 2
    assert pending["checklist_digest"] is None
    assert not any(item["stage"] in {"verify-plan", "verify-pack", "response-pack-plan", "plan-pack", "plan"}
                   for item in pending["artifacts"])
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, first["checklist_digest"], "stale-revision-confirm", domain_revision=1)
    second = await execute()
    assert second["status"] == "waiting-confirmation"
    assert second["checklist_digest"] != first["checklist_digest"]
    assert not _task_calls(calls, "extract", 2)
    assert any(item["requirement_id"] == changed["requirement_id"] and item["veto_risk"]
               for payload in _task_calls(calls, "verify", 2) for item in payload["requirements"])
    await store.confirm(run["run_id"], 1, second["checklist_digest"], "revision-confirm-two", domain_revision=2)
    second_ready = await execute()
    assert second_ready["status"] == "response-plan-ready"
    assert next(item["artifact_ref"] for item in second_ready["artifacts"] if item["stage"] == "freeze") == original_frozen
    for stage in ("verify-plan", "response-pack-plan"):
        assert output(artifacts, second_ready, stage)["plan_digest"] != old_plan_digests[stage]
    assert all(receipt["domain_revision"] == 2 for receipt in second_ready["artifacts"]
               if receipt["stage"] in {"verify-plan", "verify-pack", "response-pack-plan", "plan-pack"})
    assert any(item["requirement_id"] == changed["requirement_id"] and item["veto_risk"]
               for payload in _task_calls(calls, "plan", 2) for item in payload["requirements"])


@pytest.mark.parametrize("dependency_kind,code", [
    ("unknown", "dependency_reference_unresolved"),
    ("cross_document", "cross_document_dependency"),
])
async def test_unrepresentable_dependency_blocks_affected_input_without_model_send(runtime, dependency_kind, code):
    store, artifacts, _, _ = runtime
    document_ids = ("left-document", "right-document") if dependency_kind == "cross_document" else ("left-document",)
    run = await _submit(runtime, document_ids=document_ids)
    execute, calls, _ = _executor(runtime, run)
    first = await execute()
    assert first["status"] == "waiting-confirmation"
    _, extractions = _extractions(artifacts, first)
    left = extractions[0]["requirements"][0]
    right = extractions[-1]["requirements"][-1]
    left["cross_references"] = ["missing-requirement" if dependency_kind == "unknown" else right["requirement_id"]]
    await _correct(runtime, first, extractions)
    blocked = await execute()
    assert blocked["status"] == "blocked"
    checked = output(artifacts, blocked, "verify")
    codes = {item["code"] for item in checked["issues"]}
    assert code in codes
    assert "verification_semantic_failure" not in codes
    assert not any(item["requirement_id"] == left["requirement_id"]
                   for payload in _task_calls(calls, "verify", 2) for item in payload["requirements"])
    assert not _task_calls(calls, "plan", 2)
    with pytest.raises(InvalidConfirmation):
        await store.confirm(run["run_id"], 1, blocked["checklist_digest"], "cannot-force-dependency", domain_revision=2)


@pytest.mark.parametrize("reference_kind", ["requirement", "range"])
async def test_plan_dependency_on_superseded_requirement_blocks_affected_send(runtime, reference_kind):
    store, artifacts, _, _ = runtime
    run = await _submit(runtime, document_ids=("main-input", "amend-input"), suffixes="ab")
    execute, calls, _ = _executor(runtime, run, basis_repeats=10)
    first = await execute()
    assert first["status"] == "waiting-confirmation"
    _, extractions = _extractions(artifacts, first)
    original = next(extraction for extraction in extractions if any(
        item["document_id"] == "main-input" for item in extraction["source_ranges"].values()))
    replaced_range_id = original["requirements"][-1]["evidence"][0]["range_id"]
    manifest = artifacts.read_json(first["manifest_ref"])
    amendment = next(document for document in manifest["documents"] if document["document_id"] == "amend-input")
    amendment["role"] = "amendment"
    amendment["replaces"] = [{"document_id": "main-input", "range_id": replaced_range_id}]
    await store.revise(run["run_id"], artifacts.put_json(manifest), "explicit-amendment-target")
    amended = await execute()
    assert amended["status"] == "waiting-confirmation", output(artifacts, amended, "verify")["issues"]
    _, extractions = _extractions(artifacts, amended)
    original = next(extraction for extraction in extractions if any(
        item["document_id"] == "main-input" for item in extraction["source_ranges"].values()))
    left, replaced = original["requirements"]
    left["cross_references"] = [replaced["requirement_id"] if reference_kind == "requirement"
                                else replaced["evidence"][0]["range_id"]]
    await _correct(runtime, amended, extractions)
    checked_row = await execute()
    checked = output(artifacts, checked_row, "verify")
    assert checked_row["status"] == "waiting-confirmation", checked["issues"]
    assert checked["applicability_decisions"][replaced["requirement_id"]]["applicable"] is False
    await store.confirm(run["run_id"], 2, checked_row["checklist_digest"], "confirm-explicit-amendment", domain_revision=2)
    blocked = await execute()
    assert blocked["status"] == "blocked"
    plan = output(artifacts, blocked, "plan")
    assert "inactive_requirement_dependency" in {item["code"] for item in plan["issues"]}
    assert not any(item["requirement_id"] in {left["requirement_id"], replaced["requirement_id"]}
                   for payload in _task_calls(calls, "plan", 2) for item in payload["requirements"])


async def test_recombined_condition_excerpt_keeps_identity_without_repeating_source_text(runtime):
    store, artifacts, _, _ = runtime
    run = await _submit(runtime, suffixes=[prefix + letter for prefix in "ab" for letter in "abcdefghijklmnopqrstuvwxyz"],
                        condition=True)
    execute, calls, _ = _executor(runtime, run, basis_repeats=35, include_conditions=True)
    first = await execute()
    assert first["status"] == "waiting-confirmation", output(artifacts, first, "verify")["issues"]
    original = output(artifacts, first, "pack-plan")
    _, extractions = _extractions(artifacts, first)
    assert len(original["packs"]) > 1
    left = next(item for extraction in extractions for item in extraction["requirements"])
    right = next(item for extraction in reversed(extractions) for item in reversed(extraction["requirements"])
                 if item["conditions"])
    condition_evidence = next(evidence for evidence in right["evidence"] if evidence["quote"] == "仅第一标段适用。")
    original_condition_identity = deepcopy(condition_evidence)
    left["cross_references"] = [right["requirement_id"]]
    await _correct(runtime, first, extractions)
    second = await execute()
    checked = output(artifacts, second, "verify")
    assert second["status"] == "waiting-confirmation", checked["issues"]
    payloads = _task_calls(calls, "verify", 2)
    _assert_exact_coverage(extractions, payloads)
    assert len(payloads) > 1, "a shared support condition must not make every dependent body indivisible"
    connected = next(payload for payload in payloads if left["requirement_id"] in {
        item["requirement_id"] for item in payload["requirements"]})
    preserved = next(item for item in connected["requirements"] if item["requirement_id"] == right["requirement_id"])
    assert original_condition_identity in preserved["evidence"]
    assert preserved["conditions"] == right["conditions"]
    alias = next(item for item in connected["source_aliases"] if item["range_id"] == condition_evidence["range_id"])
    assert "text" not in alias
    sources = {item["range_id"]: item for item in [*connected["body"], *connected["supporting"]]}
    rendered_alias = "".join(
        sources[segment["range_id"]]["text"][segment["start"] - sources[segment["range_id"]].get("start", 0):
                                              segment["end"] - sources[segment["range_id"]].get("start", 0)]
        for segment in alias["segments"])
    assert rendered_alias == condition_evidence["quote"]
    for payload in payloads:
        intervals = {}
        for source in [*payload["body"], *payload["supporting"]]:
            intervals.setdefault(source.get("source_range_id", source["range_id"]), []).append(
                (source.get("start", 0), source.get("end", len(source["text"]))))
        for spans in intervals.values():
            ordered = sorted(spans)
            assert all(left_span[1] <= right_span[0] for left_span, right_span in zip(ordered, ordered[1:]))
    await store.confirm(run["run_id"], 1, second["checklist_digest"], "condition-alias-confirm", domain_revision=2)
    ready = await execute()
    assert ready["status"] == "response-plan-ready", output(artifacts, ready, "plan")
    assert any(original_condition_identity in item["evidence"] and item["conditions"] == right["conditions"]
               for payload in _task_calls(calls, "plan", 2) for item in payload["requirements"]
               if item["requirement_id"] == right["requirement_id"])


async def test_complete_plan_indivisible_input_blocks_after_admitted_verification(runtime):
    store, artifacts, settings, _ = runtime
    run = await _submit(runtime, suffixes="a")
    budget = RequestBudget(context_tokens=32768)
    calls = []

    class BoundaryModel(FixtureModel):
        def request_budget(self):
            return budget

        async def call(self, task, payload, images=()):
            budget.require(task, payload, images)
            calls.append((task, deepcopy(payload)))
            result = await super().call(task, payload, images)
            if task == "extract":
                requirement = result["requirements"][0]
                # This exact range dependency is carried in the complete plan
                # source_context as well as the immutable requirement reference.
                requirement["cross_references"] = [requirement["evidence"][0]["range_id"]]
                measured = budget.measure("verify", {
                    **payload, "requirements": result["requirements"], "dispositions": result["dispositions"],
                })
                padding = budget.effective_input_budget_tokens - measured["input_tokens"] - 32
                assert padding > 0
                sentence = " The cited source supports this applicability decision."
                requirement["applicability_basis"] += sentence * (padding // len(sentence)) + " " * (padding % len(sentence))
            return result

    async def execute():
        command = (await store.get_run(run["run_id"]))["commands"][-1]
        await execute_command(run["run_id"], command["command_id"], 1, settings,
                              lambda s, a, lease: TenderNodes(s, a, lease, BoundaryModel(s, lease)))
        return await store.get_run(run["run_id"])

    checked_row = await execute()
    assert checked_row["status"] == "waiting-confirmation", output(artifacts, checked_row, "verify")["issues"]
    verify_calls = [payload for task, payload in calls if task == "verify"]
    assert len(verify_calls) == 1 and budget.fits("verify", verify_calls[0])
    await store.confirm(run["run_id"], 1, checked_row["checklist_digest"], "indivisible-plan-confirm")
    blocked = await execute()
    assert blocked["status"] == "blocked"
    plan = output(artifacts, blocked, "plan")
    codes = {item["code"] for item in plan["issues"]}
    assert "indivisible_plan_input_exceeded" in codes
    assert "plan_semantic_failure" not in codes
    assert not any(task == "plan" for task, _ in calls)
    assert not any(request["task"] == "plan" for request in blocked["usage"])
    descriptor = artifacts.read_json(artifacts.read_json(plan["pack_plan_ref"])["packs"][0]["artifact_ref"])
    assert descriptor["planning_details"]["budget"]["fits"] is False
    assert descriptor["supporting"] and descriptor["requirement_refs"]
    assert all("text" not in selection for selection in descriptor["supporting"])
    before = len(calls)
    await execute()
    assert len(calls) == before
