"""Async JSON model calls with durable audit and no hidden SDK retries."""

from __future__ import annotations

import asyncio
from copy import deepcopy
import hashlib
import json
import math
import time
from types import MappingProxyType
from typing import Mapping, Sequence, get_args
from uuid import uuid4

import httpx
from openai import APIConnectionError, APIStatusError, AsyncOpenAI

from docreader.llm.budget import RequestBudget
from docreader.llm.failures import http_failure
from docreader.llm.config import ModelConfig, ModelEndpoint, completions_url
from docreader.llm.request import (
    ImageInput, ModelConfigurationError, ModelCorrectionStalled, ModelInfrastructureError,
    ModelJSONError, ModelSemanticError,
    UncertainModelResponse, request_digest,
)
from docreader.llm.usage import UsageLedger, reported_usage
from docreader.llm.streaming import ActiveResponseStream, IdleDeadline, StreamResponse, invalid_json_constant
from docreader.tender.schema import ArtifactRef, ModelRequestStatus, Task
from docreader.workflow.state import WORKFLOW_PROTOCOL


class OpenAIModel:
    def __init__(self, config: ModelConfig, store, lease, prompts: Mapping[str, str] | None = None,
                 concurrency: int = 2, timeout_seconds: float = 180,
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
            "workflow_protocol": deepcopy(WORKFLOW_PROTOCOL),
            "chat": endpoint_spec(self.config.chat),
            "vlm": endpoint_spec(self.config.vision),
            "prompts": {task: hashlib.sha256(prompt.encode("utf-8")).hexdigest()
                        for task, prompt in sorted(self._prompt_snapshot().items())},
            "budget": {"chat": self.request_budget().spec(),
                       "vlm": self.request_budget(images=True).spec() if self.config.vision is not None else None},
            "timeout_seconds": self._timeout,
            "timeout_policy": "raw_response_idle_v1",
            "concurrency": self._concurrency,
        }

    def request_budget(self, *, images: bool = False, correction: bool = False) -> RequestBudget:
        endpoint = self.config.for_images() if images else self.config.chat
        # Relay prompt_tokens equal o200k_base counts plus a fixed prompt for the chat model.
        return RequestBudget(context_tokens=endpoint.context_tokens, model=endpoint.model,
                             prompts=self._prompt_snapshot(), correction=correction,
                             tokenizer=None if images else "o200k_base")

    def _client(self, endpoint: ModelEndpoint) -> AsyncOpenAI:
        if endpoint not in self._clients:
            self._clients[endpoint] = AsyncOpenAI(
                api_key=endpoint.api_key, base_url=endpoint.sdk_base_url, max_retries=0,
                timeout=httpx.Timeout(self._timeout, read=None),
                http_client=httpx.AsyncClient(timeout=httpx.Timeout(self._timeout, read=None), transport=self._transport,
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

    async def call(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = (), *, correction: bool = False) -> dict:
        from docreader.llm.wire import WirePayload

        wire = WirePayload(task, payload)
        endpoint = self.config.for_images() if images else self.config.chat
        budget = self.request_budget(images=bool(images), correction=correction)
        request = budget.request(task, payload, images)
        context = budget.require_request(request, task)
        digest = request_digest(endpoint.base_url, request)
        logical_digest = digest
        if "contract_feedback" in payload:
            baseline = {key: value for key, value in payload.items() if key != "contract_feedback"}
            logical_digest = request_digest(endpoint.base_url, budget.request(task, baseline, images))

        async with self._semaphore:
            await self.store.pin_execution_spec(self.lease, self.execution_spec())
            if wire.enabled:
                if self.artifacts is None:
                    raise ModelConfigurationError("short references require a durable artifact store")
                mapping_ref = await asyncio.to_thread(self.artifacts.put_json,
                    {"protocol": wire.VERSION, "payload": payload, "mapping": wire.mapping})
                await self.store.commit_artifact(self.lease, "model-wire", mapping_ref,
                                                 scope=digest, input_digest=digest)
            request_id = uuid4().hex
            row = await self.store.begin_request(self.lease, request_id, endpoint.model, task, metadata={
                **context, "request_digest": digest, "logical_request_digest": logical_digest,
                "operation": payload.get("operation", task), "work_unit_id": payload.get("work_unit_id", ""),
            })
            if row["status"] == "succeeded":
                return wire.decode(await self._cached_response(row, digest))
            # Durable attempt limits and active-send fencing happen before HTTP.
            await self.store.finish_request(request_id, "sent")
            started = time.monotonic()
            stream = StreamResponse()
            deadline = IdleDeadline(self._timeout)
            raw_response = None
            try:
                manager = self._client(endpoint).chat.completions.with_streaming_response.create(**request)
                raw_response = await deadline.wait(manager.__aenter__)
                response = raw_response.http_response
                response.stream = ActiveResponseStream(response.stream, deadline, stream.counts)
                iterator = raw_response.iter_bytes().__aiter__()
                while not stream.done and stream.provider_error is None:
                    try:
                        chunk = await iterator.__anext__()
                    except StopAsyncIteration:
                        break
                    if chunk:
                        # Before SSE decoding: comments, incomplete events, thinking,
                        # and tool arguments all keep the response alive.
                        if "first_data_seconds" not in stream.counts:
                            stream.counts["first_data_seconds"] = round(time.monotonic() - started, 3)
                        stream.feed(chunk)
                if (stream.provider_error is None
                        and response.headers.get("content-type", "").split(";", 1)[0].strip() != "text/event-stream"):
                    stream.failure = "invalid_stream_content_type"
                    raise ModelSemanticError("model endpoint did not return an SSE response")
                result = stream.result()
            except asyncio.CancelledError:
                await asyncio.shield(self._finish_stream(request_id, "uncertain", stream, digest,
                                                         "cancelled_after_send", started))
                raise
            except APIStatusError as error:
                # A received HTTP rejection is distinct from a lost response.
                status = error.status_code
                response_ref, usage = await self._received_rejection(error.response, digest)
                await self._finish(request_id, "failed", usage, f"http_{status}", started, response_ref)
                _, failure = http_failure(status)
                if failure is ModelInfrastructureError:
                    raise ModelInfrastructureError(f"model endpoint returned HTTP {status}") from None
                if failure is ModelConfigurationError:
                    raise ModelConfigurationError(f"model endpoint rejected its configuration (HTTP {status})") from None
                raise ModelSemanticError(f"model endpoint rejected the request (HTTP {status})") from None
            except (APIConnectionError, httpx.TransportError, TimeoutError) as error:
                cause = error.__cause__ if isinstance(error, APIConnectionError) else error
                if isinstance(cause, (httpx.ConnectError, httpx.ConnectTimeout, httpx.PoolTimeout)):
                    await self._finish_stream(request_id, "failed", stream, digest, "connection_before_send", started)
                    raise ModelInfrastructureError("model connection failed before send") from None
                if isinstance(cause, (TimeoutError, httpx.ReadTimeout)):
                    if not stream.counts["raw_received_chunks"]:
                        # Owner-accepted risk: the relay may have billed an execution that sent no byte.
                        await self._finish_stream(request_id, "failed", stream, digest, "first_data_idle_timeout", started)
                        raise ModelInfrastructureError("model endpoint sent no data before the idle deadline") from None
                    failure = "between_data_idle_timeout"
                else:
                    failure = "transport_response_unknown"
                await self._finish_stream(request_id, "uncertain", stream, digest, failure, started)
                raise UncertainModelResponse("model response was lost after send; provider execution and billing are uncertain") from None
            except UncertainModelResponse:
                await self._finish_stream(request_id, "uncertain", stream, digest,
                                          stream.failure or "stream_missing_done", started)
                raise
            except httpx.DecodingError:
                # HTTP decompression can fail after paid response bytes arrived.
                # It is not a complete semantic rejection or permission to resend.
                stream.failure = "invalid_stream_encoding"
                try:
                    await self._finish_stream(request_id, "uncertain", stream, digest, stream.failure, started)
                except Exception:
                    # An unavailable audit store leaves the active sent fence in
                    # place. Recovery cannot send concurrently with that attempt.
                    raise UncertainModelResponse("model response decoding and audit failed; provider execution and billing are uncertain") from None
                raise UncertainModelResponse("model response decoding failed after send; provider execution and billing are uncertain") from None
            except ModelJSONError as error:
                await self._finish_stream(request_id, "failed", stream, digest, "invalid_json_response", started)
                error.redact(endpoint.api_key for endpoint in (self.config.chat, self.config.vision)
                             if endpoint is not None)
                raise
            except ModelSemanticError:
                await self._finish_stream(request_id, "failed", stream, digest,
                                          stream.failure or "invalid_json_response", started)
                raise ModelSemanticError("model endpoint returned an invalid or incomplete JSON stream") from None
            finally:
                if raw_response is not None:
                    await raw_response.close()
            await self._finish_stream(request_id, "succeeded", stream, digest, None, started)
            return wire.decode(result)

    async def _cached_response(self, row: dict, digest: str) -> dict:
        # ArtifactStore verifies size, path and SHA256 before decoding. Recovery
        # receives the cache error and can request a correction with real feedback.
        try:
            if self.artifacts is None:
                raise ValueError
            reference = ArtifactRef.model_validate(row["response_ref"]).model_dump(mode="json")
            envelope = await asyncio.to_thread(self.artifacts.read_json, reference)
            if (envelope["protocol"] != StreamResponse.PROTOCOL or envelope["request_digest"] != digest
                    or envelope["outcome"] != "completed"):
                raise ValueError
            stream = StreamResponse()
            stream.feed(envelope["raw_body"].encode("utf-8"))
            return stream.result()
        except (OSError, ValueError, TypeError, KeyError, AttributeError, UncertainModelResponse):
            raise ModelCorrectionStalled("cached model response cannot be verified; provider was not called") from None

    async def _save_response(self, envelope: dict) -> dict | None:
        if self.artifacts is None:
            return None
        secrets = tuple(endpoint.api_key for endpoint in (self.config.chat, self.config.vision)
                        if endpoint is not None and endpoint.api_key)
        reference = await asyncio.to_thread(self.artifacts.put_json, _redact_response(envelope, secrets))
        return ArtifactRef.model_validate(reference).model_dump(mode="json")

    async def _finish_stream(self, request_id, status, stream, digest, error_type, started):
        response_ref = await self._save_response(stream.audit(digest, error_type or "completed"))
        await self._finish(request_id, status, stream.usage, error_type, started, response_ref,
                           provider_error=stream.provider_error,
                           activity=_first_data_activity(stream.counts))

    async def _received_rejection(self, response: httpx.Response, digest: str) -> tuple[dict | None, dict | None]:
        try:
            body = response.json(parse_constant=invalid_json_constant)
        except ValueError:
            body = {"unparsed_body": response.text}
        usage = reported_usage(body.get("usage")) if isinstance(body, dict) else None
        envelope = {"protocol": "http.rejection.v1", "request_digest": digest, "status_code": response.status_code,
                    "body": body, "usage": usage, "usage_status": "reported" if usage is not None else "unknown"}
        return await self._save_response(envelope), usage

    async def _finish(self, request_id, status: ModelRequestStatus, usage, error_type, started, response_ref=None,
                      *, provider_error=None, activity=None):
        error = {"type": error_type, "elapsed_seconds": round(time.monotonic() - started, 6)} if error_type else None
        if error is not None and provider_error is not None:
            secrets = tuple(endpoint.api_key for endpoint in (self.config.chat, self.config.vision)
                            if endpoint is not None and endpoint.api_key)
            error["provider_error"] = _redact_response(provider_error, secrets)
        await self.store.finish_request(request_id, status, usage=usage, error=error, response_ref=response_ref,
                                        activity=activity)
        self.usage.record(status, usage)


def _first_data_activity(counts) -> dict | None:
    value = counts.get("first_data_seconds") if isinstance(counts, dict) else None
    if type(value) not in {int, float} or isinstance(value, bool) or value < 0:
        return None
    return {"first_data_seconds": value}


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
            for representation in (secret, json.dumps(secret, ensure_ascii=False)[1:-1],
                                   json.dumps(secret, ensure_ascii=True)[1:-1]):
                value = value.replace(representation, "[REDACTED]")
    return value
