"""Small persistent selectors over one immutable, cached native source index."""
from __future__ import annotations

import hashlib
import threading
from copy import deepcopy
from typing import Iterator

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.tender.schema import SourceRangeIdentity


_SELECTOR_FIELDS = (
    "range_id", "source_range_id", "parent_range_id", "start", "end", "reason",
    "scope_binding", "split_lineage", "previous_fragment_id", "next_fragment_id",
    "fragment_ordinal", "text_boundaries",
)


def selector(entry: dict) -> dict:
    return {key: deepcopy(entry[key]) for key in _SELECTOR_FIELDS if key in entry}


def fragment(entry: dict, start: int, end: int) -> dict:
    """Exact character intervals; row strings remain rooted in their native grid."""
    if not entry["start"] <= start <= end <= entry["end"]:
        raise ValueError("fragment lies outside its immutable source range")
    identity = f"{entry['source_range_id']}\0{start}\0{end}"
    value = {**entry, "range_id": "fragment-" + hashlib.sha256(identity.encode()).hexdigest()[:24],
             "start": start, "end": end,
             "text": entry["text"][start - entry["start"]:end - entry["start"]],
             "split_lineage": [*entry.get("split_lineage", []), entry["range_id"]]}
    value["text_boundaries"] = {
        "start": start == entry["start"] and entry.get("text_boundaries", {}).get("start", True),
        "end": end == entry["end"] and entry.get("text_boundaries", {}).get("end", True),
    }
    return value


def _locator(value):
    if not isinstance(value, dict):
        return value
    # Native cells are retained once in canonical raw, not once per row/request.
    return {key: _locator(item) if isinstance(item, dict) else item
            for key, item in value.items() if key not in {"cells", "cell_refs", "rendered_spans"}}


class SourceIndex:
    def __init__(self, document: dict, raw: dict):
        from docreader.tender.evidence.packing import source_ranges
        self.document = document
        self.raw = raw
        self.ranges = source_ranges(document, raw)
        identity_fields = SourceRangeIdentity.model_fields
        for entry in self.ranges:
            SourceRangeIdentity.model_validate({key: value for key, value in entry.items() if key in identity_fields})
        self.by_id = {entry["range_id"]: entry for entry in self.ranges}
        if len(self.by_id) != len(self.ranges):
            raise ArtifactIntegrityError("duplicate native source range identity")

    def descriptor(self, raw_ref: dict) -> dict:
        return {"schema_version": 1, "document": {
            key: self.document[key] for key in ("document_id", "document_version", "original_ref")},
            "raw_ref": raw_ref, "ranges": [
                {key: entry[key] for key in ("range_id", "unit_id", "start", "end", "text_basis", "row_start", "row_end") if key in entry}
                for entry in self.ranges]}

    def materialize(self, reference: dict) -> dict:
        source_id = reference.get("source_range_id", reference.get("range_id"))
        try:
            base = self.by_id[source_id]
        except KeyError:
            raise ArtifactIntegrityError("pack refers to an unknown native source range") from None
        start, end = reference["start"], reference["end"]
        if not isinstance(start, int) or not isinstance(end, int) or not base["start"] <= start <= end <= base["end"]:
            raise ArtifactIntegrityError("pack character range exceeds its native source")
        value = {key: item for key, item in base.items() if key not in {"cells", "cell_intervals", "paragraph_breaks"}}
        value.update(deepcopy(reference))
        value["text"] = base["text"][start - base["start"]:end - base["start"]]
        value["locator"] = _locator(base.get("locator"))
        value["physical_locator"] = _locator(base.get("physical_locator"))
        if "cell_intervals" in base:
            # Coordinates/merge spans and offsets preserve blank-field semantics
            # without duplicating the row's text in every cell.
            blank_template = base.get("empty") and base.get("non_requirement_reason") == "empty_table_row"
            value["cells"] = [{**{key: item for key, item in cell.items() if key not in {"text", "start", "end"}},
                               "start": 0 if blank_template else max(start, cell["start"]),
                               "end": 0 if blank_template else min(end, cell["end"])}
                              for cell in base["cell_intervals"]
                              if blank_template or (cell["start"] < end and start < cell["end"])
                              or (cell["start"] == cell["end"] and start <= cell["start"] <= end)]
        return value


class EvidenceRepository:
    def __init__(self, artifacts: ArtifactStore):
        self.artifacts = artifacts
        self._sources: dict[str, SourceIndex] = {}
        self._source_lock = threading.Lock()

    def source_index(self, reference: dict) -> SourceIndex:
        key = reference["sha256"]
        with self._source_lock:
            if key not in self._sources:
                descriptor = self.artifacts.read_json(reference)
                raw = self.artifacts.read_json(descriptor["raw_ref"])
                source = SourceIndex(descriptor["document"], raw)
                if source.descriptor(descriptor["raw_ref"])["ranges"] != descriptor["ranges"]:
                    raise ArtifactIntegrityError("source index does not match frozen native content")
                self._sources[key] = source
        return self._sources[key]

    def load_pack(self, pack_ref: dict) -> dict:
        descriptor = self.artifacts.read_json(pack_ref.get("artifact_ref", pack_ref))
        if descriptor.get("schema_version") != 2:
            raise ArtifactIntegrityError("unsupported unpublished pack schema")
        source = self.source_index(descriptor["source_index_ref"])
        return {**descriptor,
                "body": [source.materialize(item) for item in descriptor["body"]],
                "supporting": [source.materialize(item) for item in descriptor["supporting"]]}

    def iter_packs(self, index_ref: dict) -> Iterator[dict]:
        for entry in self.artifacts.read_json(index_ref)["packs"]:
            yield self.load_pack(entry)
