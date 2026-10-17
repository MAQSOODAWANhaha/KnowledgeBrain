"""Bounded, source-backed analysis review. Business meaning stays with the model.

Plans contain immutable selectors and digests; independent successful operations
have their own receipts so recovery never repeats a committed model call.
"""
from __future__ import annotations

import asyncio
from collections import Counter
from copy import deepcopy
import hashlib
import itertools
import json
import re

from pydantic import ValidationError

from docreader.llm import ModelSemanticError
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender.evidence.packing import SupportingIndex
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.evidence.view import (
    host_reviewed_ranges, resolve_evidence_quote, source_view, supplied_evidence_texts,
)
from docreader.tender.schema import (
    AssociationResult, Decision, LocateResult, Relation, enforce_issue_policy, is_hard_blocking, issue,
    recover_issue_routing,
)


def _digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                     separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def assign_issues(issues, namespace: str) -> list[dict]:
    """Host identities are stable and idempotent, including across receipt reuse."""
    result = {}
    for original in issues:
        problem = enforce_issue_policy(deepcopy(original))
        identity = problem.get("issue_id") or f"{namespace}:q:{_digest(problem)}"
        problem["issue_id"] = identity
        if identity in result and result[identity] != problem:
            raise ValueError("one issue identity refers to different facts")
        result[identity] = problem
    return list(result.values())


def _pdf_pages(source) -> dict:
    """Only frozen PDF physical-page contracts authorize page navigation."""
    if source.document["original_ref"].get("media_type") != "application/pdf":
        return {}
    count = source.raw.get("metadata", {}).get("page_count")
    manifest = source.raw["source_contract"].get("page_manifest", [])
    if type(count) is not int or count < 1 or len(manifest) != count:
        return {}
    ordinals = [page.get("page_ordinal") for page in manifest]
    if any(type(page) is not int for page in ordinals) or set(ordinals) != set(range(count)):
        return {}
    return {page["page_ordinal"]:page for page in manifest}


def build_catalog(index, artifacts) -> list[dict]:
    repository = EvidenceRepository(artifacts)
    result = []
    for document in index["documents"]:
        groups = {}
        pages = {}

        def add(source):
            section = source.get("section_id")
            heading = source.get("heading_path", "")
            if section in groups and groups[section] != heading:
                raise ValueError("one frozen section identity has inconsistent headings")
            groups[section] = heading

        if document.get("source_index_ref"):
            source = repository.source_index(document["source_index_ref"])
            pages = _pdf_pages(source)
            for entry in source.ranges:
                add(entry)
        for pack in index.get("packs", []):
            if pack["document_id"] != document["document_id"]:
                continue
            for entry in repository.load_pack(pack)["body"]:
                add(entry)
        result.append({"document_id": document["document_id"],
                       "document_version": document["document_version"], "role": document["role"],
                       **({"page_count":len(pages)} if pages else {}),
                       "sections": [{"section_id": section, "heading_path": heading}
                                    for section, heading in (groups or {None: ""}).items()]})
    return result


def _view_ids(views):
    return {source["range_id"] for view in views for key in ("body", "supporting", "source_aliases")
            for source in view.get(key, [])} | {
        range_id for view in views for table in view.get("blank_table_templates", [])
        for row in table["rows"] for range_id in row["range_ids"]}


def _public_issues(problems):
    identities = {p["issue_id"] for p in problems}
    result = []
    for problem in problems:
        value = {key: value for key, value in problem.items()
                 if key in {"issue_id", "code", "detail", "blocking", "range_id", "item_ids", "needs_source"}}
        links = [i for i in problem.get("related_issue_ids", []) if i in identities]
        if links:
            value["related_issue_ids"] = links
        result.append(value)
    return result


def _issue_groups(problems):
    """Connected work dependencies, not merged business facts."""
    parents = {p["issue_id"]: p["issue_id"] for p in problems}

    def root(identity):
        while parents[identity] != identity:
            parents[identity] = parents[parents[identity]]
            identity = parents[identity]
        return identity

    for problem in problems:
        for linked in problem.get("related_issue_ids", []):
            if linked in parents:
                parents[root(linked)] = root(problem["issue_id"])
    groups = {}
    for problem in problems:
        groups.setdefault(root(problem["issue_id"]), []).append(problem)
    return list(groups.values())


