"""Independent review observations under the caller's existing durable lease.

This module does not create an analysis, confirmation, CLI, or semantic score.
The caller supplies an isolated Store/Lease/OpenAIModel and keeps its heartbeat
alive. A completed evaluation is a model observation, never human ground truth.
"""

from __future__ import annotations

import asyncio
from collections import Counter
from copy import deepcopy
import hashlib
import json

import psycopg
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.llm.recovery import call_validated
from docreader.llm.request import ModelBudgetError, ModelContractError, ModelWorkUnitExhausted, request_digest
from docreader.llm.usage import reported_usage
from docreader.tender.checks.analysis import _view_ids
from docreader.tender.schema import (
    AnalysisExtraction, AnalysisReviewResponse, Decision, ScopeDecision, SourceView, recover_issue_routing,
)


_PAYLOAD_FIELDS = frozenset({"schema_version", "operation", "work_unit_id", "source_version", "domain_revision",
    "source_views", "items", "relations", "decisions", "dispositions", "issues", "candidates"})
_PROTOCOL = "tender_semantic_evaluation_v1"


def _digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
        separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def _input(payload):
    if (not isinstance(payload, dict) or not _PAYLOAD_FIELDS <= payload.keys()
            or payload.keys() - (_PAYLOAD_FIELDS | {"scope_decisions"})
            or payload["schema_version"] != 2 or payload["operation"] != "review"
            or not isinstance(payload["work_unit_id"], str) or not payload["work_unit_id"].strip()
            or any(type(payload[key]) is not int or payload[key] < 1
                   for key in ("source_version", "domain_revision")) or payload["candidates"] != []):
        raise ValueError("semantic evaluation requires annotation-free, independent v2 review inputs")
    for view in payload["source_views"]:
        SourceView.model_validate(view)
    AnalysisExtraction.model_validate({**{key: payload[key] for key in
        ("schema_version", "items", "relations", "dispositions")},
        "issues": [{key: value for key, value in issue.items() if key != "issue_id"}
                   for issue in payload["issues"]]})
    for decision in payload["decisions"]:
        Decision.model_validate(decision)
    for decision in payload.get("scope_decisions", []):
        ScopeDecision.model_validate(decision)
    return deepcopy(payload)


def validate_review_response(response, payload):
    """Check the current response shape and supplied references, not its meaning."""
    response, diagnostics = deepcopy(response), []
    if isinstance(response, dict):
        for name in ("range", "item", "relation", "decision"):
            response.pop(f"reviewed_{name}_ids", None)
        if isinstance(response.get("findings"), list):
            allowed = {issue["issue_id"] for issue in payload["issues"]}
            response["findings"] = [recover_issue_routing(finding, allowed, f"findings[{index}]", diagnostics)
                                    for index, finding in enumerate(response["findings"])]
    try:
        value = AnalysisReviewResponse.model_validate(response).model_dump(exclude_unset=True)
    except ValidationError as error:
        raise ModelContractError([{"path": list(entry["loc"]), "code": entry["type"], "detail": entry["msg"]}
            for entry in error.errors(include_url=False, include_input=False)]) from None
    supplied = {"range": _view_ids(payload["source_views"]),
                "item": {item["item_id"] for item in payload["items"]},
                "relation": {relation["relation_id"] for relation in payload["relations"]},
                "decision": {decision["decision_id"] for decision in payload["decisions"]}}
    if not set(value["accepted_decision_ids"]) <= supplied["decision"]:
        raise ModelContractError([{"path": ["accepted_decision_ids"], "code": "unknown_decision",
            "detail": "Accept only decisions supplied in this request"}])
    ranges = supplied["range"] | {issue["range_id"] for issue in payload["issues"] if issue.get("range_id")}
    for index, finding in enumerate(value["findings"]):
        if ((finding.get("range_id") and finding["range_id"] not in ranges)
                or not set(finding.get("item_ids", [])) <= supplied["item"]):
            raise ModelContractError([{"path": ["findings", index], "code": "unknown_reference",
                "detail": "Finding refers to an identity absent from its request"}])
    value["accepted_decision_ids"] = list(dict.fromkeys(value["accepted_decision_ids"]))
    value.update({f"reviewed_{name}_ids": sorted(ids) for name, ids in supplied.items()})
    if diagnostics:
        value["diagnostics"] = diagnostics
    return value


def prepare_semantic_evaluation(cases, *, model):
    """Build all complete requests without calling the model or reading labels."""
    budget = model.request_budget()
    if budget.prompts.get("verify", "").splitlines()[:1] != [
            "# Independent tender analysis verification — tender-phase1-v2"]:
        raise ValueError("semantic evaluation requires the current v2 review prompt")
    entries, identities = [], set()
    for case in cases:
        identity = case.get("case_id") if isinstance(case, dict) else None
        if not isinstance(identity, str) or not identity.strip() or identity in identities:
            raise ValueError("semantic evaluation requires unique nonempty local case identities")
        identities.add(identity)
        payload = _input(case.get("payload"))
        request = budget.request("verify", payload)
        digest = request_digest(model.config.chat.base_url, request)
        entries.append({"case_id": identity, "payload": payload, "request": request, "request_digest": digest,
            "input_digest": _digest({"request_digest": digest, "budget_spec": budget.spec()}),
            "budget": budget.measure_request(request, "verify")})
    if not entries:
        raise ValueError("semantic evaluation requires at least one independent case")
    return {"protocol": _PROTOCOL, "model_spec": model.execution_spec(), "cases": entries,
            "planned_requests": len(entries), "maximum_sends": 4 * len(entries)}


