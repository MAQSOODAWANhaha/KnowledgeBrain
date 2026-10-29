"""Fix the actually available analysis input without changing its analysis state."""

from __future__ import annotations

import asyncio
import hashlib
import json

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Cancelled, InvalidConfirmation, Store
from docreader.tender.schema import ArtifactRef, AuthoringInput, RunIdentity, validate_issue_resolutions
from docreader.tender.status import RunStatus


_ROOTS = {"freeze": "frozen_ref", "pack-plan": "pack_plan_ref", "extract": "items_ref",
          "verify": "verification_ref", "confirm": "confirmation_ref"}


async def assert_authoring_active(store: Store, authoring_input: dict) -> None:
    """A newer analysis revision does not invalidate work on a fixed snapshot."""
    row = await store.get_run(authoring_input["run_id"])
    if row["cancel_requested"] or row["status"] == RunStatus.CANCELLED:
        raise Cancelled("parent analysis cancellation requested")


async def read_authoring_input(
    store: Store, artifacts: ArtifactStore, *, run_id: str, source_version: int,
    domain_revision: int, checklist_digest: str | None = None,
) -> dict:
    """Capture current readable receipts; absent analysis work stays explicitly absent."""
    try:
        identity = RunIdentity.model_validate({"run_id": run_id, "source_version": source_version,
                                               "domain_revision": domain_revision}).model_dump()
    except ValueError as exc:
        raise InvalidConfirmation("authoring requires a valid analysis identity") from exc
    row = await store.get_run(run_id)
    if row["cancel_requested"] or row["status"] == RunStatus.CANCELLED:
        raise Cancelled("parent analysis cancellation requested")
    if (any(row[key] != value for key, value in identity.items())
            or checklist_digest is not None and row["checklist_digest"] != checklist_digest):
        raise InvalidConfirmation("authoring requires the requested current analysis identity")
    return await asyncio.to_thread(_available_input, artifacts, row, identity)


def _available_input(artifacts: ArtifactStore, row: dict, identity: dict) -> dict:
    value = {"schema_version": 2, **identity, "checklist_digest": row.get("checklist_digest"),
             "extract_refs": [], "issues": []}
    roots = {}

    def issue(code, detail):
        value["issues"].append({"code": code, "detail": detail})

    def current(entry, stage):
        revision = 0 if stage == "freeze" else identity["domain_revision"]
        return (entry.get("stage") == stage and entry.get("run_id") == identity["run_id"]
                and entry.get("source_version") == identity["source_version"]
                and entry.get("domain_revision") == revision)

    def load(reference, stage):
        try:
            reference = ArtifactRef.model_validate(reference).model_dump()
            root = artifacts.read_json(reference)
            if not isinstance(root, dict) or (stage != "confirm" and (
                    type(root.get("schema_version")) is not int or root["schema_version"] != 2)):
                raise ValueError("unsupported analysis artifact")
            return reference, root
        except (OSError, ValueError, TypeError):
            issue("authoring_input_unavailable", f"{stage} 产物不可读取或格式不适用；保留待补充状态。")
            return None, None

    for stage, field in _ROOTS.items():
        entries = [entry for entry in row["artifacts"] if current(entry, stage) and entry.get("scope") == ""]
        value[field] = None
        if len(entries) != 1:
            issue("authoring_input_missing", f"{stage} 尚无唯一的当前根产物；继续使用已有资料。")
            continue
        reference, root = load(entries[0]["artifact_ref"], stage)
        if root is None:
            continue
        if (stage == "freeze" and not isinstance(root.get("documents"), list)
                or stage == "extract" and not isinstance(root.get("packs"), list)
                or stage == "pack-plan" and (root.get("frozen_ref") != value["frozen_ref"]
                    or root.get("run_identity") != identity or not isinstance(root.get("packs"), list))
                or stage == "verify" and (not value["items_ref"] or root.get("items_ref") != value["items_ref"])):
            issue("authoring_input_inconsistent", f"{stage} 引用不属于本次可用输入；不作为有效资料使用。")
            continue
        roots[stage], value[field] = root, reference

    if value["items_ref"] is None:
        seen = set()
        for entry in row["artifacts"]:
            if not current(entry, "extract-pack"):
                continue
            reference, root = load(entry["artifact_ref"], "extract-pack")
            if root is not None and isinstance(root.get("items"), list) and reference["sha256"] not in seen:
                seen.add(reference["sha256"])
                value["extract_refs"].append(reference)
        value["extract_refs"].sort(key=lambda ref: ref["sha256"])

    # A checklist digest continues to mean exactly the existing verify serialization.
    checklist_fields = ("schema_version", "source_version", "domain_revision", "frozen_ref",
                        "pack_plan_ref", "items_ref", "verification_ref")
    complete = all(value[field] is not None for field in checklist_fields)
    digest = (hashlib.sha256(json.dumps({key: value[key] for key in checklist_fields},
                                      sort_keys=True).encode()).hexdigest() if complete else None)
    if digest != value["checklist_digest"]:
        value["checklist_digest"] = None
        issue("authoring_checklist_unavailable", "现有产物未形成一致的核验清单；不得视为已确认分析。")

    confirmed, verification = roots.get("confirm"), roots.get("verify")
    if confirmed is not None:
        try:
            if (digest is None or digest != value["checklist_digest"] or verification is None
                    or any(confirmed.get(key) != value[key] for key in (
                        "source_version", "domain_revision", "checklist_digest"))):
                raise ValueError("confirmation does not identify this snapshot")
            resolutions = validate_issue_resolutions(verification["human_issue_ids"], confirmed.get("issue_resolutions"))
            if (resolutions != confirmed.get("issue_resolutions") or not any(
                    all(entry.get(key) == value[key] for key in (*RunIdentity.model_fields, "checklist_digest"))
                    and entry.get("issue_resolutions") == resolutions for entry in row["confirmations"])):
                raise ValueError("confirmation is not persisted")
        except (ValueError, KeyError, TypeError):
            value["confirmation_ref"] = None
            issue("authoring_confirmation_unavailable", "人工确认未与当前资料一致匹配；继续生成并保留未确认状态。")
    if verification and (verification.get("blocking") or verification.get("hard_blocking")):
        issue("authoring_analysis_issues", "分析仍有待核对问题；原问题随输入保留，不中断编制。")
    return AuthoringInput.model_validate(value).model_dump()
