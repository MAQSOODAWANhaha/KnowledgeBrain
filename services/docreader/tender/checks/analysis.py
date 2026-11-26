"""Bounded, source-backed analysis review. Business meaning stays with the model.

Plans contain immutable selectors and digests; independent successful operations
have their own receipts so recovery never repeats a committed model call.
"""
from __future__ import annotations

import asyncio
from collections import Counter, defaultdict
from typing import Any, cast
from copy import deepcopy
import hashlib
from itertools import groupby, pairwise
import json
import re

from pydantic import ValidationError

from docreader.integrations.store import Cancelled, StaleExecution
from docreader.llm import ModelSemanticError
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelConfigurationError, ModelContractError, ModelWorkUnitExhausted
from docreader.tender.evidence.packing import SupportingIndex
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.evidence.view import host_reviewed_ranges, source_view
from docreader.tender.schema import (
    assign_issues, is_hard_blocking, issue, recover_issue_routing,
)


def _digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                     separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def review_preparation_digest(source_plan, items_index, identity, budget_spec):
    return _digest({"planner": "source-review-requests-v4", "source_plan": source_plan,
                    "items_index": items_index, "identity": identity, "budget": budget_spec})


def _review_json(value):
    # Wire construction depends on request-local reference traversal order.
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False)


def _stable_unit(prefix, content):
    """Content-stable work-unit identity; host receipts keep run/source/revision."""
    return f"{prefix}-{_digest(content)[:12]}"


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


def _pdf_projection_key(line):
    # Ignore layout spacing without merging distinct ASCII tokens such as 1 23.
    return re.sub(r"(?<![A-Za-z0-9]) | (?![A-Za-z0-9])", "", " ".join(line.split()))


