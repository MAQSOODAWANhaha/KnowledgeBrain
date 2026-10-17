"""Validate/export draft v2 semantic review inputs; never call a model or accept semantics."""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

from docreader.llm.budget import RequestBudget
from docreader.tender.schema import AnalysisExtraction, Decision, SourceView


CORPUS = Path(__file__).resolve().parents[4] / "docs/design/python-tender-phase1/semantic-cases.json"
PROMPT = Path(__file__).resolve().parents[2] / "tender/prompts/verify.md"


def require(condition, detail):
    if not condition:
        raise ValueError(detail)


def check_citation(citation, sources):
    require(citation["range_id"] in sources, "unknown citation range")
    source = sources[citation["range_id"]]
    texts = ([source["text"]] if "text" in source else []) + [cell["text"] for cell in source.get("cells", [])]
    require(citation["quote"].strip() and any(citation["quote"] in text for text in texts),
            "citation must be a visible nonempty source substring")


def check_input(payload):
    require(set(payload) == {"schema_version", "operation", "work_unit_id", "source_version", "domain_revision",
                            "source_views", "items", "relations", "decisions", "dispositions", "issues", "candidates"},
            "unexpected input field or leaked annotation")
    require(payload["schema_version"] == 2 and payload["operation"] == "review", "expected v2 review")
    require(type(payload["source_version"]) is int and payload["source_version"] > 0
            and type(payload["domain_revision"]) is int and payload["domain_revision"] > 0, "invalid revision")
    require(isinstance(payload["work_unit_id"], str) and payload["work_unit_id"].strip(), "missing work unit")
    sources, body, documents = {}, set(), set()
    for view in payload["source_views"]:
        SourceView.model_validate(view)
        require(view.get("document_id") and view.get("document_version") and view.get("role"), "missing document identity")
        require(view["document_id"] not in documents, "duplicate document")
        documents.add(view["document_id"])
        sections = {s["section_id"] for s in view["sections"]}
        require(len(sections) == len(view["sections"]), "duplicate section")
        for source in [*view["body"], *view["supporting"]]:
            require(source["range_id"] not in sources, "duplicate source range")
            require(source.get("section_id") in sections, "unknown source section")
            sources[source["range_id"]] = source
        body.update(s["range_id"] for s in view["body"])
        for binding in view["scope_bindings"]:
            require(binding["range_id"] in sources, "missing bound source")
            require(set(binding["scope_binding"]["applies_to_range_ids"]) <= sources.keys(), "missing bound target")
    require(bool(documents), "missing source views")
    issue_ids = [p.get("issue_id") for p in payload["issues"]]
    require(all(isinstance(i, str) and i.strip() for i in issue_ids)
            and len(set(issue_ids)) == len(issue_ids), "invalid input issue identity")
    AnalysisExtraction.model_validate({**{key: payload[key] for key in
                                      ("schema_version", "items", "relations", "dispositions")},
        "issues": [{k: v for k, v in p.items() if k != "issue_id"} for p in payload["issues"]]})
    items = {item["item_id"]: item for item in payload["items"]}
    require(len(items) == len(payload["items"]), "duplicate item")
    for item in items.values():
        for citation in item["evidence"]:
            check_citation(citation, sources)
    counts = Counter(d["range_id"] for d in payload["dispositions"])
    require(set(counts) <= body and all(n == 1 for n in counts.values()), "invalid omission targets")
    for disposition in payload["dispositions"]:
        expected = {identity for identity, item in items.items()
                    if any(e["range_id"] == disposition["range_id"] for e in item["evidence"])}
        require(set(disposition["item_ids"]) == expected, "disposition citation index disagrees")
    for relation in payload["relations"]:
        require({relation["from_item_id"], relation["to_item_id"]} <= items.keys(), "unknown relation item")
        for citation in relation["evidence"]:
            check_citation(citation, sources)
    for problem in payload["issues"]:
        require(not problem.get("range_id") or problem["range_id"] in sources, "unknown issue source")
        require(set(problem.get("item_ids", [])) <= items.keys(), "unknown issue item")
        require(set(problem.get("related_issue_ids", [])) <= set(issue_ids), "unknown issue continuation")
    decisions = {d["decision_id"]: d for d in payload["decisions"]}
    require(len(decisions) == len(payload["decisions"]), "duplicate decision")
    for decision in decisions.values():
        Decision.model_validate(decision)
        require(set(decision["issue_ids"]) <= set(issue_ids), "unknown decision issue")
        for citation in decision["evidence"]:
            check_citation(citation, sources)
        if amendment := decision.get("amendment"):
            require({amendment["target_item_id"], *amendment["replacement_item_ids"]} <= items.keys(),
                    "unknown amendment item")
            for rule in amendment["retained_rules"]:
                for citation in rule["evidence"]:
                    check_citation(citation, sources)
    # Discovery remains a separate experiment; these are source-review inputs.
    require(payload["candidates"] == [], "unsupported evaluation candidate")
    return sources, items, decisions