class _Analysis:
    # Token-accurate admission would put about three times more source into one
    # omission review; keep the granularity of the former byte-estimated budget.
    REVIEW_INPUT_TOKENS = 32768

    def __init__(self, source_plan, items_index, artifacts, identity, budget):
        self.plan, self.artifacts, self.identity, self.budget = source_plan, artifacts, identity, budget
        self.repo = EvidenceRepository(artifacts)
        self.catalog = build_catalog(source_plan, artifacts)
        self.roles = {document["document_id"]: document["role"] for document in source_plan["documents"]}
        self.document_order = {document["document_id"]:number for number,document in enumerate(source_plan["documents"])}
        self.sources, self.indices, self.owners = {}, {}, {}
        self.positions, self.pages, self.unreadable_pages = {}, {}, set()
        for document in source_plan["documents"]:
            if document.get("source_index_ref"):
                source = self.repo.source_index(document["source_index_ref"])
                self.indices[document["document_id"]] = source
                self.positions[document["document_id"]] = {entry["range_id"]:number
                    for number,entry in enumerate(source.ranges)}
                for entry in source.ranges:
                    self.sources[entry["range_id"]] = entry
        self.packs = {entry["pack_id"]: self.repo.load_pack(entry) for entry in source_plan["packs"]}
        if len(items_index["packs"]) != len(self.packs) or {
            entry["pack_id"] for entry in items_index["packs"]} != self.packs.keys():
            raise ModelSemanticError("Analysis ledger must cover every source pack exactly once")
        self.items, self.item_refs, self.relations, self.dispositions = {}, [], {}, {}
        self.problems = assign_issues(source_plan["issues"], "source")
        self.protected = {problem["issue_id"] for problem in self.problems}
        # Registration/freeze/OCR codes can be parser-defined; any of them leaves the analysis incomplete.
        self.source_issue_ids = set(self.protected)
        for pack in self.packs.values():
            for entry in [*pack["body"], *pack["supporting"]]:
                self.sources[entry["range_id"]] = entry
            for entry in pack["body"]:
                if entry["range_id"] in self.owners:
                    raise ModelSemanticError("Source body occurs in multiple extraction packs")
                self.owners[entry["range_id"]] = pack
        for entry in items_index["packs"]:
            value = artifacts.read_json(entry["artifact_ref"])
            if value.get("schema_version") != 2:
                raise ModelSemanticError("Analysis ledger requires schema version two")
            pack = self.packs[entry["pack_id"]]
            for item in value["items"]:
                if item["item_id"] in self.items:
                    raise ModelSemanticError("Duplicate analysis item identity")
                self.items[item["item_id"]] = item
                self.item_refs.append({"item_id": item["item_id"], "artifact_ref": entry["artifact_ref"]})
            for relation in value["relations"]:
                if relation["relation_id"] in self.relations:
                    raise ModelSemanticError("Duplicate analysis relation identity")
                self.relations[relation["relation_id"]] = relation
            self.dispositions.update({d["range_id"]: d for d in value["dispositions"]})
            problems = assign_issues(value["issues"], pack["pack_id"])
            self.problems.extend(problems)
            unresolved = {d["range_id"] for d in value["dispositions"] if d["status"] == "unresolved"}
            self.protected.update(p["issue_id"] for p in problems
                                  if p["code"] == "unresolved_source_range" and p.get("range_id") in unresolved)
            self.protected.update(p["issue_id"] for p in assign_issues(pack.get("issues", []), pack["pack_id"]))
        self.problems = assign_issues(self.problems, "analysis")
        self.sections = {}
        for entry in self.sources.values():
            self.sections.setdefault((entry["document_id"], entry.get("section_id")), set()).add(entry["range_id"])
        for document_id,source in self.indices.items():
            identities = {entry["unit_id"]:entry for entry in source.raw["source_contract"].get("units",[])}
            for page_ordinal,page in _pdf_pages(source).items():
                key = (document_id,page_ordinal)
                expected = set(page.get("unit_ids",[])) | set(page.get("image_unit_ids",[]))
                selected = [entry for entry in self.sources.values() if entry["document_id"] == document_id
                    and (entry["unit_id"] in expected
                         or (entry.get("physical_locator") or entry.get("locator") or {}).get("page_ordinal") == page_ordinal)]
                self.pages[key] = {entry["range_id"] for entry in selected}
                units = {entry["unit_id"] for entry in selected}
                if (not selected or not expected <= units
                        or page.get("classification") not in {"text","scanned","blank"}
                        or any(identities.get(unit,{}).get("completeness") != "complete" for unit in units
                               if not unit.startswith("blank:page:"))
                        or any(entry.get("ocr_status","complete") != "complete" for entry in selected)
                        or any(source.raw.get("ocr",{}).get(unit,{}).get("issues") for unit in units)):
                    self.unreadable_pages.add(key)
        if self.dispositions.keys() != self.owners.keys():
            raise ModelSemanticError("Analysis dispositions must cover all original body ranges")
        self.operation_refs = []
        self.incomplete_requests = []
        self.local_candidates, self.problem_routing, self.problem_dependencies = [], [], {}

    def payload(self, operation, work_unit, **fields):
        return {"schema_version": 2, "operation": operation, "work_unit_id": work_unit,
                "source_version": self.identity["source_version"],
                "domain_revision": self.identity["domain_revision"],
                **({"catalog": self.catalog} if operation == "locate" else {}), "source_views": [],
                "items": [], "relations": [], "decisions": [], "dispositions": [],
                "issues": [], "candidates": [],
                **({"scope_decisions": deepcopy(self.plan["scope_decisions"])} if self.plan.get("scope_decisions") else {}),
                **fields}

    def views(self, ranges, *, complete=False):
        """Load original targets and their actual bound conditions, never a fake document."""
        by_document, aliases = {}, {}
        ranges = tuple(set(ranges))
        for source in sorted((self.sources[identity] for identity in ranges if identity in self.sources),
                             key=self.source_order):
            range_id = source["range_id"]
            if complete:
                canonical = self.indices[source["document_id"]].by_id[source.get("source_range_id", range_id)]
                by_document.setdefault(source["document_id"], {})[canonical["range_id"]] = canonical
                if canonical["range_id"] != range_id:
                    aliases.setdefault(source["document_id"], {})[range_id] = source
            else:
                by_document.setdefault(source["document_id"], {})[range_id] = source
        result = []
        for document_id in self.document_order:
            if document_id not in by_document:
                continue
            selected = by_document[document_id]
            body = list(selected.values())
            support, pending, _, bindings = SupportingIndex(
                self.indices[document_id].ranges, self.plan.get("scope_decisions", []), analysis=True).select(body)
            support = {entry["range_id"]: entry for entry in support}
            support.update(aliases.get(document_id, {}))
            for range_id in ranges:
                pack = self.owners.get(range_id)
                if pack and pack["document_id"] == document_id:
                    support.update({entry["range_id"]: entry for entry in pack["supporting"]})
                    pending.extend(pack.get("pending_scope_context", []))
                    bindings.extend(pack.get("scope_bindings", []))
            pack = {"pack_id": "analysis-source:" + document_id,
                    "document_id": document_id,
                    "document_version": self.indices[document_id].document["document_version"],
                    "role": self.roles[document_id],
                    "body": sorted(body,key=self.source_order),
                    "supporting": sorted(support.values(),key=self.source_order),
                    "pending_scope_context": list({p["range_id"]: p for p in pending}.values()),
                    "scope_bindings": list({_digest(b): b for b in bindings}.values())}
            result.append(source_view(pack))
        return result

    def source_order(self, source):
        document = source["document_id"]
        canonical = source.get("source_range_id",source["range_id"])
        return (self.document_order[document],self.positions[document][canonical],
                source["start"],source["end"],source["range_id"])

    def review_payload(self, ranges, work_unit):
        selected = set(ranges)
        ids = set()
        while True:
            views = self.views(selected)
            supplied = _view_ids(views)
            items = [item for item in self.items.values() if item["item_id"] in ids
                     or any(e["range_id"] in selected for e in item["evidence"])]
            ids.update(item["item_id"] for item in items)
            problems = [p for p in self.problems if (not p.get("range_id") and not p.get("item_ids"))
                        or p.get("range_id") in supplied or set(p.get("item_ids", [])) & ids]
            expanded = ids | {item_id for p in problems for item_id in p.get("item_ids", []) if item_id in self.items}
            sources = selected | {p["range_id"] for p in problems if p.get("range_id") in self.sources}
            sources.update(e["range_id"] for item in items for e in item["evidence"])
            relations = [r for r in self.relations.values() if {r["from_item_id"], r["to_item_id"]} <= ids]
            sources.update(e["range_id"] for relation in relations for e in relation["evidence"])
            if expanded == ids and sources == selected:
                break
            ids, selected = expanded, sources
        return self.payload("review", work_unit, source_views=views, items=items,
                            relations=relations, dispositions=[self.dispositions[r] for r in ranges],
                            issues=_public_issues(problems))

    def findings(self, values, payload):
        allowed = _view_ids(payload["source_views"])
        if payload["operation"] == "locate":
            allowed |= {e["range_id"] for item in payload["items"] for e in item["evidence"]}
        allowed |= {p["range_id"] for p in payload["issues"] if p.get("range_id")}
        items = {item["item_id"] for item in payload["items"]}
        for index, finding in enumerate(values):
            if (finding.get("range_id") and finding["range_id"] not in allowed
                    or not set(finding.get("item_ids", [])) <= items):
                raise ModelContractError([{"path": ["findings", index], "code": "unknown_reference",
                                           "detail": "Finding refers to an identity absent from its request"}])
        return assign_issues(values, payload["work_unit_id"])

    def evidence(self, values, payload, path="", *, offset=0):
        allowed = _view_ids(payload["source_views"])
        texts = supplied_evidence_texts(payload["source_views"], self.sources)
        diagnostics = []
        for index, value in enumerate(values):
            for number, citation in enumerate(value["evidence"]):
                resolved = (resolve_evidence_quote(citation["range_id"], citation["quote"], self.sources, texts)
                            if citation["range_id"] in allowed else None)
                if resolved is None:
                    raise ModelSemanticError("Analysis evidence is not an exact supplied source substring")
                if resolved["quote"] != citation["quote"]:
                    citation["quote"] = resolved["quote"]
                    diagnostics.append({"code": "evidence_quote_restored",
                        "path": f"{path}[{index + offset}].evidence[{number}].quote", "range_id": citation["range_id"],
                        "start": resolved["start"], "end": resolved["end"]})
        return diagnostics

    def unresolved_association(self, declaration, payload, path, reason):
        items = {item["item_id"]: item for item in payload["items"]}
        ids = {declaration[key] for key in ("from_item_id", "to_item_id")
               if isinstance(declaration.get(key), str) and declaration[key] in items}
        amendment = declaration.get("amendment")
        if isinstance(amendment, dict):
            replacements = amendment.get("replacement_item_ids", [])
            ids.update(item_id for item_id in [amendment.get("target_item_id"),
                       *(replacements if isinstance(replacements, list) else [])]
                       if isinstance(item_id, str) and item_id in items)
        issue_ids = declaration.get("issue_ids", [])
        ids.update(item_id for problem in payload["issues"]
                   if isinstance(issue_ids, list) and problem["issue_id"] in issue_ids
                   for item_id in problem.get("item_ids", []) if item_id in items)
        supplied = _view_ids(payload["source_views"])
        citations = declaration.get("evidence", [])
        ranges = [citation["range_id"] for citation in (citations if isinstance(citations, list) else [])
                  if isinstance(citation, dict) and isinstance(citation.get("range_id"), str)
                  and citation["range_id"] in supplied]
        ranges.extend(e["range_id"] for item_id in sorted(ids) for e in items[item_id]["evidence"]
                      if e["range_id"] in supplied)
        pending = {key: declaration[key] for key in ("relation_id", "from_item_id", "to_item_id", "relation",
                   "decision_id", "issue_ids", "reason", "evidence", "relation_ids", "amendment") if key in declaration}
        detail = (f"Association {payload['work_unit_id']} {path} needs independent source review ({reason}). "
                  f"Pending declaration: {json.dumps(pending, ensure_ascii=False, separators=(',', ':'))}")
        return issue("analysis_association_unresolved", detail, ranges[0] if ranges else None,
                     **({"item_ids": sorted(ids)} if ids else {}))

    def validate(self, response, payload):
        from docreader.tender.schema import AnalysisReviewResponse

        operation = payload["operation"]
        schema = {"review": AnalysisReviewResponse, "locate": LocateResult, "associate": AssociationResult}[operation]
        response = deepcopy(response)
        declarations = None
        selector_diagnostics = []
        if isinstance(response, dict) and isinstance(response.get("findings"), list):
            allowed = {p["issue_id"] for p in payload["issues"]}
            response["findings"] = [recover_issue_routing(finding, allowed, f"findings[{index}]", selector_diagnostics)
                                    for index, finding in enumerate(response["findings"])]
        if operation == "review" and isinstance(response, dict):
            ignored = [f"reviewed_{name}_ids" for name in ("range", "item", "relation", "decision")
                       if f"reviewed_{name}_ids" in response]
            for key in ignored:
                response.pop(key)
            if ignored:
                selector_diagnostics.append({"code": "model_review_scope_ignored", "fields": ignored})
        if operation == "locate" and isinstance(response, dict) and isinstance(response.get("candidates"), list):
            for index, candidate in enumerate(response["candidates"]):
                ranges = candidate.get("range_ids") if isinstance(candidate, dict) else None
                if not isinstance(ranges, list) or not all(isinstance(identity, str) for identity in ranges):
                    continue
                repeated = [identity for identity, count in Counter(ranges).items() if count > 1]
                if repeated:
                    candidate["range_ids"] = list(dict.fromkeys(ranges))
                    selector_diagnostics.append({"code": "candidate_range_ids_deduplicated",
                                                 "path": f"candidates[{index}].range_ids", "range_ids": repeated})
        if operation == "associate" and isinstance(response, dict):
            if not all(isinstance(response.get(key), list) for key in ("relations", "decisions")):
                raise ModelContractError([{"path": [key], "code": "list_type", "detail": "Expected an array"}
                    for key in ("relations", "decisions") if not isinstance(response.get(key), list)])
            declarations = {key: response[key] for key in ("relations", "decisions")}
            response.update(relations=[], decisions=[])
        try:
            value = schema.model_validate(response).model_dump(exclude_unset=True)
        except ValidationError as error:
            raise ModelContractError([{"path": list(entry["loc"]), "code": entry["type"], "detail": entry["msg"]}
                                      for entry in error.errors(include_url=False, include_input=False)]) from None
        if selector_diagnostics:
            value["diagnostics"] = selector_diagnostics
        self.findings(value["findings"], payload)
        if operation == "review":
            expected = {"range": _view_ids(payload["source_views"]),
                        "item": {i["item_id"] for i in payload["items"]},
                        "relation": {i["relation_id"] for i in payload["relations"]},
                        "decision": {i["decision_id"] for i in payload["decisions"]}}
            accepted = value["accepted_decision_ids"]
            if not set(accepted) <= expected["decision"]:
                raise ModelContractError([{"path": ["accepted_decision_ids"], "code": "unknown_decision",
                                           "detail": "Accept only decisions supplied in this request"}])
            value["accepted_decision_ids"] = list(dict.fromkeys(accepted))
            # Receipt scope comes from the immutable input, never a model's ID echo.
            for name, identities in expected.items():
                value[f"reviewed_{name}_ids"] = sorted(identities)
        elif operation == "locate":
            allowed_ranges = (_view_ids(payload["source_views"])
                | {e["range_id"] for item in payload["items"] for e in item["evidence"]}
                | {p["range_id"] for p in payload["issues"] if p.get("range_id")})
            allowed_sections = {(document["document_id"], section["section_id"])
                                for document in payload["catalog"] for section in document["sections"]}
            allowed_pages = {document["document_id"]:document["page_count"]
                             for document in payload["catalog"] if "page_count" in document}
            allowed_items = {i["item_id"] for i in payload["items"]}
            allowed_issues = {p["issue_id"] for p in payload["issues"]}
            seen = set()
            for index, candidate in enumerate(value["candidates"]):
                self.namespace(candidate["candidate_id"], payload, "c", seen, ["candidates", index, "candidate_id"])
                if (not set(candidate["range_ids"]) <= allowed_ranges
                        or not set(candidate["item_ids"]) <= allowed_items
                        or not set(candidate["issue_ids"]) <= allowed_issues
                        or not {(ref["document_id"], ref["section_id"])
                                for ref in candidate.get("section_refs", [])} <= allowed_sections
                        or any(ref["document_id"] not in allowed_pages
                               or not 0 <= ref["page_ordinal"] < allowed_pages[ref["document_id"]]
                               for ref in candidate.get("page_refs", []))):
                    raise ModelContractError([{"path": ["candidates", index], "code": "unknown_reference",
                        "detail": "Candidate refers to an identity absent from its discovery request"}])
        else:
            item_map = {i["item_id"]: i for i in payload["items"]}
            items = set(item_map)
            issue_map = {p["issue_id"]: p for p in payload["issues"]}
            issues = set(issue_map)
            relations = {r["relation_id"] for r in payload["relations"]}
            rejected_relations = set()
            diagnostics = []
            seen = set()
            for index, declaration in enumerate(declarations["relations"]):
                if not isinstance(declaration, dict):
                    raise ModelContractError([{"path": ["relations", index], "code": "invalid_reference",
                                               "detail": "Associated relation has no bindable identity"}])
                self.namespace(declaration.get("relation_id"), payload, "l", seen, ["relations", index, "relation_id"])
                path = f"relations[{index}]"
                try:
                    relation = Relation.model_validate(declaration).model_dump(exclude_unset=True)
                    diagnostics.extend(self.evidence([relation], payload, "relations", offset=index))
                except (ValidationError, ModelSemanticError):
                    rejected_relations.add(declaration["relation_id"])
                    value["findings"].append(self.unresolved_association(declaration, payload, path, "invalid evidence or fields"))
                    continue
                if not {relation["from_item_id"], relation["to_item_id"]} <= items:
                    raise ModelContractError([{"path": ["relations", index], "code": "invalid_reference",
                                               "detail": "Associated relation has unknown item endpoints"}])
                relations.add(relation["relation_id"])
                value["relations"].append(relation)
            for index, declaration in enumerate(declarations["decisions"]):
                if not isinstance(declaration, dict):
                    raise ModelContractError([{"path": ["decisions", index], "code": "invalid_reference",
                                               "detail": "Associated decision has no bindable identity"}])
                self.namespace(declaration.get("decision_id"), payload, "d", seen, ["decisions", index, "decision_id"])
                path = f"decisions[{index}]"
                try:
                    decision = Decision.model_validate(declaration).model_dump(exclude_unset=True)
                    diagnostics.extend(self.evidence([decision], payload, "decisions", offset=index))
                    if set(decision.get("relation_ids", [])) & rejected_relations:
                        raise ModelSemanticError("Decision refers to a relation without valid source evidence")
                except (ValidationError, ModelSemanticError):
                    value["findings"].append(self.unresolved_association(declaration, payload, path, "invalid evidence or fields"))
                    continue
                if (not set(decision["issue_ids"]) <= issues
                        or not set(decision.get("relation_ids", [])) <= relations):
                    raise ModelContractError([{"path": ["decisions", index], "code": "invalid_reference",
                                               "detail": "Decision has unknown resolution targets"}])
                cited = {self.source_identity(e["range_id"]) for e in decision["evidence"]}
                amendment = decision.get("amendment")
                if amendment:
                    if (amendment["target_item_id"] not in items
                            or not set(amendment["replacement_item_ids"]) <= items
                            or amendment["target_item_id"] in amendment["replacement_item_ids"]):
                        raise ModelContractError([{"path": ["decisions", index, "amendment"], "code": "invalid_reference",
                                               "detail": "Amendment has unknown or cyclic item targets"}])
                try:
                    for issue_id in decision["issue_ids"]:
                        source_id = issue_map[issue_id].get("range_id")
                        if source_id and self.source_identity(source_id) not in cited:
                            raise ModelSemanticError("missing original issue source evidence")
                    if amendment:
                        for item_id in [amendment["target_item_id"], *amendment["replacement_item_ids"]]:
                            original = {self.source_identity(e["range_id"]) for e in item_map[item_id]["evidence"]}
                            if not original & cited:
                                raise ModelSemanticError("missing original or replacement source evidence")
                        diagnostics.extend(self.evidence(amendment["retained_rules"], payload,
                                                          f"{path}.amendment.retained_rules"))
                except ModelSemanticError as error:
                    value["findings"].append(self.unresolved_association(declaration, payload, path, str(error)))
                    continue
                value["decisions"].append(decision)
            if diagnostics:
                value.setdefault("diagnostics", []).extend(diagnostics)
        return value

    def source_identity(self, range_id):
        return self.sources.get(range_id, {}).get("source_range_id", range_id)

    @staticmethod
    def namespace(identity, payload, suffix, seen, path):
        if (not isinstance(identity, str) or identity in seen
                or re.fullmatch(re.escape(payload["work_unit_id"]) + rf":{suffix}[1-9][0-9]*", identity) is None):
            raise ModelContractError([{"path": path, "code": "invalid_identity",
                                       "detail": "Use a unique identity in this work unit namespace"}])
        seen.add(identity)

    def operation_plan(self, phase, payloads):
        descriptors = []
        for payload in payloads:
            descriptors.append({"work_unit_id": payload["work_unit_id"], "input_digest": _digest(payload),
                                "budget": self.budget.measure("verify", payload),
                                "range_ids": sorted(_view_ids(payload["source_views"])),
                                **{f"{name}_ids": [v[f"{name}_id"] for v in payload[name + "s"]]
                                   for name in ("item", "relation", "decision", "candidate")}})
        plan = {"schema_version": 2, "phase": phase, "requests": descriptors,
                "source_packs": self.plan["packs"], "item_refs": self.item_refs,
                "budget_digest": _digest(self.budget.spec())}
        return plan, _digest(plan)

    async def execute(self, phase, payloads, model, save):
        plan, plan_digest = await asyncio.to_thread(self.operation_plan, phase, payloads)
        descriptors = plan["requests"]

        async def planned():
            return plan

        plan_ref = await save(f"analysis-{phase}-plan", planned, inputs={"plan_digest": plan_digest})
        results = []
        # Each call is a bounded sibling with its own immutable paid-call receipt.
        for payload, descriptor in zip(payloads, descriptors, strict=True):
            if descriptor["budget"]["fits"] is False:
                problems = assign_issues([issue("analysis_budget_exceeded",
                    "Complete indivisible analysis input exceeds the admission budget; the host request grouping "
                    "must be fixed before rerunning, a source revision cannot shrink it.")],
                    payload["work_unit_id"])
                self.problems.extend(problems)
                self.protected.update(p["issue_id"] for p in problems)
                self.incomplete_requests.append(payload["work_unit_id"])
                results.append(None)
                continue

            async def produce(payload=payload):
                if hasattr(model, "call_validated"):
                    return await model.call_validated("verify", payload, lambda value: self.validate(value, payload))
                value = await model.call("verify", payload)
                return await asyncio.to_thread(self.validate, value, payload)

            ref = await save(f"analysis-{phase}", produce, scope=payload["work_unit_id"],
                             inputs={"plan_ref": plan_ref, "payload_digest": descriptor["input_digest"]}, model=True)
            value = await asyncio.to_thread(self.artifacts.read_json, ref)
            self.operation_refs.append({"operation": phase, "work_unit_id": payload["work_unit_id"], "artifact_ref": ref})
            self.problems.extend(self.findings(value["findings"], payload))
            results.append(value)
        return plan_ref, results

    def review_requests(self):
        payloads = []
        for pack in self.packs.values():
            ranges = [r["range_id"] for r in pack["body"] if r["range_id"] not in host_reviewed_ranges(pack)]
            current = []
            for range_id in ranges:
                trial = self.review_payload([*current, range_id], f"review-{len(payloads) + 1}")
                measured = self.budget.measure("verify", trial)
                if current and (measured["fits"] is False or (
                        measured["method"] == RequestBudget.TOKENIZED_METHOD
                        and measured["input_tokens"] > self.REVIEW_INPUT_TOKENS)):
                    payloads.append(self.review_payload(current, f"review-{len(payloads) + 1}"))
                    current = []
                current.append(range_id)
            if current:
                payloads.append(self.review_payload(current, f"review-{len(payloads) + 1}"))
        reviewed_relations = {r["relation_id"] for p in payloads for r in p["relations"]}
        for relation_id, relation in self.relations.items():
            if relation_id in reviewed_relations:
                continue
            endpoints = [self.items[relation[key]] for key in ("from_item_id", "to_item_id")]
            ranges = sorted({e["range_id"] for item in endpoints for e in item["evidence"]
                             if e["range_id"] in self.dispositions})
            # Relationship semantics require both endpoints and all evidence in
            # one request, even when ordinary source reviews were repartitioned.
            payloads.append(self.review_payload(ranges, f"review-{len(payloads) + 1}"))
            reviewed_relations.update(r["relation_id"] for r in payloads[-1]["relations"])
        for decision in self.plan.get("scope_decisions", []):
            # Human scope interpretation, including local_only, must actually
            # reach independent review with its original and target evidence.
            ranges = {decision["range_id"], *decision["target_range_ids"],
                      *(e["range_id"] for e in decision["evidence"])}
            canonical = {self.source_identity(r) for r in ranges}
            items = [i for i in self.items.values() if any(self.source_identity(e["range_id"]) in canonical
                                                         for e in i["evidence"])]
            ranges.update(e["range_id"] for i in items for e in i["evidence"])
            ids = {i["item_id"] for i in items}
            relations = [r for r in self.relations.values() if {r["from_item_id"], r["to_item_id"]} <= ids]
            ranges.update(e["range_id"] for r in relations for e in r["evidence"])
            payloads.append(self.payload("review", f"scope-review-{len(payloads) + 1}",
                source_views=self.views(ranges, complete=True), items=items, relations=relations,
                dispositions=[d for r, d in self.dispositions.items() if self.source_identity(r) in canonical],
                issues=_public_issues(self.problems)))
        return payloads

    def locate_requests(self):
        """Cover every item pair with block unions; never keyword filtering."""
        def discovery(items, work_unit):
            selected = {}
            for item in items:
                for evidence in item["evidence"]:
                    source = self.sources[evidence["range_id"]]
                    key = (source["document_id"], source.get("section_id"))
                    selected.setdefault(key, {})[evidence["range_id"]] = None
            catalog = []
            for document in self.catalog:
                sections = [{**section, "range_ids": list(selected[key])}
                            for section in document["sections"]
                            if (key := (document["document_id"], section["section_id"])) in selected]
                if sections:
                    catalog.append({**document, "sections": sections})
            return self.payload("locate", work_unit, items=items, catalog=catalog)

        all_items = list(self.items.values())
        if all_items and self.budget.measure("verify", discovery(all_items, "discovery-1"))["fits"] is not False:
            requests = [discovery(all_items, "discovery-1")]
        else:
            blocks, current = [], []
            for item in all_items:
                # Reserve both items and local navigation for an arbitrary
                # counterpart whose documents/sections may all be different.
                trial = discovery([*current, item], "discovery-capacity")
                trial["items"] *= 2
                trial["catalog"] *= 2
                if current and self.budget.measure("verify", trial)["fits"] is False:
                    blocks.append(current)
                    current = []
                current.append(item)
            if current:
                blocks.append(current)
            # With multiple blocks, their unions already cover every within-block pair.
            groups = [a + b for a, b in itertools.combinations(blocks, 2)] if len(blocks) > 1 else blocks
            requests = [discovery(group, f"discovery-{n + 1}") for n, group in enumerate(groups)]
        def reference(problems, work_unit):
            ids = {identity for problem in problems for identity in problem.get("item_ids", [])}
            ranges = {problem["range_id"] for problem in problems if problem.get("range_id")}
            items = [i for i in all_items if i["item_id"] in ids or any(e["range_id"] in ranges for e in i["evidence"])]
            ranges.update(e["range_id"] for i in items for e in i["evidence"])
            return self.payload("locate", work_unit, source_views=self.views(ranges), items=items,
                                issues=_public_issues(problems))

        # Freeze routing before localization. Later findings cannot recursively
        # reschedule the phase; successful receipts rebuild this exact plan.
        self.local_candidates, self.problem_routing, self.problem_dependencies = [], [], {}
        reference_groups = []
        for group in _issue_groups(assign_issues(self.problems, "analysis")):
            identities = [p["issue_id"] for p in group]
            for identity in identities:
                self.problem_dependencies[identity] = set(identities)
            needs = [p["needs_source"] for p in group if "needs_source" in p]
            route = "reference"
            reason = "需要额外原文或尚未判断，定位一次。"
            if True not in needs and not any(p["blocking"] for p in group):
                route, reason = "note", "非阻断提示保留展示。"
            elif True not in needs and False in needs:
                ids = sorted({i for p in group for i in p.get("item_ids", []) if i in self.items})
                ranges = sorted({p["range_id"] for p in group if p.get("range_id") in self.sources})
                if set(identities) & self.protected:
                    route, reason = "revision", "既有来源或范围保护需显式修订。"
                elif not ids and not ranges:
                    route, reason = "unresolved", "没有可绑定的原文或事项，保留问题待核对。"
                else:
                    route, reason = "local", "模型判断现有原文足够，核对本地问题及续报。"
                    self.local_candidates.append({"candidate_id": f"local-{len(self.local_candidates) + 1}:c1",
                        "issue_ids": identities, "range_ids": ranges, "item_ids": ids, "reason": reason})
            if route == "reference":
                reference_groups.append(group)
            self.problem_routing.append({"issue_ids": identities, "route": route, "reason": reason})

        # Independent groups may share a request. An indivisible continuation
        # group is never cut to satisfy the eight-record batching preference.
        current, batch = [], None
        for group in reference_groups:
            if current and len(current) + len(group) > 8:
                requests.append(batch)
                current = []
            trial = reference([*current, *group], f"reference-{len(requests) + 1}")
            if current and self.budget.measure("verify", trial)["fits"] is False:
                requests.append(batch)
                current = []
                trial = reference(group, f"reference-{len(requests) + 1}")
            current.extend(group)
            batch = trial
        if current:
            requests.append(batch)
        return requests

    def association_requests(self, candidates):
        groups = []
        for candidate in candidates:
            # Jointly resolve one concrete question's alternative targets. A
            # shared source or item alone does not make separate candidates one.
            keys = {linked for identity in candidate["issue_ids"]
                    for linked in self.problem_dependencies.get(identity, {identity})}
            group = [candidate]
            rest = []
            for previous, previous_keys in groups:
                if keys & previous_keys:
                    group.extend(previous)
                    keys |= previous_keys
                else:
                    rest.append((previous, previous_keys))
            # A newly merged component can connect an earlier component too.
            while any(keys & prior_keys for _, prior_keys in rest):
                previous, prior_keys = next((g, k) for g, k in rest if keys & k)
                rest.remove((previous, prior_keys))
                group.extend(previous)
                keys |= prior_keys
            groups = [*rest, (group, keys)]
        requests = []
        for number, (group, issue_ids) in enumerate(groups):
            ranges = {r for c in group for r in c["range_ids"]}
            selected_pages = [(ref["document_id"],ref["page_ordinal"])
                              for candidate in group for ref in candidate.get("page_refs", [])]
            if any(page not in self.pages or page in self.unreadable_pages for page in selected_pages):
                problems = assign_issues([issue("analysis_page_source_unavailable",
                    "The selected complete physical PDF page has missing or partial source; source revision is required.")],
                    f"associate-{number + 1}")
                self.problems.extend(problems)
                self.protected.update(problem["issue_id"] for problem in problems)
                self.incomplete_requests.append(f"associate-{number + 1}")
                continue
            ranges.update(range_id for page in selected_pages for range_id in self.pages[page])
            selected_sections = [self.sections.get((ref["document_id"], ref["section_id"]), set())
                                 for candidate in group for ref in candidate.get("section_refs", [])]
            if any(not selected for selected in selected_sections):
                problems = assign_issues([issue("analysis_section_source_unavailable",
                    "The selected complete section has no readable frozen source; source revision is required.")],
                    f"associate-{number + 1}")
                self.problems.extend(problems)
                self.protected.update(problem["issue_id"] for problem in problems)
                self.incomplete_requests.append(f"associate-{number + 1}")
                continue
            ranges.update(range_id for selected in selected_sections for range_id in selected)
            ids = {i for c in group for i in c["item_ids"]}
            problems = [p for p in self.problems if p["issue_id"] in issue_ids]
            ranges.update(p["range_id"] for p in problems if p.get("range_id"))
            ids.update(i for p in problems for i in p.get("item_ids", []))
            canonical = {self.sources[r].get("source_range_id", r) for r in ranges if r in self.sources}
            items = [i for i in self.items.values() if i["item_id"] in ids or any(
                self.sources[e["range_id"]].get("source_range_id", e["range_id"]) in canonical for e in i["evidence"])]
            ids.update(i["item_id"] for i in items)
            ranges.update(e["range_id"] for i in items for e in i["evidence"])
            relations = [r for r in self.relations.values() if {r["from_item_id"], r["to_item_id"]} <= ids]
            ranges.update(e["range_id"] for r in relations for e in r["evidence"])
            views = self.views(ranges, complete=True)
            supplied = _view_ids(views)
            # Context does not carry a complete item inventory. Never expose a
            # disposition whose item identities are absent from this request.
            dispositions = [d for r, d in self.dispositions.items()
                            if r in ranges and r in supplied and set(d["item_ids"]) <= ids]
            requests.append(self.payload("associate", f"associate-{number + 1}", source_views=views,
                items=items, relations=relations, candidates=group, issues=_public_issues(problems),
                dispositions=dispositions))
        return requests


