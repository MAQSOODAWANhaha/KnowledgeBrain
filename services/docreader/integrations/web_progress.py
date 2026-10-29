"""Read-only progress from immutable work receipts and recorded request states."""

from collections import Counter

from docreader.integrations.store import SOURCE_STAGES
from docreader.tender.status import AUTHORING_MODEL_TASKS


_IDENTITY = ("run_id", "source_version", "domain_revision")
_AUTHORING_TASKS = AUTHORING_MODEL_TASKS
_ANALYSIS_PHASES = {
    "register": "register",
    "freeze-document": "freeze",
    "freeze": "freeze",
    "ocr": "ocr",
    "pack-plan": "pack-plan",
    "extract-pack": "extract",
    "extract": "extract",
    **{
        f"analysis-{phase}": f"analysis-{phase}"
        for phase in ("review", "locate", "associate", "joint")
    },
    "verify": "verify",
    "confirm": "confirm",
}
_ACCEPTED_ANALYSIS = {
    "ocr",
    "extract-pack",
    "analysis-review",
    "analysis-locate",
    "analysis-associate",
    "analysis-joint",
}
_AUTHORING_PHASES = {
    "document-materials": "materials",
    "knowledge-snapshot": "knowledge",
    "document-outline": "outline",
}
_PHASE_ORDER = (
    "register",
    "freeze",
    "ocr",
    "pack-plan",
    "extract",
    "analysis-review",
    "analysis-locate",
    "analysis-associate",
    "analysis-joint",
    "verify",
    "confirm",
    "materials",
    "knowledge",
    *_AUTHORING_TASKS,
    "result",
    "other",
)


def _request_phase(row):
    task = row.get("task")
    if task == "verify":
        metadata = row.get("metadata", {})
        operation = metadata.get("operation")
        if operation == "review" and metadata.get("work_unit_id", "").startswith(
            "joint-"
        ):
            return "analysis-joint"
        return (
            f"analysis-{operation}"
            if operation in {"review", "locate", "associate", "joint"}
            else "verify"
        )
    return task if task in {"ocr", "extract", *_AUTHORING_TASKS} else "other"


def _when(value):
    return value.isoformat() if hasattr(value, "isoformat") else str(value or "")


def _unique(receipts):
    seen = set()
    for row in receipts:
        identity = row["stage"], row["scope"]
        if identity not in seen:
            seen.add(identity)
            yield row


def _project(identity, state, owner, requests, accepted, last_phase, rejected):
    # A retry is another attempt of one logical request, not another completed unit.
    latest = {}
    for index, row in sorted(
        enumerate(requests),
        key=lambda entry: (_when(entry[1].get("created_at")), entry[0]),
    ):
        metadata = row.get("metadata", {})
        digest = (
            metadata.get("logical_request_digest")
            or metadata.get("request_digest")
            or row.get("request_id")
            or index
        )
        latest[row["task"], digest] = index, row
    rows = sorted(
        latest.values(), key=lambda entry: (_when(entry[1].get("created_at")), entry[0])
    )
    states = dict.fromkeys(("prepared", "sent", "uncertain", "failed", "received"), 0)
    active, current = {}, None
    live = (
        state == "running"
        and owner.get("lease_live")
        and not owner.get("cancel_requested")
    )
    for _, row in rows:
        status = "received" if row["status"] == "succeeded" else row["status"]
        if status in states:
            states[status] += 1
        if (
            live
            and row.get("epoch") == owner.get("epoch")
            and status in {"prepared", "sent"}
        ):
            current = _request_phase(row)
            active.setdefault(current, {"phase": current, "prepared": 0, "sent": 0})[
                status
            ] += 1
    counts = Counter(accepted)
    return {
        "identity": identity,
        "state": state,
        "current_phase": current,
        "active_tasks": [active[phase] for phase in _PHASE_ORDER if phase in active],
        "latest_request_phase": _request_phase(rows[-1][1]) if rows else None,
        "last_completed_phase": last_phase,
        "accepted_units": sum(counts.values()),
        "accepted_by_phase": [
            {"phase": phase, "count": counts[phase]}
            for phase in _PHASE_ORDER
            if counts[phase]
        ],
        "request_states": states,
        "rejected_responses": rejected,
        "total_units": None,
    }


def analysis_progress(run):
    identity = {key: run[key] for key in _IDENTITY}

    def current(row, *, source_receipt=False):
        return all(
            row.get(key)
            == (0 if key == "domain_revision" and source_receipt else value)
            for key, value in identity.items()
        )

    receipts = list(
        _unique(
            row
            for row in run["artifacts"]
            if current(row, source_receipt=row["stage"] in SOURCE_STAGES)
        )
    )
    accepted = [
        _ANALYSIS_PHASES[row["stage"]]
        for row in receipts
        if row["stage"] in _ACCEPTED_ANALYSIS
    ]
    milestones = [
        row
        for row in receipts
        if row["stage"] in _ANALYSIS_PHASES and row.get("committed_at") is not None
    ]
    last = max(milestones, key=lambda row: _when(row["committed_at"]), default=None)
    return _project(
        identity,
        run["status"],
        run,
        [row for row in run["usage"] if current(row)],
        accepted,
        _ANALYSIS_PHASES[last["stage"]] if last else None,
        sum(
            row["stage"] == "model-rejection"
            or (
                row["stage"] == "model-group-outcome"
                and row.get("metadata", {}).get("rejected_response") is True
            )
            for row in receipts
        ),
    )


def authoring_progress(job, execution, *, status=None):
    owner = execution or {}
    snapshot = (
        owner.get("authoring_input")
        or job["payload"].get("identity")
        or job["payload"].get("snapshot")
        or {}
    )
    identity = {key: snapshot[key] for key in _IDENTITY if key in snapshot}
    identity["execution_id"] = job["execution_id"]
    state = status if status is not None else owner.get("status", job["status"])
    receipts = list(_unique(owner.get("receipts", [])))
    accepted, last = [], None
    # Authoring receipts have no timestamp; their durable append order is the observation order.
    for row in receipts:
        if row["stage"] == "document-model-response":
            phase = row["scope"].partition(":")[0]
            if phase in _AUTHORING_TASKS:
                accepted.append(phase)
                last = phase
        elif row["stage"] in _AUTHORING_PHASES:
            last = _AUTHORING_PHASES[row["stage"]]
    if state == "succeeded" and owner.get("result_ref"):
        last = "result"
    return _project(
        identity,
        state,
        owner,
        owner.get("requests", []),
        accepted,
        last,
        sum(row["stage"] == "model-rejection" for row in receipts),
    )
