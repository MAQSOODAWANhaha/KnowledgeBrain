"""Issue tiers: hard blockers have a non-confirmation exit; semantic doubts need a human verdict."""

from pathlib import Path
import re

import pytest

from docreader.tender.schema import HOST_ISSUE_EXITS, ISSUE_EXITS, assign_issues, is_hard_blocking, validate_issue_resolutions
from docreader.tender.evidence.references import EvidenceRepository
from docreader.llm.request import ModelContractError
from docreader.tender.requirements import extract_items, validate_item_correction
from test_analysis_checks import (
    ExtractModel, IDENTITY, Model, OperationBudget, reference_candidate, resolution, run, scenario,
)

ROOT = Path(__file__).resolve().parents[2]
ANALYSIS_MODULES = ["tender/checks/analysis.py", "tender/requirements/extract.py", "tender/evidence/registration.py",
                    "tender/evidence/freeze.py", "tender/evidence/planner.py", "tender/evidence/context.py"]
HOST_CODE = re.compile(r'(?:\bissue\(|\bpending\(|"code": )"([a-z_]+)"(?:,\s*"blocking")?')


def emitted_codes():
    codes = set()
    for module in ANALYSIS_MODULES:
        for match in HOST_CODE.finditer((ROOT / module).read_text(encoding="utf-8")):
            if not match.group(0).startswith('"code"') or match.group(0).endswith('"blocking"'):
                codes.add(match.group(1))
    return codes


def test_every_host_issue_raised_by_the_analysis_path_has_a_registered_exit():
    assert emitted_codes() - HOST_ISSUE_EXITS.keys() == set()
    assert set(HOST_ISSUE_EXITS.values()) <= ISSUE_EXITS


@pytest.mark.parametrize("finding,hard", [
    ({"code": "analysis_budget_exceeded", "blocking": True}, True),
    ({"code": "missing_document", "blocking": True}, True),
    ({"code": "condition_scope_unresolved", "blocking": True}, True),
    ({"code": "extraction_item_unresolved", "blocking": True}, False),
    ({"code": "OMITTED_REQUIREMENT", "blocking": True}, False),
    ({"code": "analysis_budget_exceeded", "blocking": False}, False),
])
def test_only_registered_incompleteness_is_a_hard_blocker(finding, hard):
    assert is_hard_blocking(finding) is hard


def resolutions(*entries):
    return {"schema_version": 1, "resolutions": [
        {"issue_id": issue_id, "verdict": verdict, "reason": "人工核对原文后作出结论。"} for issue_id, verdict in entries]}


def test_resolutions_must_cover_exactly_the_pending_semantic_issues():
    accepted = validate_issue_resolutions(["b", "a"], resolutions(("b", "acknowledged"), ("a", "dismissed")))
    assert [entry["issue_id"] for entry in accepted["resolutions"]] == ["a", "b"]
    assert validate_issue_resolutions([], None) is None
    assert validate_issue_resolutions([], resolutions()) is None
    for pending, submitted in [
        (["a"], None), (["a", "b"], resolutions(("a", "dismissed"))),
        (["a"], resolutions(("a", "dismissed"), ("c", "dismissed"))),
        (["a"], resolutions(("a", "dismissed"), ("a", "acknowledged"))),
        (["a"], resolutions(("a", "accepted"))),
        (["a"], {"schema_version": 1, "resolutions": [{"issue_id": "a", "verdict": "dismissed", "reason": " "}]}),
        ([], resolutions(("a", "dismissed"))),
    ]:
        with pytest.raises(ValueError):
            validate_issue_resolutions(pending, submitted)


@pytest.mark.asyncio
async def test_model_finding_waits_for_a_human_verdict_but_budget_overflow_stays_hard(tmp_path):
    def flag(payload, value):
        if payload["operation"] == "review" and payload["work_unit_id"].startswith("review-"):
            value["findings"].append({"code": "OMITTED_REQUIREMENT", "detail": "疑似漏抽一项资格要求。", "blocking": True})

    semantic = await run(await scenario(tmp_path / "semantic"), Model(mutate=flag))
    flagged = [p["issue_id"] for p in semantic["issues"] if p["code"] == "OMITTED_REQUIREMENT"]
    assert semantic["blocking"] is True and semantic["hard_blocking"] is False
    assert flagged and set(flagged) <= set(semantic["human_issue_ids"])
    assert semantic["human_issue_ids"] == sorted(semantic["human_issue_ids"])

    admission = OperationBudget(lambda payload: payload["operation"] == "review")
    overflow = await run(await scenario(tmp_path / "overflow"), Model(), admission)
    exceeded = {p["issue_id"] for p in overflow["issues"] if p["code"] == "analysis_budget_exceeded"}
    assert exceeded and overflow["hard_blocking"] is True
    assert not exceeded & set(overflow["human_issue_ids"])
    incomplete = {p["issue_id"] for p in overflow["issues"] if p["code"] == "analysis_review_incomplete"}
    assert incomplete and not incomplete & set(overflow["human_issue_ids"])


