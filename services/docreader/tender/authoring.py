"""Outline semantics are delegated; identities and frozen templates stay with the host."""

from __future__ import annotations

from collections import Counter
from copy import deepcopy

from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.llm.request import ModelContractError
from docreader.tender.checks.analysis import current_items
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.evidence.references import selector
from docreader.tender.authoring_excerpts import bind_source_excerpts, response_row_id
from docreader.tender.evidence.view import source_view
from docreader.tender.schema import (
    AUTHORING_GROUNDING_POLICY, AuthoringInput, AuthoringOutline, OutlineResponse, SourceUnitRef,
)


def load_outline_source(artifacts, reference: dict, *, repository=None) -> dict:
    """Load the unchanged native unit and its frozen OCR/image/cell dependencies."""
    value = SourceUnitRef.model_validate(reference).model_dump()
    repository = repository if repository is not None else EvidenceRepository(artifacts)
    index = repository.source_index(value["source_index_ref"])
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


def project_source_sections(sources):
    """Share complete heading paths once, without changing host source records."""
    paths, projected = {}, []
    for source in sources:
        heading = source.get("heading_path", "")
        if heading not in paths:
            paths[heading] = f"outline-section-{len(paths) + 1}"
        projected.append({**{key: value for key, value in source.items() if key != "heading_path"},
                          "section_id": paths[heading]})
    return [{"section_id": identity, "heading_path": heading} for heading, identity in paths.items()], projected


def _check_reference_ids(identities, allowed, path, detail="Select references supplied in this request.",
                         *, code="outline_unknown_reference"):
    for number, identity in enumerate(identities):
        if identity not in allowed:
            raise ModelContractError([{"path": [*path, number], "code": code,
                                       "detail": detail}])


def validate_outline_references(value, *, rule_ids, source_ids, template_part_ids, constraint_source_ids=None):
    """Check a parsed outline against its actual request, reporting one scalar.

    Context/evidence aliases remain useful to targets but do not authorize a
    submission constraint whose complete source was absent from this read.
    """
    rule_ids, source_ids, template_part_ids = set(rule_ids), set(source_ids), set(template_part_ids)
    constraint_sources = source_ids if constraint_source_ids is None else set(constraint_source_ids)
    _check_reference_ids(value["constraint_rule_ids"], rule_ids, ["constraint_rule_ids"])
    _check_reference_ids(value["constraint_range_ids"], constraint_sources, ["constraint_range_ids"],
                         "Select actual sources supplied in this request.")

    def chapters(entries, path):
        for number, chapter in enumerate(entries):
            current = [*path, number]
            for ordinal, target in enumerate(chapter["targets"]):
                for field, allowed in (("rule_ids", rule_ids), ("range_ids", source_ids),
                                       ("template_part_ids", template_part_ids)):
                    _check_reference_ids(target[field], allowed, [*current, "targets", ordinal, field])
                for row_number, row in enumerate((target.get("response_table") or {}).get("rows", [])):
                    for field, allowed in (("rule_ids", rule_ids), ("range_ids", source_ids)):
                        _check_reference_ids(row.get(field, []), allowed,
                            [*current, "targets", ordinal, "response_table", "rows", row_number, field])
            chapters(chapter["children"], [*current, "children"])

    chapters(value["chapters"], ["chapters"])
    for number, finding in enumerate(value["findings"]):
        _check_reference_ids(finding["rule_ids"], rule_ids, ["findings", number, "rule_ids"])
    for number, intent in enumerate(value["search_intents"]):
        _check_reference_ids(intent["for_ids"], rule_ids | source_ids, ["search_intents", number, "for_ids"])


