"""Small request contracts; document content is never included in exceptions."""

import base64
import binascii
import hashlib
import json
from dataclasses import dataclass, field
from typing import Sequence


class ModelConfigurationError(ValueError):
    """Deployment/model configuration was rejected; execution retries are bounded."""


class ModelSemanticError(ValueError):
    """A complete response or its domain contract was rejected."""


class ModelJSONError(ModelSemanticError):
    """A completed stream has invalid JSON; only bounded wire feedback escapes."""

    def __init__(self, code: str, message: str, source: str, *, line: int, column: int, offset: int):
        self.code, self.message = code, message
        self.line, self.column, self.offset = line, column, offset
        self._source = source
        super().__init__("Model response is not a complete JSON object")

    def redact(self, secrets):
        # Mask before clipping, including JSON escapes, so a fragment boundary
        # cannot expose part of a credential. Equal length preserves positions.
        literals = {literal for secret in secrets if secret for literal in (
            secret, json.dumps(secret, ensure_ascii=False)[1:-1], json.dumps(secret)[1:-1])}
        for literal in sorted(literals, key=len, reverse=True):
            self._source = self._source.replace(literal, "*" * len(literal))

    @property
    def feedback(self):
        feedback = {"code": self.code, "message": self.message,
                    "line": self.line, "column": self.column, "offset": self.offset}
        radius = 64
        while True:
            feedback["fragment"] = self._source[max(0, self.offset - radius):self.offset + radius]
            if len(json.dumps(feedback, ensure_ascii=False, separators=(",", ":"))) <= 384:
                return feedback
            radius //= 2


class ModelWorkUnitExhausted(RuntimeError):
    """A logical work unit has used its initial call and three retries."""

    def __init__(self, task=None):
        self.task = task
        super().__init__("Model work unit exhausted its automatic attempts")


class ModelContractError(ModelSemanticError):
    """Durable feedback for an unaccepted response; details stay out of logs."""

    def __init__(self, errors):
        self.errors = errors
        super().__init__("Model response violated its host contract")


class ModelCorrectionStalled(ModelSemanticError):
    """A cached response or historical correction cannot be reused as accepted."""


class ModelBudgetError(ValueError):
    """An unsent request exceeds admission limits; it is not a model response."""


class ModelInfrastructureError(ConnectionError):
    """The bounded work-unit recovery policy may retry this failure."""


class ModelRateLimitError(ModelInfrastructureError):
    """An HTTP 429 rejection with an optional parsed provider retry delay."""

    def __init__(self, retry_after_seconds=None):
        self.retry_after_seconds = retry_after_seconds
        super().__init__("model endpoint returned HTTP 429")


class UncertainModelResponse(RuntimeError):
    """Execution/billing may have occurred; never automatically replay payment."""


@dataclass(frozen=True)
class ImageInput:
    data: bytes = field(repr=False)
    media_type: str

    def data_url(self) -> str:
        if self.media_type not in {"image/png", "image/jpeg", "image/webp", "image/gif"}:
            raise ModelSemanticError("unsupported inline image media type")
        if not self.data:
            raise ModelSemanticError("empty inline image")
        return f"data:{self.media_type};base64,{base64.b64encode(self.data).decode('ascii')}"


def image_data_url(image: str | ImageInput) -> str:
    if isinstance(image, ImageInput):
        return image.data_url()
    if not isinstance(image, str) or not image.startswith("data:image/"):
        raise ModelSemanticError("model images must be inline image data, not remote URLs")
    header, separator, encoded = image.partition(",")
    if not separator or header not in {
        "data:image/png;base64", "data:image/jpeg;base64", "data:image/webp;base64", "data:image/gif;base64",
    }:
        raise ModelSemanticError("unsupported inline image encoding")
    try:
        decoded = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error):
        raise ModelSemanticError("invalid inline image encoding") from None
    if not decoded:
        raise ModelSemanticError("empty inline image")
    return image


def build_request(model: str, prompt: str, payload: dict,
                  images: Sequence[str | ImageInput] = (), *, task: str | None = None) -> dict:
    """The same complete API body is used for packing probes and sending.

    Tender response schemas are part of this actual prompt, so estimates and
    request identity include them. Reserves never become provider output limits.
    """
    try:
        serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
    except (TypeError, ValueError):
        raise ModelSemanticError("model payload is not JSON serializable") from None
    if task is not None:
        from docreader.llm.contracts import schema_prompt

        prompt = schema_prompt(prompt, task, payload)
    content = [{"type": "text", "text": serialized}]
    content.extend({"type": "image_url", "image_url": {"url": image_data_url(image)}} for image in images)
    return {"model": model,
            "messages": [{"role": "system", "content": prompt}, {"role": "user", "content": content}],
            "response_format": {"type": "json_object"},
            "stream": True, "stream_options": {"include_usage": True}}


def request_digest(endpoint_url: str, request: dict) -> str:
    """Hash the normalized endpoint and complete body, never credentials/headers."""
    from docreader.llm.config import completions_url

    encoded = json.dumps({"endpoint": completions_url(endpoint_url), "body": request},
                         ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()