@pytest.mark.asyncio
@pytest.mark.parametrize("code", ["condition_scope_unresolved", "analysis_budget_exceeded"])
@pytest.mark.parametrize("origin", ["source", "model"])
async def test_host_code_does_not_turn_a_model_doubt_into_a_host_blocker(tmp_path, code, origin):
    problem = {"code": code, "detail": "待核对本事项的条件是否完整。", "blocking": True}
    data = await scenario(tmp_path, source_issues=[problem] if origin == "source" else [])

    def flag(payload, value):
        if origin == "model" and payload["operation"] == "review":
            value["findings"].append(dict(problem))

    checked = await run(data, Model(mutate=flag))
    identities = {p["issue_id"] for p in checked["issues"] if p["detail"] == problem["detail"]}
    assert identities and checked["blocking"] is True
    assert checked["hard_blocking"] is (origin == "source")
    if origin == "source":
        assert not identities & set(checked["human_issue_ids"])
    else:
        assert identities <= set(checked["human_issue_ids"])
        submitted = resolutions(*((identity, "acknowledged") for identity in checked["human_issue_ids"]))
        assert validate_issue_resolutions(checked["human_issue_ids"], submitted) is not None


@pytest.mark.asyncio
@pytest.mark.parametrize("code", ["condition_scope_unresolved", "analysis_budget_exceeded"])
@pytest.mark.parametrize("origin", ["pack", "model"])
async def test_extraction_keeps_model_issue_provenance_separate_from_pack_blockers(tmp_path, code, origin):
    data = await scenario(tmp_path)
    artifacts, plan, items, _ = data
    problem = {"code": code, "detail": "待核对本事项的条件是否完整。", "blocking": True}
    descriptor = plan["packs"][0]
    if origin == "pack":
        saved_pack = artifacts.read_json(descriptor["artifact_ref"])
        saved_pack["issues"] = [problem]
        descriptor["artifact_ref"] = artifacts.put_json(saved_pack)
    pack = EvidenceRepository(artifacts).load_pack(descriptor)
    original = artifacts.read_json(items["packs"][0]["artifact_ref"])
    response = {key: original[key] for key in ("schema_version", "items", "relations", "dispositions")}
    response["issues"] = [problem] if origin == "model" else []
    normalized = await extract_items(pack, ExtractModel(response), identity=IDENTITY)
    assert any(p["detail"] == problem["detail"] for p in normalized["issues"])
    items["packs"][0]["artifact_ref"] = artifacts.put_json(normalized)

    checked = await run(data, Model())
    identities = {p["issue_id"] for p in checked["issues"] if p["detail"] == problem["detail"]}
    assert identities and checked["blocking"] is True
    assert checked["hard_blocking"] is (origin == "pack")
    assert bool(identities & set(checked["human_issue_ids"])) is (origin == "model")


@pytest.mark.asyncio
async def test_extraction_budget_refusal_remains_a_trusted_host_blocker(tmp_path):
    data = await scenario(tmp_path)
    artifacts, plan, items, _ = data
    pack = EvidenceRepository(artifacts).load_pack(plan["packs"][0])
    pack.setdefault("planning_details", {}).setdefault("budget", {})["fits"] = False
    normalized = await extract_items(pack, ExtractModel(None), identity=IDENTITY)
    exceeded = {p["issue_id"] for p in assign_issues(normalized["issues"], pack["pack_id"])
                if p["code"] == "extraction_budget_exceeded"}
    assert exceeded and exceeded <= set(normalized["host_issue_ids"])
    items["packs"][0]["artifact_ref"] = artifacts.put_json(normalized)

    checked = await run(data, Model())
    assert checked["hard_blocking"] is True
    assert not exceeded & set(checked["human_issue_ids"])


@pytest.mark.asyncio
async def test_model_cannot_supply_trusted_host_issue_ids(tmp_path):
    artifacts, plan, items, _ = await scenario(tmp_path)
    pack = EvidenceRepository(artifacts).load_pack(plan["packs"][0])
    original = artifacts.read_json(items["packs"][0]["artifact_ref"])
    response = {key: original[key] for key in ("schema_version", "items", "relations", "dispositions", "issues")}
    response["host_issue_ids"] = ["forged:host-issue"]

    with pytest.raises(ModelContractError):
        await extract_items(pack, ExtractModel(response), identity=IDENTITY)


