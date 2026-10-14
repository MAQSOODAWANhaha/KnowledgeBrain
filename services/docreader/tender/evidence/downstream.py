"""Complete verify/plan requests over immutable extraction and source references."""

from collections import Counter, defaultdict
from copy import deepcopy
from itertools import groupby
import json

from docreader.tender.checks.aggregate import apply_applicability_decisions
from docreader.tender.checks.verify import verify_payload
from docreader.tender.evidence.freeze import _digest
from docreader.tender.evidence.planner import StructuralPlanner
from docreader.tender.evidence.references import EvidenceRepository, selector
from docreader.tender.planning.plan import plan_payload
from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.schema import PackDescriptor, PackPlan, ResponsePackDescriptor, issue


PLANNER_VERSION = "complete-downstream-input-v2"
DEPENDENCY_ISSUES = frozenset({"dependency_reference_unresolved", "cross_document_dependency", "inactive_requirement_dependency"})


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


def select_extraction(pack, extractions):
    result = {}
    for field, refs, identity in (("requirements", "requirement_refs", "requirement_id"),
                                  ("dispositions", "disposition_refs", "range_id")):
        selected = []
        for reference in pack[refs]:
            extraction = extractions[reference["artifact_ref"]["sha256"]]
            matches = [item for item in extraction[field] if item[identity] == reference[identity]]
            if len(matches) != 1:
                raise ArtifactIntegrityError("downstream selector must identify one immutable output")
            selected.append(matches[0])
        result[field] = selected
    return result


def _range_requirements(source, original, extraction, owned_body):
    """Resolve explicit range dependencies against obligation body intervals."""
    sources = {**source.by_id, **{item["range_id"]: item for item in [*original["supporting"], *original["body"]]}}
    at_source = defaultdict(list)
    dispositions = defaultdict(set)
    for item in extraction["dispositions"]:
        for requirement_id in item["requirement_ids"]:
            dispositions[requirement_id].add(item["range_id"])
    for requirement in extraction["requirements"]:
        key = requirement["requirement_id"]
        ids = {item["range_id"] for item in requirement["evidence"] if item["range_id"] in owned_body[key]}
        for range_id in ids | dispositions[key]:
            if range_id in sources:
                entry = sources[range_id]
                at_source[entry["source_range_id"]].append((key, entry["start"], entry["end"]))
    result = {}
    for ref in {ref for item in extraction["requirements"] for ref in item["cross_references"] if ref in sources}:
        entry = sources[ref]
        result[ref] = {key for key, start, end in at_source[entry["source_range_id"]]
                       if entry["start"] < end and start < entry["end"]}
    return result