async def verify_analysis(source_plan, items_index, artifacts, model, budget, *, identity, save) -> dict:
    context = await asyncio.to_thread(_Analysis, source_plan, items_index, artifacts, identity, budget)
    # Full-source planning can outlast a lease; keep heartbeat I/O schedulable.
    source_reviews = await asyncio.to_thread(context.review_requests)
    review_plan_ref, source_reviewed = await context.execute("review", source_reviews, model, save)
    locate_requests = await asyncio.to_thread(context.locate_requests)
    locate_plan_ref, located = await context.execute("locate", locate_requests, model, save)
    candidates = [c for result in located if result for c in result["candidates"]] + context.local_candidates
    associations = await asyncio.to_thread(context.association_requests, candidates)
    association_plan_ref, associated = await context.execute("associate", associations, model, save)
    joint, decisions, relations = [], [], list(context.relations.values())
    for payload, result in zip(associations, associated, strict=True):
        if result is None:
            continue
        decisions.extend(result["decisions"])
        relations.extend(result["relations"])
        joint.append({**payload, "operation": "review", "work_unit_id": payload["work_unit_id"].replace("associate-", "joint-"),
                      "relations": [*payload["relations"], *result["relations"]], "decisions": result["decisions"],
                      "issues": _public_issues(assign_issues([*payload["issues"], *result["findings"]],
                                                             payload["work_unit_id"]))})
    joint_plan_ref, reviewed = await context.execute("joint", joint, model, save)
    completed = [result for result in [*source_reviewed, *reviewed] if result]
    required_ranges = {r["range_id"] for pack in context.packs.values() for r in pack["body"]
                       if r["range_id"] not in host_reviewed_ranges(pack)}
    required = {"range": required_ranges, "item": set(context.items),
                "relation": {r["relation_id"] for r in relations},
                "decision": {d["decision_id"] for d in decisions}}
    coverage = {name: {identity for result in completed for identity in result[f"reviewed_{name}_ids"]}
                for name in required}
    missing = {name: sorted(ids - coverage[name]) for name, ids in required.items() if ids - coverage[name]}
    joint_candidates = {c["candidate_id"] for payload, result in zip(joint, reviewed, strict=True)
                        if result for c in payload["candidates"]}
    if {c["candidate_id"] for c in candidates} - joint_candidates:
        missing["candidate"] = sorted({c["candidate_id"] for c in candidates} - joint_candidates)
    if missing:
        context.problems.extend(assign_issues([issue("analysis_review_incomplete",
            "Necessary source, item, relation, decision or joint-candidate coverage is incomplete.")], "coverage"))
    accepted = set()
    for payload, result in zip(joint, reviewed, strict=True):
        if result and not any(enforce_issue_policy(p)["blocking"] for p in result["findings"]):
            accepted.update(result["accepted_decision_ids"])
        elif result is None:
            continue
    # Source integrity, scope bindings and missing extraction require an explicit
    # revision. Even an accepted proposal cannot mutate those host-owned facts.
    for decision in decisions:
        if set(decision["issue_ids"]) & context.protected:
            accepted.discard(decision["decision_id"])
        if decision["decision_id"] not in accepted:
            context.problems.extend(assign_issues([issue("decision_not_accepted",
                "A proposed resolution was not independently accepted or requires an explicit revision.")], decision["decision_id"]))
    amended = [d for d in decisions if d["decision_id"] in accepted and d.get("amendment")]
    targets = Counter(d["amendment"]["target_item_id"] for d in amended)
    replacement_ids = {i for d in amended for i in d["amendment"]["replacement_item_ids"]}
    for decision in amended:
        target = decision["amendment"]["target_item_id"]
        if targets[target] > 1 or target in replacement_ids:
            accepted.discard(decision["decision_id"])
            context.problems.extend(assign_issues([issue("amendment_effect_unresolved",
                "Overlapping or chained net amendment decisions require one complete reviewed effect.")], target))
    resolved = {q for d in decisions if d["decision_id"] in accepted for q in d["issue_ids"]}
    all_issues = assign_issues(context.problems, "analysis")
    active = [p for p in all_issues if p["issue_id"] not in resolved]
    hard = {p["issue_id"] for p in active
            if p["blocking"] and (p["issue_id"] in context.source_issue_ids or is_hard_blocking(p))}
    human = sorted(p["issue_id"] for p in active if p["blocking"] and p["issue_id"] not in hard)
    relations_ref = await asyncio.to_thread(artifacts.put_json, relations)
    decisions_ref = await asyncio.to_thread(artifacts.put_json, decisions)
    candidates_ref = await asyncio.to_thread(artifacts.put_json, candidates)
    return {"schema_version": 2, "blocking": bool(missing or context.incomplete_requests) or any(p["blocking"] for p in active),
            "hard_blocking": bool(missing or context.incomplete_requests or hard), "human_issue_ids": human,
            "issues": active, "all_issues": all_issues, "resolved_issue_ids": sorted(resolved),
            "accepted_decision_ids": sorted(accepted), "item_refs": context.item_refs,
            "relations_ref": relations_ref, "decisions_ref": decisions_ref, "candidates_ref": candidates_ref,
            "review_plan_ref": review_plan_ref, "locate_plan_ref": locate_plan_ref,
            "association_plan_ref": association_plan_ref, "joint_plan_ref": joint_plan_ref,
            "operation_refs": context.operation_refs,
            "problem_routing": context.problem_routing,
            "coverage": {"reviewed": {name: sorted(ids) for name, ids in coverage.items()},
                         "joint_candidate_ids": sorted(joint_candidates), "missing": missing,
                         "incomplete_work_unit_ids": context.incomplete_requests},
            "counts": {"items": len(context.items), "ranges": len(context.owners), "relations": len(relations),
                       "decisions": len(decisions), "candidates": len(candidates),
                       "blocking_issues": sum(p["blocking"] for p in active),
                       "model_calls": len(context.operation_refs)}}


