"""Confirmed analysis references remain a read-only, version-bound authoring input."""

from copy import deepcopy
import hashlib
import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.store import InvalidConfirmation
from docreader.tender.checks.analysis import current_items
from docreader.tender.evidence.references import EvidenceRepository, SourceIndex, fragment, selector
from docreader.tender.schema import AuthoringInput

from test_structural_packing import grid, source


IDENTITY = {"run_id": "authoring-test", "source_version": 2, "domain_revision": 3}
STAGES = {"freeze": "frozen_ref", "pack-plan": "pack_plan_ref", "extract": "items_ref",
          "verify": "verification_ref", "confirm": "confirmation_ref"}


def snapshot(artifacts, *, roots=None, resolutions=None):
    roots = roots or {
        "freeze": {"schema_version": 2, "documents": []},
        "extract": {"schema_version": 2, "packs": []},
    }
    frozen_ref = artifacts.put_json(roots["freeze"])
    items_ref = artifacts.put_json(roots["extract"])
    roots.setdefault("pack-plan", {"schema_version": 2, "frozen_ref": frozen_ref,
                                    "run_identity": IDENTITY, "packs": []})
    roots.setdefault("verify", {"schema_version": 2, "items_ref": items_ref, "hard_blocking": False,
                               "blocking": False, "human_issue_ids": []})
    refs = {STAGES[stage]: artifacts.put_json(value) for stage, value in roots.items()}
    checklist = {"schema_version": 2, "source_version": IDENTITY["source_version"],
                 "domain_revision": IDENTITY["domain_revision"], **refs}
    digest = hashlib.sha256(json.dumps(checklist, sort_keys=True).encode()).hexdigest()
    confirmed = {"source_version": IDENTITY["source_version"], "domain_revision": IDENTITY["domain_revision"],
                 "checklist_digest": digest, **({"issue_resolutions": resolutions} if resolutions else {})}
    roots["confirm"] = confirmed
    refs["confirmation_ref"] = artifacts.put_json(confirmed)
    value = {"schema_version": 2, **IDENTITY, "checklist_digest": digest, **refs}
    row = {**IDENTITY, "status": "analysis-ready", "cancel_requested": False, "checklist_digest": digest,
           "revision": {"execution_spec": {"normalization_version": 1}},
           "artifacts": [{**IDENTITY, "domain_revision": 0 if stage == "freeze" else IDENTITY["domain_revision"],
                          "stage": stage, "scope": "", "artifact_ref": refs[field]}
                         for stage, field in STAGES.items()],
           "confirmations": [{**IDENTITY, **confirmed, "issue_resolutions": resolutions}],
           "history": {"artifacts": []}}
    return SimpleNamespace(artifacts=artifacts, roots=roots, row=row, expected=value,
                           store=SimpleNamespace(get_run=AsyncMock(return_value=row)))


@pytest.fixture
def confirmed(tmp_path):
    return snapshot(ArtifactStore(tmp_path / "artifacts"))


async def read(data, **expected):
    return await read_authoring_input(data.store, data.artifacts, **{
        **IDENTITY, "checklist_digest": data.expected["checklist_digest"], **expected})


async def test_returns_only_shared_contract_without_writes_or_protocol_upgrade(confirmed):
    before = {p: p.read_bytes() for p in confirmed.artifacts.root.rglob("*") if p.is_file()}
    row = deepcopy(confirmed.row)
    assert await read(confirmed) == confirmed.expected == await read(confirmed)
    assert AuthoringInput.model_validate(confirmed.expected).model_dump() == confirmed.expected
    assert confirmed.row == row
    assert {p: p.read_bytes() for p in confirmed.artifacts.root.rglob("*") if p.is_file()} == before


@pytest.mark.parametrize("status", ["pending", "running", "waiting-confirmation", "blocked", "failed", "cancelled"])
async def test_requires_completed_confirmation(confirmed, status):
    confirmed.row["status"] = status
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


async def test_cancellation_overrides_ready(confirmed):
    confirmed.row["cancel_requested"] = True
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


@pytest.mark.parametrize("field,value", [("run_id", "another"), ("source_version", 1), ("domain_revision", 2),
                                         ("checklist_digest", "0" * 64), ("source_version", True)])