def check_corpus(corpus):
    require(corpus["schema_version"] == 2 and corpus["evidence_kind"] == "synthetic_semantic_review_cases",
            "unsupported corpus")
    require(corpus["annotation_status"] == "agent_authored_draft", "annotations must remain draft")
    require(all(corpus[key] is False for key in
                ("human_review_completed", "model_evaluation_completed", "semantic_acceptance")),
            "offline fixtures cannot claim human or model acceptance")
    identities = []
    for case in corpus["cases"]:
        identities.append(case["case_id"])
        require(case["provenance"]["kind"] == "synthetic", "source must be synthetic")
        sources, items, decisions = check_input(case["payload"])
        expected = case["expected"]
        require(expected["verdict"] in {"consistent", "incorrect", "conflict", "unresolved"}, "unknown draft verdict")
        require("needs_source" not in expected or type(expected["needs_source"]) is bool, "invalid route expectation")
        require(set(expected.get("continued_issue_ids", [])) <= {p["issue_id"] for p in case["payload"]["issues"]},
                "unknown expected continuation")
        require(expected["rationale"].strip() and expected["source_evidence"], "missing draft basis")
        for citation in expected["source_evidence"]:
            check_citation(citation, sources)
        require(Counter(expected["reviewed_range_ids"]) == Counter({r: 1 for r in sources}), "draft review coverage incomplete")
        require(set(expected["affected_item_ids"]) <= items.keys(), "unknown affected item")
        for key in ("affected_decision_ids", "accepted_decision_ids"):
            require(set(expected.get(key, [])) <= decisions.keys(), "unknown expected decision")
        if expected["verdict"] == "consistent":
            require(not expected["affected_item_ids"] and not expected.get("affected_decision_ids"), "inconsistent draft labels")
    require(len(identities) == len(set(identities)) and identities, "missing or duplicate cases")
    return {"review_cases": len(identities),
            "cross_document_cases": sum(len(c["payload"]["source_views"]) > 1 for c in corpus["cases"]),
            "decision_cases": sum(bool(c["payload"]["decisions"]) for c in corpus["cases"])}


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def prepare_evaluation(output_dir, corpus_bytes, prompt_bytes, *, budget=None):
    corpus = json.loads(corpus_bytes)
    check_corpus(corpus)
    prompt = prompt_bytes.decode("utf-8")
    require(prompt.splitlines()[0] == "# Independent tender analysis verification — tender-phase1-v2", "requires v2 Verify prompt")
    budget = budget or RequestBudget(prompts={"verify": prompt})
    require(budget.prompts["verify"] == prompt, "budget and exported prompt differ")
    entries = []
    for number, case in enumerate(corpus["cases"], 1):
        record_id = f"v{number:03d}"
        entries.append({"record_id": record_id, "case_id": case["case_id"],
                        "payload_path": f"model-inputs/{record_id}.json", "budget": budget.measure("verify", case["payload"])})
    output_dir.mkdir(parents=True, exist_ok=False)
    (output_dir / "model-inputs").mkdir()
    (output_dir / "review").mkdir()
    (output_dir / "review/corpus.json").write_bytes(corpus_bytes)
    (output_dir / "verify.md").write_bytes(prompt_bytes)
    for entry, case in zip(entries, corpus["cases"], strict=True):
        path = output_dir / entry["payload_path"]
        from docreader.llm.wire import WirePayload

        wire = WirePayload("verify", case["payload"])
        write_json(path, wire.payload)
        mapping_path = f"review/{entry['record_id']}-references.json"
        write_json(output_dir / mapping_path, wire.mapping)
        entry["reference_mapping_path"] = mapping_path
        entry["payload_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    write_json(output_dir / "review/manifest.json", {
        "schema_version": 2, "dataset_id": corpus["dataset_id"], "execution_status": "offline_preparation_only",
        "semantic_acceptance": False, "corpus_path": "review/corpus.json",
        "corpus_sha256": hashlib.sha256(corpus_bytes).hexdigest(), "prompt_path": "verify.md",
        "prompt_sha256": hashlib.sha256(prompt_bytes).hexdigest(), "budget_spec": budget.spec(), "entries": entries,
        "budget": {"planned_requests": len(entries), "input_estimate": sum(e["budget"]["input_tokens"] for e in entries),
                   "max_input_estimate": max(e["budget"]["input_tokens"] for e in entries),
                   "over_budget_requests": sum(e["budget"]["fits"] is False for e in entries)},
    })
    write_json(output_dir / "review/records.json", [{"record_id": e["record_id"], "status": "not_run",
        "annotation_review": None, "model_observation": None, "host_observation": None, "adjudication": None} for e in entries])
    return len(entries)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", type=Path, metavar="NEW_DIRECTORY", help="export immutable inputs and pending review records")
    args = parser.parse_args()
    corpus_bytes = CORPUS.read_bytes()
    result = {"structural_check": "passed", **check_corpus(json.loads(corpus_bytes)), "semantic_acceptance": False}
    if args.prepare is not None:
        result["exported_inputs"] = prepare_evaluation(args.prepare, corpus_bytes, PROMPT.read_bytes())
        result["execution_status"] = "offline_preparation_only"
    print(json.dumps(result))
