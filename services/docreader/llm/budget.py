"""Per-request admission estimates, separate from cumulative reported usage."""

import hashlib
import json
from functools import lru_cache
from types import MappingProxyType
from typing import Mapping, Sequence, get_args

from docreader.llm.request import (
    ImageInput,
    ModelBudgetError,
    ModelConfigurationError,
    build_request,
)
from docreader.tender.schema import RequestLimits, Task
from docreader.tender.status import AUTHORING_MODEL_TASKS


class RequestBudget:
    """Conservative admission for decoded message content, not billed usage.

    No tokenizer is assumed for an unverified model name (including gpt-6-luna).
    UTF8 bytes conservatively estimate text tokens unless an explicitly
    verified tokenizer is configured; it then counts TOKENIZED_TASKS and any
    separately enabled, calibrated operation.
    Actual image data is excluded from text estimation and charged a documented
    16384-token estimate per image. Body/support are marginal contributions of
    those fields to decoded message text; locators, schema and other context
    remain in overhead. HTTP JSON escaping is measured separately as wire bytes.
    """

    METHOD = "utf8_bytes_decoded_messages_v2"
    BUILDER = "chat_json_object_stream_short_refs_schema_v3"
    IMAGE_TOKENS = 16384
    FRAMING_TOKENS = 32  # 16 + 8 per message for our two-message body.
    TOKENIZERS = ("o200k_base",)
    # Extract pack size stays bound by the byte estimate until its much larger
    # output is calibrated; a token-accurate input would merge packs.
    TOKENIZED_TASKS = frozenset({"verify"})
    TOKENIZED_AUTHORING_TASKS = frozenset(AUTHORING_MODEL_TASKS)
    TOKENIZED_METHOD = "o200k_base_decoded_messages_v1"
    TOKENIZED_FRAMING_TOKENS = (
        512  # The relay adds a fixed hidden prompt of about 306 tokens.
    )
    # Accurate input admits larger whole-ledger discovery requests whose output
    # grows with items; capped at a quarter of smaller provider windows.
    TOKENIZED_OUTPUT_RESERVE = 32768
    ANALYSIS_BATCH_OUTPUT_PER_TARGET = 24576
    ANALYSIS_BATCH_OUTPUT_MINIMUM = 32768

    def __init__(
        self,
        context_tokens: int | None = 131072,
        model: str = "gpt-6-luna",
        prompts: Mapping[str, str] | None = None,
        output_reserve: int = 16384,
        *,
        correction: bool = False,
        tokenizer: str | None = None,
        tokenized_outline_assembly: bool = False,
        tokenized_authoring: bool = False,
    ):
        if context_tokens is not None and (
            type(context_tokens) is not int or context_tokens <= 0
        ):
            raise ModelConfigurationError(
                "request context capacity must be a positive integer"
            )
        if type(output_reserve) is not int or output_reserve < 0:
            raise ModelConfigurationError(
                "request output reserve must be a nonnegative integer"
            )
        if type(correction) is not bool or (correction and context_tokens is None):
            raise ModelConfigurationError(
                "correction input allowance requires a configured provider total context"
            )
        if tokenizer is not None and tokenizer not in self.TOKENIZERS:
            raise ModelConfigurationError("unsupported request tokenizer")
        if type(tokenized_outline_assembly) is not bool:
            raise ModelConfigurationError(
                "tokenized outline assembly requires a boolean flag"
            )
        if tokenized_outline_assembly and tokenizer != "o200k_base":
            raise ModelConfigurationError(
                "tokenized outline assembly requires the verified o200k_base tokenizer"
            )
        if type(tokenized_authoring) is not bool:
            raise ModelConfigurationError("tokenized authoring requires a boolean flag")
        if tokenized_authoring and tokenizer != "o200k_base":
            raise ModelConfigurationError(
                "tokenized authoring requires the verified o200k_base tokenizer"
            )
        self.tokenizer = tokenizer
        self.tokenized_outline_assembly = tokenized_outline_assembly
        self.tokenized_authoring = tokenized_authoring
        if prompts is None:
            from docreader.tender.prompts import get_prompt

            prompts = {task: get_prompt(task) for task in get_args(Task)}
        self.prompts = MappingProxyType(dict(prompts))
        self.context_tokens, self.model, self.output_reserve = (
            context_tokens,
            model,
            output_reserve,
        )
        self.limits = RequestLimits(
            context_tokens=context_tokens, correction=correction
        )

    @property
    def effective_input_budget_tokens(self) -> int:
        return self._input_budget(self.output_reserve)

    def _input_budget(self, output_reserve: int) -> int:
        limit = (
            self.limits.correction_input_budget_tokens
            if self.limits.correction
            else self.limits.input_budget_tokens
        )
        return (
            min(limit, self.context_tokens - output_reserve - self.safety)
            if self.context_tokens is not None
            else limit
        )

    @property
    def safety(self) -> int:
        return (
            min(4096, self.context_tokens // 16)
            if self.context_tokens is not None
            else 4096
        )

    @property
    def tokenized_output_reserve(self) -> int:
        return (
            min(self.TOKENIZED_OUTPUT_RESERVE, self.context_tokens // 4)
            if self.context_tokens is not None
            else self.TOKENIZED_OUTPUT_RESERVE
        )

    def spec(self) -> dict:
        from docreader.llm.contracts import output_contract_spec

        spec = {
            "method": self.METHOD,
            "builder": self.BUILDER,
            "model": self.model,
            **self.limits.model_dump(),
            "output_reserve": self.output_reserve,
            "safety": self.safety,
            "image_tokens_each": self.IMAGE_TOKENS,
            "framing_tokens": self.FRAMING_TOKENS,
            "output_contracts": output_contract_spec(),
            "prompts": {
                task: hashlib.sha256(prompt.encode()).hexdigest()
                for task, prompt in sorted(self.prompts.items())
            },
        }
        spec["analysis_batch_output"] = {
            "minimum": self.ANALYSIS_BATCH_OUTPUT_MINIMUM,
            "per_target": self.ANALYSIS_BATCH_OUTPUT_PER_TARGET,
        }
        if self.tokenizer is not None:
            tasks = self.TOKENIZED_TASKS | (
                self.TOKENIZED_AUTHORING_TASKS
                if self.tokenized_authoring
                else frozenset()
            )
            spec["tokenized"] = {
                "tokenizer": self.tokenizer,
                "tasks": sorted(tasks),
                "method": self.TOKENIZED_METHOD,
                "framing_tokens": self.TOKENIZED_FRAMING_TOKENS,
                "output_reserve": self.tokenized_output_reserve,
            }
            if self.tokenized_outline_assembly:
                spec["tokenized"]["operations"] = {"outline": ["assemble"]}
        return spec

    def request(
        self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()
    ) -> dict:
        if task not in get_args(Task):
            raise ModelConfigurationError("unknown tender model task")
        try:
            prompt = self.prompts[task]
        except KeyError:
            raise ModelConfigurationError(
                "model task has no configured prompt"
            ) from None
        from docreader.llm.wire import WirePayload

        return build_request(
            self.model, prompt, WirePayload(task, payload).payload, images, task=task
        )

    def measure(
        self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()
    ) -> dict:
        return self.measure_request(self.request(task, payload, images), task)

    def measure_request(self, request: dict, task: str | None = None) -> dict:
        """Count actual decoded text/schema/tools, without HTTP JSON escaping."""
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        tokenized_assembly = (
            self.tokenized_outline_assembly
            and task == "outline"
            and isinstance(payload, dict)
            and payload.get("operation") == "assemble"
        )
        tokenized = self.tokenizer is not None and (
            task in self.TOKENIZED_TASKS
            or tokenized_assembly
            or (self.tokenized_authoring and task in self.TOKENIZED_AUTHORING_TASKS)
        )
        count = _o200k_tokens if tokenized else _utf8_bytes
        framing = self.TOKENIZED_FRAMING_TOKENS if tokenized else self.FRAMING_TOKENS
        output_reserve = (
            self.tokenized_output_reserve if tokenized else self.output_reserve
        )
        if task == "verify" and isinstance(payload, dict) and "work_units" in payload:
            targets = payload.get("target_work_unit_ids")
            if not isinstance(targets, list) or not targets:
                raise ModelConfigurationError(
                    "analysis batch requires explicit nonempty answering targets"
                )
            output_reserve = max(
                self.ANALYSIS_BATCH_OUTPUT_MINIMUM,
                self.ANALYSIS_BATCH_OUTPUT_PER_TARGET * len(targets),
            )
        image_count = 0
        text_tokens = 0
        for message in request["messages"]:
            content = message["content"]
            if isinstance(content, str):
                text_tokens += count(content)
            if isinstance(content, list):
                for part in content:
                    if part.get("type") == "image_url":
                        image_count += 1
                    elif part.get("type") == "text":
                        text_tokens += count(part["text"])
            for field in ("tool_calls", "function_call"):
                if field in message:
                    text_tokens += count(_json(message[field]))
        for field in ("tools", "functions"):
            if field in request:
                text_tokens += count(_json(request[field]))
        if "json_schema" in request.get("response_format", {}):
            text_tokens += count(_json(request["response_format"]["json_schema"]))
        # This JSON is the message content itself, with only its own escaping.
        source_views = (
            payload.get("source_views", []) if isinstance(payload, dict) else []
        )
        views = (
            [payload, *source_views] if isinstance(source_views, list) else [payload]
        )

        def contribution(field):
            return sum(
                max(0, count(_json(view[field])) - count(_json([])))
                for view in views
                if isinstance(view, dict) and field in view
            )

        body_tokens, support_tokens = contribution("body"), contribution("supporting")
        image_tokens = image_count * self.IMAGE_TOKENS
        input_tokens = text_tokens + framing + image_tokens
        total = input_tokens + output_reserve + self.safety
        input_budget = self._input_budget(output_reserve)
        return {
            "input_tokens": input_tokens,
            "text_tokens": text_tokens,
            "wire_bytes": _json_bytes(request),
            "body_tokens": body_tokens,
            "support_tokens": support_tokens,
            "overhead_tokens": text_tokens - body_tokens - support_tokens,
            "framing_tokens": framing,
            "image_tokens": image_tokens,
            "image_count": image_count,
            "output_reserve": output_reserve,
            "safety": self.safety,
            **self.limits.model_dump(),
            "total_context_tokens": total,
            "effective_input_budget_tokens": input_budget,
            "fits": (
                input_tokens <= input_budget
                if self.context_tokens is not None or input_tokens > input_budget
                else None
            ),
            "method": self.TOKENIZED_METHOD if tokenized else self.METHOD,
        }

    def fits(
        self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()
    ) -> bool:
        return self.measure(task, payload, images)["fits"] is True

    def require(
        self, task: Task, payload: dict, images: Sequence[str | ImageInput] = ()
    ) -> dict:
        return self.require_request(self.request(task, payload, images), task)

    def require_request(self, request: dict, task: str | None = None) -> dict:
        measured = self.measure_request(request, task)
        if measured["fits"] is False:
            raise ModelBudgetError(
                "complete request exceeds the input budget or provider total context after reserves"
            )
        # An independent VLM may have no configured capacity. Preserve that
        # existing deployment meaning, without reporting a proven fit.
        return measured

    def admit_request(self, request: dict, task: str | None = None) -> dict:
        """Send complete assembly/review while retaining an exceeded estimate."""
        try:
            return self.require_request(request, task)
        except ModelBudgetError:
            payload = json.loads(request["messages"][1]["content"][0]["text"])
            if not isinstance(payload, dict) or (task, payload.get("operation")) not in (
                ("outline", "assemble"),
                ("authoring_structure", "structure"),
            ):
                raise
            return {
                **self.measure_request(request, task),
                "admission_warning": "local_context_estimate_exceeded",
            }


def _json(value) -> str:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    )


def _json_bytes(value) -> int:
    return _utf8_bytes(_json(value))


def _utf8_bytes(text: str) -> int:
    return len(text.encode("utf-8"))


@lru_cache(maxsize=1)
def _o200k():
    import tiktoken

    return tiktoken.get_encoding("o200k_base")


def _o200k_tokens(text: str) -> int:
    return len(_o200k().encode(text, disallowed_special=()))
