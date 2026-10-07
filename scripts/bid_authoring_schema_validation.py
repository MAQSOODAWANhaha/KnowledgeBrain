#!/usr/bin/env python3
"""Draft 2020-12 checks for the live bidding tool schemas."""

from __future__ import annotations

import json
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_DIR = ROOT / "crates/bidding/schemas"
LIVE = {
    "outline-tools-v1.schema.json",
    "response-tools-v1.schema.json",
    "tender-analysis-tools-v1.schema.json",
}


def main() -> None:
    present = {path.name for path in SCHEMA_DIR.glob("*.schema.json")}
    assert present == LIVE, f"schema directory drifted: {sorted(present)}"
    for name in ("outline-tools-v1.schema.json", "response-tools-v1.schema.json"):
        schema = json.loads((SCHEMA_DIR / name).read_text(encoding="utf-8"))
        Draft202012Validator.check_schema({"type": "array", "items": schema[0]})
        for tool in schema:
            parameters = tool["function"]["parameters"]
            assert parameters.get("additionalProperties") is False, name
            Draft202012Validator.check_schema(parameters)
            validator = Draft202012Validator(parameters)
            assert list(validator.iter_errors({"unexpected": True})), name
    print(f"live schema validation: {len(LIVE)} files: PASS")


if __name__ == "__main__":
    main()
