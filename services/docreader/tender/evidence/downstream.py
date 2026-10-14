"""Complete verify/plan requests over immutable extraction and source references."""

from copy import deepcopy
from itertools import groupby
import json

from docreader.tender.checks.aggregate import apply_applicability_decisions
from docreader.tender.checks.verify import verify_payload
from docreader.tender.evidence.freeze import _digest
from docreader.tender.evidence.planner import StructuralPlanner
from docreader.tender.evidence.references import EvidenceRepository, selector
from docreader.tender.planning.plan import plan_payload
from docreader.tender.schema import PackDescriptor, PackPlan, ResponsePackDescriptor, issue


PLANNER_VERSION = "complete-downstream-input-v1"


def _components(count, links):
    """Connected obligations are indivisible; unrelated intervening rows are not."""
    parents = list(range(count))

    def root(index):
        while parents[index] != index:
            parents[index] = parents[parents[index]]
            index = parents[index]
        return index

    for link in links:
        if link:
            first = root(link[0])
            for index in link[1:]:
                parents[root(index)] = first
    result = {}
    for index in range(count):
        result.setdefault(root(index), []).append(index)
    return list(result.values())


def _groups(components, key):
    return [[item for component in group for item in component]
            for _, group in groupby(components, key=lambda component: tuple(dict.fromkeys(key(item) for item in component)))]


def select_extraction(pack, extraction):
    requirements = set(pack["requirement_ids"])
    ranges = set(pack["disposition_range_ids"])
    return {"requirements": [item for item in extraction["requirements"] if item["requirement_id"] in requirements],
            "dispositions": [item for item in extraction["dispositions"] if item["range_id"] in ranges]}