def _usage(rows):
    sent = [row for row in rows if row["sent_at"] is not None]
    logical = {row["metadata"]["logical_request_digest"] for row in sent}
    result = {"sent_requests": len(sent), "retry_requests": len(sent) - len(logical),
        "estimated_input_tokens": 0, "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0,
        "unknown_usage_requests": 0}
    for row in sent:
        estimate = row["metadata"].get("input_tokens")
        if type(estimate) is int and estimate >= 0:
            result["estimated_input_tokens"] += estimate
        reported = reported_usage(row.get("usage")) or {}
        fields = ("prompt_tokens", "completion_tokens", "total_tokens")
        known = {field: reported[field] for field in fields if type(reported.get(field)) is int}
        if len(known) != len(fields):
            result["unknown_usage_requests"] += 1
        for field in fields:
            result[field] += known.get(field, 0)
    return result


async def execute_semantic_evaluation(cases, *, evaluation_id, store, artifacts, lease, model, concurrency=2):
    """Return a durable report reference; the caller owns isolation and heartbeat.

    Each independent request uses the shared initial-plus-three retry allowance.
    A case that exhausts that allowance remains in the denominator while other
    cases continue. Persistence, cancellation and lease failures escape so their
    caller can recover ownership without pretending they are semantic judgments.
    """
    if (not isinstance(evaluation_id, str) or not evaluation_id.strip()
            or type(concurrency) is not int or concurrency < 1):
        raise ValueError("evaluation identity and positive concurrency are required")
    plan = prepare_semantic_evaluation(cases, model=model)
    plan_digest = _digest(plan)
    cached = await store.receipt(lease, "semantic-evaluation-report", evaluation_id, input_digest=plan_digest)
    if cached is not None:
        return cached["artifact_ref"]
    plan_ref = await asyncio.to_thread(artifacts.put_json, plan)
    await store.commit_artifact(lease, "semantic-evaluation-plan", plan_ref,
        scope=evaluation_id, input_digest=plan_digest)
    before = {row["request_id"] for row in (await store.get_run(lease.run_id))["usage"]}
    semaphore, locks = asyncio.Semaphore(concurrency), {}

    async def execute(entry):
        identity = entry["input_digest"]
        async with semaphore, locks.setdefault(identity, asyncio.Lock()):
            saved = await store.receipt(lease, "semantic-evaluation-case", identity, input_digest=identity)
            if saved is not None:
                return {**await asyncio.to_thread(artifacts.read_json, saved["artifact_ref"]), "reused": True}
            value = {"request_digest": entry["request_digest"], "status": "budget_exceeded", "response_ref": None,
                     "request_ref": await asyncio.to_thread(artifacts.put_json, entry["request"]),
                     "payload_ref": await asyncio.to_thread(artifacts.put_json, entry["payload"])}
            try:
                model.request_budget().require_request(entry["request"], "verify")
            except ModelBudgetError:
                value["error_type"] = "ModelBudgetError"
            else:
                try:
                    response = await call_validated(model, store, artifacts, lease, "verify", entry["payload"],
                        lambda raw: validate_review_response(raw, entry["payload"]),
                        passthrough_exceptions=(psycopg.Error, OSError, ArtifactIntegrityError))
                except ModelWorkUnitExhausted:
                    value.update(status="exhausted", error_type="ModelWorkUnitExhausted")
                else:
                    value.update(status="succeeded", response_ref=await asyncio.to_thread(artifacts.put_json, response))
            reference = await asyncio.to_thread(artifacts.put_json, value)
            await store.commit_artifact(lease, "semantic-evaluation-case", reference, scope=identity, input_digest=identity)
            return {**value, "reused": False}

    work = [asyncio.create_task(execute(entry)) for entry in plan["cases"]]
    try:
        observations = await asyncio.gather(*work)
    finally:
        for task in work:
            if not task.done():
                task.cancel()
        await asyncio.gather(*work, return_exceptions=True)
    run = await store.get_run(lease.run_id)
    digests = {entry["request_digest"] for entry in plan["cases"]}
    rows = [row for row in run["usage"] if row["task"] == "verify"
            and row["metadata"].get("logical_request_digest") in digests]
    records = []
    for entry, observed in zip(plan["cases"], observations, strict=True):
        requests = [{key: deepcopy(row.get(key)) for key in
            ("request_id", "task", "status", "metadata", "response_ref", "usage", "error")}
            for row in rows if row["metadata"]["logical_request_digest"] == entry["request_digest"]]
        recovery_digest = _digest({"task": "verify", "payload": entry["payload"]})
        rejections = [row["artifact_ref"] for row in run.get("artifacts", [])
            if row["stage"] == "model-rejection" and row["input_digest"] == recovery_digest]
        records.append({**observed, "case_id": entry["case_id"], "budget": entry["budget"], "requests": requests,
                        "rejection_refs": rejections, "annotation_review": None, "adjudication": None})
    counts = Counter(record["status"] for record in records)
    report = {"protocol": _PROTOCOL, "evaluation_id": evaluation_id, "plan_ref": plan_ref,
        "total_cases": len(records), "planned_requests": plan["planned_requests"], "maximum_sends": plan["maximum_sends"],
        "counts": {status: counts[status] for status in ("succeeded", "exhausted", "budget_exceeded")},
        "cases": records, "usage": _usage(rows),
        "new_usage": _usage([row for row in rows if row["request_id"] not in before]),
        "model_execution_completed": counts["succeeded"] + counts["exhausted"] == len(records),
        "human_review_completed": False, "semantic_acceptance": False}
    reference = await asyncio.to_thread(artifacts.put_json, report)
    await store.commit_artifact(lease, "semantic-evaluation-report", reference,
        scope=evaluation_id, input_digest=plan_digest)
    return reference
