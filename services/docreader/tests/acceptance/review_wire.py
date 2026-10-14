"""Review actual SDK serialization locally; keep source payloads private and report only costs."""

import argparse
import asyncio
import copy
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import httpx
from openai import AsyncOpenAI


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--private-root", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--output-private", required=True, type=Path)
    parser.add_argument("--label", required=True)
    return parser.parse_args()


def encoded(v):
    return json.dumps(
        v, ensure_ascii=False, separators=(",", ":"), allow_nan=False
    ).encode()


def digest(v):
    return hashlib.sha256(v).hexdigest()


def contribution(payload, name):
    return max(0, len(encoded(payload[name])) - 2) if name in payload else 0


def coverage_duplicates(payload):
    seen = {}
    duplicate = total = 0
    for r in [*payload.get("body", []), *payload.get("supporting", [])]:
        key = r.get("source_range_id", r["range_id"])
        start = r.get("start", 0)
        end = r.get("end", start + len(r["text"]))
        for a, b in seen.get(key, []):
            duplicate += max(0, min(end, b) - max(start, a))
        seen.setdefault(key, []).append((start, end))
        total += end - start
    return {
        "duplicate_source_characters": duplicate,
        "source_characters": total,
        "source_duplicate_ratio": round(duplicate / max(1, total), 6),
    }