def _pdf_table_line_rows(ranges, *, projected=False):
    """Match a unique, complete literal line inside its frozen table."""
    literals, cells = defaultdict(list), defaultdict(list)
    for source in ranges:
        locator = source.get("physical_locator") or source.get("locator") or {}
        page = locator.get("page_ordinal")
        if type(page) is not int or not all(isinstance(locator.get(key), (int, float))
                                           for key in ("left", "bottom", "right", "top")):
            continue
        if source.get("text_basis") == "pdf_literal" and source["text"].strip():
            text = source["text"]
            if not projected or len(text.splitlines()) == 1:
                key = _pdf_projection_key(text) if projected else text
                literals[page, key].append((source["range_id"], locator))
        elif locator.get("locator_kind") == "page_table":
            row_cells = source.get("cells", [])
            for cell in row_cells:
                text = cell["text"]
                has_options = any(mark in text for mark in "☑☒□☐")
                for line in text.splitlines():
                    if line.strip():
                        key = _pdf_projection_key(line) if projected else line
                        cells[page, key].append((source["range_id"], locator, has_options))
            if projected:
                # A literal line can span two adjacent cells, e.g. an item
                # number and the opening line of its description.
                for left, right in pairwise(sorted(row_cells, key=lambda cell: cell["column"])):
                    if left["column"] + left["col_span"] != right["column"]:
                        continue
                    first = left["text"].splitlines()
                    second = right["text"].splitlines()
                    if first and first[0].strip() and second and second[0].strip():
                        key = _pdf_projection_key(f"{first[0]} {second[0]}")
                        cells[page, key].append((source["range_id"], locator, False))

    result = {}
    for key, occurrences in literals.items():
        if len(occurrences) != 1:
            continue
        (range_id, line), = occurrences
        # PDF grid rows share the table box. The cell's exact line, rather
        # than the box or the checkbox wording, establishes a unique row.
        matches = [(row_id, has_options) for row_id, table, has_options in cells.get(key, ())
                   if table["left"] <= line["left"] <= line["right"] <= table["right"]
                   and table["bottom"] <= line["bottom"] <= line["top"] <= table["top"]]
        if len(matches) == 1 and (projected or matches[0][1]):
            result[range_id] = matches[0][0]
    return result


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
        self.roles = {document["document_id"]: document["role"] for document in source_plan["documents"]}
        self.document_order = {document["document_id"]:number for number,document in enumerate(source_plan["documents"])}
        self.sources, self.indices, self.owners = {}, {}, {}
        self.positions, self.pages, self.unreadable_pages = {}, {}, set()
        self.pdf_choice_rows = {}
        self.pdf_projected_ranges = defaultdict(set)
        for document in source_plan["documents"]:
            if document.get("source_index_ref"):
                source = self.repo.source_index(document["source_index_ref"])
                self.indices[document["document_id"]] = source
                self.positions[document["document_id"]] = {entry["range_id"]:number
                    for number,entry in enumerate(source.ranges)}
                for entry in source.ranges:
                    self.sources[entry["range_id"]] = entry
                if source.document["original_ref"].get("media_type") == "application/pdf":
                    self.pdf_choice_rows.update(_pdf_table_line_rows(source.ranges))
                    for line, row in _pdf_table_line_rows(source.ranges, projected=True).items():
                        self.pdf_projected_ranges[line].add(row)
                        self.pdf_projected_ranges[row].add(line)
        self.packs = {entry["pack_id"]: self.repo.load_pack(entry) for entry in source_plan["packs"]}
        if len(items_index["packs"]) != len(self.packs) or {
            entry["pack_id"] for entry in items_index["packs"]} != self.packs.keys():
            raise ModelSemanticError("Analysis ledger must cover every source pack exactly once")
        self.items, self.item_refs, self.relations, self.dispositions = {}, [], {}, {}
        self.problems = assign_issues(source_plan["issues"], "source")
        self.protected = {problem["issue_id"] for problem in self.problems}
        # Registration/freeze/OCR codes can be parser-defined; any of them leaves the analysis incomplete.
        self.source_issue_ids = set(self.protected)
        self.host_issue_ids = set(self.protected)
        self.review_targets = set()
        for pack in self.packs.values():
            for entry in [*pack["body"], *pack["supporting"]]:
                self.sources[entry["range_id"]] = entry
            host_reviewed = host_reviewed_ranges(pack)
            for entry in pack["body"]:
                range_id = entry["range_id"]
                if range_id in self.owners:
                    raise ModelSemanticError("Source body occurs in multiple extraction packs")
                self.owners[range_id] = pack
                if range_id not in host_reviewed:
                    self.review_targets.add(range_id)
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
            # Old artifacts lack provenance; keep their conservative classification.
            self.host_issue_ids.update(value.get("host_issue_ids", [p["issue_id"] for p in problems]))
            unresolved = {d["range_id"] for d in value["dispositions"] if d["status"] == "unresolved"}
            self.protected.update(p["issue_id"] for p in problems
                                  if p["code"] == "unresolved_source_range" and p.get("range_id") in unresolved)
            self.protected.update(p["issue_id"] for p in assign_issues(pack.get("issues", []), pack["pack_id"]))
            self.protected.update(p["issue_id"] for p in problems
                                  if p["issue_id"] in self.host_issue_ids and is_hard_blocking(p))
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
        self.completed_model_requests = 0
        self.incomplete_requests = []
        self.local_candidates, self.problem_routing, self.problem_dependencies = [], [], {}
        indexed_documents = {source["document_id"] for source in self.sources.values()}
        # Source completeness also applies when request preparation is replayed.
        for key in sorted(self.unreadable_pages):
            if key[0] in indexed_documents:
                self._flag_unreadable_page(key)

    def payload(self, operation, work_unit, **fields):
        return {"schema_version": 2, "operation": operation, "work_unit_id": work_unit,
                "source_views": [],
                "items": [], "relations": [], "decisions": [], "dispositions": [],
                "issues": [], "candidates": [],
                **fields}

    def stable_payload(self, operation, prefix, **fields):
        draft = self.payload(operation, prefix, **fields)
        draft["work_unit_id"] = _stable_unit(prefix, {key: value for key, value in draft.items()
                                                     if key != "work_unit_id"})
        return draft

    def views(self, ranges, *, complete=False, supporting_ranges=()):
        """Load original targets and their actual bound conditions, never a fake document."""
        by_document, aliases = {}, {}
        sources = sorted((self.sources[identity] for identity in set(ranges) if identity in self.sources),
                         key=self.source_order)
        for source in sources:
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
            for source in sources:
                pack = self.owners.get(source["range_id"])
                if pack and pack["document_id"] == document_id:
                    support.update({entry["range_id"]: entry for entry in pack["supporting"]})
                    pending.extend(pack.get("pending_scope_context", []))
                    bindings.extend(pack.get("scope_bindings", []))
            for range_id in supporting_ranges:
                source = self.sources[range_id]
                if source["document_id"] == document_id:
                    support.setdefault(range_id, source)
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

    def review_payload(self, ranges, work_unit=None, *, check_omissions=True):
        # Matched projections supply existing items, never extra omission targets.
        dispositions = [self.dispositions[r] for r in ranges
                        if r in self.review_targets and r in self.dispositions] if check_omissions else []
        projected = {peer for target in dispositions
                     for peer in self.pdf_projected_ranges.get(target["range_id"], ())}
        selected = set(ranges)
        ids = set()
        while True:
            views = self.views(selected)
            supplied = _view_ids(views)
            items = [item for item in self.items.values() if item["item_id"] in ids
                     or any(e["range_id"] in selected or e["range_id"] in projected for e in item["evidence"])]
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
        choice_context = {self.pdf_choice_rows[original] for target in dispositions
                          if (original := self.source_identity(target["range_id"])) in self.pdf_choice_rows} - supplied
        if choice_context:
            # Scope evidence alone must not pull other items, issues or
            # relation endpoints into this request's review assignments.
            views = self.views(selected, supporting_ranges=choice_context)
        fields = dict(source_views=views, items=items, relations=relations, dispositions=dispositions,
                      issues=_public_issues(problems))
        # BUG-299: only decisions this request actually reads, so unrelated request identities stay stable.
        read = {self.source_identity(r) for r in _view_ids(views)}
        decisions = [deepcopy(decision) for decision in self.plan.get("scope_decisions", [])
                     if read & {self.source_identity(r) for r in (decision["range_id"], *decision["target_range_ids"])}]
        if decisions:
            fields["scope_decisions"] = decisions
        if work_unit is None:
            return self.stable_payload("review", "review", **fields)
        return self.payload("review", work_unit, **fields)

    def findings(self, values, payload):
        allowed = _view_ids(payload["source_views"])
        allowed |= {p["range_id"] for p in payload["issues"] if p.get("range_id")}
        items = {item["item_id"] for item in payload["items"]}
        for index, finding in enumerate(values):
            if (finding.get("range_id") and finding["range_id"] not in allowed
                    or not set(finding.get("item_ids", [])) <= items):
                raise ModelContractError([{"path": ["findings", index], "code": "unknown_reference",
                                           "detail": "Finding refers to an identity absent from its request"}])
        return assign_issues(values, payload["work_unit_id"])

    def validate(self, response, payload):
        from docreader.tender.schema import AnalysisReviewResponse

        operation = payload["operation"]
        if operation != "review":
            raise ModelConfigurationError(f"Unsupported verify operation: {operation}")
        response = deepcopy(response)
        selector_diagnostics = []
        if isinstance(response, dict) and isinstance(response.get("findings"), list):
            allowed = {p["issue_id"] for p in payload["issues"]}
            response["findings"] = [recover_issue_routing(finding, allowed, f"findings[{index}]", selector_diagnostics)
                                    for index, finding in enumerate(response["findings"])]
        if isinstance(response, dict):
            ignored = [f"reviewed_{name}_ids" for name in ("range", "item", "relation", "decision")
                       if f"reviewed_{name}_ids" in response]
            for key in ignored:
                response.pop(key)
            if ignored:
                selector_diagnostics.append({"code": "model_review_scope_ignored", "fields": ignored})
        try:
            value = AnalysisReviewResponse.model_validate(response).model_dump(exclude_unset=True)
        except ValidationError as error:
            raise ModelContractError([{"path": list(entry["loc"]), "code": entry["type"], "detail": entry["msg"]}
                                      for entry in error.errors(include_url=False, include_input=False)]) from None
        if selector_diagnostics:
            value["diagnostics"] = selector_diagnostics
        self.findings(value["findings"], payload)
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
        return value

    def source_identity(self, range_id):
        return self.sources.get(range_id, {}).get("source_range_id", range_id)

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
        semaphore = getattr(model, "semaphore", None) or asyncio.Semaphore(2)
        scope_locks = {payload["work_unit_id"]: asyncio.Lock() for payload in payloads}
        stopped = asyncio.Event()

        # Each call is a bounded sibling with its own immutable paid-call receipt.
        async def worker(payload, descriptor):
            if descriptor["budget"]["fits"] is False:
                return None
            async with scope_locks[payload["work_unit_id"]], semaphore:
                if stopped.is_set():
                    return None  # The stage raises before these skipped slots are aggregated.
                try:
                    async def produce():
                        if hasattr(model, "call_validated"):
                            return await model.call_validated("verify", payload, lambda value: self.validate(value, payload))
                        value = await model.call("verify", payload)
                        return await asyncio.to_thread(self.validate, value, payload)

                    ref = await save(f"analysis-{phase}", produce, scope=payload["work_unit_id"],
                                     inputs={"plan_ref": plan_ref, "payload_digest": descriptor["input_digest"]}, model=True)
                    return ref, await asyncio.to_thread(self.artifacts.read_json, ref)
                except BaseException:
                    # Stop new units before releasing capacity; started siblings
                    # retain their existing retries and settle independently.
                    stopped.set()
                    raise

        # Settle successful siblings before propagating a failure. External
        # cancellation still cancels and awaits all workers through gather.
        settled = await asyncio.gather(*(worker(payload, descriptor)
            for payload, descriptor in zip(payloads, descriptors, strict=True)), return_exceptions=True)
        failures = [result for result in settled if isinstance(result, BaseException)]
        if failures:
            stop = (asyncio.CancelledError, Cancelled, StaleExecution)
            fallback = next((failure for failure in failures if isinstance(failure, ModelWorkUnitExhausted)), failures[0])
            raise next((failure for failure in failures if isinstance(failure, stop)), fallback)
        results = []
        for payload, result in zip(payloads, settled, strict=True):
            if result is None:
                problems = assign_issues([issue("analysis_budget_exceeded",
                    "Complete indivisible analysis input exceeds the admission budget; the host request grouping "
                    "must be fixed before rerunning, a source revision cannot shrink it.")],
                    payload["work_unit_id"])
                self.problems.extend(problems)
                self.protected.update(p["issue_id"] for p in problems)
                self.host_issue_ids.update(p["issue_id"] for p in problems)
                self.incomplete_requests.append(payload["work_unit_id"])
                results.append(None)
                continue

            ref, value = cast(tuple[Any, Any], result)
            self.operation_refs.append({"operation": phase, "work_unit_id": payload["work_unit_id"], "artifact_ref": ref})
            self.completed_model_requests += 1
            self.problems.extend(self.findings(value["findings"], payload))
            results.append(value)
        return plan_ref, results

    def _flag_unreadable_page(self, key):
        unit = _stable_unit("review-page", {"document_id": key[0], "page_ordinal": key[1]})
        if unit in self.incomplete_requests:
            return
        problems = assign_issues([issue("analysis_page_source_unavailable",
            "The selected complete physical PDF page has missing or partial source; source revision is required.")],
            unit)
        self.problems.extend(problems)
        self.protected.update(problem["issue_id"] for problem in problems)
        self.host_issue_ids.update(problem["issue_id"] for problem in problems)
        self.incomplete_requests.append(unit)

    def _expand_review_ranges(self, ranges):
        """Load frozen page and section siblings into the same review. Do not ask the model to name them."""
        selected = set(ranges)
        for range_id in list(selected):
            for key, page_ranges in self.pages.items():
                if range_id in page_ranges and key not in self.unreadable_pages:
                    selected.update(page_ranges)
            source = self.sources.get(range_id)
            if source is not None:
                selected.update(self.sections.get((source["document_id"], source.get("section_id")), ()))
        # Set order differs between processes; packing and paid request bytes must not.
        return sorted((range_id for range_id in selected if range_id in self.sources),
                      key=lambda range_id: self.source_order(self.sources[range_id]))

    def review_requests(self):
        payloads = []
        # Expand once across all extraction packs. Batching the union keeps
        # adjacent source together without assigning siblings a second review.
        expanded = self._expand_review_ranges(self.review_targets)
        for _, ranges in groupby(expanded, key=lambda range_id: self.sources[range_id]["document_id"]):
            current = []
            for range_id in ranges:
                trial = self.review_payload([*current, range_id])
                measured = self.budget.measure("verify", trial)
                if current and self._over_review_target(measured):
                    payloads.append(self.review_payload(current))
                    current = []
                current.append(range_id)
            if current:
                payloads.append(self.review_payload(current))
        return self._supplement_review_requests(payloads)

    def _over_review_target(self, measured):
        return measured["fits"] is False or (
            measured["method"] == RequestBudget.TOKENIZED_METHOD and measured["input_tokens"] > self.REVIEW_INPUT_TOKENS
        )

    def _historical_review_groups(self, old_plan, old_items, old_requests):
        previous_identity = {**self.identity, "domain_revision": self.identity["domain_revision"] - 1}
        if old_plan.get("run_identity") != previous_identity or {**old_plan, "run_identity": self.identity} != self.plan:
            return None
        previous = _Analysis(old_plan, old_items, self.artifacts, previous_identity, self.budget)
        expanded = self._expand_review_ranges(self.review_targets)
        if previous._expand_review_ranges(previous.review_targets) != expanded:
            return None
        ordinals = {identity: i for i, identity in enumerate(expanded)}
        main = [request for request in old_requests if request["dispositions"]]
        groups, start = [], 0
        for i, request in enumerate(main):
            last = ordinals[request["dispositions"][-1]["range_id"]]
            next_first = (ordinals[main[i + 1]["dispositions"][0]["range_id"]]
                          if i + 1 < len(main) else len(expanded))
            if start > last or last >= next_first:
                return None
            expected = _review_json(request)
            for end in range(last + 1, next_first + 1):
                group = expanded[start:end]
                if self.sources[group[0]]["document_id"] != self.sources[group[-1]]["document_id"]:
                    return None
                # Views also contain transitive evidence. Only the original
                # ordered source slice may be inherited as the group boundary.
                if _review_json(previous.review_payload(group)) == expected:
                    groups.append(group)
                    start = end
                    break
            else:
                return None
        return groups if groups and start == len(expanded) else None

    def reused_review_requests(self, old_plan, old_items, old_requests):
        try:
            groups = self._historical_review_groups(old_plan, old_items, old_requests)
        except (KeyError, TypeError, ValueError, OSError, AttributeError, IndexError):
            # A historical artifact is a planning hint. The current plan and
            # its execution identity do not depend on being able to read it.
            return None
        if groups is None:
            return None

        def fit_group(group):
            payload = self.review_payload(group)
            measured = self.budget.measure("verify", payload)
            if len(group) == 1 or not self._over_review_target(measured):
                return [payload]
            middle = len(group) // 2
            return [*fit_group(group[:middle]), *fit_group(group[middle:])]

        payloads = self._supplement_review_requests([payload for group in groups for payload in fit_group(group)])
        if (len({p["work_unit_id"] for p in payloads}) != len(payloads)
                or Counter(d["range_id"] for p in payloads for d in p["dispositions"]) != Counter(self.review_targets)
                or {i["item_id"] for p in payloads for i in p["items"]} != self.items.keys()
                or {r["relation_id"] for p in payloads for r in p["relations"]} != self.relations.keys()
                or any(self.budget.measure("verify", p)["fits"] is False for p in payloads)):
            return None
        return payloads

    def _supplement_review_requests(self, payloads):
        reviewed_relations = {r["relation_id"] for p in payloads for r in p["relations"]}
        pending_ranges, pending = set(), None
        for relation_id, relation in self.relations.items():
            if relation_id in reviewed_relations:
                continue
            endpoints = [self.items[relation[key]] for key in ("from_item_id", "to_item_id")]
            ranges = {e["range_id"] for item in endpoints for e in item["evidence"]
                      if e["range_id"] in self.dispositions}
            # Relationship semantics require both endpoints and all evidence in
            # one request; combine complete supplements within the review budget.
            combined = pending_ranges | ranges
            trial = self.review_payload(sorted(combined), check_omissions=False)
            measured = self.budget.measure("verify", trial)
            if pending is not None and self._over_review_target(measured):
                payloads.append(pending)
                combined = ranges
                trial = self.review_payload(sorted(combined), check_omissions=False)
            pending_ranges, pending = combined, trial
            # A rejected union may contain extra relations absent from both
            # retained batches; only an accepted complete payload owns coverage.
            reviewed_relations.update(r["relation_id"] for r in pending["relations"])
        if pending is not None:
            payloads.append(pending)
        scope_dependencies = [(decision, {decision["range_id"], *decision["target_range_ids"],
                                          *(e["range_id"] for e in decision["evidence"])})
                              for decision in self.plan.get("scope_decisions", [])]
        for _, dependencies in scope_dependencies:
            # Human scope interpretation, including local_only, must actually
            # reach independent review with its original and target evidence.
            ranges = set(dependencies)
            canonical = {self.source_identity(r) for r in ranges}
            items = [i for i in self.items.values() if any(self.source_identity(e["range_id"]) in canonical
                                                         for e in i["evidence"])]
            ranges.update(e["range_id"] for i in items for e in i["evidence"])
            ids = {i["item_id"] for i in items}
            relations = [r for r in self.relations.values() if {r["from_item_id"], r["to_item_id"]} <= ids]
            ranges.update(e["range_id"] for r in relations for e in r["evidence"])
            scope_views = self.views(ranges, complete=True)
            supplied = _view_ids(scope_views)
            decisions = [deepcopy(decision) for decision, required in scope_dependencies if required <= supplied]
            payloads.append(self.stable_payload("review", "scope-review",
                source_views=scope_views, items=items, relations=relations,
                issues=_public_issues(self.problems), scope_decisions=decisions))
        return payloads

    def route_problems(self):
        """Host bookkeeping for existing problems; it builds no model request."""
        # Freeze routing once. Later findings cannot recursively reschedule it.
        self.local_candidates, self.problem_routing, self.problem_dependencies = [], [], {}
        for group in _issue_groups(assign_issues(self.problems, "analysis")):
            identities = [p["issue_id"] for p in group]
            for identity in identities:
                self.problem_dependencies[identity] = set(identities)
            needs = [p["needs_source"] for p in group if "needs_source" in p]
            route = "reference"
            reason = "需要额外原文或尚未判断，保留待核对。"
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
                    local = _stable_unit("local", {"issue_ids": identities, "range_ids": ranges, "item_ids": ids})
                    self.local_candidates.append({"candidate_id": f"{local}:c1",
                        "issue_ids": identities, "range_ids": ranges, "item_ids": ids, "reason": reason})
            self.problem_routing.append({"issue_ids": identities, "route": route, "reason": reason})



