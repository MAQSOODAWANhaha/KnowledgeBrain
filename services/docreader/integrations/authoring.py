"""Read an existing confirmed analysis without copying or changing its facts."""

from __future__ import annotations

import asyncio
import hashlib
import json

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import InvalidConfirmation, Store
from docreader.tender.schema import AuthoringInput, RunIdentity, validate_issue_resolutions


_ROOTS = {"freeze": "frozen_ref", "pack-plan": "pack_plan_ref", "extract": "items_ref",
          "verify": "verification_ref", "confirm": "confirmation_ref"}


async def read_authoring_input(
    store: Store, artifacts: ArtifactStore, *, run_id: str, source_version: int,
    domain_revision: int, checklist_digest: str,
) -> dict:
    """A version-bound snapshot, not a lease authorizing future authoring work."""
    try:
        identity = RunIdentity.model_validate({"run_id": run_id, "source_version": source_version,
                                               "domain_revision": domain_revision}).model_dump()
    except ValueError as exc:
        raise InvalidConfirmation("authoring requires a valid analysis identity") from exc
    row = await store.get_run(run_id)
    if (row["status"] != "analysis-ready" or row["cancel_requested"]
            or any(row[key] != value for key, value in identity.items())
            or row["checklist_digest"] != checklist_digest):
        raise InvalidConfirmation("authoring requires the current confirmed analysis revision and digest")

    refs = {}
    for stage, field in _ROOTS.items():
        revision = 0 if stage == "freeze" else domain_revision
        receipts = [entry for entry in row["artifacts"]
                    if entry["stage"] == stage and entry["scope"] == ""
                    and entry["run_id"] == run_id and entry["source_version"] == source_version
                    and entry["domain_revision"] == revision]
        if len(receipts) != 1:
            raise InvalidConfirmation(f"authoring requires one current root receipt for {stage}")
        refs[field] = receipts[0]["artifact_ref"]
    try:
        value = AuthoringInput.model_validate({"schema_version": 2, **identity,
                                               "checklist_digest": checklist_digest, **refs}).model_dump()
    except ValueError as exc:
        raise InvalidConfirmation("authoring input references do not match the shared contract") from exc
    await asyncio.to_thread(_check_artifacts, artifacts, row, value)
    return value


def _check_artifacts(artifacts: ArtifactStore, row: dict, value: dict) -> None:
    roots = {stage: artifacts.read_json(value[field]) for stage, field in _ROOTS.items()}
    for stage, root in roots.items():
        if not isinstance(root, dict) or (stage != "confirm" and (
                type(root.get("schema_version")) is not int or root["schema_version"] != 2)):
            raise InvalidConfirmation(f"authoring requires v2 analysis artifacts: {stage}")
    identity = {key: value[key] for key in RunIdentity.model_fields}
    plan, verification, confirmed = roots["pack-plan"], roots["verify"], roots["confirm"]
    if plan.get("frozen_ref") != value["frozen_ref"] or plan.get("run_identity") != identity:
        raise InvalidConfirmation("pack plan does not refer to this analysis source and revision")
    if verification.get("items_ref") != value["items_ref"] or verification.get("hard_blocking") is not False:
        raise InvalidConfirmation("authoring requires the verified current items without hard blocking")

    # Preserve the exact existing verify-node serialization, including spaces.
    checklist = {key: value[key] for key in ("schema_version", "source_version", "domain_revision",
                                            "frozen_ref", "pack_plan_ref", "items_ref", "verification_ref")}
    digest = hashlib.sha256(json.dumps(checklist, sort_keys=True).encode()).hexdigest()
    if digest != value["checklist_digest"]:
        raise InvalidConfirmation("analysis artifact references do not match the confirmed checklist digest")
    if any(confirmed.get(key) != value[key] for key in ("source_version", "domain_revision", "checklist_digest")):
        raise InvalidConfirmation("confirmation artifact does not match the current checklist")
    try:
        resolutions = validate_issue_resolutions(verification["human_issue_ids"], confirmed.get("issue_resolutions"))
    except ValueError as exc:
        raise InvalidConfirmation("confirmation must resolve exactly the pending semantic issues") from exc
    if resolutions != confirmed.get("issue_resolutions"):
        raise InvalidConfirmation("confirmation resolutions must retain their persisted canonical form")
    if not any(all(entry.get(key) == value[key] for key in (*RunIdentity.model_fields, "checklist_digest"))
               and entry.get("issue_resolutions") == resolutions for entry in row["confirmations"]):
        raise InvalidConfirmation("confirmation artifact does not match the persisted human confirmation")
