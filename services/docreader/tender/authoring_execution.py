"""One independently fenced outline execution, resumed by its stable host ID."""

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
from docreader.integrations.authoring import assert_authoring_active, read_authoring_input
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Cancelled, Conflict, StaleExecution
from docreader.llm.budget import RequestBudget
from docreader.llm.client import OpenAIModel
from docreader.llm.config import completions_url
from docreader.llm.recovery import call_validated
from docreader.llm.request import ModelBudgetError, ModelConfigurationError, ModelWorkUnitExhausted, request_digest
from docreader.tender.authoring import organize_outline
from docreader.tender.schema import AuthoringInput, AuthoringOutline
from docreader.tender.status import AuthoringExecutionStatus


class _OutlineModel:
    def __init__(self, client, store, artifacts, lease, budget, spec, task="outline"):
        self.client, self.store, self.artifacts, self.lease = client, store, artifacts, lease
        self.budget, self.spec = budget, spec
        self.task = task

    async def call(self, task, payload):
        baseline = payload["request"]
        request = deepcopy(baseline)
        if "contract_feedback" in payload:
            # Feedback belongs to the semantic JSON, never Chat API parameters.
            content = request["messages"][1]["content"][0]
            value = json.loads(content["text"])
            value["contract_feedback"] = payload["contract_feedback"]
            content["text"] = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
        self.budget.require_request(request, task)
        digest = request_digest(self.client.config.chat.base_url, request)
        logical = request_digest(self.client.config.chat.base_url, baseline)
        ref = await asyncio.to_thread(self.artifacts.put_json, request)
        await self.store.commit_artifact(self.lease, "model-request", ref, scope=digest, input_digest=logical)
        return await self.client.call_request(request, self.budget, task=task, execution_spec=self.spec,
                                              logical_request=baseline)

    async def __call__(self, request, validate):
        return await call_validated(self, self.store, self.artifacts, self.lease, self.task,
                                    {"request": request}, validate,
                                    owner_metadata={"execution_id": self.lease.execution_id, "epoch": self.lease.epoch})


async def execute_outline(analysis_store, artifacts, config, budget, *, execution_id: str,
                          run_id: str, source_version: int, domain_revision: int, checklist_digest: str | None = None,
                          transport=None, lease_seconds: float = 60, heartbeat_seconds: float = 20,
                          timeout_seconds: float = 180) -> dict:
    """Run or resume T2 without acquiring or updating the parent analysis lease.

    Re-enter with the same execution_id after an interrupted caller. This entry
    point does not enqueue work or install a background recovery dispatcher.
    """
    if (not isinstance(execution_id, str) or not execution_id.strip()
            or any(type(value) not in {int, float} or not math.isfinite(value) or value <= 0
                   for value in (lease_seconds, heartbeat_seconds, timeout_seconds))
            or heartbeat_seconds >= lease_seconds):
        raise ModelConfigurationError("outline execution requires a stable ID and valid lease/heartbeat/timeout")
    if (budget.model != config.chat.model or budget.context_tokens != config.chat.context_tokens
            or budget.output_reserve <= 0):
        raise ModelConfigurationError("outline requires its configured Chat model, window and positive output reserve")
    identity = {"run_id": run_id, "source_version": source_version, "domain_revision": domain_revision,
                "checklist_digest": checklist_digest}
    store = AuthoringStore(analysis_store.dsn)
    await store.setup()
    existing = await store.get_execution(execution_id, missing_ok=True)
    if existing is None:
        approved = await read_authoring_input(analysis_store, artifacts, **identity)
    else:
        approved = AuthoringInput.model_validate(existing["authoring_input"]).model_dump()
        if any(approved[key] != identity[key] for key in ("run_id", "source_version", "domain_revision")):
            raise Conflict("execution_id already identifies a different authoring input")
    await assert_authoring_active(analysis_store, approved)
    prompt = files("docreader.tender.prompts").joinpath("outline.md").read_text(encoding="utf-8")
    # Snapshot only the settings used here; analysis prompts and verify tokenizer
    # cannot alter this execution's identity or expand its input admission.
    admitted = RequestBudget(context_tokens=budget.context_tokens, model=budget.model,
                             prompts={"outline": prompt}, output_reserve=budget.output_reserve,
                             correction=budget.limits.correction)
    spec = {"protocol": "tender_outline_execution_v1", "endpoint": completions_url(config.chat.base_url),
            "model": config.chat.model, "context_tokens": config.chat.context_tokens,
            "prompt_digest": hashlib.sha256(prompt.encode()).hexdigest(), "budget": admitted.spec(),
            "timeout_seconds": float(timeout_seconds), "timeout_policy": "raw_response_idle_v1"}
    await store.submit(execution_id, approved, spec)
    lease = await store.acquire(execution_id, lease_seconds=lease_seconds)
    if lease is None:
        previous = await store.get_execution(execution_id)
        if previous["cancel_requested"]:
            raise Cancelled("outline execution was cancelled")
        if previous["status"] != AuthoringExecutionStatus.SUCCEEDED:
            raise Conflict("outline execution already has an active owner")
        value = await asyncio.to_thread(artifacts.read_json, previous["result_ref"])
        if AuthoringOutline.model_validate(value).authoring_input.model_dump() != approved:
            raise ArtifactIntegrityError("stored outline does not refer to its fixed analysis snapshot")
        await assert_authoring_active(analysis_store, approved)
        return previous["result_ref"]

    async def invoke():
        async with OpenAIModel(config, store, lease, prompts={}, concurrency=1, timeout_seconds=timeout_seconds,
                               artifacts=artifacts, transport=transport) as client:
            executor = _OutlineModel(client, store, artifacts, lease, admitted, spec)
            return await organize_outline(analysis_store, artifacts, executor, admitted,
                                          **identity, authoring_input=approved)

    async def heartbeat():
        while True:
            await asyncio.sleep(heartbeat_seconds)
            await assert_authoring_active(analysis_store, approved)
            await store.heartbeat(lease, lease_seconds=lease_seconds)

    work, heart = asyncio.create_task(invoke()), asyncio.create_task(heartbeat())
    try:
        done, _ = await asyncio.wait({work, heart}, return_when=asyncio.FIRST_COMPLETED)
        if heart in done:
            await heart
        result = await work
        # Keep renewing through client cleanup, then drain the heartbeat before
        # publication. gather preserves cancellation of the calling task.
        heart.cancel()
        heartbeat_result, = await asyncio.gather(heart, return_exceptions=True)
        if isinstance(heartbeat_result, BaseException) and not isinstance(heartbeat_result, asyncio.CancelledError):
            raise heartbeat_result
        # The existing transaction checks cancellation and the authoring
        # fence; no background heartbeat can mistake this success for a lost lease.
        await store.release(lease, AuthoringExecutionStatus.SUCCEEDED, result_ref=result)
        return result
    except BaseException as error:
        failure = (error.__cause__ if isinstance(error, ModelWorkUnitExhausted)
                   and isinstance(error.__cause__, ModelBudgetError) else error)
        # Stop the stream before releasing ownership; cancellation is audited by
        # the shared client as uncertain after send. Caller can resume the ID.
        work.cancel()
        with suppress(asyncio.CancelledError, Exception):
            await work
        with suppress(StaleExecution, psycopg.Error):
            await asyncio.shield(store.release(lease, AuthoringExecutionStatus.FAILED, error={"type": type(failure).__name__}))
        if failure is not error:
            raise failure from error
        raise
    finally:
        for task in (work, heart):
            task.cancel()
        for task in (work, heart):
            with suppress(asyncio.CancelledError, Exception):
                await task
