"""Independent, resumable document compilation over a fixed available snapshot."""

from __future__ import annotations

import asyncio
from contextlib import suppress
from copy import deepcopy
import hashlib
from importlib.resources import files
import json
import math

import psycopg

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.integrations.authoring import (
    assert_authoring_active,
    read_authoring_input,
)
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Cancelled, Conflict, StaleExecution
from docreader.llm.budget import RequestBudget
from docreader.llm.client import OpenAIModel
from docreader.llm.config import completions_url
from docreader.llm.recovery import call_validated
from docreader.llm.request import (
    ModelConfigurationError,
    ModelSemanticError,
    request_digest,
)
from docreader.llm.usage import reported_usage
from docreader.tender.authoring_execution import _OutlineModel
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.schema import (
    AUTHORING_GROUNDING_POLICY,
    ArtifactRef,
    AuthoringDocument,
    AuthoringInput,
    AuthoringOutline,
    AuthoringReviewRequest,
    AuthoringUsage,
    KnowledgeMaterial,
    SourceUnitRef,
)
from docreader.tender.status import AUTHORING_MODEL_TASKS, AuthoringExecutionStatus


_TASKS = AUTHORING_MODEL_TASKS
# Structure review is a separate task but the same prompt bytes as content review.
_PROMPT_FILES = {
    "outline": "outline.md",
    "authoring_basis": "authoring_basis.md",
    "authoring_write": "authoring_write.md",
    "authoring_review": "authoring_review.md",
    "authoring_structure": "authoring_review.md",
    "outline_assemble": "outline_assemble.md",
}
_PROMPTS = tuple(_PROMPT_FILES)
_PROTOCOL = "tender_document_execution_v1"


def load_prompts():
    package = files("docreader.tender.prompts")
    return {
        task: package.joinpath(filename).read_text(encoding="utf-8")
        for task, filename in _PROMPT_FILES.items()
    }


def _digest(value):
    return hashlib.sha256(
        json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        ).encode()
    ).hexdigest()


def _usage(requests):
    sent = [row for row in requests if row["sent_at"] is not None]
    logical = {(row["task"], row["metadata"]["logical_request_digest"]) for row in sent}
    usage = AuthoringUsage(
        sent_requests=len(sent), retry_requests=len(sent) - len(logical)
    ).model_dump()
    for row in sent:
        estimate = row["metadata"].get("input_tokens")
        if type(estimate) is int and estimate >= 0:
            usage["estimated_input_tokens"] += estimate
        reported = reported_usage(row.get("usage")) or {}
        fields = ("prompt_tokens", "completion_tokens", "total_tokens")
        known = {
            field: reported[field]
            for field in fields
            if type(reported.get(field)) is int
        }
        if len(known) != len(fields):
            usage["unknown_usage_requests"] += 1
        for field in fields:
            usage[field] += known.get(field, 0)
    return usage


class _DocumentModel(_OutlineModel):
    async def __call__(self, request, validate):
        return await call_validated(
            self,
            self.store,
            self.artifacts,
            self.lease,
            self.task,
            {"request": request},
            validate,
            owner_metadata={
                "execution_id": self.lease.execution_id,
                "epoch": self.lease.epoch,
            },
            passthrough_exceptions=(psycopg.Error, OSError, ArtifactIntegrityError),
        )


