"""SSE decoding after raw-response activity, independent of business deltas."""

import asyncio
import json
import re
from time import monotonic

import httpx

from docreader.llm.request import ModelJSONError, ModelSemanticError, UncertainModelResponse
from docreader.llm.usage import reported_usage


class IdleDeadline:
    """Headers and first body share one deadline; only received bytes reset it."""

    def __init__(self, seconds: float):
        self.seconds = seconds
        self.deadline = monotonic() + seconds

    async def wait(self, operation):
        remaining = self.deadline - monotonic()
        if remaining <= 0:
            raise TimeoutError
        result = await asyncio.wait_for(operation(), remaining)
        if monotonic() >= self.deadline:
            raise TimeoutError
        return result

    def received(self):
        self.deadline = monotonic() + self.seconds


class ActiveResponseStream(httpx.AsyncByteStream):
    """Observe wire chunks before HTTP decompression, line or SDK aggregation."""

    def __init__(self, stream, deadline: IdleDeadline, counts: dict):
        self.stream, self.deadline, self.counts = stream, deadline, counts

    async def __aiter__(self):
        iterator = self.stream.__aiter__()
        while True:
            try:
                chunk = await self.deadline.wait(iterator.__anext__)
            except StopAsyncIteration:
                return
            if chunk:
                self.deadline.received()
                self.counts["raw_received_chunks"] += 1
                self.counts["raw_received_bytes"] += len(chunk)
            yield chunk

    async def aclose(self):
        await self.stream.aclose()


def invalid_json_constant(value):
    raise ValueError("non-finite JSON number")


class StreamResponse:
    """Retain one raw body, and distinguish content from all protocol activity."""

    PROTOCOL = "chat.completions.sse.v1"
    _boundary = re.compile(rb"\r\n\r\n|\n\n|\r\r")

    def __init__(self):
        self.raw = bytearray()
        self.pending = b""
        self.content = []
        self.usage = None
        self.done = False
        self.finish_reason = None
        self.failure = None
        self.provider_error = None
        self.counts = {key: 0 for key in (
            "raw_received_chunks", "raw_received_bytes", "received_chunks", "received_bytes", "sse_events", "heartbeat_events",
            "content_deltas", "thinking_deltas", "tool_deltas", "usage_events", "done_events",
        )}

    def feed(self, chunk: bytes):
        if not chunk:
            return
        self.raw.extend(chunk)
        self.counts["received_chunks"] += 1
        self.counts["received_bytes"] += len(chunk)
        self.pending += chunk
        while match := self._boundary.search(self.pending):
            event, self.pending = self.pending[:match.start()], self.pending[match.end():]
            self._event(event)

    def _event(self, raw: bytes):
        self.counts["sse_events"] += 1
        try:
            lines = re.split(rb"\r\n|\r|\n", raw)
            data, event_type = [], None
            heartbeat = False
            for line in lines:
                if line.startswith(b":"):
                    heartbeat = True
                    continue
                name, _, value = line.partition(b":")
                value = value.removeprefix(b" ")
                if name == b"data":
                    data.append(value)
                elif name == b"event":
                    event_type = value.decode("utf-8")
            if event_type == "error":
                # The explicit protocol error fences replay even when its
                # diagnostic payload is absent, malformed, or not JSON.
                self.provider_error = {}
                self.failure = "provider_stream_error"
            if heartbeat or event_type in {"ping", "heartbeat"}:
                self.counts["heartbeat_events"] += 1
            if not data or event_type in {"ping", "heartbeat"}:
                return
            if self.done:
                raise ValueError
            value = b"\n".join(data).decode("utf-8")
            if value.strip() == "[DONE]":
                self.done = True
                self.counts["done_events"] += 1
                return
            envelope = json.loads(value, parse_constant=invalid_json_constant)
            if not isinstance(envelope, dict):
                raise ValueError
            if "error" in envelope or event_type == "error":
                error = envelope.get("error", envelope)
                error = error if isinstance(error, dict) else {}
                # Preserve diagnostic identifiers, never provider prose, URLs,
                # request bodies, or arbitrary nested data in operator errors.
                self.provider_error = {
                    key: value for key in ("type", "code")
                    if isinstance(value := error.get(key), str)
                    and re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", value)
                }
                self.failure = "provider_stream_error"
                return
            usage = reported_usage(envelope.get("usage"))
            if usage is not None:
                self.usage = usage
                self.counts["usage_events"] += 1
            choices = envelope.get("choices")
            if not isinstance(choices, list):
                raise ValueError
            for choice in choices:
                if not isinstance(choice, dict) or choice.get("index") != 0:
                    raise ValueError
                delta = choice.get("delta")
                if not isinstance(delta, dict):
                    raise ValueError
                if delta.get("refusal"):
                    self.failure = "model_refusal"
                for key in ("reasoning", "reasoning_content", "thinking"):
                    if delta.get(key):
                        self.counts["thinking_deltas"] += 1
                if delta.get("tool_calls") or delta.get("function_call"):
                    self.counts["tool_deltas"] += 1
                content = delta.get("content")
                if content is not None:
                    if not isinstance(content, str) or self.finish_reason is not None:
                        raise ValueError
                    self.content.append(content)
                    self.counts["content_deltas"] += 1
                finish = choice.get("finish_reason")
                if finish is not None:
                    if self.finish_reason is not None or not isinstance(finish, str):
                        raise ValueError
                    self.finish_reason = finish
        except (ValueError, TypeError, UnicodeError):
            self.failure = self.failure or "invalid_stream_envelope"

    def result(self) -> dict:
        if self.provider_error is not None:
            raise UncertainModelResponse("model provider reported a stream error; provider execution and billing are uncertain")
        if not self.done:
            raise UncertainModelResponse("model stream ended without completion; provider execution and billing are uncertain")
        if self.failure or self.pending.strip():
            raise ModelSemanticError("model endpoint returned an invalid or refused stream")
        if self.counts["tool_deltas"] or self.finish_reason != "stop":
            self.failure = "tool_call_response" if self.counts["tool_deltas"] else "incomplete_finish"
            raise ModelSemanticError("model refused or did not finish its JSON response")
        source = "".join(self.content)
        try:
            value = json.loads(source, parse_constant=invalid_json_constant)
        except json.JSONDecodeError as error:
            raise ModelJSONError("json_syntax_error", error.msg, source,
                                 line=error.lineno, column=error.colno, offset=error.pos) from None
        except (ValueError, TypeError):
            raise ModelSemanticError("model response is not a complete JSON object") from None
        if not isinstance(value, dict):
            offset = len(source) - len(source.lstrip())
            line = source.count("\n", 0, offset) + 1
            column = offset - source.rfind("\n", 0, offset)
            raise ModelJSONError("json_object_required", "JSON response must be an object", source,
                                 line=line, column=column, offset=offset)
        return value

    def audit(self, digest: str, outcome: str) -> dict:
        return {"protocol": self.PROTOCOL, "request_digest": digest,
                "raw_body": self.raw.decode("utf-8", errors="replace"),
                "activity": dict(self.counts), "finish_reason": self.finish_reason,
                "done": self.done, "usage": self.usage,
                "provider_error": self.provider_error,
                "usage_status": "reported" if self.usage is not None else "unknown",
                "outcome": outcome}
