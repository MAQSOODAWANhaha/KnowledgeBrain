"""Shared model adapter for document compilation."""

from __future__ import annotations

import asyncio
from copy import deepcopy
import json

from docreader.llm.recovery import call_validated
from docreader.llm.request import request_digest


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
