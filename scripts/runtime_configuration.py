"""Read the same nonsecret request defaults embedded by the Rust runtime."""
import json
from pathlib import Path


def effective_provider_tuning(environment):
    defaults = json.loads((Path(__file__).resolve().parents[1] /
                           "crates/bidding/config/request-defaults.json").read_text())
    effective = {}
    for field, key in (("output_token_reserve", "KB_AUTHORING_OUTPUT_TOKEN_RESERVE"),
                       ("timeout_ms", "KB_AUTHORING_TIMEOUT_MS")):
        raw = environment.get(key, "").strip()
        if raw:
            if not raw.isascii() or not raw.isdecimal():
                raise ValueError(f"{key} must be a positive integer")
            value = int(raw)
        else:
            value = defaults[field]
        ceiling = (1 << (32 if field == "output_token_reserve" else 64)) - 1
        if not 0 < value <= ceiling:
            raise ValueError(f"{key} must be a positive integer")
        effective[field] = value
    return effective
