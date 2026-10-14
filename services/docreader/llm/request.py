"""Small request contracts; document content is never included in exceptions."""

import base64
import binascii
import json
from dataclasses import dataclass, field
from typing import Sequence


class ModelConfigurationError(ValueError):
    """Deployment/model configuration needs correction, not a queue retry."""


class ModelSemanticError(ValueError):
    """A domain node must expose this result or perform its bounded repair."""


class ModelInfrastructureError(ConnectionError):
    """The single queue retry policy may retry this failure."""


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
                  images: Sequence[str | ImageInput] = ()) -> dict:
    """The same complete API body is used for packing probes and sending.

    Schema instructions are part of the actual prompt, not an invented API
    schema field. Admission reserves never become provider output limits.
    """
    try:
        serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
    except (TypeError, ValueError):
        raise ModelSemanticError("model payload is not JSON serializable") from None
    content = [{"type": "text", "text": serialized}]
    content.extend({"type": "image_url", "image_url": {"url": image_data_url(image)}} for image in images)
    return {"model": model,
            "messages": [{"role": "system", "content": prompt}, {"role": "user", "content": content}],
            "response_format": {"type": "json_object"}}