def current_items(artifacts, verification: dict) -> list[dict]:
    """Read-only projection. Original extracted facts and their refs never change."""
    originals, loaded = {}, set()
    for ref in verification["item_refs"]:
        if ref["artifact_ref"]["sha256"] not in loaded:
            originals.update({item["item_id"]: item for item in artifacts.read_json(ref["artifact_ref"])["items"]})
            loaded.add(ref["artifact_ref"]["sha256"])
    amendments = {d["amendment"]["target_item_id"]: d for d in artifacts.read_json(verification["decisions_ref"])
                  if d["decision_id"] in verification["accepted_decision_ids"] and d.get("amendment")}
    replacements = {}
    for decision in amendments.values():
        for item_id in decision["amendment"]["replacement_item_ids"]:
            replacements.setdefault(item_id, []).append(decision["decision_id"])
    result = []
    for ref in verification["item_refs"]:
        item_id = ref["item_id"]
        if item_id not in amendments:
            result.append({"item_ref": ref, "item": deepcopy(originals[item_id]),
                           **({"decision_ids": replacements[item_id]} if item_id in replacements else {})})
        else:
            decision = amendments[item_id]
            for rule in decision["amendment"]["retained_rules"]:
                result.append({"item_ref": ref, "decision_id": decision["decision_id"], "effective_rule": deepcopy(rule)})
    return result
