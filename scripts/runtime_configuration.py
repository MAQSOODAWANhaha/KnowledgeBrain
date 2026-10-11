"""Read the same nonsecret request defaults embedded by the Rust runtime."""
import json
from pathlib import Path


def context_tokens(environment):
    key = "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS"
    raw = environment.get(key, "").strip()
    if not raw.isascii() or not raw.isdecimal() or not 1 <= int(raw) <= 4_194_304:
        raise ValueError(f"{key} must be an integer between 1 and 4194304")
    return int(raw)


def effective_provider_tuning(environment):
    defaults = json.loads((Path(__file__).resolve().parents[1] /
                           "crates/bidding/config/request-defaults.json").read_text())
    context = context_tokens(environment)
    effective = {}
    for field, key in (("output_token_reserve", "KB_AUTHORING_OUTPUT_TOKEN_RESERVE"),
                       ("timeout_ms", "KB_AUTHORING_TIMEOUT_MS")):
        raw = environment.get(key, "").strip()
        if raw:
            if not raw.isascii() or not raw.isdecimal():
                raise ValueError(f"{key} must be a positive integer")
            value = int(raw)
        else:
            value = min(defaults[field], max(1, context // 4)) if field == "output_token_reserve" else defaults[field]
        ceiling = (1 << (32 if field == "output_token_reserve" else 64)) - 1
        if not 0 < value <= ceiling:
            raise ValueError(f"{key} must be a positive integer")
        effective[field] = value
    if effective["output_token_reserve"] + min(4096, max(1, context // 16)) >= context:
        raise ValueError("output reserve plus safety reserve must fit model context")
    return effective
