"""Pure associate/joint transport planning; semantic dependency groups stay intact."""

from copy import deepcopy
from fractions import Fraction

from docreader.tender.schema import AnalysisWorkUnit


MAX_WORK_UNITS = 4
_POOLS = {"items": "item_id", "relations": "relation_id", "decisions": "decision_id",
          "dispositions": "range_id", "issues": "issue_id", "candidates": "candidate_id",
          "scope_decisions": "range_id"}
_SOURCE_POOLS = {"body": "range_id", "supporting": "range_id", "sections": "section_id",
                 "pending_scope_context": "range_id", "scope_bindings": "range_id",
                 "source_aliases": "range_id", "blank_table_templates": None}


class _Conflict(ValueError):
    """The same source/object identity carries different content; do not co-batch."""


def _merge(entries, additions, identity):
    known = {identity(entry): entry for entry in entries}
    for entry in additions:
        key = identity(entry)
        if key in known:
            if known[key] != entry:
                raise _Conflict("Shared identity has different complete content")
        else:
            copied = deepcopy(entry)
            entries.append(copied)
            known[key] = copied


def _template_identity(template):
    return template["table_id"], tuple(r for rows in template["rows"] for r in rows["range_ids"])


def _merge_views(views, additions):
    documents = {view["document_id"]: view for view in views}
    for view in additions:
        document = view["document_id"]
        if document not in documents:
            copied = deepcopy(view)
            views.append(copied)
            documents[document] = copied
            continue
        shared = documents[document]
        metadata = {key: value for key, value in view.items() if key not in _SOURCE_POOLS}
        if metadata != {key: value for key, value in shared.items() if key not in _SOURCE_POOLS}:
            raise _Conflict("Shared document view metadata differs")
        for field, identity in _SOURCE_POOLS.items():
            if field in view:
                _merge(shared.setdefault(field, []), view[field],
                       _template_identity if identity is None else lambda entry: entry[identity])
        # Overlapping compressed intervals cannot give one original row two positions/structures.
        rows = {}
        for template in shared.get("blank_table_templates", []):
            metadata = {key: value for key, value in template.items() if key != "rows"}
            for interval in template["rows"]:
                for offset, identity in enumerate(interval["range_ids"]):
                    value = metadata, interval["row_start"] + offset
                    if identity in rows and rows[identity] != value:
                        raise _Conflict("Shared template row has conflicting structure")
                    rows[identity] = value


def _source_ids(payload):
    identities = set()

    def collect(value, document):
        if isinstance(value, dict):
            for key, entry in value.items():
                if key == "range_id" and isinstance(entry, str):
                    identities.add((document, entry))
                elif key == "range_ids" and isinstance(entry, list):
                    identities.update((document, identity) for identity in entry)
                else:
                    collect(entry, document)
        elif isinstance(value, list):
            for entry in value:
                collect(entry, document)

    for view in payload["source_views"]:
        collect(view, view["document_id"])
    return identities


def _ordered(payloads):
    """One O(G²) identity-only ordering; no repeated search during admission."""
    remaining = {payload["work_unit_id"]: payload for payload in payloads}
    if len(remaining) != len(payloads):
        raise ValueError("Logical work unit identities must be unique")
    sources = {identity: _source_ids(payload) for identity, payload in remaining.items()}
    previous = None
    while remaining:
        def key(identity):
            if previous is None:
                return -len(sources[identity]), identity
            overlap = len(sources[previous] & sources[identity])
            union = len(sources[previous] | sources[identity])
            return -Fraction(overlap, union or 1), -overlap, identity

        identity = min(remaining, key=key)
        yield remaining.pop(identity)
        previous = identity


def _source_position(entry, source_order):
    if "segments" in entry:
        position = min(source_order(segment["range_id"]) for segment in entry["segments"])
    else:
        position = source_order(entry.get("source_range_id") or entry["range_id"])
    return position, entry.get("start", 0), entry.get("end", 0), entry["range_id"]


