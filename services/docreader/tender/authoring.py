"""Outline semantics are delegated; identities and frozen templates stay with the host."""

from __future__ import annotations

import asyncio
from collections import Counter
from copy import deepcopy
from importlib.resources import files

from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.integrations.authoring import read_authoring_input
from docreader.llm.request import ModelConfigurationError, ModelContractError, build_request
from docreader.llm.wire import WirePayload
from docreader.tender.checks.analysis import current_items
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.evidence.view import source_view
from docreader.tender.schema import AuthoringInput, AuthoringOutline, OutlineResponse, SourceUnitRef


def load_outline_source(artifacts, reference: dict) -> dict:
    """Load the unchanged native unit and its frozen OCR/image/cell dependencies."""
    value = SourceUnitRef.model_validate(reference).model_dump()
    index = EvidenceRepository(artifacts).source_index(value["source_index_ref"])
    unit = next((u for u in index.raw["units"] if u["unit_id"] == value["unit_id"]), None)
    if unit is None:
        raise ArtifactIntegrityError("outline source refers to an unknown frozen unit")
    loaded = deepcopy({"document": index.document, "unit": unit})
    if unit["kind"] == "image_region":
        for field, mapping, key in (("ocr", "ocr", unit["unit_id"]),
                                    ("image_ref", "images", unit["locator"]["image_key"]),
                                    ("image_anchor", "image_anchors", unit["unit_id"])):
            if key in index.raw.get(mapping, {}):
                loaded[field] = deepcopy(index.raw[mapping][key])
    if "cell_refs" in unit["locator"]:
        try:
            cells = {key: deepcopy(index.raw["spreadsheet_cells"][key]) for key in unit["locator"]["cell_refs"]}
            grid_ids = {cell["grid_cell_ref"]["unit_id"] for cell in cells.values() if cell.get("grid_value_fields")}
            values = {(u["unit_id"], cell["row"], cell["column"]): cell["text"]
                      for u in index.raw["units"] if u["unit_id"] in grid_ids for cell in u["grid"]["cells"]}
            for cell in cells.values():
                for field in cell.get("grid_value_fields", []):
                    ref = cell["grid_cell_ref"]
                    cell[field] = values[(ref["unit_id"], ref["row"], ref["column"])]
        except KeyError:
            raise ArtifactIntegrityError("outline source has an unresolved frozen cell reference") from None
        loaded["spreadsheet_cells"] = cells
    return loaded