class _DocumentExecutor:
    def __init__(self, client, store, artifacts, lease, budget, spec, previous):
        self.client, self.store, self.artifacts, self.lease = (
            client,
            store,
            artifacts,
            lease,
        )
        self.budget, self.spec, self.previous = budget, spec, previous
        self._request_locks = {}

    async def __call__(self, task, request, validate):
        if task not in _TASKS:
            raise ModelConfigurationError("unsupported document model task")
        self.budget.require_request(request, task)
        digest = request_digest(self.client.config.chat.base_url, request)
        lock = self._request_locks.setdefault((task, digest), asyncio.Lock())
        async with lock:
            return await self._execute(task, request, validate, digest)

    async def _execute(self, task, request, validate, digest):
        scope = f"{task}:{digest}"
        receipt = await self.store.receipt(
            self.lease, "document-model-response", scope, input_digest=digest
        )
        inherited = False
        if receipt is None and self.previous is not None:
            receipt = next(
                (
                    row
                    for row in self.previous["receipts"]
                    if row["stage"] == "document-model-response"
                    and row["scope"] == scope
                    and row["input_digest"] == digest
                ),
                None,
            )
            inherited = receipt is not None
        if receipt is not None:
            saved = await asyncio.to_thread(
                self.artifacts.read_json, receipt["artifact_ref"]
            )
            if (
                not isinstance(saved, dict)
                or saved.get("task") != task
                or saved.get("input_digest") != digest
            ):
                raise ArtifactIntegrityError(
                    "accepted document response belongs to another request"
                )
            try:
                value = await asyncio.to_thread(validate, deepcopy(saved["response"]))
            except ModelSemanticError:
                if not inherited:
                    raise ArtifactIntegrityError(
                        "accepted document response no longer matches its pinned contract"
                    ) from None
            else:
                if inherited:
                    await self.store.commit_artifact(
                        self.lease,
                        "document-model-response",
                        receipt["artifact_ref"],
                        scope=scope,
                        input_digest=digest,
                        metadata={
                            "reused_from_execution_id": self.previous["execution_id"]
                        },
                    )
                return value

        accepted = None

        def receive(response):
            nonlocal accepted
            original = deepcopy(response)
            value = validate(response)
            accepted = original
            return value

        model = _DocumentModel(
            self.client,
            self.store,
            self.artifacts,
            self.lease,
            self.budget,
            self.spec,
            task,
        )
        value = await model(request, receive)
        reference = await asyncio.to_thread(
            self.artifacts.put_json,
            {"task": task, "input_digest": digest, "response": accepted},
        )
        await self.store.commit_artifact(
            self.lease,
            "document-model-response",
            reference,
            scope=scope,
            input_digest=digest,
        )
        return value


class _FrozenKnowledge:
    def __init__(self, provider, spec, store, artifacts, lease):
        self.provider, self.spec = provider, spec
        self.store, self.artifacts, self.lease = store, artifacts, lease

    async def search(self, queries, artifacts):
        grouped, materials, issues = {}, {}, []
        original_queries = {
            entry.get("query")
            for entry in queries
            if entry.get("include_original") is True
        }
        for entry in queries:
            target, query = entry.get("target_id"), entry.get("query")
            if not isinstance(target, str) or not target:
                raise ValueError("knowledge queries require a host target_id")
            if not isinstance(query, str) or not query.strip():
                issues.append(
                    {
                        "code": "knowledge_query_missing",
                        "detail": "未提供检索内容，使用现有资料继续编制。",
                        "target_id": target,
                    }
                )
                continue
            targets = grouped.setdefault(query, [])
            if target not in targets:
                targets.append(target)
        for query, targets in grouped.items():
            value = await self._query(
                query, artifacts, include_original=query in original_queries
            )
            issues.extend(
                {**issue, "target_id": target}
                for issue in value["issues"]
                for target in targets
            )
            for material in value["materials"]:
                material = deepcopy(material)
                locator = material.setdefault("locator", {})
                locator["target_ids"] = targets.copy()
                identity = material["material_id"]
                if identity not in materials:
                    materials[identity] = material
                    continue
                if (
                    material.get("original_ref") is not None
                    and materials[identity].get("original_ref") is None
                ):
                    materials[identity]["original_ref"] = material["original_ref"]
                    materials[identity]["attachment"] = material.get(
                        "attachment", False
                    )
                    for field in ("file_name", "object_ref"):
                        if field in locator:
                            materials[identity]["locator"][field] = locator[field]
                existing = materials[identity]["locator"]
                existing["target_ids"] = list(
                    dict.fromkeys([*existing["target_ids"], *targets])
                )
                for search in locator.get("searches", []):
                    if search not in existing.setdefault("searches", []):
                        existing["searches"].append(search)
        return {"materials": list(materials.values()), "issues": issues}

    async def _query(self, query, artifacts, *, include_original=False):
        digest = _digest(
            {
                "query": query,
                "provider": self.spec,
                "include_original": include_original,
            }
        )
        receipt = await self.store.receipt(
            self.lease, "knowledge-snapshot", digest, input_digest=digest
        )
        if receipt is not None:
            value = await asyncio.to_thread(
                self.artifacts.read_json, receipt["artifact_ref"]
            )
            if (
                not isinstance(value, dict)
                or value.get("query") != query
                or value.get("provider") != self.spec
                or value.get("include_original", False) != include_original
            ):
                raise ArtifactIntegrityError(
                    "knowledge receipt belongs to another query or retrieval scope"
                )
            return value
        # A single query has one fixed local target. Real target relationships
        # belong to the current outline and never enter the reusable receipt.
        request = {"query": query, "target_id": "query"}
        if include_original:
            request["include_original"] = True
        result = await self.provider.search([request], artifacts)
        value = {
            "query": query,
            "provider": self.spec,
            "include_original": include_original,
            "materials": deepcopy(result["materials"]),
            "issues": [
                {key: item for key, item in issue.items() if key != "target_id"}
                for issue in result["issues"]
            ],
        }
        value["issues"] = list(
            {_digest(issue): issue for issue in value["issues"]}.values()
        )
        for material in value["materials"]:
            material.setdefault("locator", {}).pop("target_ids", None)
        reference = await asyncio.to_thread(self.artifacts.put_json, value)
        await self.store.commit_artifact(
            self.lease,
            "knowledge-snapshot",
            reference,
            scope=digest,
            input_digest=digest,
        )
        return value


