"""Execute shared-source requests while preserving each logical review boundary."""
from __future__ import annotations

import asyncio
from copy import deepcopy

from pydantic import ValidationError

from docreader.integrations.store import Cancelled, StaleExecution
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender.checks.analysis import _digest, _public_issues, _view_ids
from docreader.tender.checks.analysis_batches import member_payload, split_targets
from docreader.tender.schema import assign_issues, issue


def _invalid(path, code, detail):
    raise ModelContractError([{"path": path, "code": code, "detail": detail}])


def validate_shared(context, response, payload):
    """Validate common objections before accepting any member of this response."""
    from docreader.tender.schema import AnalysisBatchResponse, AnalysisSharedFinding

    try:
        envelope = AnalysisBatchResponse.model_validate(response)
    except ValidationError as error:
        raise ModelContractError([{"path": list(e["loc"]), "code": e["type"], "detail": e["msg"]}
            for e in error.errors(include_url=False, include_input=False)]) from None
    members = {unit["work_unit_id"] for unit in payload["work_units"]}
    targets = set(payload["target_work_unit_ids"])
    seen = set()
    for number, unit in enumerate(envelope.work_units):
        if not isinstance(unit, dict):
            continue  # Recovery records a missing target without rejecting valid siblings.
        identity = unit.get("work_unit_id")
        if identity is None:
            continue
        if not isinstance(identity, str) or identity not in targets:
            _invalid(["work_units", number, "work_unit_id"], "unknown_work_unit",
                     "Return results only for target_work_unit_ids")
        if identity in seen:
            _invalid(["work_units", number, "work_unit_id"], "duplicate_work_unit", "Return each target at most once")
        seen.add(identity)
    shared = []
    for number, original in enumerate(envelope.shared_findings):
        try:
            value = AnalysisSharedFinding.model_validate(original).model_dump(exclude_unset=True)
        except ValidationError as error:
            raise ModelContractError([{"path": ["shared_findings", number, *e["loc"]],
                                      "code": e["type"], "detail": e["msg"]}
                for e in error.errors(include_url=False, include_input=False)]) from None
        if not set(value["work_unit_ids"]) <= members:
            _invalid(["shared_findings", number, "work_unit_ids"], "unknown_work_unit",
                     "Address only members supplied in work_units, including non-target siblings")
        value["work_unit_ids"] = list(dict.fromkeys(value["work_unit_ids"]))
        context.findings([value["finding"]], member_payload(payload, value["work_unit_ids"][0]))
        shared.append(value)
    return shared


def validate_member(context, batch, identity, response):
    if not isinstance(response, dict) or response.get("work_unit_id") != identity:
        _invalid(["work_unit_id"], "unknown_work_unit", "Use the requested work unit identity")
    payload = member_payload(batch, identity)
    value = {key: entry for key, entry in response.items() if key != "work_unit_id"}
    if "shared_findings" in value:
        if type(value["shared_findings"]) is list and not value["shared_findings"]:
            value.pop("shared_findings")
        else:
            _invalid(["shared_findings"], "extra_forbidden",
                     "Only [] may be omitted here. Put shared findings in outer shared_findings "
                     "with explicit work_unit_ids; do not discard their content.")
    value["schema_version"] = batch["schema_version"]
    if batch["operation"] == "associate":
        unit = next(unit for unit in batch["work_units"] if unit["work_unit_id"] == identity)
        allowed = set(unit["issue_ids"])
        for number, decision in enumerate(value.get("decisions", []) if isinstance(value.get("decisions"), list) else []):
            if isinstance(decision, dict) and isinstance(decision.get("issue_ids"), list):
                ids = decision["issue_ids"]
                if any(not isinstance(i, str) or i not in allowed for i in ids):
                    _invalid(["decisions", number, "issue_ids"], "foreign_resolution_target",
                             "Resolve only issues assigned to this work unit; report cross-unit objections explicitly")
    return context.validate(value, payload)