class _OutlineContext:
    def __init__(self, artifacts, authoring_input: dict):
        self.artifacts = artifacts
        self.input = AuthoringInput.model_validate(authoring_input).model_dump()
        verification = artifacts.read_json(self.input["verification_ref"])
        plan = artifacts.read_json(self.input["pack_plan_ref"])
        confirmation = artifacts.read_json(self.input["confirmation_ref"])
        self.rules, rule_views, retained = {}, [], Counter()
        for ordinal, entry in enumerate(current_items(artifacts, verification), 1):
            identity = f"rule-{ordinal}"
            reference = {"item_ref": entry["item_ref"]}
            if "effective_rule" in entry:
                key = (entry["item_ref"]["artifact_ref"]["sha256"], entry["item_ref"]["item_id"], entry["decision_id"])
                reference.update(decision_id=entry["decision_id"], rule_index=retained[key])
                retained[key] += 1
                semantic = {"item_id": entry["item_ref"]["item_id"], "effective_rule": entry["effective_rule"]}
            else:
                semantic = {"item": entry["item"]}
            self.rules[identity] = reference
            rule_views.append({"rule_id": identity, **semantic})
        self.payload = {"schema_version": 2, "rules": rule_views,
                        "relations": artifacts.read_json(verification["relations_ref"]),
                        "unverified_relation_ids": verification["unverified_relation_ids"],
                        "issues": verification["issues"],
                        "issue_resolutions": confirmation.get("issue_resolutions"),
                        "scope_decisions": plan.get("scope_decisions", [])}
        self.sources, self.read_units = {}, set()
        self._sources(plan)

    def _sources(self, plan):
        repository = EvidenceRepository(self.artifacts)
        documents = self.artifacts.read_json(self.input["frozen_ref"])["documents"]
        indexes = {d["source_index_ref"]["sha256"]: repository.source_index(d["source_index_ref"])
                   for d in documents}
        native_units = {digest: {unit["unit_id"]: unit for unit in index.raw["units"]}
                        for digest, index in indexes.items()}
        ranges, catalog, packs = {}, [], []

        def bind(source, source_ref):
            units = native_units[source_ref["sha256"]]
            # Spreadsheet rows can cover several side-by-side table grids.
            tables = source.get("table_ids") or [source.get("table_id")]
            unit_ids = [u for u in tables if u in units and units[u].get("grid")]
            native = units.get(source["unit_id"])
            if unit_ids and native and native["kind"] == "table_row":
                locator = native["locator"]
                cells = ([indexes[source_ref["sha256"]].raw["spreadsheet_cells"][key]
                          for key in locator["cell_refs"]] if "cell_refs" in locator else locator.get("cells", []))
                regions = [units[u]["locator"]["region"] for u in unit_ids]
                if any(not any(region["start_row"] <= cell["row"] <= region["end_row"]
                               and region["start_column"] <= cell["column"] <= region["end_column"]
                               for region in regions) for cell in cells):
                    unit_ids.append(native["unit_id"])
            if not unit_ids and source["unit_id"] in units:
                unit_ids = [source["unit_id"]]
            if unit_ids:
                self.sources[source["range_id"]] = [{"source_index_ref": source_ref, "unit_id": u}
                                                      for u in dict.fromkeys(unit_ids)]
            ranges[source["range_id"]] = source
            return unit_ids

        for document in documents:
            source_ref = document["source_index_ref"]
            index = indexes[source_ref["sha256"]]
            units = set()
            for source in index.ranges:
                unit_ids = bind(source, source_ref)
                if set(unit_ids) - units:
                    units.update(unit_ids)
                    catalog.append({"range_id": source["range_id"], "document_id": document["document_id"],
                                    "role": document["role"], "kind": source["kind"],
                                    "heading_path": source.get("heading_path", "")})
        for descriptor in plan["packs"]:
            pack = self.artifacts.read_json(descriptor["artifact_ref"])
            index = indexes[pack["source_index_ref"]["sha256"]]
            packs.append(pack)
            for reference in [*pack["body"], *pack["supporting"],
                              *pack.get("scope_bindings", []), *pack.get("pending_scope_context", [])]:
                source = index.materialize(reference)
                bind(source, pack["source_index_ref"])

        needed = set()

        def collect(value):
            if isinstance(value, list):
                for entry in value:
                    collect(entry)
            elif isinstance(value, dict):
                for key, entry in value.items():
                    if key == "range_id" and entry in self.sources:
                        needed.add(entry)
                    elif key in {"target_range_ids", "applies_to_range_ids", "reviewed_target_range_ids"}:
                        needed.update(i for i in entry if i in self.sources)
                    else:
                        collect(entry)

        for rule in self.payload["rules"]:
            for evidence in rule.get("item", rule.get("effective_rule"))["evidence"]:
                if evidence["range_id"] not in self.sources:
                    raise ArtifactIntegrityError("effective rule refers to an unknown frozen source")
        collect(self.payload)

        def unit_keys(range_id):
            return {(reference["source_index_ref"]["sha256"], reference["unit_id"])
                    for reference in self.sources.get(range_id, [])}

        selected = set().union(*(unit_keys(identity) for identity in needed))
        related_packs = [pack for pack in packs if any(unit_keys(r["range_id"]) & selected for r in pack["body"])]
        for pack in related_packs:
            needed.update(r["range_id"] for r in pack["supporting"] if r["range_id"] in self.sources)
        self.read_units = set().union(*(unit_keys(identity) for identity in needed))
        views = []
        for document in documents:
            source_ref = document["source_index_ref"]
            index = indexes[source_ref["sha256"]]
            body = [r for r in index.ranges if unit_keys(r["range_id"]) & self.read_units]
            if not body:
                continue
            supporting = [ranges[i] for i in sorted(needed) if self.sources[i][0]["source_index_ref"] == source_ref
                          and ranges[i]["range_id"] != ranges[i]["source_range_id"]]
            local_packs = [p for p in related_packs if p["source_index_ref"] == source_ref]
            views.append(source_view({"pack_id": "outline:" + document["document_id"],
                "document_id": document["document_id"], "document_version": document["document_version"],
                "role": document["role"], "body": body, "supporting": supporting,
                "scope_bindings": [b for p in local_packs for b in p.get("scope_bindings", [])],
                "pending_scope_context": [b for p in local_packs for b in p.get("pending_scope_context", [])]}))
        self.payload.update(source_views=views, source_catalog=catalog)

    def validate(self, response: dict) -> dict:
        try:
            parsed = OutlineResponse.model_validate(response).model_dump()
        except ValidationError as error:
            raise ModelContractError([{"path": list(e["loc"]), "code": e["type"], "detail": e["msg"]}
                                      for e in error.errors(include_input=False)]) from None
        issues, assigned = [], set()

        def references(identities, mapping, path):
            unknown = [identity for identity in identities if identity not in mapping]
            if unknown:
                raise ModelContractError([{"path": path, "code": "outline_unknown_reference",
                                           "detail": "Select references supplied in this request."}])
            return [deepcopy(mapping[i]) for i in dict.fromkeys(identities)]

        def problem(code, detail, refs, target_id=None):
            issues.append({"code": code, "detail": detail, "rule_refs": refs,
                           **({"target_id": target_id} if target_id else {})})

        def chapters(entries, parent=(), path=("chapters",)):
            result = []
            for ordinal, chapter in enumerate(entries, 1):
                position = (*parent, ordinal)
                chapter_id = "chapter-" + ".".join(map(str, position))
                current_path = (*path, ordinal - 1)
                targets = []
                for number, target in enumerate(chapter["targets"], 1):
                    target_id = f"{chapter_id}:target-{number}"
                    target_path = [*current_path, "targets", number - 1]
                    refs = references(target["rule_ids"], self.rules, [*target_path, "rule_ids"])
                    sources = [source for group in references(target["range_ids"], self.sources, [*target_path, "range_ids"])
                               for source in group]
                    sources = list({(s["source_index_ref"]["sha256"], s["unit_id"]): s for s in sources}.values())
                    assigned.update(target["rule_ids"])
                    if not refs:
                        problem("outline_target_unbound", "该响应位置尚未绑定有效要求，需核对其用途。", [], target_id)
                    if target["kind"] == "form" and not sources:
                        problem("outline_template_missing", "规定格式/表格尚未定位源模板，需补充来源。", refs, target_id)
                    if any((s["source_index_ref"]["sha256"], s["unit_id"]) not in self.read_units for s in sources):
                        problem("outline_source_not_read", "已定位目录来源，但模型尚未阅读正文，生成前须装载核对。", refs, target_id)
                    targets.append({"target_id": target_id, "kind": target["kind"], "purpose": target["purpose"],
                                    "rule_refs": refs, "source_refs": sources})
                result.append({"chapter_id": chapter_id, "title": chapter["title"], "targets": targets,
                               "children": chapters(chapter["children"], position, (*current_path, "children"))})
            return result

        organized = chapters(parsed["chapters"])
        for ordinal, finding in enumerate(parsed["findings"]):
            refs = references(finding["rule_ids"], self.rules, ["findings", ordinal, "rule_ids"])
            problem(finding["code"], finding["detail"], refs)
        for identity in self.rules:
            if identity not in assigned:
                problem("outline_rule_unassigned", "该有效规则尚未安排响应位置，需补充大纲映射。", [self.rules[identity]])
        return AuthoringOutline.model_validate({"schema_version": 2, "authoring_input": self.input,
                                                "chapters": organized, "issues": issues}).model_dump(exclude_unset=True)


