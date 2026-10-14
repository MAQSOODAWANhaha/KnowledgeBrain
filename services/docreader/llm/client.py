"""Async JSON model calls with durable audit and no hidden SDK retries."""

from __future__ import annotations

import asyncio
import hashlib
import json
import time
from typing import Mapping, Sequence
from uuid import uuid4

import httpx
from openai import APIConnectionError, APIResponseValidationError, APIStatusError, AsyncOpenAI

from docreader.llm.budget import check_context
from docreader.llm.config import ModelConfig, ModelEndpoint
from docreader.llm.request import (
    ImageInput, ModelConfigurationError, ModelInfrastructureError, ModelSemanticError, image_data_url,
)
from docreader.llm.usage import UsageLedger, reported_usage


class OpenAIModel:
    def __init__(self, config: ModelConfig, store, lease, prompts: Mapping[str, str] | None = None,
                 concurrency: int = 4, timeout_seconds: float = 120,
                 *, transport: httpx.AsyncBaseTransport | None = None):
        if concurrency < 1 or timeout_seconds <= 0:
            raise ModelConfigurationError("model concurrency and timeout must be positive")
        self.config, self.store, self.lease, self.prompts = config, store, lease, prompts
        self.usage = UsageLedger()
        self._semaphore = asyncio.Semaphore(concurrency)
        self._timeout, self._transport = timeout_seconds, transport
        self._clients: dict[ModelEndpoint, AsyncOpenAI] = {}

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

    async def call(self, task: str, payload: dict, images: Sequence[str | ImageInput] = ()) -> dict:
        if self.prompts is None:
            from docreader.tender.prompts import get_prompt
            system_prompt = get_prompt(task)
        else:
            try:
                system_prompt = self.prompts[task]
            except KeyError:
                raise ModelConfigurationError("model task has no configured prompt") from None
        inline_images = [image_data_url(image) for image in images]
        endpoint = self.config.for_images() if inline_images else self.config.chat
        try:
            serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        except (TypeError, ValueError):
            raise ModelSemanticError("model payload is not JSON serializable") from None
        context = check_context(system_prompt, serialized, len(inline_images), endpoint.max_context_tokens)
        content = [{"type": "text", "text": serialized}]
        content.extend({"type": "image_url", "image_url": {"url": image}} for image in inline_images)
        messages = [{"role": "system", "content": system_prompt}, {"role": "user", "content": content}]

        async with self._semaphore:
            request_id = uuid4().hex
            await self.store.begin_request(self.lease, request_id, endpoint.model, task, metadata={
                **context.metadata(), "payload_sha256": hashlib.sha256(serialized.encode()).hexdigest(),
            })
            # Persistence failure here prevents the external call entirely.
            await self.store.finish_request(request_id, "sent")
            started = time.monotonic()
            try:
                response = await self._client(endpoint).chat.completions.create(
                    model=endpoint.model, messages=messages, response_format={"type": "json_object"},
                )
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
                await self._finish(request_id, "uncertain" if uncertain else "failed", None,
                                   f"http_{status}", started)
                if retryable:
                    raise ModelInfrastructureError(f"model endpoint returned HTTP {status}") from None
                if status in {401, 403, 404} or 300 <= status < 400:
                    raise ModelConfigurationError(f"model endpoint rejected its configuration (HTTP {status})") from None
                raise ModelSemanticError(f"model endpoint rejected the request (HTTP {status})") from None
            except (APIResponseValidationError, json.JSONDecodeError):
                await self._finish(request_id, "failed", None, "invalid_response_envelope", started)
                raise ModelSemanticError("model endpoint returned an invalid response envelope") from None

            usage = reported_usage(getattr(response, "usage", None))
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
                await self._finish(request_id, "failed", usage, "invalid_json_response", started)
                raise ModelSemanticError("model response is not a complete JSON object") from None
            await self._finish(request_id, "succeeded", usage, None, started)
            return result

    async def _finish(self, request_id, status, usage, error_type, started):
        error = {"type": error_type, "elapsed_seconds": round(time.monotonic() - started, 6)} if error_type else None
        await self.store.finish_request(request_id, status, usage=usage, error=error)
        self.usage.record(status, usage)


def _invalid_json_constant(value):
    raise ModelSemanticError("model JSON contains a non-finite number")
