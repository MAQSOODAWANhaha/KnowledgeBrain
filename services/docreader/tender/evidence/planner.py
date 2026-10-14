"""Native-section-first planning admitted by the actual complete request budget."""
from __future__ import annotations

import hashlib
import json
import re
from bisect import bisect_right
from itertools import groupby

from docreader.tender.evidence.packing import SupportingIndex
from docreader.tender.evidence.references import SourceIndex, fragment, selector


PLANNER_VERSION = "native-sections-full-request-v1"


def _groups(entries, key):
    return [list(values) for _, values in groupby(entries, key=key)]


def _split(entry: dict) -> list[dict]:
    start, end = entry["start"], entry["end"]
    if end - start <= 1:
        return []
    middle = (start + end) // 2
    # A single row first splits between cells; only a single cell/paragraph
    # reaches sentence and then Unicode boundaries. Python offsets are chars.
    boundaries = [cell["start"] for cell in entry.get("cell_intervals", []) if start < cell["start"] < end]
    if not boundaries:
        boundaries = [start + match.end() for match in re.finditer(r"\n+|[。！？；;!?]|\.(?=\s|$)", entry["text"])
                      if start + (end - start) // 4 < start + match.end() < end - (end - start) // 4]
    cut = min(boundaries, key=lambda item: abs(item - middle)) if boundaries else middle
    children = [fragment(entry, start, cut), fragment(entry, cut, end)]
    for ordinal, child in enumerate(children):
        # Reserve a final neighbour identity even when the next split boundary
        # has not been selected yet. Final links are exact, never longer.
        child["previous_fragment_id"] = children[ordinal - 1]["range_id"] if ordinal else "fragment-" + "0" * 24
        child["next_fragment_id"] = children[ordinal + 1]["range_id"] if ordinal + 1 < len(children) else "fragment-" + "0" * 24
    return children


class StructuralPlanner:
    def __init__(self, source: SourceIndex, budget, scope_decisions=()):
        self.source, self.budget = source, budget
        self.support = SupportingIndex(source.ranges, scope_decisions)
        self.probe_count = 0
        self.packs = []
        self.capacity_hint = None
        self.weights = {entry["range_id"]: len(json.dumps(source.materialize(selector(entry)), ensure_ascii=False).encode())
                        for entry in source.ranges}

    def weight(self, entry):
        base = self.source.by_id[entry["source_range_id"]]
        return max(1, self.weights[base["range_id"]] * max(1, entry["end"] - entry["start"]) // max(1, base["end"] - base["start"]))

    def candidate(self, body: list[dict], reason: str) -> dict:
        from docreader.tender.requirements.extract import extract_payload
        supporting, pending, issues, bindings = self.support.select(body)
        digest = hashlib.sha256("\0".join(entry["range_id"] for entry in body).encode()).hexdigest()[:24]
        pack = {"schema_version": 2, "pack_id": "pack-" + digest,
                "document_id": self.source.document["document_id"],
                "document_version": self.source.document["document_version"],
                "body": [selector(entry) for entry in body],
                "supporting": [selector(entry) for entry in supporting],
                "pending_scope_context": pending, "scope_bindings": bindings, "issues": issues,
                "compacted_blank_ranges": [entry["range_id"] for entry in body if entry.get("empty")],
                "planning_details": {"planner_version": PLANNER_VERSION, "boundary_reason": reason,
                         "section_ids": list(dict.fromkeys(entry.get("section_id") for entry in body))}}
        materialized = {**pack, "body": [self.source.materialize(entry) for entry in pack["body"]],
                        "supporting": [self.source.materialize(entry) for entry in pack["supporting"]]}
        measurement = self.budget.measure("extract", extract_payload(materialized))
        self.probe_count += 1
        pack["planning_details"]["budget"] = measurement
        if measurement.get("input_tokens", 0) > 0 and measurement.get("context_tokens"):
            available = max(1, measurement["context_tokens"] - measurement.get("output_reserve", 0) - measurement.get("safety", 0))
            self.capacity_hint = max(1, sum(self.weight(entry) for entry in body) * available // measurement["input_tokens"])
        return pack

    @staticmethod
    def admitted(pack: dict) -> bool:
        return pack["planning_details"]["budget"]["fits"] is True

    def blocked(self, pack: dict):
        pack["issues"].append({"code": "indivisible_source_context_exceeded", "blocking": True,
                               "pack_id": pack["pack_id"], "document_id": pack["document_id"],
                               "detail": "Minimum native body and complete required support do not fit the configured request context."})
        pack["planning_details"]["boundary_reason"] = "indivisible_context_exceeded"
        self.packs.append(pack)

    def partition(self, groups: list[list[dict]], reason: str, fallback):
        """Probe complete prefixes; hints propose boundaries but never grant fit."""
        offset = 0
        prefix = [0]
        for group in groups:
            prefix.append(prefix[-1] + sum(self.weight(entry) for entry in group))
        while offset < len(groups):
            upper = len(groups) - offset + 1
            count = (max(1, bisect_right(prefix, prefix[offset] + self.capacity_hint, lo=offset + 1) - offset - 1)
                     if self.capacity_hint else len(groups) - offset)
            while True:
                body = [entry for group in groups[offset:offset + count] for entry in group]
                pack = self.candidate(body, reason)
                if self.admitted(pack):
                    # A conservative hint may stop before another complete group
                    # that fits. Grow exponentially, then narrow the failed gap;
                    # never rebuild/probe every remaining suffix or assume fit
                    # is monotone. Only a measured success can replace the pack.
                    step, narrowing = 1, False
                    while count + 1 < upper:
                        trial_count = ((count + upper) // 2 if narrowing
                                       else min(count + step, upper - 1))
                        trial = self.candidate([entry for group in groups[offset:offset + trial_count]
                                                for entry in group], reason)
                        if self.admitted(trial):
                            count, pack = trial_count, trial
                            step *= 2
                        else:
                            upper, narrowing = trial_count, True
                    self.packs.append(pack)
                    offset += count
                    break
                if count > 1:
                    upper = count
                    # A failed complete envelope changes the boundary. Half is
                    # only a proposal; every resulting envelope is checked.
                    hint = self.capacity_hint
                    count = min(count - 1, max(1, bisect_right(prefix, prefix[offset] + hint, lo=offset + 1,
                                                                               hi=offset + count + 1) - offset - 1)) if hint else max(1, count // 2)
                    continue
                fallback(groups[offset], pack)
                offset += 1
                break

    def split_range(self, entries: list[dict], rejected: dict):
        entry = entries[0]
        children = _split(entry)
        if not children:
            self.blocked(rejected)
            return
        # Detect mandatory context that is already impossible with a scalar.
        # Do not create thousands of failed one-character packages.
        minimum = fragment(entry, entry["start"], min(entry["end"], entry["start"] + 1))
        probe = self.candidate([minimum], "minimum_body_probe")
        def scope(pack):
            return [(item["source_range_id"], item["start"], item["end"]) for item in pack["supporting"]]
        def coverage(pack):
            # A header can straddle body/support after source deduplication.
            # Its mandatory union is unchanged even though support offsets move.
            merged = []
            for item in sorted([*pack["body"], *pack["supporting"]],
                               key=lambda value: (value["source_range_id"], value["start"])):
                span = (item["source_range_id"], item["start"], item["end"])
                if merged and merged[-1][0] == span[0] and merged[-1][2] >= span[1]:
                    prior = merged.pop()
                    span = (span[0], prior[1], max(span[2], prior[2]))
                merged.append(span)
            return merged
        if not self.admitted(probe) and (scope(probe) == scope(rejected) or coverage(probe) == coverage(rejected)):
            self.blocked(rejected)
            return
        self.partition([[child] for child in children], "text_or_cell_fragment", self.split_range)

    def split_structure(self, entries: list[dict], rejected: dict):
        if len(entries) == 1:
            self.split_range(entries, rejected)
        else:
            self.partition([[entry] for entry in entries], "native_row_group", self.split_range)

    def split_section(self, entries: list[dict], rejected: dict):
        structures = _groups(entries, lambda entry: (
            tuple(entry.get("table_ids") or [entry.get("table_id")]) if entry.get("table_id") else entry["unit_id"]))
        if len(structures) == 1:
            self.split_structure(entries, rejected)
        else:
            self.partition(structures, "native_structure_prefix", self.split_structure)

    def plan(self) -> list[dict]:
        chapters = _groups(self.source.ranges, lambda entry: (entry["document_id"], entry.get("section_id")))
        self.partition(chapters, "complete_adjacent_sections", self.split_section)
        # Split chapter tails were completed within their own partition and are
        # never backfilled with a following chapter. No cross-document merge.
        fragments = {}
        for pack in self.packs:
            for entry in pack["body"]:
                fragments.setdefault(entry["source_range_id"], []).append(entry)
        for items in fragments.values():
            if len(items) <= 1:
                continue
            for index, entry in enumerate(items):
                entry["previous_fragment_id"] = items[index - 1]["range_id"] if index else None
                entry["next_fragment_id"] = items[index + 1]["range_id"] if index + 1 < len(items) else None
        from docreader.tender.requirements.extract import extract_payload
        for pack in self.packs:
            rendered = {**pack, "body": [self.source.materialize(item) for item in pack["body"]],
                        "supporting": [self.source.materialize(item) for item in pack["supporting"]]}
            final = self.budget.measure("extract", extract_payload(rendered))
            self.probe_count += 1
            if self.admitted(pack) and final["fits"] is not True:
                raise RuntimeError("final source links changed an admitted request budget")
            pack["planning_details"]["budget"] = final
        return self.packs


def make_packs(source: SourceIndex, budget, scope_decisions=()) -> tuple[list[dict], dict]:
    planner = StructuralPlanner(source, budget, scope_decisions)
    packs = planner.plan()
    return packs, {"probe_count": planner.probe_count, "planner_version": PLANNER_VERSION}
