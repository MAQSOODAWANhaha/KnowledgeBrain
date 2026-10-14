"""Offline final SDK wire audit with real sources and explicitly synthetic outputs.

No model endpoint is contacted. Private source, synthetic evidence quotes and wire
bodies stay below output-private; the report contains counts, hashes and costs.
This budget/coverage experiment cannot confirm source scope or model semantics.
"""

import argparse
import asyncio
from collections import Counter
from copy import deepcopy
import json
from pathlib import Path
import subprocess

import httpx
from openai import AsyncOpenAI

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.checks.verify import verify_payload
from docreader.tender.evidence import EvidenceRepository
from docreader.tender.evidence.downstream import plan_downstream, select_extraction
from docreader.tender.evidence.view import host_reviewed_ranges, source_view
from docreader.tender.planning.plan import plan_payload
from docreader.tender.requirements.extract import extract_payload, validate_correction

from prepare_real_pdf import propagate_private_attributes, validate_coverage
from review_wire import contribution, coverage_duplicates, digest, encoded


def synthetic_extractions(packs):
    """Schema/evidence-valid stress inputs; never attributed to the source/model."""
    results = []
    for pack in packs:
        host = host_reviewed_ranges(pack)
        requirements, dispositions = [], []
        for source in pack["body"]:
            range_id = source["range_id"]
            if source["text"].strip() and range_id not in host:
                requirement_id = "synthetic-" + digest(range_id.encode())[:24]
                requirements.append({
                    "requirement_id": requirement_id,
                    "text": "SYNTHETIC budget fixture: review the cited source before drafting a response.",
                    "subject": "SYNTHETIC offline audit fixture; not an extracted tender obligation",
                    "evidence": [{"range_id": range_id, "quote": source["text"]}],
                    "conditions": ["SYNTHETIC applicability condition retained in every downstream request."],
                    "exceptions": ["SYNTHETIC exception retained in every downstream request."],
                    "numbers": [], "scoring": {}, "veto_risk": False,
                    "cross_references": [], "applicable": True,
                    "applicability_basis": "SYNTHETIC stress fixture only; source applicability remains unaccepted.",
                })
                dispositions.append({"range_id": range_id, "status": "requirements",
                                     "reason": "SYNTHETIC evidence-backed budget fixture; not a semantic disposition.",
                                     "requirement_ids": [requirement_id]})
            else:
                dispositions.append({"range_id": range_id, "status": "non_requirement",
                                     "reason": "SYNTHETIC fixture: preserve this empty template's fields and source identity for independent review.",
                                     "requirement_ids": []})
        results.append(validate_correction(pack, {"requirements": requirements,
                                                 "dispositions": dispositions, "issues": []}))
    # A small, explicit dependency crosses the original extraction-pack boundary.
    # It is a test dependency, not a claim about the source's real obligations.
    nonempty = [result for result in results if result["requirements"]]
    if len(nonempty) > 1:
        first = nonempty[0]["requirements"][-1]
        second = nonempty[1]["requirements"][0]
        first["cross_references"] = [second["requirement_id"]]
        second["cross_references"] = [first["requirement_id"]]
        if len(nonempty) > 2:
            first["cross_references"].append(nonempty[2]["requirements"][0]["evidence"][0]["range_id"])
    return results


