"""Per-request admission estimates, separate from cumulative reported usage."""

import hashlib
import json
from types import MappingProxyType
from typing import Mapping, Sequence, get_args

from docreader.llm.request import ImageInput, ModelConfigurationError, ModelSemanticError, build_request
from docreader.tender.schema import Task


class RequestBudget:
    """Conservative admission for the exact JSON request, not billed usage.

    No tokenizer is assumed for an unverified model name (including gpt-6-luna).
    UTF8 bytes conservatively estimate text tokens for every configured model.
    Actual image data is excluded from text estimation and charged a documented
    16384-token estimate per image. Body/support are marginal contributions of
    those fields to the serialized request; locators, schema and other context
    remain in overhead. All three add up to the complete serialized text body.
    """

    METHOD = "utf8_bytes_canonical_request_v1"
    BUILDER = "chat_json_object_v1"
    IMAGE_TOKENS = 16384
    FRAMING_TOKENS = 32  # 16 + 8 per message for our two-message body.

    def __init__(self, context_tokens: int | None = 131072, model: str = "gpt-6-luna",
                 prompts: Mapping[str, str] | None = None, output_reserve: int = 16384):
        if context_tokens is not None and (type(context_tokens) is not int or context_tokens <= 0):
            raise ModelConfigurationError("request context capacity must be a positive integer")
        if type(output_reserve) is not int or output_reserve < 0:
            raise ModelConfigurationError("request output reserve must be a nonnegative integer")
        if prompts is None:
            from docreader.tender.prompts import get_prompt
            prompts = {task: get_prompt(task) for task in get_args(Task)}
        self.prompts = MappingProxyType(dict(prompts))
        self.context_tokens, self.model, self.output_reserve = context_tokens, model, output_reserve

    @property
    def safety(self) -> int:
        return min(4096, self.context_tokens // 16) if self.context_tokens is not None else 4096

    def spec(self) -> dict:
        return {"method": self.METHOD, "builder": self.BUILDER, "model": self.model,
                "context_tokens": self.context_tokens, "output_reserve": self.output_reserve,
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
        """Measure a body from request(), retaining all actual non-image fields."""
        image_count = 0
        messages = []
        for message in request["messages"]:
            content = message["content"]
            if isinstance(content, list):
                parts = []
                for part in content:
                    if part.get("type") == "image_url":
                        image_count += 1
                        part = {**part, "image_url": {**part["image_url"], "url": ""}}
                    parts.append(part)
                content = parts
            messages.append({**message, "content": content})
        text_tokens = _json_bytes({**request, "messages": messages})
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        # Payload JSON is itself inside a JSON text string: account for both
        # layers of escaping, rather than counting only the underlying prose.
        def contribution(field):
            if not isinstance(payload, dict) or field not in payload:
                return 0
            serialized = json.dumps(payload[field], ensure_ascii=False, separators=(",", ":"), allow_nan=False)
            return max(0, _json_bytes(serialized) - _json_bytes("[]"))

        body_tokens, support_tokens = contribution("body"), contribution("supporting")
        image_tokens = image_count * self.IMAGE_TOKENS
        input_tokens = text_tokens + self.FRAMING_TOKENS + image_tokens
        total = input_tokens + self.output_reserve + self.safety
        return {"input_tokens": input_tokens, "text_tokens": text_tokens,
                "body_tokens": body_tokens, "support_tokens": support_tokens,
                "overhead_tokens": text_tokens - body_tokens - support_tokens,
                "framing_tokens": self.FRAMING_TOKENS, "image_tokens": image_tokens,
                "image_count": image_count, "output_reserve": self.output_reserve,
                "safety": self.safety, "context_tokens": self.context_tokens, "total_context_tokens": total,
                "fits": total <= self.context_tokens if self.context_tokens is not None else None,
                "method": self.METHOD}

    def fits(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> bool:
        return self.measure(task, payload, images)["fits"] is True

    def require(self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()) -> dict:
        return self.require_request(self.request(task, payload, images))

    def require_request(self, request: dict) -> dict:
        measured = self.measure_request(request)
        if measured["fits"] is False:
            raise ModelSemanticError("complete request and reserves do not fit the configured model context; split the source package")
        # An independent VLM may have no configured capacity. Preserve that
        # existing deployment meaning, without reporting a proven fit.
        return measured


def _json_bytes(value) -> int:
    return len(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode())