async def verify_analysis(source_plan, items_index, artifacts, model, budget, *, identity, save, previous_review=None) -> dict:
    context = await asyncio.to_thread(_Analysis, source_plan, items_index, artifacts, identity, budget)
    # Preparation pins the plan for this revision, including across planner
    # changes. Regrouping only happens when no preparation exists for the inputs.
    review_inputs = await asyncio.to_thread(review_preparation_digest, source_plan, items_index, identity, budget.spec())

    def prepare_reviews(previous):
        # CAS sorts object fields; retain their original order inside the string
        # because short-reference traversal and model message text depend on it.
        requests = context.reused_review_requests(*previous) if previous is not None else None
        return {"requests_json": _review_json(requests if requests is not None else context.review_requests())}

    async def prepared_reviews():
        previous = await previous_review() if previous_review is not None else None
        return await asyncio.to_thread(prepare_reviews, previous)

    prepared_ref = await save("analysis-review-requests", prepared_reviews, scope=review_inputs,
                              inputs={"planning_digest": review_inputs})
    prepared = await asyncio.to_thread(artifacts.read_json, prepared_ref)
    source_reviews = await asyncio.to_thread(json.loads, prepared["requests_json"])
    if not isinstance(source_reviews, list):
        raise ValueError("Cached source review requests must be an array")
    review_plan_ref, source_reviewed = await context.execute("review", source_reviews, model, save)
    # Routing is host bookkeeping. It must not become another model round.
    await asyncio.to_thread(context.route_problems)
    candidates = list(context.local_candidates)
    decisions, relations = [], list(context.relations.values())
    completed = [result for result in source_reviewed if result]
    required = {"range": context.review_targets, "item": set(context.items),
                "relation": {r["relation_id"] for r in relations},
                "decision": set()}
    coverage = {name: {identity for result in completed for identity in result[f"reviewed_{name}_ids"]}
                for name in required if name != "range"}
    coverage["range"] = set()
    for payload, result in zip(source_reviews, source_reviewed, strict=True):
        if result:
            assigned = {entry["range_id"] for entry in payload["dispositions"]}
            coverage["range"].update(assigned.intersection(result["reviewed_range_ids"]))
    missing = {name: sorted(ids - coverage[name]) for name, ids in required.items() if ids - coverage[name]}
    if missing:
        problems = assign_issues([issue("analysis_review_incomplete",
            "Necessary source, item, relation or decision coverage is incomplete.")], "coverage")
        context.problems.extend(problems)
        context.host_issue_ids.update(problem["issue_id"] for problem in problems)
    accepted: set[str] = set()
    resolved: set[str] = set()
    all_issues = assign_issues(context.problems, "analysis")
    active = [p for p in all_issues if p["issue_id"] not in resolved]
    hard = {p["issue_id"] for p in active
            if p["blocking"] and (p["issue_id"] in context.source_issue_ids
                                 or p["issue_id"] in context.host_issue_ids and is_hard_blocking(p))}
    human = sorted(p["issue_id"] for p in active if p["blocking"] and p["issue_id"] not in hard)
    relations_ref = await asyncio.to_thread(artifacts.put_json, relations)
    decisions_ref = await asyncio.to_thread(artifacts.put_json, decisions)
    candidates_ref = await asyncio.to_thread(artifacts.put_json, candidates)
    return {"schema_version": 2, "blocking": bool(missing or context.incomplete_requests) or any(p["blocking"] for p in active),
            "hard_blocking": bool(missing or context.incomplete_requests or hard), "human_issue_ids": human,
            "unverified_relation_ids": [],
            "issues": active, "all_issues": all_issues, "resolved_issue_ids": sorted(resolved),
            "accepted_decision_ids": sorted(accepted), "item_refs": context.item_refs,
            "relations_ref": relations_ref, "decisions_ref": decisions_ref, "candidates_ref": candidates_ref,
            "review_plan_ref": review_plan_ref, "locate_plan_ref": None,
            "association_plan_ref": None, "joint_plan_ref": None,
            "operation_refs": context.operation_refs,
            "problem_routing": context.problem_routing,
            "coverage": {"reviewed": {name: sorted(ids) for name, ids in coverage.items()},
                         "joint_candidate_ids": [], "missing": missing,
                         "incomplete_work_unit_ids": context.incomplete_requests},
            "counts": {"items": len(context.items), "ranges": len(context.owners), "relations": len(relations),
                       "decisions": len(decisions), "candidates": len(candidates),
                       "blocking_issues": sum(p["blocking"] for p in active),
                       "model_calls": context.completed_model_requests}}


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