def costs(payload, actual, wire):
    prompt = actual["messages"][0]["content"]
    text = actual["messages"][1]["content"][0]["text"]
    assert json.loads(text) == payload
    view = payload.get("source_context", payload)
    body = contribution(view, "body")
    support = contribution(view, "supporting")
    body_text = sum(len(encoded(row.get("text", ""))) - 2 for row in view.get("body", []))
    support_text = sum(len(encoded(row.get("text", ""))) - 2 for row in view.get("supporting", []))
    scope = sum(contribution(view, field) for field in ("pending_scope_context", "scope_bindings"))
    aliases = contribution(view, "source_aliases")
    shared = sum(contribution(view, field) for field in ("sections", "blank_table_templates"))
    requirements = contribution(payload, "requirements")
    dispositions = contribution(payload, "dispositions")
    constraints = contribution(payload, "constraint_bindings")
    prompt_bytes = len(prompt.encode())
    schema = sum(len(block.split("\n", 1)[-1].encode()) for block in prompt.split("```")[1::2])
    values = {
        "body_source_text": body_text, "body_metadata_and_structure": body - body_text,
        "support_source_text": support_text, "support_metadata_and_structure": support - support_text,
        "scope_reference_metadata": scope, "shared_section_and_template_structure": shared,
        "source_alias_metadata": aliases,
        "synthetic_requirements_with_exact_source_quotes": requirements,
        "synthetic_dispositions": dispositions, "constraint_bindings": constraints,
        "payload_keys_and_other_fields": len(text.encode()) - body - support - scope - shared
        - requirements - dispositions - constraints - aliases,
        "system_schema_text": schema, "system_other_instructions": prompt_bytes - schema,
        "http_envelope_and_escaping": len(wire) - len(text.encode()) - prompt_bytes,
    }
    assert min(values.values()) >= 0 and sum(values.values()) == len(wire)
    return values


def assert_source_view_complete(pack, payload):
    """Check the actual sending projection, including text-free citation aliases."""
    sources = {row["range_id"]: row for row in [*pack["body"], *pack["supporting"]]}
    host = host_reviewed_ranges(pack)
    sent = {row["range_id"]: row for row in [*payload["body"], *payload["supporting"]]}
    templates = {range_id for template in payload.get("blank_table_templates", [])
                 for row in template["rows"] for range_id in row["range_ids"]}
    aliases = {row["range_id"]: row for row in payload.get("source_aliases", [])}
    assert set(sources) - host <= set(sent) | templates | set(aliases)
    alias_segments = {segment["range_id"] for alias in aliases.values() for segment in alias["segments"]}
    assert set(sent) - set(sources) <= alias_segments
    assert templates | set(aliases) <= set(sources)
    for range_id, source in sources.items():
        if range_id in host:
            continue
        if range_id in templates:
            assert not source["text"].strip() and source["non_requirement_reason"] == "empty_table_row"
        elif range_id in sent:
            assert sent[range_id]["text"] == source["text"]
        else:
            cursor, pieces = source["start"], []
            for segment in aliases[range_id]["segments"]:
                assert segment["start"] == cursor
                row = sent[segment["range_id"]]
                offset = row.get("start", 0)
                pieces.append(row["text"][segment["start"] - offset:segment["end"] - offset])
                cursor = segment["end"]
            assert cursor == source["end"] and "".join(pieces) == source["text"]


