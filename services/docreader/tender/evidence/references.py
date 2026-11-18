"""Small persistent selectors over one immutable, cached native source index."""
from __future__ import annotations

import hashlib
import math
import threading
from copy import deepcopy
from pathlib import Path
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


def literal_source_profile():
    """Version the tender producer independently of knowledge Markdown parsing."""
    from docreader.tender.evidence.pdf_literal import CONTRACT

    root = Path(__file__).parent
    return {'contract': CONTRACT, 'sources': {
        name: hashlib.sha256((root / name).read_bytes()).hexdigest()
        for name in ('native.py', 'freeze.py', 'packing.py', 'pdf_literal.py', 'references.py')}}


def validate_literal_source(raw, literal, document=None):
    """Revalidate every projected glyph against its immutable original snapshot."""
    from docreader.tender.evidence.pdf_literal import CONTRACT

    def require(condition, detail):
        if not condition:
            raise ArtifactIntegrityError('invalid PDF literal source: ' + detail)

    try:
        contract = raw['source_contract']
        require(literal['contract'] == contract['literal_contract'] == CONTRACT, 'contract')
        require(literal['source_sha256'] == raw['original_ref']['sha256']
                == contract['document_revision'], 'original identity')
        if document is not None:
            require(all(document[key] == raw[key] for key in
                        ('document_id', 'document_version', 'original_ref')), 'document identity')
        require(literal['coordinate_system'] == 'pdf_bottom_left'
                and literal['text_order'] == 'pdfium_character_stream', 'coordinate or text basis')
        pages = literal['pages']
        require(type(literal['page_count']) is int and literal['page_count'] == len(pages), 'page count')
        glyphs, eligible_unreadable = {}, set()
        for page_number, page in enumerate(pages):
            chars = page['characters']
            require(type(page['page_ordinal']) is int and page['page_ordinal'] == page_number, 'page identity')
            require(type(page['char_count']) is int and page['char_count'] == len(chars), 'character count')
            require(all(math.isfinite(page[key]) and page[key] > 0 for key in ('width', 'height')), 'page geometry')
            unreadable = {item['char_index'] for item in page['issues'] if item['code'] in {
                'pdf_literal_character_text_empty', 'pdf_literal_character_text_unreadable'}}
            for index, char in enumerate(chars):
                require(type(char['char_index']) is int and char['char_index'] == index, 'character identity')
                require(isinstance(char['text'], str), 'character text')
                char['text'].encode('utf-8', errors='strict')
                box = char['bbox']
                require(box is None or len(box) == 4 and all(math.isfinite(value) for value in box), 'character geometry')
                glyphs[page_number, index] = char
                if not char['text'] and index in unreadable:
                    eligible_unreadable.add((page_number, index))
            require(page['text_stream'] == ''.join(char['text'] for char in chars), 'text stream')
        units = {unit['unit_id']: unit for unit in raw['units']}
        identities = {unit['unit_id']: unit for unit in contract['units']}
        require(len(units) == len(raw['units']) and len(identities) == len(contract['units'])
                and units.keys() == identities.keys(), 'unit identities')
        mappings = raw['literal_units']
        require(set(mappings) == {key for key, unit in units.items()
                                 if unit.get('text_basis') == 'pdf_literal'}, 'unit mappings')
        covered = set()
        for unit_id, mapping in mappings.items():
            unit, identity = units[unit_id], identities[unit_id]
            page_number, indices = mapping['page_ordinal'], mapping['char_indices']
            require(type(page_number) is int and 0 <= page_number < len(pages), 'mapping page')
            require(mapping['text_order'] == 'pdfium_character_stream' and indices
                    and all(type(index) is int for index in indices)
                    and all(first < second for first, second in zip(indices, indices[1:])), 'mapping order')
            points = [(page_number, index) for index in indices]
            require(all(point in glyphs and point not in covered for point in points), 'duplicate or missing glyph')
            chars = [glyphs[point] for point in points]
            text = ''.join(char['text'] for char in chars)
            require(unit['kind'] == identity['kind'] == 'section' and not unit.get('grid')
                    and text == unit['text'] and identity['text_sha256'] == hashlib.sha256(text.encode()).hexdigest(),
                    'unit text')
            location = {'locator_kind': 'page', 'page_ordinal': page_number}
            boxes = [char['bbox'] for char in chars if char['bbox'] is not None]
            if boxes:
                location.update(left=min(box[0] for box in boxes), bottom=min(box[1] for box in boxes),
                                right=max(box[2] for box in boxes), top=max(box[3] for box in boxes))
            require(unit['locator'] == unit['physical_locator'] == identity['physical_locator'] == location
                    and unit['physical_path'] == identity['physical_path'], 'unit location')
            covered.update(points)
        dispositions = set()
        for item in raw['literal_projection_dispositions']:
            point = (item['page_ordinal'], item['char_index'])
            require(all(type(value) is int for value in point) and point in glyphs, 'disposition identity')
            char, reason = glyphs[point], item['reason']
            if reason == 'native_grid_candidate':
                # A bounding-box candidate is not proof that the grid carries
                # this glyph. It does not contribute to source coverage.
                require(all(identity in units and units[identity].get('grid') for identity in item['unit_ids']),
                        'grid candidate')
                continue
            require(reason == 'physical_line_separator' and (
                        char['text'] in ('\r', '\n', '\r\n') or char['pdfium_unicode'] in (10, 13))
                    or reason == 'unreadable_glyph' and point in eligible_unreadable, 'unsupported glyph exclusion')
            require(point not in dispositions and point not in covered, 'duplicate disposition')
            dispositions.add(point)
        require(covered | dispositions == glyphs.keys(), 'unaccounted original glyphs')
    except (KeyError, TypeError, ValueError, IndexError) as error:
        if isinstance(error, ArtifactIntegrityError):
            raise
        raise ArtifactIntegrityError('invalid PDF literal source structure') from error


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
                if ('literal_source_ref' in raw or 'literal_units' in raw
                        or raw.get('source_contract', {}).get('literal_contract')):
                    if not raw.get('literal_source_ref'):
                        raise ArtifactIntegrityError('PDF literal source reference is missing')
                    validate_literal_source(raw, self.artifacts.read_json(raw['literal_source_ref']),
                                            descriptor['document'])
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