async def test_rejects_stale_or_invalid_expected_identity(confirmed, field, value):
    with pytest.raises(InvalidConfirmation):
        await read(confirmed, **{field: value})


@pytest.mark.parametrize("stage", STAGES)
async def test_history_and_scoped_receipts_cannot_substitute_current_root(confirmed, stage):
    receipt = next(r for r in confirmed.row["artifacts"] if r["stage"] == stage)
    confirmed.row["history"]["artifacts"].append(deepcopy(receipt))
    receipt["scope"] = "child-pack"
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


@pytest.mark.parametrize("field,value", [("run_id", "another"), ("source_version", 1), ("domain_revision", 1)])
async def test_wrong_revision_receipt_cannot_supply_root(confirmed, field, value):
    confirmed.row["artifacts"][-1][field] = value
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


def replace_root(data, stage, value):
    receipt = next(r for r in data.row["artifacts"] if r["stage"] == stage)
    receipt["artifact_ref"] = data.artifacts.put_json(value)


@pytest.mark.parametrize("stage,field,value", [
    ("pack-plan", "frozen_ref", {}), ("pack-plan", "run_identity", {**IDENTITY, "domain_revision": 1}),
    ("verify", "items_ref", {}), ("verify", "hard_blocking", True),
    ("confirm", "source_version", 1), ("confirm", "domain_revision", 1),
    ("confirm", "checklist_digest", "0" * 64),
])
async def test_inconsistent_artifacts_are_not_authoring_input(confirmed, stage, field, value):
    replace_root(confirmed, stage, {**confirmed.roots[stage], field: value})
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


@pytest.mark.parametrize("stage", ["freeze", "pack-plan", "extract", "verify"])
async def test_only_v2_analysis_roots_are_supported(confirmed, stage):
    replace_root(confirmed, stage, {**confirmed.roots[stage], "schema_version": 1})
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


async def test_digest_recomputed_even_if_confirmed_digest_matches_run(confirmed):
    replace_root(confirmed, "extract", {"schema_version": 2, "packs": [], "changed": True})
    changed_ref = next(r["artifact_ref"] for r in confirmed.row["artifacts"] if r["stage"] == "extract")
    replace_root(confirmed, "verify", {**confirmed.roots["verify"], "items_ref": changed_ref})
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


@pytest.mark.parametrize("stage", STAGES)
async def test_content_integrity_is_checked(confirmed, stage):
    ref = confirmed.expected[STAGES[stage]]
    path = confirmed.artifacts.path(ref)
    path.chmod(0o644)
    path.write_bytes(b"x" * ref["size"])
    with pytest.raises(ArtifactIntegrityError):
        await read(confirmed)


@pytest.mark.parametrize("mutation", ["missing", "digest", "resolutions"])
async def test_requires_persisted_matching_confirmation(confirmed, mutation):
    if mutation == "missing":
        confirmed.row["confirmations"] = []
    else:
        confirmed.row["confirmations"][0]["checklist_digest" if mutation == "digest" else "issue_resolutions"] = "wrong"
    with pytest.raises(InvalidConfirmation):
        await read(confirmed)


@pytest.mark.parametrize("field,value", [("schema_version", True), ("source_version", "2"),
                                         ("domain_revision", 0), ("analysis_ref", {})])
def test_shared_schema_is_strict(confirmed, field, value):
    with pytest.raises(ValidationError):
        AuthoringInput.model_validate({**confirmed.expected, field: value})