def _combine(payloads, source_order=None, document_order=None):
    first = payloads[0]
    common = {key: value for key, value in first.items()
              if key not in {*_POOLS, "source_views", "work_unit_id"}}
    batch = {**deepcopy(common), "source_views": [], "work_units": [], "target_work_unit_ids": []}
    for payload in payloads:
        if common != {key: value for key, value in payload.items()
                      if key not in {*_POOLS, "source_views", "work_unit_id"}}:
            raise _Conflict("Logical requests use different operations or shared metadata")
        _merge_views(batch["source_views"], payload["source_views"])
        for field, identity in _POOLS.items():
            if field in payload:
                _merge(batch.setdefault(field, []), payload[field], lambda entry: entry[identity])
        member = {"work_unit_id": payload["work_unit_id"],
                  "candidate_ids": [c["candidate_id"] for c in payload["candidates"]],
                  "issue_ids": [p["issue_id"] for p in payload["issues"]]}
        if payload["operation"] == "review":
            member["decision_ids"] = [d["decision_id"] for d in payload["decisions"]]
        batch["work_units"].append(AnalysisWorkUnit.model_validate(member).model_dump(exclude_unset=True))
        batch["target_work_unit_ids"].append(payload["work_unit_id"])
    for view in batch["source_views"]:
        body = {entry["range_id"]: entry for entry in view["body"]}
        supporting = []
        for entry in view["supporting"]:
            original = body.get(entry["range_id"])
            if original is not None:
                # Selection explanations belong to their role; never drop them as duplicate prose.
                if ({key: value for key, value in original.items() if key != "reason"}
                        != {key: value for key, value in entry.items() if key != "reason"}):
                    raise _Conflict("One source identity has inconsistent literal content or location")
                if original == entry:
                    continue
            supporting.append(entry)
        view["supporting"] = supporting
    if document_order is not None:
        batch["source_views"].sort(key=lambda view: document_order(view["document_id"]))
    if source_order is not None:
        for view in batch["source_views"]:
            for field, identity in _SOURCE_POOLS.items():
                if identity == "range_id" and field in view:
                    view[field].sort(key=lambda entry: _source_position(entry, source_order))
            view.get("blank_table_templates", []).sort(key=lambda template: min(
                source_order(identity) for row in template["rows"] for identity in row["range_ids"]))
    return batch


def make_batches(payloads, budget, *, source_order=None, document_order=None):
    """Keep complete conflicting/oversized groups separate for their existing exits."""
    batches, current = [], []
    for payload in _ordered(payloads):
        try:
            trial = _combine([*current, payload], source_order, document_order)
        except _Conflict:
            trial = None
        if current and (trial is None or len(current) == MAX_WORK_UNITS
                        or budget.measure("verify", trial)["fits"] is False):
            batches.append(_combine(current, source_order, document_order))
            current = []
        current.append(payload)
    if current:
        batches.append(_combine(current, source_order, document_order))
    return batches


def member_payload(batch, work_unit_id):
    """Validate one target against the complete basis actually visible to the model."""
    member = next(unit for unit in batch["work_units"] if unit["work_unit_id"] == work_unit_id)
    payload = {key: deepcopy(value) for key, value in batch.items()
               if key not in {"work_units", "target_work_unit_ids"}}
    payload["work_unit_id"] = work_unit_id
    payload["candidates"] = [c for c in payload["candidates"] if c["candidate_id"] in member["candidate_ids"]]
    if batch["operation"] == "review":
        payload["decisions"] = [d for d in payload["decisions"] if d["decision_id"] in member["decision_ids"]]
    return payload


def split_targets(batch, budget):
    """Shrink answering targets only; the frozen basis and all members stay visible."""
    batches, current = [], []
    for identity in batch["target_work_unit_ids"]:
        trial = {**batch, "target_work_unit_ids": [*current, identity]}
        if current and (len(current) == MAX_WORK_UNITS or budget.measure("verify", trial)["fits"] is False):
            batches.append(deepcopy({**batch, "target_work_unit_ids": current}))
            current = []
        current.append(identity)
    if current:
        batches.append(deepcopy({**batch, "target_work_unit_ids": current}))
    return batches
