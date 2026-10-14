"""Small request contracts; document content is never included in exceptions."""

import base64
import binascii
from dataclasses import dataclass, field


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