async def test_partial_amendment_scoring_and_retained_doubts_are_not_reinterpreted(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    evidence = [{"range_id": "r1", "quote": "每项2分，最高6分；二标段改为20天。"}]
    items = [
        {"item_id": "score", "kind": "scoring_rule", "text": "每项2分，最高6分", "evidence": evidence,
         "scoring": {"points": 2, "max_points": 6}},
        {"item_id": "delivery", "kind": "obligation", "text": "一、二标段均30天", "evidence": evidence},
        {"item_id": "deleted", "kind": "obligation", "text": "已删除规则", "evidence": evidence},
    ]
    item_ref = artifacts.put_json({"schema_version": 2, "items": items})
    rule = {"text": "一标段30天", "scope": "一标段", "evidence": evidence}
    decisions = [{"decision_id": identity, "amendment": {"target_item_id": target,
                  "retained_rules": retained, "replacement_item_ids": []}}
                 for identity, target, retained in [("partial", "delivery", [rule]), ("delete", "deleted", [])]]
    issue = {"issue_id": "doubt", "blocking": True, "detail": "适用范围仍有疑问"}
    resolutions = {"schema_version": 1, "resolutions": [
        {"issue_id": "doubt", "verdict": "acknowledged", "reason": "知悉并保留疑问"}]}
    frozen = artifacts.put_json({"schema_version": 2, "documents": []})
    index = {"schema_version": 2, "packs": [{"pack_id": "pack", "artifact_ref": item_ref}]}
    roots = {"freeze": artifacts.read_json(frozen), "extract": index,
             "verify": {"schema_version": 2, "items_ref": artifacts.put_json(index),
                        "hard_blocking": False, "blocking": True, "human_issue_ids": ["doubt"],
                        "issues": [issue], "unverified_relation_ids": ["unverified"],
                        "relations_ref": artifacts.put_json([{"relation_id": "unverified"}]),
                        "item_refs": [{"item_id": i["item_id"], "artifact_ref": item_ref} for i in items],
                        "accepted_decision_ids": ["partial", "delete"], "decisions_ref": artifacts.put_json(decisions)}}
    data = snapshot(artifacts, roots=roots, resolutions=resolutions)
    value = await read(data)
    verified = artifacts.read_json(value["verification_ref"])
    projected = current_items(artifacts, verified)
    assert projected[0]["item"]["scoring"] == {"points": 2, "max_points": 6}
    assert projected[1] == {"item_ref": {"item_id": "delivery", "artifact_ref": item_ref},
                            "decision_id": "partial", "effective_rule": rule}
    assert len(projected) == 2
    assert verified["issues"] == [issue] and verified["unverified_relation_ids"] == ["unverified"]
    assert artifacts.read_json(value["confirmation_ref"])["issue_resolutions"] == resolutions


async def test_source_fragments_recover_original_location_and_keep_background_and_tables(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    original_ref = artifacts.put_bytes(b"synthetic original")
    native = source([{"text": "项目背景；服务须在2小时内响应。", "heading": "项目与服务", "section": "service"},
                     {"grid": grid(rows=5), "heading": "人员表", "section": "personnel"}])
    native = SourceIndex({**native.document, "original_ref": original_ref}, native.raw)
    raw_ref = artifacts.put_json(native.raw)
    index_ref = artifacts.put_json(native.descriptor(raw_ref))
    complete = native.ranges[0]
    selected = fragment(complete, len("项目背景；"), complete["end"])
    pack = {"schema_version": 2, "pack_id": "service-pack", "source_index_ref": index_ref,
            "body": [selector(selected)], "supporting": []}
    frozen = {"schema_version": 2, "documents": [{**native.document, "raw_ref": raw_ref,
                                                  "source_index_ref": index_ref}]}
    plan = {"schema_version": 2, "frozen_ref": artifacts.put_json(frozen), "run_identity": IDENTITY,
            "scope_decisions": [], "packs": [{"pack_id": "service-pack", "artifact_ref": artifacts.put_json(pack)}]}
    data = snapshot(artifacts, roots={"freeze": frozen, "extract": {"schema_version": 2, "packs": []},
                                     "pack-plan": plan})
    value = await read(data)
    repository = EvidenceRepository(artifacts)
    persisted_plan = artifacts.read_json(value["pack_plan_ref"])
    loaded = repository.load_pack(persisted_plan["packs"][0])["body"][0]
    assert selected["range_id"] not in repository.source_index(index_ref).by_id
    assert loaded["text"] == "服务须在2小时内响应。"
    assert loaded["source_range_id"] == complete["range_id"]
    assert loaded["locator"]["page_ordinal"] == complete["locator"]["page_ordinal"]
    document = artifacts.read_json(value["frozen_ref"])["documents"][0]
    original = artifacts.read_json(document["raw_ref"])
    assert original["units"][0]["text"] == "项目背景；服务须在2小时内响应。"
    assert original["units"][1]["grid"] == native.raw["units"][1]["grid"]
    assert artifacts.read_bytes(document["original_ref"]) == b"synthetic original"