async def main(args):
    sys.path.insert(0, str(args.runtime / "services"))
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.llm.budget import RequestBudget
    from docreader.tender.evidence import EvidenceRepository
    from docreader.tender.requirements import extract_payload

    args.output_private.mkdir(parents=True, exist_ok=True, mode=0o700)
    artifacts = ArtifactStore(args.private_root / "artifacts")
    ref = json.loads((args.private_root / "frozen-index-ref.json").read_text())
    original_digest = digest(args.source.read_bytes())
    plan = artifacts.read_json(ref)
    assert len(plan["documents"]) == 1
    raw = artifacts.read_json(plan["documents"][0]["raw_ref"])
    assert raw["original_ref"]["sha256"] == original_digest
    packs = list(EvidenceRepository(artifacts).iter_packs(ref))
    budget = RequestBudget(context_tokens=131072)
    captured = []

    async def mock(request):
        captured.append(request.content)
        return httpx.Response(
            200,
            json={
                "id": "offline-wire-review",
                "object": "chat.completion",
                "model": "offline",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "{}"},
                        "finish_reason": "stop",
                    }
                ],
            },
        )

    rows = []
    field_totals = {}
    async with AsyncOpenAI(
        api_key="offline-dummy-key",
        base_url="https://offline.invalid/v1",
        max_retries=0,
        http_client=httpx.AsyncClient(transport=httpx.MockTransport(mock)),
    ) as sdk:
        for ordinal, pack in enumerate(packs, 1):
            for task in ("extract", "verify"):
                if task == "extract":
                    payload = extract_payload(pack)
                elif args.label == "before":
                    payload = {
                        "pack_id": pack["pack_id"],
                        "body": pack["body"],
                        "supporting": pack["supporting"],
                        "pending_scope_context": pack.get("pending_scope_context", []),
                        "scope_bindings": pack.get("scope_bindings", []),
                        "requirements": [],
                        "dispositions": [],
                    }
                else:
                    from docreader.tender.evidence.view import source_view

                    payload = {
                        **source_view(pack),
                        "requirements": [],
                        "dispositions": [],
                    }
                request = budget.request(task, payload)
                # The transport captures the actual SDK serialization, never headers.
                # stream responses are deliberately not consumed: serialization is the subject here.
                result = await sdk.chat.completions.with_raw_response.create(**request)
                wire = captured[-1]
                if request.get("stream"):
                    parsed = result.parse()
                    await parsed.close()
                actual = json.loads(wire)
                assert actual == request
                (args.output_private / f"{task}-{ordinal:02}.json").write_bytes(wire)
                prompt = actual["messages"][0]["content"]
                actual_payload = actual["messages"][1]["content"][0]["text"]
                assert json.loads(actual_payload) == payload

                def prose(field):
                    return sum(
                        len(encoded(r.get("text", ""))) - 2
                        for r in payload.get(field, [])
                    )

                body_total = contribution(payload, "body")
                support_total = contribution(payload, "supporting")
                body_prose = prose("body")
                support_prose = prose("supporting")
                scope = sum(
                    contribution(payload, f)
                    for f in ("pending_scope_context", "scope_bindings")
                )
                shared = sum(
                    contribution(payload, f)
                    for f in ("sections", "blank_table_templates")
                )
                prompt_bytes = len(prompt.encode())
                schema_bytes = sum(
                    len(block.split("\n", 1)[-1].encode())
                    for block in prompt.split("```")[1::2]
                )
                categories = {
                    "body_source_text": body_prose,
                    "body_metadata_and_structure": body_total - body_prose,
                    "support_source_text": support_prose,
                    "support_metadata_and_structure": support_total - support_prose,
                    "scope_reference_metadata": scope,
                    "shared_section_and_template_structure": shared,
                    "payload_keys_and_other_fields": len(actual_payload.encode())
                    - body_total
                    - support_total
                    - scope
                    - shared,
                    "system_schema_text": schema_bytes,
                    "system_other_instructions": prompt_bytes - schema_bytes,
                    "http_envelope_and_escaping": len(wire)
                    - len(actual_payload.encode())
                    - prompt_bytes,
                }
                assert sum(categories.values()) == len(wire)
                measured = budget.measure(task, payload)
                fields = {}
                repetitions = {}
                for field in sorted({k for r in payload.get("body", []) for k in r}):
                    stripped = copy.deepcopy(payload)
                    for r in stripped.get("body", []):
                        r.pop(field, None)
                    fields[field] = len(encoded(payload)) - len(encoded(stripped))
                    if task == "extract":
                        field_totals[field] = field_totals.get(field, 0) + fields[field]
                    values = [
                        encoded(r[field]) for r in payload.get("body", []) if field in r
                    ]
                    repetitions[field] = {
                        "occurrences": len(values),
                        "unique_values": len(set(values)),
                        "repeated_value_bytes": sum(map(len, values))
                        - sum(map(len, set(values))),
                    }
                rows.append(
                    {
                        "ordinal": ordinal,
                        "task": task,
                        "pack_id": pack["pack_id"],
                        "boundary_reason": pack["planning_details"]["boundary_reason"],
                        "body_ranges": len(payload.get("body", [])),
                        "support_ranges": len(payload.get("supporting", [])),
                        "template_ranges": sum(
                            len(x["range_ids"])
                            for t in payload.get("blank_table_templates", [])
                            for x in t["rows"]
                        ),
                        "wire_bytes": len(wire),
                        "wire_sha256": digest(wire),
                        "decoded_message_text_bytes": len(actual_payload.encode())
                        + prompt_bytes,
                        "categories_bytes": categories,
                        "categories_percent": {
                            k: round(v / len(wire) * 100, 3)
                            for k, v in categories.items()
                        },
                        "body_field_marginal_payload_bytes": fields,
                        "body_field_repetition": repetitions,
                        "body_source_utf8_bytes": sum(
                            len(r.get("text", "").encode())
                            for r in payload.get("body", [])
                        ),
                        "support_source_utf8_bytes": sum(
                            len(r.get("text", "").encode())
                            for r in payload.get("supporting", [])
                        ),
                        "measurement": measured,
                        "available_input_capacity": 131072
                        - measured["output_reserve"]
                        - measured["safety"],
                        "input_capacity_utilization": round(
                            measured["input_tokens"]
                            / (
                                131072 - measured["output_reserve"] - measured["safety"]
                            ),
                            6,
                        ),
                        **coverage_duplicates(payload),
                        "api_fields": sorted(actual),
                        "tools_count": len(actual.get("tools", [])),
                        "response_format": actual["response_format"],
                        "verify_requirements_note": "Source-only lower bound; no real extraction has been run."
                        if task == "verify"
                        else None,
                    }
                )
    aggregate = {}
    for task in ("extract", "verify"):
        values = [r for r in rows if r["task"] == task]
        aggregate[task] = {
            "request_count": len(values),
            "wire_bytes": sum(r["wire_bytes"] for r in values),
            "body_source_utf8_bytes": sum(r["body_source_utf8_bytes"] for r in values),
            "support_source_utf8_bytes": sum(
                r["support_source_utf8_bytes"] for r in values
            ),
            "decoded_message_text_bytes": sum(
                r["decoded_message_text_bytes"] for r in values
            ),
            "estimated_input_tokens": sum(
                r["measurement"]["input_tokens"] for r in values
            ),
            "categories_bytes": {
                k: sum(r["categories_bytes"][k] for r in values)
                for k in values[0]["categories_bytes"]
            },
            "duplicate_source_characters": sum(
                r["duplicate_source_characters"] for r in values
            ),
            "over_capacity": sum(r["measurement"]["fits"] is False for r in values),
        }
    original = args.source
    for p in args.output_private.glob("*.json"):
        for name in os.listxattr(original):
            os.setxattr(p, name, os.getxattr(original, name))
        p.chmod(0o600)
    report = {
        "schema_version": 1,
        "evidence_kind": "actual_sdk_wire_review_" + args.label,
        "runtime_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=args.runtime, text=True
        ).strip(),
        "runtime_dirty": bool(subprocess.check_output(
            ["git", "status", "--porcelain", "--", "services/docreader"], cwd=args.runtime, text=True
        ).strip()),
        "runtime_source_sha256": {
            str(path.relative_to(args.runtime)): digest(path.read_bytes())
            for directory in ("llm", "tender")
            for path in sorted((args.runtime / "services/docreader" / directory).rglob("*"))
            if path.is_file() and path.suffix in {".py", ".md"}
        },
        "source_sha256": original_digest,
        "model_calls": 0,
        "network_calls": 0,
        "actual_provider_usage": None,
        "capture": "AsyncOpenAI + local httpx.MockTransport; request.content final bytes; dummy credential and offline.invalid, no network",
        "counting": "UTF8 byte categories sum exactly to captured HTTP body; model input estimate is explicitly conservative, not provider tokenizer/usage; field marginal costs overlap category totals and must not be added twice.",
        "aggregate": aggregate,
        "extract_body_field_marginal_payload_bytes": field_totals,
        "requests": rows,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(
        json.dumps(
            {"report": str(args.report), "aggregate": aggregate}, ensure_ascii=False
        )
    )


if __name__ == "__main__":
    asyncio.run(main(parse_args()))
