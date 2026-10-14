"""One environment snapshot matching the existing CHAT/VLM alias semantics."""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import Mapping
from urllib.parse import urlsplit, urlunsplit

from docreader.llm.request import ModelConfigurationError

_VISION_MODELS = {
    "gpt-6-luna", "gpt-4o", "gpt-4o-2024-05-13", "gpt-4o-2024-08-06",
    "gpt-4o-2024-11-20", "gpt-4o-mini", "gpt-4o-mini-2024-07-18", "gpt-4.1",
    "gpt-4.1-2025-04-14", "gpt-4.1-mini", "gpt-4.1-mini-2025-04-14",
    "gpt-4.1-nano", "gpt-4.1-nano-2025-04-14",
}


def _setting(values: Mapping[str, str], *keys: str, secret: bool = False) -> str:
    selected = None
    for key in keys:
        raw = values.get(key, "")
        if not raw.strip():
            continue
        value = raw if secret else raw.strip()
        if selected is not None and selected != value:
            raise ModelConfigurationError(f"conflicting configuration: {' / '.join(keys)}")
        selected = value
    return selected or ""


def _valid_url(value: str) -> bool:
    try:
        url = urlsplit(value)
        return bool(
            url.scheme in {"http", "https"} and url.hostname and not url.username
            and url.password is None and "?" not in value and "#" not in value
            and (url.port is None or 0 < url.port < 65536)
        )
    except ValueError:
        return False


def completions_url(base_url: str) -> str:
    """Normalize only endpoint identity; never use this for credential inheritance."""
    base = base_url.rstrip("/")
    if base.endswith("/v1"):
        base += "/chat/completions"
    elif not base.endswith("/chat/completions"):
        base += "/v1/chat/completions"
    url = urlsplit(base)
    host = (url.hostname or "").lower()
    if ":" in host:
        host = f"[{host}]"
    if url.port is not None and (url.scheme, url.port) not in {("https", 443), ("http", 80)}:
        host += f":{url.port}"
    return urlunsplit((url.scheme.lower(), host, url.path, "", ""))


@dataclass(frozen=True)
class ModelEndpoint:
    base_url: str
    api_key: str = field(repr=False)
    model: str
    context_tokens: int | None

    @property
    def sdk_base_url(self) -> str:
        return completions_url(self.base_url).removesuffix("chat/completions")


@dataclass(frozen=True)
class ModelConfig:
    chat: ModelEndpoint
    vision: ModelEndpoint | None
    vision_error: str | None = field(default=None, repr=False)

    @classmethod
    def from_env(cls, mapping: Mapping[str, str] | None = None) -> ModelConfig:
        values = dict(os.environ if mapping is None else mapping)
        base_url = _setting(values, "KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL")
        key = _setting(values, "KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY", secret=True)
        model = _setting(values, "KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL")
        if not _valid_url(base_url) or not key.strip() or not model or model == "stub-chat":
            raise ModelConfigurationError("valid CHAT base URL, API key and model are required")
        raw_context = _setting(values, "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS")
        try:
            context = int(raw_context)
        except ValueError:
            context = 0
        if not raw_context.isascii() or not raw_context.isdecimal() or not 1 <= context <= 4_194_304:
            raise ModelConfigurationError("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS must be an integer between 1 and 4194304")
        image_support = _setting(values, "KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES")
        if image_support not in {"", "true", "false"}:
            raise ModelConfigurationError("KNOWLEDGEBRAIN_CHAT_SUPPORTS_IMAGES must be true or false")
        supports_images = image_support == "true" if image_support else model in _VISION_MODELS
        chat = ModelEndpoint(base_url, key, model, context)
        try:
            vision = _vision(values, chat, supports_images)
        except ModelConfigurationError as error:
            # A text-only run does not need image support or an independent VLM.
            return cls(chat, None, str(error))
        return cls(chat, vision)

    def for_images(self) -> ModelEndpoint:
        if self.vision is None:
            raise ModelConfigurationError(self.vision_error or "VLM configuration is required")
        return self.vision


def _vision(values: Mapping[str, str], chat: ModelEndpoint, supports_images: bool) -> ModelEndpoint:
    url = _setting(values, "KNOWLEDGEBRAIN_VLM_BASE_URL")
    key = _setting(values, "KNOWLEDGEBRAIN_VLM_API_KEY", secret=True)
    model = _setting(values, "KNOWLEDGEBRAIN_VLM_MODEL")
    independent = bool(url) and url.rstrip("/") != chat.base_url.rstrip("/")
    if independent and (not key or not model):
        raise ModelConfigurationError("independent VLM base URL requires its own API key and model; Chat credentials are never forwarded")
    effective_url, effective_model = url or chat.base_url, model or chat.model
    if not _valid_url(effective_url) or effective_model == "stub-vlm":
        raise ModelConfigurationError("valid VLM base URL and model are required")
    uses_chat_model = completions_url(effective_url) == completions_url(chat.base_url) and effective_model == chat.model
    if uses_chat_model and not supports_images:
        raise ModelConfigurationError("OCR inheritance requires confirmed Chat image support or an independent VLM model")
    return ModelEndpoint(effective_url, key or chat.api_key, effective_model,
                         chat.context_tokens if uses_chat_model else None)
