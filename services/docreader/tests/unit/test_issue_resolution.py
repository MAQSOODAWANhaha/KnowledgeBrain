"""Issue tiers: hard blockers have a non-confirmation exit; semantic doubts need a human verdict."""

from pathlib import Path
import re

import pytest

from docreader.tender.schema import HOST_ISSUE_EXITS, ISSUE_EXITS, is_hard_blocking, validate_issue_resolutions
from test_analysis_checks import Model, OperationBudget, run, scenario

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