class _OutlineContext:
    def __init__(self, artifacts, authoring_input: dict):
        self.artifacts = artifacts
        self.repository = EvidenceRepository(artifacts)
        self.input = AuthoringInput.model_validate(authoring_input).model_dump()
        self.availability_issues = deepcopy(self.input["issues"])
        self.rule_origin = "source_only"
        verification = self._read(self.input["verification_ref"], "核验", {})
        plan = self._read(self.input["pack_plan_ref"], "分包计划", {})
        confirmation = self._read(self.input["confirmation_ref"], "人工确认", {})
        self.rules, rule_views, retained = {}, [], Counter()
        for ordinal, entry in enumerate(self._items(verification), 1):
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
        self.payload = {"schema_version": 2, "grounding_policy": AUTHORING_GROUNDING_POLICY,
                        "rules": rule_views, "rule_origin": self.rule_origin,
                        "relations": self._read(verification.get("relations_ref"), "事项关系", []),
                        "unverified_relation_ids": verification.get("unverified_relation_ids", []),
                        "issues": [*verification.get("issues", []), *self.availability_issues],
                        "issue_resolutions": confirmation.get("issue_resolutions"),
                        "scope_decisions": plan.get("scope_decisions", [])}
        self.sources, self.read_units, self.range_roots, self.template_parts = {}, set(), {}, {}
        self.excerpts = {}
        self._sources(plan)

    def _read(self, reference, label, default):
        if reference is None:
            return deepcopy(default)
        try:
            value = self.artifacts.read_json(reference)
            if not isinstance(value, type(default)):
                raise ValueError("unexpected authoring artifact shape")
            return value
        except (OSError, ValueError, TypeError):
            self.availability_issues.append({"code": "authoring_input_unavailable",
                                            "detail": f"{label}资料无法读取，保留待补充位置。"})
            return deepcopy(default)

    def _items(self, verification):
        if verification:
            try:
                items = current_items(self.artifacts, verification)
                self.rule_origin = "verified_projection"
                return items
            except (OSError, ValueError, KeyError, TypeError):
                self.availability_issues.append({"code": "authoring_projection_unavailable",
                    "detail": "核验后的有效事项投影不可读取；以下为已有抽取事项，尚需核对补遗和适用范围。"})
        index = self._read(self.input["items_ref"], "事项台账", {})
        refs = [entry["artifact_ref"] for entry in index.get("packs", [])]
        refs.extend(self.input["extract_refs"])
        items, seen = [], set()
        for reference in refs:
            if reference["sha256"] in seen:
                continue
            seen.add(reference["sha256"])
            for item in self._read(reference, "抽取事项", {}).get("items", []):
                items.append({"item_ref": {"item_id": item["item_id"], "artifact_ref": reference}, "item": item})
        if items:
            self.rule_origin = "extraction"
            self.availability_issues.append({"code": "authoring_unverified_items",
                "detail": "使用已有抽取事项继续编制，尚未形成可读的有效事项投影；补遗及适用范围需核对。"})
        return items

    def _sources(self, plan):
        documents, indexes = [], {}
        frozen = self._read(self.input["frozen_ref"], "冻结原文", {})
        for document in frozen.get("documents", []):
            try:
                reference = document["source_index_ref"]
                indexes[reference["sha256"]] = self.repository.source_index(reference)
                documents.append(document)
            except (OSError, ValueError, KeyError, TypeError):
                self.availability_issues.append({"code": "authoring_source_unavailable",
                    "detail": "部分原文索引不可读取，相关位置保留待补充。"})
        native_units = {digest: {unit["unit_id"]: unit for unit in index.raw["units"]}
                        for digest, index in indexes.items()}
        ranges, catalog, packs = {}, [], []

        def bind(source, source_ref):
            self.range_roots[source["range_id"]] = source.get("source_range_id", source["range_id"])
            units = native_units[source_ref["sha256"]]
            # Spreadsheet rows can cover several side-by-side table grids.
            tables = source.get("table_ids") or [source.get("table_id")]
            unit_ids = [u for u in tables if u in units and units[u].get("grid")]
            native = units.get(source["unit_id"])
            if native is not None:
                self.excerpts[source["range_id"]] = [{"source_ref": {
                    "source_index_ref": deepcopy(source_ref), "unit_id": source["unit_id"]},
                    "selectors": [selector(source)]}]
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
        for descriptor in plan.get("packs", []):
            pack = self._read(descriptor["artifact_ref"], "来源分包", {})
            if not pack or pack["source_index_ref"]["sha256"] not in indexes:
                continue
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
                    self.availability_issues.append({"code": "authoring_source_unavailable",
                        "detail": "部分事项的原文位置不可读取，保留事项并提示核对来源。"})
        collect(self.payload)
        if not self.payload["rules"]:
            needed.update(self.sources)

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
        self.payload["issues"] = [*self.payload["issues"],
                                 *(i for i in self.availability_issues if i not in self.payload["issues"])]

    def validate(self, response: dict) -> dict:
        try:
            parsed = OutlineResponse.model_validate(response).model_dump()
        except ValidationError as error:
            raise ModelContractError([{"path": list(e["loc"]), "code": e["type"], "detail": e["msg"]}
                                      for e in error.errors(include_input=False)]) from None
        issues = [{**issue, "rule_refs": []} for issue in self.availability_issues]
        assigned = set()

        def references(identities, mapping, path):
            _check_reference_ids(identities, mapping, path)
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
                    excerpts = bind_source_excerpts(self, target["range_ids"], path=[*target_path, "range_ids"])
                    rows = []
                    for row_number, row in enumerate((target.get("response_table") or {}).get("rows", [])):
                        row_path = [*target_path, "response_table", "rows", row_number]
                        row_excerpts = bind_source_excerpts(self, row["range_ids"], path=[*row_path, "range_ids"])
                        row_rules = references(row["rule_ids"], self.rules, [*row_path, "rule_ids"])
                        row_id = response_row_id(target_id, row_excerpts, row_rules)
                        if not any(entry["row_id"] == row_id for entry in rows):
                            rows.append({"row_id": row_id, "source_excerpts": row_excerpts, "rule_refs": row_rules})
                        refs.extend(ref for ref in row_rules if ref not in refs)
                        assigned.update(row["rule_ids"])
                    sources = [part["source_ref"] for part in excerpts]
                    sources.extend(part["source_ref"] for row in rows for part in row["source_excerpts"])
                    sources = list({(s["source_index_ref"]["sha256"], s["unit_id"]): s for s in sources}.values())
                    templates = references(target["template_part_ids"], self.template_parts,
                                           [*target_path, "template_part_ids"])
                    assigned.update(target["rule_ids"])
                    if not (refs or sources or templates or rows):
                        problem("outline_target_unbound", "该响应位置尚无规则、来源或原格式依据，需核对其用途。", [], target_id)
                    if target["kind"] == "form" and not templates and not rows:
                        problem("outline_template_missing", "规定格式/表格尚未定位源模板，需补充来源。", refs, target_id)
                    mapped = [ref for identity in target["range_ids"] for ref in self.sources.get(identity, [])]
                    if any((s["source_index_ref"]["sha256"], s["unit_id"]) not in self.read_units for s in mapped):
                        problem("outline_source_not_read", "已定位目录来源，但模型尚未阅读正文，生成前须装载核对。", refs, target_id)
                    targets.append({"target_id": target_id, "kind": target["kind"], "purpose": target["purpose"],
                                    "rule_refs": refs, "source_refs": sources, "template_refs": templates,
                                    **({"source_excerpts": excerpts} if excerpts else {}),
                                    **({"response_table": {"rows": rows}} if rows else {}),
                                    "manual_fill": target["manual_fill"],
                                    "support_origins": target["support_origins"]})
                result.append({"chapter_id": chapter_id, "title": chapter["title"], "targets": targets,
                               "is_volume": chapter["is_volume"],
                               "children": chapters(chapter["children"], position, (*current_path, "children"))})
            return result

        organized = chapters(parsed["chapters"])
        search_intents = []
        views = {entry["rule_id"]: entry for entry in self.payload["rules"]}
        for ordinal, intent in enumerate(parsed["search_intents"]):
            _check_reference_ids(intent["for_ids"], self.rules.keys() | self.sources.keys(),
                                 ["search_intents", ordinal, "for_ids"])
            identities = list(dict.fromkeys(intent["for_ids"]))
            rule_ids = [identity for identity in identities if identity in self.rules]
            range_ids = [identity for identity in identities if identity in self.sources]
            for identity in rule_ids:
                rule = views[identity]
                semantic = rule.get("item", rule.get("effective_rule", {}))
                range_ids.extend(e["range_id"] for e in semantic.get("evidence", [])
                                 if e["range_id"] in self.sources)
            range_ids = list(dict.fromkeys(range_ids))
            refs = [ref for identity in range_ids for ref in self.sources[identity]]
            refs = list({(ref["source_index_ref"]["sha256"], ref["unit_id"]): ref for ref in refs}.values())
            search_intents.append({"query": intent["query"], "rule_refs": [deepcopy(self.rules[i]) for i in rule_ids],
                                   "source_refs": deepcopy(refs), "range_ids": range_ids})
        constraint_refs = references(parsed["constraint_rule_ids"], self.rules, ["constraint_rule_ids"])
        submission_refs = [source for group in references(parsed["constraint_range_ids"], self.sources,
                                                         ["constraint_range_ids"]) for source in group]
        submission_refs = list({(ref["source_index_ref"]["sha256"], ref["unit_id"]): ref
                                for ref in submission_refs}.values())
        assigned.update(parsed["constraint_rule_ids"])
        for ordinal, finding in enumerate(parsed["findings"]):
            refs = references(finding["rule_ids"], self.rules, ["findings", ordinal, "rule_ids"])
            problem(finding["code"], finding["detail"], refs)
        for identity in self.rules:
            if identity not in assigned:
                problem("outline_rule_unassigned", "该有效规则尚未安排响应位置，需补充大纲映射。", [self.rules[identity]])
        return AuthoringOutline.model_validate({"schema_version": 2, "authoring_input": self.input,
                                                "chapters": organized, "issues": issues,
                                                "constraint_rule_refs": constraint_refs,
                                                "submission_refs": submission_refs,
                                                "search_intents": search_intents}).model_dump(exclude_unset=True)