async def audit(args):
    args.output_private.mkdir(parents=True, exist_ok=True, mode=0o700)
    args.output_private.chmod(0o700)
    artifacts = ArtifactStore(args.private_root / "artifacts")
    frozen_ref = json.loads((args.private_root / "frozen-index-ref.json").read_text())
    index = artifacts.read_json(frozen_ref)
    source_digest = digest(args.source.read_bytes())
    assert len(index["documents"]) == 1
    raw = artifacts.read_json(index["documents"][0]["raw_ref"])
    assert raw["original_ref"]["sha256"] == source_digest
    repository = EvidenceRepository(artifacts)
    originals = list(repository.iter_packs(frozen_ref))
    extractions = synthetic_extractions(originals)
    extraction_entries = [{"pack_id": result["pack_id"], "artifact_ref": artifacts.put_json(result)}
                          for result in extractions]
    requirements_ref = artifacts.put_json({"packs": extraction_entries})
    by_sha = {entry["artifact_ref"]["sha256"]: result for entry, result in zip(extraction_entries, extractions)}
    budget = RequestBudget(context_tokens=131072)
    original_measures = [budget.measure("verify", verify_payload(pack, extraction))
                         for pack, extraction in zip(originals, extractions)]
    verify_ref = plan_downstream(index, requirements_ref, artifacts, budget, task="verify")
    verification_plan = artifacts.read_json(verify_ref)
    verify_packs = list(repository.iter_packs(verify_ref))
    # This is only the planner's input contract, not a successful Verification or
    # confirmation artifact. Original blockers remain in the report and index.
    hypothetical_checked_ref = artifacts.put_json({
        "applicability_decisions": {}, "synthetic_budget_probe_only": True,
        "source_scope_blockers_preserved": len(index["issues"]),
    })
    plan_ref = plan_downstream(index, requirements_ref, artifacts, budget, task="plan",
                               verification_ref=hypothetical_checked_ref)
    response_plan = artifacts.read_json(plan_ref)
    plan_packs = list(repository.iter_packs(plan_ref))
    (args.output_private / "audit-input-refs.json").write_bytes(encoded({
        "source_plan_ref": frozen_ref, "synthetic_requirements_ref": requirements_ref,
        "verification_plan_ref": verify_ref, "response_plan_probe_ref": plan_ref,
        "hypothetical_checked_ref": hypothetical_checked_ref,
    }))
    all_requirements = [item for extraction in extractions for item in extraction["requirements"]]
    expected_requirements = {item["requirement_id"]: item for item in all_requirements}
    expected_ranges = Counter(row["range_id"] for pack in originals for row in pack["body"])
    selected_verify = [select_extraction(pack, by_sha) for pack in verify_packs]
    selected_plan = [select_extraction(pack, by_sha) for pack in plan_packs]
    assert Counter(row["range_id"] for pack in verify_packs for row in pack["body"]) == expected_ranges
    assert Counter(row["range_id"] for selected in selected_verify for row in selected["dispositions"]) == expected_ranges
    for pack, selected in [*zip(verify_packs, selected_verify), *zip(plan_packs, selected_plan)]:
        ids = {item["requirement_id"] for item in selected["requirements"]}
        source_ids = {item["range_id"] for item in [*pack["body"], *pack["supporting"]]}
        for item in selected["requirements"]:
            assert item == expected_requirements[item["requirement_id"]]
            assert set(item["cross_references"]).issubset(ids | source_ids)
    for selections in (selected_verify, selected_plan):
        assert Counter(item["requirement_id"] for selected in selections for item in selected["requirements"]) == Counter(expected_requirements.keys())
    coverage = validate_coverage(raw, verify_packs, index)
    assert coverage["passed"]
    captured = []

    async def mock(request):
        captured.append(request.content)
        return httpx.Response(200, json={"id": "offline-synthetic-wire-review", "object": "chat.completion",
                                        "model": "offline", "choices": []})

    rows = []
    inputs = [("extract", pack, extract_payload(pack)) for pack in originals]
    inputs += [("verify", pack, verify_payload(pack, selected)) for pack, selected in zip(verify_packs, selected_verify)]
    inputs += [("plan", pack, plan_payload(selected["requirements"], source_view(pack) if pack["supporting"] else None))
               for pack, selected in zip(plan_packs, selected_plan)]
    async with AsyncOpenAI(api_key="offline-dummy-key", base_url="https://offline.invalid/v1", max_retries=0,
                           http_client=httpx.AsyncClient(transport=httpx.MockTransport(mock))) as sdk:
        for ordinal, (task, pack, payload) in enumerate(inputs, 1):
            if task in {"extract", "verify"}:
                assert_source_view_complete(pack, payload)
            elif "source_context" in payload:
                assert_source_view_complete(pack, payload["source_context"])
            measured = budget.measure(task, payload)
            assert measured["fits"] and not measured["correction"]
            assert pack["planning_details"]["budget"] == measured
            request = budget.request(task, payload)
            result = await sdk.chat.completions.with_raw_response.create(**request)
            await result.parse().close()
            wire = captured[-1]
            actual = json.loads(wire)
            assert actual == request and actual["stream"] is True
            assert "max_tokens" not in actual and "max_completion_tokens" not in actual
            (args.output_private / f"{ordinal:02}-{task}.json").write_bytes(wire)
            categories = costs(payload, actual, wire)
            view = payload.get("source_context", payload)
            field_costs, repeated_values = {}, {}
            for field in sorted({key for row in view.get("body", []) for key in row}):
                stripped = deepcopy(payload)
                for row in stripped.get("source_context", stripped).get("body", []):
                    row.pop(field, None)
                field_costs[field] = len(encoded(payload)) - len(encoded(stripped))
                values = [encoded(row[field]) for row in view.get("body", []) if field in row]
                repeated_values[field] = {"occurrences": len(values), "unique_values": len(set(values)),
                                          "repeated_value_bytes": sum(map(len, values)) - sum(map(len, set(values)))}
            rows.append({
                "ordinal": ordinal, "task": task, "pack_id": pack["pack_id"],
                "boundary_reason": pack["planning_details"]["boundary_reason"],
                "wire_sha256": digest(wire), "wire_bytes": len(wire), "measurement": measured,
                "input_capacity_utilization": round(measured["input_tokens"] / budget.effective_input_budget_tokens, 6),
                "categories_bytes": categories,
                "categories_percent": {key: round(value / len(wire) * 100, 3) for key, value in categories.items()},
                "body_field_marginal_payload_bytes": field_costs, "body_field_repetition": repeated_values,
                "body_ranges": len(view.get("body", [])), "support_ranges": len(view.get("supporting", [])),
                "body_source_utf8_bytes": sum(len(row["text"].encode()) for row in view.get("body", [])),
                "support_source_utf8_bytes": sum(len(row["text"].encode()) for row in view.get("supporting", [])),
                "source_context_present": "source_context" in payload,
                "source_aliases": len(view.get("source_aliases", [])),
                "template_ranges": sum(len(row["range_ids"]) for template in view.get("blank_table_templates", [])
                                       for row in template["rows"]),
                "requirements": len(payload.get("requirements", [])), "dispositions": len(payload.get("dispositions", [])),
                "exact_requirement_quote_utf8_bytes": sum(len(evidence["quote"].encode())
                                                         for item in payload.get("requirements", []) for evidence in item["evidence"]),
                "api_fields": sorted(actual), **coverage_duplicates(view),
            })
    aggregate = {}
    for task in ("extract", "verify", "plan"):
        selected = [row for row in rows if row["task"] == task]
        aggregate[task] = {
            "requests": len(selected), "wire_bytes": sum(row["wire_bytes"] for row in selected),
            "estimated_input_units": sum(row["measurement"]["input_tokens"] for row in selected),
            "over_capacity": sum(not row["measurement"]["fits"] for row in selected),
            "requirements": sum(row["requirements"] for row in selected),
            "dispositions": sum(row["dispositions"] for row in selected),
            "body_source_utf8_bytes": sum(row["body_source_utf8_bytes"] for row in selected),
            "support_source_utf8_bytes": sum(row["support_source_utf8_bytes"] for row in selected),
            "categories_bytes": {key: sum(row["categories_bytes"][key] for row in selected)
                                 for key in selected[0]["categories_bytes"]},
            "duplicate_source_characters": sum(row["duplicate_source_characters"] for row in selected),
        }
    report = {
        "schema_version": 1, "evidence_kind": "real_source_synthetic_complete_downstream_sdk_wire_review",
        "runtime_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.runtime, text=True).strip(),
        "runtime_dirty": bool(subprocess.check_output(
            ["git", "status", "--porcelain", "--", "services/docreader"], cwd=args.runtime, text=True
        ).strip()),
        "runtime_source_sha256": {str(path.relative_to(args.runtime)): digest(path.read_bytes())
                                  for directory in ("llm", "tender", "workflow")
                                  for path in sorted((args.runtime / "services/docreader" / directory).rglob("*"))
                                  if path.is_file() and path.suffix in {".py", ".md"}},
        "source_sha256": source_digest, "source_bytes": args.source.stat().st_size,
        "budget": budget.spec(), "effective_input_budget_tokens": budget.effective_input_budget_tokens,
        "model_calls": 0, "network_calls": 0, "provider_usage": None, "semantic_acceptance": False,
        "capture": "Actual AsyncOpenAI serialization through local MockTransport; offline.invalid; dummy key; no network.",
        "counting": "UTF8 costs and conservative decoded-input estimate, not provider tokenizer or billed usage.",
        "synthetic_output": {
            "requirements": len(all_requirements), "dispositions": sum(len(item["dispositions"]) for item in extractions),
            "method": "One explicitly synthetic requirement per nonempty non-host range, full exact source quote, explicit synthetic conditions/exceptions; complete dispositions. One bidirectional requirement dependency and one source-range reference cross original extraction packs; the latter exercises plan source_context.",
            "schema_and_deterministic_evidence_valid": True, "real_model_output": False,
            "source_applicability_confirmed": False,
        },
        "unresolved_source_issues": dict(Counter(item["code"] for item in index["issues"])),
        "verification_source_coverage": coverage,
        "actual_source_view_text_templates_and_aliases_checked": True,
        "all_requirements_dispositions_and_cross_pack_dependencies_preserved": True,
        "original_complete_verify_requests": original_measures,
        "original_complete_verify_over_capacity": sum(not value["fits"] for value in original_measures),
        "downstream_plan_digests": {"verify": verification_plan["plan_digest"], "plan": response_plan["plan_digest"]},
        "aggregate": aggregate, "requests": rows,
        "field_necessity_review": {
            "body": "Exact source text and stable IDs let independent verification inspect every source; retained once in the source view.",
            "supporting": "Required conditions, headers and cited evidence outside body remain available; overlapping source text uses explicit aliases.",
            "source_aliases": "Text-free cited identity plus exact intervals preserves provenance when the same source text is already in body.",
            "scope": "Bindings and unresolved references preserve applicability and remaining uncertainty; pending metadata is not treated as reviewed text.",
            "templates": "Blank template ranges retain field/merge/row structure and individual identities; they still receive dispositions and verification.",
            "requirements_dispositions": "Independent review needs the complete proposed output, including evidence, conditions, exceptions and all range dispositions.",
            "synthetic_evidence_quotes": "This stress fixture intentionally quotes each entire range. These bytes are not a prediction of real output or a recommendation for minimal production quotes.",
            "constraint_bindings": "Digest binds response planning to each complete immutable requirement without another constraint-text copy.",
            "prompt_schema": "Task instructions and one strict output schema are present in system content; no duplicated tools/json_schema envelope.",
            "host_images": "Only the exact source/hash/position-reviewed decorative occurrences are omitted from wire; canonical images and review references remain private.",
        },
        "real_semantic_acceptance_ready": False,
        "limitations": [
            "Synthetic output sizes and semantics are not actual extraction output or predicted billed usage.",
            "Response planning is an offline capacity probe; no review, confirmation or scope blocker was cleared.",
            "Actual provider total context and tokenizer remain unconfirmed; configured total context stays 131072.",
            "Ordinary input budget 131072 and correction allowance 262144 are token limits, not byte limits; this ordinary probe uses no correction allowance.",
            "Image dispositions are direct agent visual review with source/hash/position binding, not completed human semantic acceptance.",
            "Thirteen source scope issues, human labels and real model semantic acceptance remain outstanding.",
        ],
    }
    propagate_private_attributes(args.source, args.private_root)
    propagate_private_attributes(args.source, args.output_private)
    for path in args.output_private.iterdir():
        if path.is_file():
            path.chmod(0o600)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"report": str(args.report), "aggregate": aggregate,
                      "original_complete_verify_over_capacity": report["original_complete_verify_over_capacity"],
                      "unresolved_source_issues": report["unresolved_source_issues"]}))


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("source", "runtime", "private-root", "output-private", "report"):
        parser.add_argument("--" + option, required=True, type=Path)
    return parser.parse_args()


if __name__ == "__main__":
    asyncio.run(audit(parse_args()))