async def execute_batches(context, phase, batches, model, save):
    def describe():
        descriptors = [{"work_unit_id": f"{phase}-batch-{_digest(batch)[:16]}",
                        "input_digest": _digest(batch), "budget": context.budget.measure("verify", batch),
                        "work_unit_ids": [u["work_unit_id"] for u in batch["work_units"]],
                        "target_work_unit_ids": batch["target_work_unit_ids"],
                        "range_ids": sorted(_view_ids(batch["source_views"]))} for batch in batches]
        return {"schema_version": 2, "phase": phase, "requests": descriptors,
                "source_packs": context.plan["packs"], "item_refs": context.item_refs,
                "budget_digest": _digest(context.budget.spec())}

    # Whole-stage token measurement must not starve the lease heartbeat.
    plan = await asyncio.to_thread(describe)
    descriptors = plan["requests"]

    async def planned():
        return plan

    plan_ref = await save(f"analysis-{phase}-plan", planned, inputs={"plan_digest": _digest(plan)})
    semaphore = getattr(model, "semaphore", None) or asyncio.Semaphore(2)
    stopped = asyncio.Event()

    async def worker(batch, descriptor):
        if descriptor["budget"]["fits"] is False:
            return None
        async with semaphore:
            if stopped.is_set():
                return None
            try:
                async def produce():
                    return await model.call_groups_validated("verify", batch,
                        lambda identity, value: validate_member(context, batch, identity, value),
                        validate_shared=lambda response, current: validate_shared(context, response, current))

                # Exact physical responses are restored inside call_groups before
                # any newly regrouped request; this receipt records its aggregate.
                ref = await save(f"analysis-{phase}-batch", produce, scope=descriptor["work_unit_id"],
                                 inputs={"payload_digest": descriptor["input_digest"]}, model=True)
                return ref, await asyncio.to_thread(context.artifacts.read_json, ref)
            except BaseException:
                stopped.set()
                raise

    settled = await asyncio.gather(*(worker(batch, descriptor)
        for batch, descriptor in zip(batches, descriptors, strict=True)), return_exceptions=True)
    failures = [result for result in settled if isinstance(result, BaseException)]
    if failures:
        stop = (asyncio.CancelledError, Cancelled, StaleExecution)
        fallback = next((e for e in failures if isinstance(e, ModelWorkUnitExhausted)), failures[0])
        raise next((e for e in failures if isinstance(e, stop)), fallback)

    # Collect objections from ALL split calls before exposing any acceptance.
    shared_by_unit = {}
    for batch, result in zip(batches, settled, strict=True):
        if result is None:
            continue
        for entry in result[1]["shared_findings"]:
            for identity in entry["work_unit_ids"]:
                shared_by_unit.setdefault(identity, []).append(entry["finding"])

    payloads, results = [], []
    for batch, result in zip(batches, settled, strict=True):
        basis = {key: value for key, value in batch.items() if key != "target_work_unit_ids"}
        values = {unit["work_unit_id"]: unit for unit in result[1]["work_units"]} if result else {}
        for identity in batch["target_work_unit_ids"]:
            payload = member_payload(batch, identity)
            payloads.append(payload)
            if result is None:
                problems = assign_issues([issue("analysis_budget_exceeded",
                    "Complete shared analysis basis does not fit even for this target; regroup the host request "
                    "or revise the source explicitly, never truncate evidence.")], identity)
                context.problems.extend(problems)
                context.protected.update(p["issue_id"] for p in problems)
                context.host_issue_ids.update(p["issue_id"] for p in problems)
                context.incomplete_requests.append(identity)
                results.append(None)
                continue
            raw = {key: value for key, value in values[identity].items() if key != "work_unit_id"}

            async def produced(raw=raw):
                return raw

            ref = await save(f"analysis-{phase}", produced, scope=identity,
                inputs={"basis_digest": _digest(basis), "work_unit_id": identity}, model=True)
            context.operation_refs.append({"operation": phase, "work_unit_id": identity,
                                           "artifact_ref": ref, "batch_ref": result[0]})
            effective = deepcopy(raw)
            effective["findings"].extend(shared_by_unit.get(identity, []))
            context.problems.extend(context.findings(effective["findings"], payload))
            results.append(effective)
        if result is not None:
            context.completed_model_requests += 1
    targets = {p["work_unit_id"] for p in payloads}
    for batch in batches:
        for unit in batch["work_units"]:
            identity = unit["work_unit_id"]
            if identity not in targets and identity in shared_by_unit:
                context.problems.extend(context.findings(shared_by_unit[identity], member_payload(batch, identity)))
    return plan_ref, payloads, results


def joint_batches(context, batches, associated):
    """Keep the original visible basis; split only the requested review targets."""
    requests = []
    for original in batches:
        batch = deepcopy(original)
        batch.update(operation="review", dispositions=[], decisions=[], target_work_unit_ids=[])
        findings = []
        for unit in batch["work_units"]:
            original_id = unit["work_unit_id"]
            unit["work_unit_id"] = original_id.replace("associate-", "joint-", 1)
            result = associated.get(original_id)
            unit["decision_ids"] = [d["decision_id"] for d in result["decisions"]] if result else []
            if result:
                batch["decisions"].extend(result["decisions"])
                batch["relations"].extend(result["relations"])
                findings.extend(assign_issues(result["findings"], original_id))
            if unit["decision_ids"]:
                batch["target_work_unit_ids"].append(unit["work_unit_id"])
        batch["issues"] = _public_issues(assign_issues([*batch["issues"], *findings], "joint"))
        requests.extend(split_targets(batch, context.budget))
    return requests
