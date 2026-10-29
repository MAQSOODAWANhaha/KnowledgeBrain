"""Authenticated human review against real SQL and synthetic analysis only."""
import copy
from types import SimpleNamespace
from uuid import uuid4

import pytest

from docreader.integrations.store import Conflict
from docreader.integrations.web import WebBridge, WebError
from docreader.integrations.web_analysis import analysis_review, confirm_analysis, correct_analysis
from docreader.tests.acceptance.test_analysis import analysis_runtime as analysis_runtime
from docreader.tests.acceptance.test_analysis import execute_analysis, result
from docreader.tests.database import initialize_schema


@pytest.fixture
async def review_runtime(analysis_runtime, monkeypatch):
    store, artifacts, settings, run, calls = analysis_runtime
    bridge = WebBridge(settings)
    project, owner, other = (str(uuid4()) for _ in range(3))
    async with store.transaction() as conn:
        await conn.execute("CREATE TABLE tender_projects(id uuid PRIMARY KEY, owner_user_id uuid, status text)")
        await conn.execute("CREATE TABLE tender_documents(id uuid PRIMARY KEY, project_id uuid, UNIQUE(id,project_id))")
        await conn.execute("INSERT INTO tender_projects VALUES(%s,%s,'open'),(%s,%s,'open')", (project, owner, other, owner))
    await initialize_schema(settings.dsn, sections=("web",))
    async with store.transaction() as conn:
        await conn.execute("INSERT INTO tender_project_analyses(run_id,project_id,request_key) VALUES(%s,%s,'fixture')",
                           (run["run_id"], project))
    registered = artifacts.read_json(run["manifest_ref"])
    async def manifest(_project):
        return registered
    monkeypatch.setattr(bridge, "manifest", manifest)
    return SimpleNamespace(bridge=bridge, project=project, owner=owner, other=other, analysis=analysis_runtime, calls=calls)


def payload(runtime, **values):
    return {"project_id": runtime.project, "owner_user_id": runtime.owner, "request_key": "human-action", **values}


async def waiting(runtime, **options):
    row = await execute_analysis(runtime.analysis, **options)
    review = (await analysis_review(runtime.bridge, payload(runtime)))["review"]
    return row, review


def confirmation(review):
    return {key: review[key] for key in ("run_id", "source_version", "domain_revision", "checklist_digest")} | {
        "issue_resolutions": {"schema_version": 1, "resolutions": [
            {"issue_id": identity, "verdict": "acknowledged", "reason": "人工已核对来源，保留此提示。"}
            for identity in review["human_issue_ids"]]}}


def correction(review):
    entry = review["correction"]["items"][0]
    return {"run_id": review["run_id"], "source_version": review["source_version"],
            "expected_revision": review["domain_revision"], "base_items_digest": review["correction"]["base_items_digest"],
            "reason": "人工核对后澄清事项表述。", "changes": [{"pack_id": entry["pack_id"],
                "item_id": entry["item"]["item_id"], "text": entry["item"]["text"] + "（人工核对）"}]}


async def test_authorization_and_run_ownership_precede_artifact_access(review_runtime, monkeypatch):
    runtime = review_runtime
    def forbidden(*_args):
        raise AssertionError("foreign project must not read artifacts")
    monkeypatch.setattr(runtime.bridge.artifacts, "read_json", forbidden)
    with pytest.raises(WebError) as unauthorized:
        await analysis_review(runtime.bridge, payload(runtime, owner_user_id=str(uuid4())))
    assert unauthorized.value.status == 404
    for operation in (confirm_analysis, correct_analysis):
        with pytest.raises(WebError) as foreign:
            await operation(runtime.bridge, payload(runtime, project_id=runtime.other, run_id=runtime.analysis[3]["run_id"]))
        assert foreign.value.status == 404