class VerificationPlanner(StructuralPlanner):
    def __init__(self, source, budget, original, extraction, requirements_ref, scope_decisions):
        super().__init__(source, budget, scope_decisions)
        self.original, self.extraction, self.requirements_ref = original, extraction, requirements_ref
        self.sources = {item["range_id"]: item for item in [*original["supporting"], *original["body"]]}
        body = original["body"]
        positions = {item["range_id"]: index for index, item in enumerate(body)}
        self.anchors = {}
        for requirement in extraction["requirements"]:
            requirement_id = requirement["requirement_id"]
            ids = {e["range_id"] for e in requirement["evidence"]}
            ids.update(requirement["cross_references"])
            ids.update(item["range_id"] for item in extraction["dispositions"] if requirement_id in item["requirement_ids"])
            # Invalid evidence still reaches deterministic review, never vanishes
            # while repartitioning an otherwise well-formed model response.
            self.anchors.setdefault(requirement_id, set()).update(positions[r] for r in ids if r in positions)
            if not self.anchors[requirement_id]:
                self.anchors[requirement_id].add(0)
        links = [sorted(anchors) for anchors in self.anchors.values()]
        for requirement in extraction["requirements"]:
            links.append(sorted(set.union(self.anchors[requirement["requirement_id"]], *(
                self.anchors[ref] for ref in requirement["cross_references"] if ref in self.anchors))))
        self.components = [[body[index] for index in component] for component in _components(len(body), links)]
        self.positions = positions

    def candidate(self, body, reason):
        body_ids = {item["range_id"] for item in body}
        positions = {self.positions[range_id] for range_id in body_ids}
        requirement_ids = [item["requirement_id"] for item in self.extraction["requirements"]
                           if self.anchors[item["requirement_id"]] & positions]
        requirements = [item for item in self.extraction["requirements"] if item["requirement_id"] in set(requirement_ids)]
        evidence_ids = {e["range_id"] for item in requirements for e in item["evidence"]}
        evidence_ids.update(ref for item in requirements for ref in item["cross_references"] if ref in self.sources)
        evidence = [value for key, value in self.sources.items() if key in evidence_ids - body_ids]
        support, pending, problems, bindings = self.support.select([*body, *evidence])
        supporting = list(evidence)
        for value in support:
            supporting.extend(self.support._uncovered(value, [*body, *supporting]))
        pack = {"schema_version": 2, "pack_id": "verify-" + _digest(sorted(body_ids))[:24],
                **{key: self.original[key] for key in ("document_id", "document_version", "raw_ref", "source_index_ref")},
                "body": [selector(item) for item in body], "supporting": [selector(item) for item in supporting],
                "pending_scope_context": pending, "scope_bindings": bindings, "issues": problems,
                "compacted_blank_ranges": [item["range_id"] for item in body if item.get("empty")],
                "requirements_ref": self.requirements_ref, "requirement_ids": requirement_ids,
                "disposition_range_ids": [item["range_id"] for item in body],
                "planning_details": {"planner_version": PLANNER_VERSION, "boundary_reason": reason,
                                     "section_ids": list(dict.fromkeys(item.get("section_id") for item in body))}}
        materialized = {**pack, "body": body, "supporting": supporting}
        measured = self.budget.measure("verify", verify_payload(materialized, select_extraction(pack, self.extraction)))
        pack["planning_details"]["budget"] = measured
        self.probe_count += 1
        self.capacity_hint = max(1, sum(self.weight(item) for item in body)
                                 * max(1, measured["effective_input_budget_tokens"]) // max(1, measured["input_tokens"]))
        return pack

    def blocked(self, pack):
        pack["issues"].append(issue("indivisible_verify_input_exceeded", "A complete source disposition and its obligation dependencies exceed the request budget.", pack_id=pack["pack_id"]))
        pack["planning_details"]["boundary_reason"] = "indivisible_dependency_input_exceeded"
        self.packs.append(pack)

    def split_structure(self, entries, rejected):
        ids = {entry["range_id"] for entry in entries}
        components = [component for component in self.components if component[0]["range_id"] in ids]
        self.partition(components, "complete_obligation_dependencies", lambda entries, pack: self.blocked(pack))

    def split_section(self, entries, rejected):
        ids = {entry["range_id"] for entry in entries}
        components = [component for component in self.components if component[0]["range_id"] in ids]
        groups = _groups(components, lambda entry: tuple(entry.get("table_ids") or [entry.get("table_id") or entry["unit_id"]]))
        self.partition(groups, "native_structure_prefix", self.split_structure)

    def plan(self):
        self.partition(_groups(self.components, lambda entry: entry.get("section_id")),
                       "complete_adjacent_sections", self.split_section)
        return self.packs


class ResponsePlanner(StructuralPlanner):
    """Reuse measured prefix selection; a response obligation is never sliced."""
    def __init__(self, budget, original, requirements, requirements_ref):
        self.budget, self.original, self.requirements_ref = budget, original, requirements_ref
        self.packs, self.probe_count, self.capacity_hint = [], 0, None
        self.sources = {item["range_id"]: item for item in [*original["supporting"], *original["body"]]}
        positions = {item["requirement_id"]: index for index, item in enumerate(requirements)}
        links = [[index, *(positions[ref] for ref in item["cross_references"] if ref in positions)]
                 for index, item in enumerate(requirements)]
        self.components = [[requirements[index] for index in component] for component in _components(len(requirements), links)]

    def weight(self, entry):
        return len(json.dumps(entry, ensure_ascii=False).encode())

    def anchor(self, requirement):
        return next((self.sources[e["range_id"]] for e in requirement["evidence"] if e["range_id"] in self.sources), {})

    def candidate(self, requirements, reason):
        measured = self.budget.measure("plan", plan_payload(requirements))
        self.probe_count += 1
        self.capacity_hint = max(1, sum(self.weight(item) for item in requirements)
                                 * max(1, measured["effective_input_budget_tokens"]) // max(1, measured["input_tokens"]))
        ids = [item["requirement_id"] for item in requirements]
        return {"schema_version": 2, "pack_id": "plan-" + _digest(ids)[:24],
                **{key: self.original[key] for key in ("document_id", "document_version")},
                "requirements_ref": self.requirements_ref, "requirement_ids": ids, "issues": [],
                "planning_details": {"planner_version": PLANNER_VERSION, "boundary_reason": reason,
                                     "section_ids": list(dict.fromkeys(self.anchor(item).get("section_id") for item in requirements)),
                                     "budget": measured}}

    def blocked(self, pack):
        pack["issues"].append(issue("indivisible_plan_input_exceeded", "A complete response obligation and its dependencies exceed the request budget.", pack_id=pack["pack_id"]))
        pack["planning_details"]["boundary_reason"] = "indivisible_dependency_input_exceeded"
        self.packs.append(pack)

    def split_structure(self, entries, rejected):
        ids = {entry["requirement_id"] for entry in entries}
        components = [component for component in self.components if component[0]["requirement_id"] in ids]
        self.partition(components, "complete_obligation_dependencies", lambda entries, pack: self.blocked(pack))

    def split_section(self, entries, rejected):
        ids = {entry["requirement_id"] for entry in entries}
        components = [component for component in self.components if component[0]["requirement_id"] in ids]
        self.partition(_groups(components, lambda item: self.anchor(item).get("unit_id")),
                       "native_structure_prefix", self.split_structure)

    def plan(self):
        self.partition(_groups(self.components, lambda item: self.anchor(item).get("section_id")),
                       "complete_adjacent_sections", self.split_section)
        return self.packs


def plan_downstream(index, requirements_ref, artifacts, budget, *, task, verification_ref=None):
    """Persist stage-owned selectors, not another copy of frozen text or outputs."""
    repository = EvidenceRepository(artifacts)
    extraction_index = artifacts.read_json(requirements_ref)
    extractions = {entry["pack_id"]: entry["artifact_ref"] for entry in extraction_index["packs"]}
    checked = artifacts.read_json(verification_ref) if verification_ref else None
    if task not in {"verify", "plan"} or (task == "plan" and checked is None):
        raise ValueError("downstream planning requires its complete stage inputs")
    planned, probes = [], 0
    for entry in index["packs"]:
        extraction_ref = extractions[entry["pack_id"]]
        extraction = artifacts.read_json(extraction_ref)
        original = repository.load_pack(entry)
        if task == "verify":
            source = repository.source_index(original["source_index_ref"])
            planner = VerificationPlanner(source, budget, original, extraction, extraction_ref, index["scope_decisions"])
        else:
            requirements = [item for item in apply_applicability_decisions(extraction["requirements"], checked["applicability_decisions"])
                            if item["applicable"]]
            planner = ResponsePlanner(budget, original, requirements, extraction_ref)
        planned.extend(planner.plan())
        probes += planner.probe_count
    budget_digest = _digest(budget.spec())
    budget_spec_ref = artifacts.put_json(budget.spec())
    inputs = {"source_plan_digest": index["plan_digest"], "requirements_ref": requirements_ref,
              "verification_ref": verification_ref, "run_identity": index.get("run_identity")}
    input_digest = _digest(inputs)
    plan_digest = _digest({"task": task, "input_digest": input_digest, "budget_digest": budget_digest, "packs": planned})
    packs = []
    for pack in planned:
        pack.update(task=task, input_digest=input_digest, budget_digest=budget_digest, plan_digest=plan_digest)
        pack["planning_details"]["budget_spec_ref"] = budget_spec_ref
        (PackDescriptor if task == "verify" else ResponsePackDescriptor).model_validate(pack)
        packs.append({"pack_id": pack["pack_id"], "document_id": pack["document_id"], "artifact_ref": artifacts.put_json(pack),
                      "range_count": len(pack.get("body", [])), "plan_digest": plan_digest})
    result = {**deepcopy(index), "task": task, "input_digest": input_digest, "budget_digest": budget_digest,
              "plan_digest": plan_digest, "budget_spec_ref": budget_spec_ref, "packs": packs,
              "requirements_ref": requirements_ref, "range_count": sum(item["range_count"] for item in packs),
              "issues": [problem for pack in planned for problem in pack["issues"]],
              "stats": {**index["stats"], "pack_count": len(packs), "probe_count": probes}}
    if verification_ref:
        result["verification_ref"] = verification_ref
    PackPlan.model_validate(result)
    return artifacts.put_json(result)