@pytest.mark.asyncio
async def test_legacy_extraction_without_issue_provenance_keeps_conservative_blocking(tmp_path):
    data = await scenario(tmp_path)
    artifacts, _, items, _ = data
    legacy = artifacts.read_json(items["packs"][0]["artifact_ref"])
    legacy.pop("host_issue_ids", None)
    legacy["issues"] = [{"code": "condition_scope_unresolved", "detail": "旧台账未记录问题来源。", "blocking": True}]
    items["packs"][0]["artifact_ref"] = artifacts.put_json(legacy)

    checked = await run(data, Model())
    identities = {p["issue_id"] for p in checked["issues"] if p["detail"] == "旧台账未记录问题来源。"}
    assert identities and checked["hard_blocking"] is True
    assert not identities & set(checked["human_issue_ids"])


@pytest.mark.asyncio
async def test_correction_rebuilds_host_provenance_without_promoting_same_code_model_issues(tmp_path):
    data = await scenario(tmp_path)
    artifacts, plan, items, _ = data
    descriptor = plan["packs"][0]
    host_problem = {"code": "condition_scope_unresolved", "detail": "宿主尚未完成范围绑定。", "blocking": True}
    model_problem = {"code": "condition_scope_unresolved", "detail": "模型疑问：条件可能存在另一种解释。", "blocking": True}
    saved_pack = artifacts.read_json(descriptor["artifact_ref"])
    saved_pack["issues"] = [host_problem]
    descriptor["artifact_ref"] = artifacts.put_json(saved_pack)
    pack = EvidenceRepository(artifacts).load_pack(descriptor)
    original = artifacts.read_json(items["packs"][0]["artifact_ref"])
    response = {key: original[key] for key in ("schema_version", "items", "relations", "dispositions")}
    response["issues"] = [model_problem]
    normalized = await extract_items(pack, ExtractModel(response), identity=IDENTITY)
    correction = {key: normalized[key] for key in ("schema_version", "items", "relations", "dispositions", "issues")}
    corrected = validate_item_correction(pack, correction)
    corrected_issues = assign_issues(corrected["issues"], pack["pack_id"])
    host_ids = {p["issue_id"] for p in corrected_issues if p["detail"] == host_problem["detail"]}
    model_ids = {p["issue_id"] for p in corrected_issues if p["detail"] == model_problem["detail"]}
    assert host_ids and host_ids <= set(corrected["host_issue_ids"])
    assert model_ids and not model_ids & set(corrected["host_issue_ids"])
    items["packs"][0]["artifact_ref"] = artifacts.put_json(corrected)

    checked = await run(data, Model())
    assert checked["hard_blocking"] is True
    assert not host_ids & set(checked["human_issue_ids"])
    assert model_ids <= set(checked["human_issue_ids"])


@pytest.mark.asyncio
@pytest.mark.parametrize("decision", ["none", "rejected", "accepted"])
async def test_recovered_disposition_needs_semantic_acceptance_after_complete_source_review(tmp_path, decision):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("交付期限为30天。", "obligation"), ("本段说明评审程序。", None)])])
    artifacts, plan, items, _ = data
    pack = EvidenceRepository(artifacts).load_pack(plan["packs"][0])
    original = artifacts.read_json(items["packs"][0]["artifact_ref"])
    response = {key: original[key] for key in ("schema_version", "items", "relations", "issues")}
    response["dispositions"] = []
    normalized = await extract_items(pack, ExtractModel(response), identity=IDENTITY)
    pending = {p["issue_id"] for p in assign_issues(normalized["issues"], pack["pack_id"])
               if p["code"] == "extraction_disposition_unresolved"}
    assert pending
    items["packs"][0]["artifact_ref"] = artifacts.put_json(normalized)
    model = Model(locate=reference_candidate, associate=resolution if decision != "none" else None,
                  accept=decision == "accepted")

    checked = await run(data, model)
    assert checked["coverage"]["missing"] == {} and checked["hard_blocking"] is False
    assert checked["counts"]["items"] == 1
    if decision == "accepted":
        assert pending <= set(checked["resolved_issue_ids"])
        assert checked["blocking"] is False and checked["human_issue_ids"] == []
        assert any(p["work_unit_id"].startswith("joint-") for p in model.calls)
    else:
        assert pending <= set(checked["human_issue_ids"])
        assert not pending & set(checked["resolved_issue_ids"])
        assert checked["blocking"] is True
        if decision == "none":
            assert not any(p["work_unit_id"].startswith("joint-") for p in model.calls)