class VerificationPlanner(StructuralPlanner):
    def __init__(self, source, budget, original, extraction, references, scope_decisions, owned_body, problems):
        super().__init__(source, budget, scope_decisions)
        self.original, self.extraction, self.references = original, extraction, references
        self.problems = problems
        self.sources = {item["range_id"]: item for item in [*original["supporting"], *original["body"]]}
        self.sources = {**source.by_id, **self.sources}
        body = original["body"]
        positions = {item["range_id"]: index for index, item in enumerate(body)}
        self.range_positions = defaultdict(set)
        for index, entry in enumerate(body):
            self.range_positions[entry["range_id"]].add(index)
            self.range_positions[entry["source_range_id"]].add(index)
        for range_id, entry in self.sources.items():
            if range_id not in self.range_positions:
                self.range_positions[range_id].update(index for index in self.range_positions[entry["source_range_id"]]
                                                      if body[index]["start"] < entry["end"] and entry["start"] < body[index]["end"])
        self.anchors = {}
        for requirement in extraction["requirements"]:
            requirement_id = requirement["requirement_id"]
            ids = {e["range_id"] for e in requirement["evidence"] if e["range_id"] in owned_body[requirement_id]}
            ids.update(item["range_id"] for item in extraction["dispositions"] if requirement_id in item["requirement_ids"])
            # Invalid evidence still reaches deterministic review, never vanishes
            # while repartitioning an otherwise well-formed model response.
            self.anchors.setdefault(requirement_id, set()).update(positions[r] for r in ids if r in positions)
            if not self.anchors[requirement_id]:
                self.anchors[requirement_id].add(0)
            for ref in requirement["cross_references"]:
                self.anchors[requirement_id].update(self.range_positions.get(ref, ()))
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
                "requirement_refs": [self.references["requirements"][key] for key in requirement_ids],
                "disposition_refs": [self.references["dispositions"][item["range_id"]] for item in body],
                "planning_details": {"planner_version": PLANNER_VERSION, "boundary_reason": reason,
                                     "section_ids": list(dict.fromkeys(item.get("section_id") for item in body))}}
        materialized = {**pack, "body": body, "supporting": supporting}
        selected = {"requirements": requirements,
                    "dispositions": [item for item in self.extraction["dispositions"] if item["range_id"] in body_ids]}
        measured = self.budget.measure("verify", verify_payload(materialized, selected))
        pack["issues"].extend(problem for key in requirement_ids for problem in self.problems.get(key, []))
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
    def __init__(self, source, budget, original, requirements, references, scope_decisions, problems, range_requirements):
        super().__init__(source, budget, scope_decisions)
        self.original, self.references, self.problems = original, references, problems
        self.packs, self.probe_count, self.capacity_hint = [], 0, None
        self.sources = {**source.by_id, **{item["range_id"]: item for item in [*original["supporting"], *original["body"]]}}
        positions = {item["requirement_id"]: index for index, item in enumerate(requirements)}
        links = [[index, *(positions[ref] for ref in item["cross_references"] if ref in positions)]
                 for index, item in enumerate(requirements)]
        links.extend([index, *(positions[key] for key in range_requirements[ref] if key in positions)]
                     for index, item in enumerate(requirements)
                     for ref in item["cross_references"] if ref in range_requirements)
        self.components = [[requirements[index] for index in component] for component in _components(len(requirements), links)]

    def weight(self, entry):
        return len(json.dumps(entry, ensure_ascii=False).encode())

    def anchor(self, requirement):
        return next((self.sources[e["range_id"]] for e in requirement["evidence"] if e["range_id"] in self.sources), {})

    def candidate(self, requirements, reason):
        from docreader.tender.evidence.view import source_view

        ids = [item["requirement_id"] for item in requirements]
        source_ids = {ref for item in requirements for ref in item["cross_references"] if ref in self.sources}
        context = [self.sources[key] for key in sorted(source_ids)]
        support, pending, problems, bindings = self.support.select(context)
        supporting = list(context)
        for value in support:
            supporting.extend(self.support._uncovered(value, supporting))
        pack = {"schema_version": 2, "pack_id": "plan-" + _digest(ids)[:24],
                **{key: self.original[key] for key in ("document_id", "document_version", "raw_ref", "source_index_ref")},
                "body": [], "supporting": [selector(item) for item in supporting],
                "pending_scope_context": pending, "scope_bindings": bindings, "compacted_blank_ranges": [],
                "requirement_refs": [self.references["requirements"][key] for key in ids], "disposition_refs": [],
                "issues": [*problems, *(problem for key in ids for problem in self.problems.get(key, []))]}
        rendered = {**pack, "supporting": supporting}
        measured = self.budget.measure("plan", plan_payload(requirements, source_view(rendered) if supporting else None))
        self.probe_count += 1
        self.capacity_hint = max(1, sum(self.weight(item) for item in requirements)
                                 * max(1, measured["effective_input_budget_tokens"]) // max(1, measured["input_tokens"]))
        return {**pack,
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
    documents, requirement_documents, range_documents = {}, defaultdict(set), defaultdict(set)
    references = {"requirements": {}, "dispositions": {}}
    owned_body, input_issues = {}, []
    if Counter(entry["pack_id"] for entry in extraction_index["packs"]) != Counter(entry["pack_id"] for entry in index["packs"]):
        raise ArtifactIntegrityError("downstream input does not cover the extraction plan")
    for entry in index["packs"]:
        extraction_ref = extractions[entry["pack_id"]]
        extraction = artifacts.read_json(extraction_ref)
        original = repository.load_pack(entry)
        document_id = original["document_id"]
        document = documents.setdefault(document_id, {
            "original": {**original, "body": [], "supporting": []},
            "extraction": {"requirements": [], "dispositions": []},
        })
        for field in ("body", "supporting"):
            document["original"][field].extend(original[field])
        for field, identity in (("requirements", "requirement_id"), ("dispositions", "range_id")):
            document["extraction"][field].extend(extraction[field])
            for item in extraction[field]:
                key = item[identity]
                if key in references[field]:
                    input_issues.append(issue("dependency_reference_unresolved", "Duplicate upstream identity prevents unambiguous downstream selection."))
                references[field][key] = {"artifact_ref": extraction_ref, identity: key}
        for requirement in extraction["requirements"]:
            key = requirement["requirement_id"]
            requirement_documents[key].add(document_id)
            owned_body[key] = {item["range_id"] for item in original["body"]}
    for document_id, document in documents.items():
        original = document["original"]
        original["supporting"] = list({item["range_id"]: item for item in original["supporting"]}.values())
        source = repository.source_index(original["source_index_ref"])
        for key in set(source.by_id) | {item["range_id"] for item in [*original["body"], *original["supporting"]]}:
            range_documents[key].add(document_id)
    # Duplicate identities are domain defects, not permission to silently select
    # one response. Preserve the diagnostic and do not send an ambiguous input.
    if not input_issues:
        for document_id, document in documents.items():
            original, extraction = document["original"], document["extraction"]
            source = repository.source_index(original["source_index_ref"])
            effective = (apply_applicability_decisions(extraction["requirements"], checked["applicability_decisions"])
                         if checked else extraction["requirements"])
            applicable = {item["requirement_id"] for item in effective if item["applicable"]}
            range_requirements = _range_requirements(source, original, extraction, owned_body)
            problems = defaultdict(list)
            for requirement in effective:
                for target in requirement["cross_references"]:
                    owners = requirement_documents.get(target) or range_documents.get(target)
                    targets = {target} if target in requirement_documents else range_requirements.get(target, set())
                    code = None
                    if (not owners or len(owners) != 1
                            or (target in requirement_documents and target in range_documents)):
                        code = "dependency_reference_unresolved"
                    elif document_id not in owners:
                        code = "cross_document_dependency"
                    elif task == "plan" and targets - applicable:
                        code = "inactive_requirement_dependency"
                    if code:
                        problems[requirement["requirement_id"]].append(issue(
                            code, "Dependency needs an unambiguous complete current input before a model request.",
                            requirement_id=requirement["requirement_id"]))
            if task == "verify":
                planner = VerificationPlanner(source, budget, original, extraction, references,
                                              index["scope_decisions"], owned_body, problems)
            else:
                planner = ResponsePlanner(source, budget, original, [item for item in effective if item["applicable"]],
                                          references, index["scope_decisions"], problems, range_requirements)
            planned.extend(planner.plan())
            probes += planner.probe_count
    budget_digest = _digest(budget.spec())
    budget_spec_ref = artifacts.put_json(budget.spec())
    inputs = {"source_plan_digest": index["plan_digest"], "requirements_ref": requirements_ref,
              "verification_ref": verification_ref, "run_identity": index.get("run_identity"), "planner_version": PLANNER_VERSION}
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
              "issues": [*input_issues, *(problem for pack in planned for problem in pack["issues"])],
              "stats": {**index["stats"], "pack_count": len(packs), "probe_count": probes}}
    if verification_ref:
        result["verification_ref"] = verification_ref
    PackPlan.model_validate(result)
    return artifacts.put_json(result)