async def organize_outline(store, artifacts, execute, budget, *, run_id: str, source_version: int,
                           domain_revision: int, checklist_digest: str) -> dict:
    """The injected executor owns sending, durable retry and response correction.

    It receives the complete request and a synchronous validator, and returns
    the validator's result. This domain function never borrows an analysis lease.
    """
    identity = {"run_id": run_id, "source_version": source_version,
                "domain_revision": domain_revision, "checklist_digest": checklist_digest}
    approved = await read_authoring_input(store, artifacts, **identity)
    context = await asyncio.to_thread(_OutlineContext, artifacts, approved)
    if budget.output_reserve <= 0:
        raise ModelConfigurationError("outline requires an explicit positive output reserve")
    prompt = budget.prompts.get("outline")
    if prompt is None:
        prompt = files("docreader.tender.prompts").joinpath("outline.md").read_text(encoding="utf-8")
    wire = WirePayload("outline", context.payload)
    request = build_request(budget.model, prompt, wire.payload)
    budget.require_request(request)
    result = await execute(request, lambda response: context.validate(wire.decode(response)))
    # A model call may outlive the confirmed analysis it was based on.
    if await read_authoring_input(store, artifacts, **identity) != approved:
        raise ArtifactIntegrityError("confirmed analysis references changed during outline organization")
    return await asyncio.to_thread(artifacts.put_json, result)
