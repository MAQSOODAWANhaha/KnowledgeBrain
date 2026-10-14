"""Per-request admission estimates, separate from cumulative reported usage."""

import hashlib
import json
from types import MappingProxyType
from typing import Mapping, Sequence, get_args

from docreader.llm.request import ImageInput, ModelBudgetError, ModelConfigurationError, build_request
from docreader.tender.schema import RequestLimits, Task


class RequestBudget:
    """Conservative admission for decoded message content, not billed usage.

    No tokenizer is assumed for an unverified model name (including gpt-6-luna).
    UTF8 bytes conservatively estimate text tokens for every configured model.
    Actual image data is excluded from text estimation and charged a documented
    16384-token estimate per image. Body/support are marginal contributions of
    those fields to decoded message text; locators, schema and other context
    remain in overhead. HTTP JSON escaping is measured separately as wire bytes.
    """

    METHOD = "utf8_bytes_decoded_messages_v2"
    BUILDER = "chat_json_object_stream_v2"
    IMAGE_TOKENS = 16384
    FRAMING_TOKENS = 32  # 16 + 8 per message for our two-message body.

    def __init__(self, context_tokens: int | None = 131072, model: str = "gpt-6-luna",
                 prompts: Mapping[str, str] | None = None, output_reserve: int = 16384,
                 *, correction: bool = False):
        if context_tokens is not None and (type(context_tokens) is not int or context_tokens <= 0):
            raise ModelConfigurationError("request context capacity must be a positive integer")
        if type(output_reserve) is not int or output_reserve < 0:
            raise ModelConfigurationError("request output reserve must be a nonnegative integer")
        if type(correction) is not bool or (correction and context_tokens is None):
            raise ModelConfigurationError("correction input allowance requires a configured provider total context")
        if prompts is None:
            from docreader.tender.prompts import get_prompt
            prompts = {task: get_prompt(task) for task in get_args(Task)}
        self.prompts = MappingProxyType(dict(prompts))
        self.context_tokens, self.model, self.output_reserve = context_tokens, model, output_reserve
        self.limits = RequestLimits(context_tokens=context_tokens, correction=correction)

    @property
    def effective_input_budget_tokens(self) -> int:
        limit = (self.limits.correction_input_budget_tokens if self.limits.correction
                 else self.limits.input_budget_tokens)
        return min(limit, self.context_tokens - self.output_reserve - self.safety) if self.context_tokens is not None else limit

    @property
    def safety(self) -> int:
        return min(4096, self.context_tokens // 16) if self.context_tokens is not None else 4096

    def spec(self) -> dict:
        return {"method": self.METHOD, "builder": self.BUILDER, "model": self.model,
                **self.limits.model_dump(), "output_reserve": self.output_reserve,
                "safety": self.safety, "image_tokens_each": self.IMAGE_TOKENS,
                "framing_tokens": self.FRAMING_TOKENS,
                "prompts": {task: hashlib.sha256(prompt.encode()).hexdigest()
                            for task, prompt in sorted(self.prompts.items())}}

    def request(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> dict:
        if task not in get_args(Task):
            raise ModelConfigurationError("unknown tender model task")
        try:
            prompt = self.prompts[task]
        except KeyError:
            raise ModelConfigurationError("model task has no configured prompt") from None
        return build_request(self.model, prompt, payload, images)

    def measure(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> dict:
        return self.measure_request(self.request(task, payload, images))

    def measure_request(self, request: dict) -> dict:
        """Count actual decoded text/schema/tools, without HTTP JSON escaping."""
        image_count = 0
        text_tokens = 0
        for message in request["messages"]:
            content = message["content"]
            if isinstance(content, str):
                text_tokens += len(content.encode("utf-8"))
            if isinstance(content, list):
                for part in content:
                    if part.get("type") == "image_url":
                        image_count += 1
                    elif part.get("type") == "text":
                        text_tokens += len(part["text"].encode("utf-8"))
            for field in ("tool_calls", "function_call"):
                if field in message:
                    text_tokens += _json_bytes(message[field])
        for field in ("tools", "functions"):
            if field in request:
                text_tokens += _json_bytes(request[field])
        if "json_schema" in request.get("response_format", {}):
            text_tokens += _json_bytes(request["response_format"]["json_schema"])
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        # This JSON is the message content itself, with only its own escaping.
        def contribution(field):
            if not isinstance(payload, dict) or field not in payload:
                return 0
            return max(0, _json_bytes(payload[field]) - _json_bytes([]))

        body_tokens, support_tokens = contribution("body"), contribution("supporting")
        image_tokens = image_count * self.IMAGE_TOKENS
        input_tokens = text_tokens + self.FRAMING_TOKENS + image_tokens
        total = input_tokens + self.output_reserve + self.safety
        return {"input_tokens": input_tokens, "text_tokens": text_tokens, "wire_bytes": _json_bytes(request),
                "body_tokens": body_tokens, "support_tokens": support_tokens,
                "overhead_tokens": text_tokens - body_tokens - support_tokens,
                "framing_tokens": self.FRAMING_TOKENS, "image_tokens": image_tokens,
                "image_count": image_count, "output_reserve": self.output_reserve,
                "safety": self.safety, **self.limits.model_dump(), "total_context_tokens": total,
                "effective_input_budget_tokens": self.effective_input_budget_tokens,
                "fits": (input_tokens <= self.effective_input_budget_tokens if self.context_tokens is not None
                         or input_tokens > self.effective_input_budget_tokens else None),
                "method": self.METHOD}

    def fits(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> bool:
        return self.measure(task, payload, images)["fits"] is True

    def require(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> dict:
        return self.require_request(self.request(task, payload, images))

    def require_request(self, request: dict) -> dict:
        measured = self.measure_request(request)
        if measured["fits"] is False:
            raise ModelBudgetError("complete request exceeds the input budget or provider total context after reserves")
        # An independent VLM may have no configured capacity. Preserve that
        # existing deployment meaning, without reporting a proven fit.
        return measured


def _json_bytes(value) -> int:
    return len(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode())