async def execute_document(
    analysis_store,
    artifacts,
    config,
    budget,
    *,
    execution_id: str,
    run_id: str,
    source_version: int,
    domain_revision: int,
    checklist_digest=None,
    materials=(),
    knowledge=None,
    previous_execution_id=None,
    transport=None,
    review_request=None,
    submission_sources=(),
    outline_only: bool = False,
    outline_ref=None,
    authoring_input_ref=None,
    concurrency: int = 2,
    lease_seconds: float = 60,
    heartbeat_seconds: float = 20,
    timeout_seconds: float = 180,
) -> dict:
    """Run or resume compilation without changing the analysis run or its lease.

    Exact accepted model responses may be inherited from an explicit predecessor;
    their IDs are rebound by the current validator, and no paid-call row is forged.
    Local exhausted model units are handled by the pipeline; storage failures,
    cancellation and loss of ownership remain recoverable execution failures.
    """
    if (
        not isinstance(execution_id, str)
        or not execution_id.strip()
        or type(outline_only) is not bool
        or type(concurrency) is not int
        or concurrency < 1
        or any(
            type(value) not in {int, float} or not math.isfinite(value) or value <= 0
            for value in (lease_seconds, heartbeat_seconds, timeout_seconds)
        )
        or heartbeat_seconds >= lease_seconds
    ):
        raise ModelConfigurationError(
            "document execution requires stable ID and valid concurrency/lease/timeout"
        )
    if (
        budget.model != config.chat.model
        or budget.context_tokens != config.chat.context_tokens
        or budget.output_reserve <= 0
    ):
        raise ModelConfigurationError(
            "document requires its configured Chat model, window and positive output reserve"
        )
    if previous_execution_id == execution_id:
        raise Conflict("a document execution cannot inherit from itself")
    if review_request is not None:
        review_request = AuthoringReviewRequest.model_validate(
            review_request
        ).model_dump()
    submission_sources = [
        SourceUnitRef.model_validate(raw).model_dump() for raw in submission_sources
    ]
    submission_sources = list(
        {_digest(ref): ref for ref in submission_sources}.values()
    )

    identity = {
        "run_id": run_id,
        "source_version": source_version,
        "domain_revision": domain_revision,
        "checklist_digest": checklist_digest,
    }
    pinned_input = None
    if authoring_input_ref is not None:
        authoring_input_ref = ArtifactRef.model_validate(
            authoring_input_ref
        ).model_dump()
        pinned_input = AuthoringInput.model_validate(
            await asyncio.to_thread(artifacts.read_json, authoring_input_ref)
        ).model_dump()
        if (
            any(
                pinned_input[key] != identity[key]
                for key in ("run_id", "source_version", "domain_revision")
            )
            or checklist_digest is not None
            and pinned_input["checklist_digest"] != checklist_digest
        ):
            raise Conflict("queued input belongs to another analysis snapshot")
    outline = None
    if outline_ref is not None:
        outline_ref = ArtifactRef.model_validate(outline_ref).model_dump()
        outline = AuthoringOutline.model_validate(
            await asyncio.to_thread(artifacts.read_json, outline_ref)
        ).model_dump(exclude_unset=True)
        fixed = AuthoringInput.model_validate(outline["authoring_input"]).model_dump()
        if (
            any(
                fixed[key] != identity[key]
                for key in ("run_id", "source_version", "domain_revision")
            )
            or checklist_digest is not None
            and fixed["checklist_digest"] != checklist_digest
        ):
            raise Conflict("fixed outline belongs to another authoring input snapshot")
        outline["authoring_input"] = fixed
    store = AuthoringStore(analysis_store.dsn)
    await store.setup()
    existing = await store.get_execution(execution_id, missing_ok=True)
    if existing is None and outline is not None:
        snapshot = outline["authoring_input"]
    elif existing is None and pinned_input is not None:
        snapshot = pinned_input
    elif existing is None:
        snapshot = await read_authoring_input(analysis_store, artifacts, **identity)
    else:
        snapshot = AuthoringInput.model_validate(
            existing["authoring_input"]
        ).model_dump()
        if (
            any(
                snapshot[key] != identity[key]
                for key in ("run_id", "source_version", "domain_revision")
            )
            or checklist_digest is not None
            and snapshot["checklist_digest"] != checklist_digest
        ):
            raise Conflict("execution_id already identifies a different document input")
    if outline is not None and outline["authoring_input"] != snapshot:
        raise Conflict("fixed outline differs from the execution input snapshot")
    if pinned_input is not None and pinned_input != snapshot:
        raise Conflict("queued input differs from the execution input snapshot")
    await assert_authoring_active(analysis_store, snapshot)

    material_snapshot = [
        KnowledgeMaterial.model_validate(value).model_dump() for value in materials
    ]
    prompts = load_prompts()
    admitted = RequestBudget(
        context_tokens=budget.context_tokens,
        model=budget.model,
        prompts=prompts,
        output_reserve=budget.output_reserve,
        correction=budget.limits.correction,
        tokenizer=budget.tokenizer,
        tokenized_outline_assembly=budget.tokenized_outline_assembly,
        tokenized_authoring=budget.tokenized_authoring,
    )
    knowledge_spec = None
    if knowledge is not None:
        knowledge_spec = (
            knowledge.spec()
            if callable(getattr(knowledge, "spec", None))
            else {
                "provider": type(knowledge).__module__
                + "."
                + type(knowledge).__qualname__
            }
        )
    spec = {
        "protocol": _PROTOCOL,
        "grounding_policy": AUTHORING_GROUNDING_POLICY,
        "endpoint": completions_url(config.chat.base_url),
        "model": config.chat.model,
        "context_tokens": config.chat.context_tokens,
        "prompts": {
            task: hashlib.sha256(prompt.encode()).hexdigest()
            for task, prompt in prompts.items()
        },
        "budget": admitted.spec(),
        "materials_digest": _digest(material_snapshot),
        "knowledge": knowledge_spec,
        "previous_execution_id": previous_execution_id,
        "concurrency": concurrency,
        "timeout_seconds": float(timeout_seconds),
        "timeout_policy": "raw_response_idle_v1",
    }
    # Keep legacy default execution identities unchanged. Non-default planning
    # and fixed-outline inputs are immutable parts of the new execution spec.
    if outline_only or outline_ref is not None:
        spec.update(outline_only=outline_only, outline_ref=outline_ref)
    if authoring_input_ref is not None:
        spec["authoring_input_ref"] = authoring_input_ref
    if review_request is not None:
        spec["review_request"] = review_request
    if submission_sources:
        spec["submission_sources"] = submission_sources
    previous = None
    if previous_execution_id is not None:
        previous = await store.get_execution(previous_execution_id)
        if (
            previous["authoring_input"]["run_id"] != run_id
            or previous["execution_spec"].get("protocol") != _PROTOCOL
            or previous["execution_spec"].get("outline_only", False) != outline_only
        ):
            raise Conflict(
                "document predecessor must belong to the same analysis run, protocol and operation"
            )
    await store.submit(execution_id, snapshot, spec)
    lease = await store.acquire(execution_id, lease_seconds=lease_seconds)
    if lease is None:
        saved = await store.get_execution(execution_id)
        if saved["cancel_requested"]:
            raise Cancelled("document execution was cancelled")
        if saved["status"] != AuthoringExecutionStatus.SUCCEEDED:
            raise Conflict("document execution already has an active owner")
        value = await asyncio.to_thread(artifacts.read_json, saved["result_ref"])
        contract = AuthoringOutline if outline_only else AuthoringDocument
        if contract.model_validate(value).authoring_input.model_dump() != snapshot:
            raise ArtifactIntegrityError(
                "stored document does not refer to its fixed input snapshot"
            )
        await assert_authoring_active(analysis_store, snapshot)
        return saved["result_ref"]

    async def invoke():
        material_ref = await asyncio.to_thread(artifacts.put_json, material_snapshot)
        await store.commit_artifact(
            lease,
            "document-materials",
            material_ref,
            input_digest=spec["materials_digest"],
        )
        frozen_knowledge = (
            _FrozenKnowledge(knowledge, knowledge_spec, store, artifacts, lease)
            if knowledge is not None
            else None
        )
        async with OpenAIModel(
            config,
            store,
            lease,
            prompts={},
            concurrency=concurrency,
            timeout_seconds=timeout_seconds,
            artifacts=artifacts,
            transport=transport,
        ) as client:
            executor = _DocumentExecutor(
                client, store, artifacts, lease, admitted, spec, previous
            )
            review_options = (
                {"review_request": review_request} if review_request is not None else {}
            )
            if submission_sources:
                review_options["submission_sources"] = submission_sources
            if outline_only:
                review_options["outline_only"] = True
            if outline is not None:
                review_options["outline"] = outline

            async def publish_outline(reference):
                await store.commit_artifact(
                    lease, "document-outline", reference, input_digest=_digest(snapshot)
                )

            reference = await generate_document(
                artifacts,
                snapshot,
                admitted,
                executor,
                materials=material_snapshot,
                knowledge=frozen_knowledge,
                concurrency=concurrency,
                on_outline=publish_outline,
                **review_options,
            )
            value = await asyncio.to_thread(artifacts.read_json, reference)
            if outline_only:
                value = AuthoringOutline.model_validate(value).model_dump(
                    exclude_unset=True
                )
                if value["authoring_input"] != snapshot:
                    raise ArtifactIntegrityError(
                        "generated outline changed the fixed input snapshot"
                    )
                return await asyncio.to_thread(artifacts.put_json, value)
            execution = await store.get_execution(execution_id)
            value["usage"] = _usage(execution["requests"])
            value = AuthoringDocument.model_validate(value).model_dump()
            if value["authoring_input"] != snapshot:
                raise ArtifactIntegrityError(
                    "generated document changed the fixed input snapshot"
                )
            return await asyncio.to_thread(artifacts.put_json, value)

    async def heartbeat():
        while True:
            await asyncio.sleep(heartbeat_seconds)
            await assert_authoring_active(analysis_store, snapshot)
            await store.heartbeat(lease, lease_seconds=lease_seconds)

    work, heart = asyncio.create_task(invoke()), asyncio.create_task(heartbeat())
    try:
        done, _ = await asyncio.wait({work, heart}, return_when=asyncio.FIRST_COMPLETED)
        if heart in done:
            await heart
        result = await work
        heart.cancel()
        (heartbeat_result,) = await asyncio.gather(heart, return_exceptions=True)
        if isinstance(heartbeat_result, BaseException) and not isinstance(
            heartbeat_result, asyncio.CancelledError
        ):
            raise heartbeat_result
        await store.release(
            lease, AuthoringExecutionStatus.SUCCEEDED, result_ref=result
        )
        return result
    except BaseException as error:
        work.cancel()
        with suppress(asyncio.CancelledError, Exception):
            await work
        with suppress(Cancelled, StaleExecution, psycopg.Error):
            await asyncio.shield(
                store.release(
                    lease,
                    AuthoringExecutionStatus.FAILED,
                    error={"type": type(error).__name__},
                )
            )
        raise
    finally:
        for task in (work, heart):
            task.cancel()
        for task in (work, heart):
            with suppress(asyncio.CancelledError, Exception):
                await task