async def test_exact_human_resolutions_and_same_key_confirmation(review_runtime):
    runtime = review_runtime
    _, review = await waiting(runtime, blocked=True)
    assert review["human_issue_ids"] and not review["hard_blocking"] and review["sources_current"]
    body = confirmation(review)
    for resolutions in ([], body["issue_resolutions"]["resolutions"][1:],
                        body["issue_resolutions"]["resolutions"] * 2,
                        [{**value, "reason": " "} for value in body["issue_resolutions"]["resolutions"]],
                        [{"issue_id": "foreign", "verdict": "dismissed", "reason": "人工理由"}]):
        with pytest.raises(WebError) as invalid:
            await confirm_analysis(runtime.bridge, payload(runtime, **(body | {"issue_resolutions": {
                "schema_version": 1, "resolutions": resolutions}})))
        assert invalid.value.status == 400
    assert not (await runtime.bridge.store.get_run(review["run_id"]))["confirmations"]
    before = len(runtime.calls)
    request = payload(runtime, **body)
    approved = await confirm_analysis(runtime.bridge, request)
    assert (await confirm_analysis(runtime.bridge, request))["command_id"] == approved["command_id"]
    changed = copy.deepcopy(request)
    changed["issue_resolutions"]["resolutions"][0]["reason"] = "不同理由"
    with pytest.raises(WebError) as reused:
        await confirm_analysis(runtime.bridge, changed)
    assert reused.value.status == 409
    ready = await execute_analysis(runtime.analysis)
    assert ready["status"] == "analysis-ready" and len(runtime.calls) == before


@pytest.mark.parametrize("field,value", [("source_version", 2), ("domain_revision", 2), ("checklist_digest", "0" * 64)])
async def test_stale_confirmation_is_conflict(review_runtime, field, value):
    _, review = await waiting(review_runtime)
    with pytest.raises(WebError) as stale:
        await confirm_analysis(review_runtime.bridge, payload(review_runtime, **(confirmation(review) | {field: value})))
    assert stale.value.status == 409


async def test_text_correction_preserves_packs_and_replays_immutable_baseline(review_runtime):
    runtime = review_runtime
    row, review = await waiting(runtime)
    artifacts = runtime.bridge.artifacts
    baseline = result(artifacts, row, "extract")
    originals = {entry["pack_id"]: artifacts.read_json(entry["artifact_ref"]) for entry in baseline["packs"]}
    request = payload(runtime, **correction(review))
    submitted = await correct_analysis(runtime.bridge, request)
    assert submitted["submitted_domain_revision"] == 2
    assert (await correct_analysis(runtime.bridge, request))["submitted_domain_revision"] == 2
    revised = await runtime.bridge.store.get_run(review["run_id"])
    assert revised["domain_revision"] == 2 and len(revised["history"]["revisions"]) == 1
    corrected = artifacts.read_json(revised["revision"]["corrected_items_ref"])
    edit = request["changes"][0]
    for entry in corrected["packs"]:
        expected = copy.deepcopy(originals[entry["pack_id"]])
        if entry["pack_id"] == edit["pack_id"]:
            next(item for item in expected["items"] if item["item_id"] == edit["item_id"])["text"] = edit["text"]
        assert artifacts.read_json(entry["artifact_ref"]) == expected
    changed = copy.deepcopy(request)
    changed["changes"][0]["text"] += "不同修改"
    with pytest.raises(Conflict):
        await correct_analysis(runtime.bridge, changed)
    with pytest.raises(Conflict):
        await correct_analysis(runtime.bridge, request | {"request_key": "stale-new-key"})
    extracts = sum(task == "extract" for task, _ in runtime.calls)
    verified = await execute_analysis(runtime.analysis)
    assert verified["status"] == "waiting-confirmation"
    assert sum(task == "extract" for task, _ in runtime.calls) == extracts
    assert result(artifacts, verified, "verify")["items_ref"] == revised["revision"]["corrected_items_ref"]


@pytest.mark.parametrize("case", ["pack", "item", "duplicate", "evidence", "empty", "oversize"])
async def test_text_correction_rejects_untrusted_or_invalid_edits(review_runtime, case):
    _, review = await waiting(review_runtime)
    body = correction(review)
    if case == "pack":
        body["changes"][0]["pack_id"] = "foreign"
    if case == "item":
        body["changes"][0]["item_id"] = "foreign"
    if case == "duplicate":
        body["changes"] *= 2
    if case == "evidence":
        body["changes"][0]["evidence"] = []
    if case == "empty":
        body["changes"] = []
    if case == "oversize":
        body["changes"][0]["text"] = "文" * 20001
    with pytest.raises(WebError) as invalid:
        await correct_analysis(review_runtime.bridge, payload(review_runtime, **body))
    assert invalid.value.status == 400
    assert (await review_runtime.bridge.store.get_run(review["run_id"]))["domain_revision"] == 1
