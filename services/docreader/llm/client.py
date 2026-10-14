"""Async JSON model calls with durable audit and no hidden SDK retries."""

from __future__ import annotations

import asyncio
import hashlib
import json
import math
import time
from types import MappingProxyType
from typing import Mapping, Sequence, get_args
from uuid import uuid4

import httpx
from openai import APIConnectionError, APIResponseValidationError, APIStatusError, AsyncOpenAI

from docreader.llm.budget import RequestBudget
from docreader.llm.config import ModelConfig, ModelEndpoint, completions_url
from docreader.llm.request import (
    ImageInput, ModelConfigurationError, ModelInfrastructureError, ModelSemanticError,
)
from docreader.llm.usage import UsageLedger, reported_usage
from docreader.tender.schema import ArtifactRef, ModelRequestStatus, Task


class OpenAIModel:
    def __init__(self, config: ModelConfig, store, lease, prompts: Mapping[str, str] | None = None,
                 concurrency: int = 4, timeout_seconds: float = 120,
                 *, artifacts=None, transport: httpx.AsyncBaseTransport | None = None):
        if type(concurrency) is not int or concurrency < 1 or not math.isfinite(timeout_seconds) or timeout_seconds <= 0:
            raise ModelConfigurationError("model concurrency and timeout must be positive")
        self.config, self.store, self.lease = config, store, lease
        self.artifacts = artifacts
        self._prompts = MappingProxyType(dict(prompts)) if prompts is not None else None
        self.usage = UsageLedger()
        self._semaphore = asyncio.Semaphore(concurrency)
        self._timeout, self._transport, self._concurrency = float(timeout_seconds), transport, concurrency
        self._clients: dict[ModelEndpoint, AsyncOpenAI] = {}

    def _prompt_snapshot(self) -> Mapping[str, str]:
        if self._prompts is None:
            from docreader.tender.prompts import get_prompt
            self._prompts = MappingProxyType({task: get_prompt(task) for task in get_args(Task)})
        return self._prompts

    def execution_spec(self) -> dict:
        """Fresh JSON of the exact non-secret settings used by this instance.

        Prompt contents are frozen once and hashed from those same strings used
        in requests. Credentials and credential hashes are deliberately absent,
        so rotating a key does not change the revision's execution identity.
        """
        def endpoint_spec(endpoint):
            if endpoint is None:
                return None
            return {"endpoint": completions_url(endpoint.base_url), "model": endpoint.model,
                    "context_tokens": endpoint.context_tokens}

        return {
            "chat": endpoint_spec(self.config.chat),
            "vlm": endpoint_spec(self.config.vision),
            "prompts": {task: hashlib.sha256(prompt.encode("utf-8")).hexdigest()
                        for task, prompt in sorted(self._prompt_snapshot().items())},
            "budget": {"chat": self.request_budget().spec(),
                       "vlm": self.request_budget(images=True).spec() if self.config.vision is not None else None},
            "timeout_seconds": self._timeout,
            "concurrency": self._concurrency,
        }

    def request_budget(self, *, images: bool = False) -> RequestBudget:
        endpoint = self.config.for_images() if images else self.config.chat
        return RequestBudget(context_tokens=endpoint.context_tokens, model=endpoint.model,
                             prompts=self._prompt_snapshot())

    def _client(self, endpoint: ModelEndpoint) -> AsyncOpenAI:
        if endpoint not in self._clients:
            self._clients[endpoint] = AsyncOpenAI(
                api_key=endpoint.api_key, base_url=endpoint.sdk_base_url, max_retries=0,
                timeout=self._timeout,
                http_client=httpx.AsyncClient(timeout=self._timeout, transport=self._transport,
                                              follow_redirects=False),
            )
        return self._clients[endpoint]

    async def close(self) -> None:
        for client in self._clients.values():
            await client.close()
        self._clients.clear()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        await self.close()

    async def call(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> dict:
        endpoint = self.config.for_images() if images else self.config.chat
        budget = self.request_budget(images=bool(images))
        request = budget.request(task, payload, images)
        context = budget.require_request(request)
        serialized = request["messages"][1]["content"][0]["text"]

        async with self._semaphore:
            # This is deliberately durable on every call, including direct use
            # outside graph nodes. A mismatch must fail before any request audit
            # is marked sent or provider credentials/document content leave here.
            await self.store.pin_execution_spec(self.lease, self.execution_spec())
            request_id = uuid4().hex
            await self.store.begin_request(self.lease, request_id, endpoint.model, task, metadata={
                **context, "payload_sha256": hashlib.sha256(serialized.encode()).hexdigest(),
            })
            # Persistence failure here prevents the external call entirely.
            await self.store.finish_request(request_id, "sent")
            started = time.monotonic()
            try:
                raw_response = await self._client(endpoint).chat.completions.with_raw_response.create(**request)
            except asyncio.CancelledError:
                await asyncio.shield(self._finish(request_id, "uncertain", None, "cancelled_after_send", started))
                raise
            except (APIConnectionError, httpx.TransportError, TimeoutError):
                await self._finish(request_id, "uncertain", None, "transport_response_unknown", started)
                raise ModelInfrastructureError("model transport failed after send; provider execution and billing are uncertain") from None
            except APIStatusError as error:
                status = error.status_code
                retryable = status in {408, 409, 425, 429} or status >= 500
                uncertain = status == 408 or status >= 500
                response_ref, usage = await self._received_response(error.response)
                await self._finish(request_id, "uncertain" if uncertain else "failed", usage,
                                   f"http_{status}", started, response_ref)
                if retryable:
                    raise ModelInfrastructureError(f"model endpoint returned HTTP {status}") from None
                if status in {401, 403, 404} or 300 <= status < 400:
                    raise ModelConfigurationError(f"model endpoint rejected its configuration (HTTP {status})") from None
                raise ModelSemanticError(f"model endpoint rejected the request (HTTP {status})") from None

            # Save the received envelope once before interpreting model content.
            # Parse failures retain the original response and any real usage;
            # transport loss has no response reference. Artifact failures remain
            # technical failures, not falsely classified as document/model JSON.
            response_ref, usage = await self._received_response(raw_response.http_response)
            try:
                response = raw_response.parse()
            except (APIResponseValidationError, json.JSONDecodeError):
                await self._finish(request_id, "failed", usage, "invalid_response_envelope", started, response_ref)
                raise ModelSemanticError("model endpoint returned an invalid response envelope") from None
            try:
                if not response.choices:
                    raise ModelSemanticError("model response has no choice")
                choice = response.choices[0]
                if choice.message.refusal or choice.finish_reason not in {"stop", None}:
                    raise ModelSemanticError("model refused or did not finish its JSON response")
                result = json.loads(choice.message.content or "", parse_constant=_invalid_json_constant)
                if not isinstance(result, dict):
                    raise ModelSemanticError("model JSON response must be an object")
            except (ValueError, AttributeError, TypeError):
                await self._finish(request_id, "failed", usage, "invalid_json_response", started, response_ref)
                raise ModelSemanticError("model response is not a complete JSON object") from None
            await self._finish(request_id, "succeeded", usage, None, started, response_ref)
            return result

    async def _received_response(self, response: httpx.Response) -> tuple[dict | None, dict | None]:
        try:
            envelope = response.json(parse_constant=_invalid_json_constant)
        except ValueError:
            envelope = {"unparsed_body": response.text}
        usage = reported_usage(envelope.get("usage")) if isinstance(envelope, dict) else None
        if self.artifacts is None:
            return None, usage
        secrets = tuple(endpoint.api_key for endpoint in (self.config.chat, self.config.vision)
                        if endpoint is not None and endpoint.api_key)
        envelope = _redact_response(envelope, secrets)
        reference = await asyncio.to_thread(self.artifacts.put_json, envelope)
        return ArtifactRef.model_validate(reference).model_dump(mode="json"), usage

    async def _finish(self, request_id, status: ModelRequestStatus, usage, error_type, started, response_ref=None):
        error = {"type": error_type, "elapsed_seconds": round(time.monotonic() - started, 6)} if error_type else None
        await self.store.finish_request(request_id, status, usage=usage, error=error, response_ref=response_ref)
        self.usage.record(status, usage)


def _invalid_json_constant(value):
    raise ModelSemanticError("model JSON contains a non-finite number")


def _redact_response(value, secrets):
    """No headers/credentials are retained, including provider credential echoes."""
    if isinstance(value, dict):
        forbidden = {"api_key", "apikey", "authorization", "password", "secret", "access_token",
                     "refresh_token", "cookie", "set_cookie"}
        return {key: _redact_response(item, secrets) for key, item in value.items()
                if key.casefold().replace("-", "_") not in forbidden}
    if isinstance(value, list):
        return [_redact_response(item, secrets) for item in value]
    if isinstance(value, str):
        for secret in secrets:
            value = value.replace(secret, "[REDACTED]")
    return value
