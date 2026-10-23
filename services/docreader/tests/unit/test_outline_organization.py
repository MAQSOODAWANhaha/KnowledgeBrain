"""Outline mappings preserve effective rules and frozen formats without re-extraction."""

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError, ModelConfigurationError, ModelContractError
from docreader.llm.wire import WirePayload
from docreader.tender.authoring import _OutlineContext, load_outline_source, organize_outline
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.schema import AuthoringOutline

from test_analysis_checks import IDENTITY, Model, run, scenario
from test_structural_packing import grid, source


def authoring_input(artifacts, plan, items, verification):
    plan = {**plan, "run_identity": IDENTITY}
    items_ref = artifacts.put_json(items)
    verification = {**verification, "items_ref": items_ref}
    return {"schema_version": 2, **IDENTITY, "checklist_digest": "a" * 64,
            "frozen_ref": plan["frozen_ref"], "pack_plan_ref": artifacts.put_json(plan),
            "items_ref": items_ref, "verification_ref": artifacts.put_json(verification),
            "confirmation_ref": artifacts.put_json({"source_version": IDENTITY["source_version"],
                "domain_revision": IDENTITY["domain_revision"], "checklist_digest": "a" * 64})}


async def context(tmp_path, **options):
    data = await scenario(tmp_path, **options)
    artifacts, plan, items, _ = data
    verification = await run(data, Model())
    value = authoring_input(artifacts, plan, items, verification)
    return artifacts, value, _OutlineContext(artifacts, value)


def response(targets=(), *, findings=()):
    return {"schema_version": 2, "chapters": [{"title": "项目实施与响应", "targets": list(targets),
                                               "children": []}], "findings": list(findings)}


def target(rule_ids=(), *, kind="narrative", range_ids=()):
    return {"kind": kind, "purpose": "说明人员配置、实施措施及提供的证明依据。",
            "rule_ids": list(rule_ids), "range_ids": list(range_ids)}


def targets(outline):
    def chapter_targets(chapters):
        for chapter in chapters:
            yield from chapter["targets"]
            yield from chapter_targets(chapter["children"])
    return list(chapter_targets(outline["chapters"]))


async def test_one_rule_has_multiple_response_positions_with_host_owned_identities(tmp_path):
    _, value, ctx = await context(tmp_path)
    rule_id = next(iter(ctx.rules))
    raw = response([target([rule_id]), target([rule_id], kind="attachment")])
    before = deepcopy(raw)
    outlined = ctx.validate(raw)
    mapped = targets(outlined)
    assert len(mapped) == 2
    assert [entry["kind"] for entry in mapped] == ["narrative", "attachment"]
    assert all(entry["rule_refs"] == [ctx.rules[rule_id]] for entry in mapped)
    assert len({entry["target_id"] for entry in mapped}) == 2
    assert outlined["chapters"][0]["chapter_id"]
    assert outlined["authoring_input"] == value
    assert AuthoringOutline.model_validate(outlined).model_dump(exclude_unset=True) == outlined
    assert ctx.validate(raw) == outlined and raw == before


async def test_unassigned_rules_and_unbound_targets_keep_partial_outline_usable(tmp_path):
    _, _, ctx = await context(tmp_path)
    raw = response([target(), target([next(iter(ctx.rules))])])
    outlined = ctx.validate(raw)
    assert len(targets(outlined)) == 2 and "blocking" not in outlined
    assert any(issue["code"] == "outline_target_unbound" for issue in outlined["issues"])
    missing = {json.dumps(ctx.rules[rule_id], sort_keys=True) for rule_id in list(ctx.rules)[1:]}
    reported = {json.dumps(reference, sort_keys=True) for issue in outlined["issues"]
                if issue["code"] == "outline_rule_unassigned" for reference in issue["rule_refs"]}
    assert reported == missing
    empty = ctx.validate({"schema_version": 2, "chapters": [], "findings": []})
    assert empty["chapters"] == []
    assert [issue["rule_refs"][0] for issue in empty["issues"]] == list(ctx.rules.values())
    assert {json.dumps(reference, sort_keys=True) for issue in empty["issues"]
            for reference in issue["rule_refs"]} == {
        json.dumps(reference, sort_keys=True) for reference in ctx.rules.values()}


@pytest.mark.parametrize("where", ["target_rule", "target_source", "finding_rule"])
async def test_unknown_selection_returns_contract_feedback_instead_of_inventing_references(tmp_path, where):
    _, _, ctx = await context(tmp_path)
    raw = response([target([next(iter(ctx.rules))])])
    if where == "target_rule":
        raw["chapters"][0]["targets"][0]["rule_ids"] = ["unknown-rule"]
    elif where == "target_source":
        raw["chapters"][0]["targets"][0]["range_ids"] = ["unknown-range"]
    else:
        raw["findings"] = [{"code": "scope_unclear", "detail": "待核对适用标段。",
                             "rule_ids": ["unknown-rule"]}]
    with pytest.raises(ModelContractError) as caught:
        ctx.validate(raw)
    assert caught.value.errors and all(error["path"] for error in caught.value.errors)


async def test_repeated_rule_and_source_selections_are_normalized_by_host(tmp_path):
    _, _, ctx = await context(tmp_path)
    rule_id = next(iter(ctx.rules))
    range_id = ctx.payload["rules"][0]["item"]["evidence"][0]["range_id"]
    single = ctx.validate(response([target([rule_id], range_ids=[range_id])]))
    repeated = ctx.validate(response([target([rule_id, rule_id], range_ids=[range_id, range_id])]))
    assert repeated == single
    assert len(targets(repeated)[0]["source_refs"]) == 1


async def test_partial_amendment_keeps_each_retained_rule_distinct_without_old_scoring(tmp_path):
    data = await scenario(tmp_path)
    artifacts, plan, items, _ = data
    original = artifacts.read_json(items["packs"][0]["artifact_ref"])
    original["items"][0]["scoring"] = {"max_points": 99}
    items["packs"][0]["artifact_ref"] = artifacts.put_json(original)
    verification = await run(data, Model())
    original_id = original["items"][0]["item_id"]
    evidence = original["items"][0]["evidence"]
    retained = [{"text": "一标段负责人应持A证。", "scope": "一标段", "evidence": evidence},
                {"text": "二标段负责人应持A证。", "scope": "二标段", "evidence": evidence}]
    decision = {"decision_id": "accepted-partial", "amendment": {"target_item_id": original_id,
                "retained_rules": retained, "replacement_item_ids": []}}
    verification["decisions_ref"] = artifacts.put_json([decision])
    verification["accepted_decision_ids"] = [decision["decision_id"]]
    ctx = _OutlineContext(artifacts, authoring_input(artifacts, plan, items, verification))
    selected = [entry for entry in ctx.payload["rules"] if "effective_rule" in entry]
    assert len(selected) == 2
    assert [entry["effective_rule"] for entry in selected] == retained
    assert all("item" not in entry and "scoring" not in entry["effective_rule"] for entry in selected)
    refs = [ctx.rules[entry["rule_id"]] for entry in selected]
    assert refs[0]["item_ref"] == refs[1]["item_ref"]
    assert [entry["rule_index"] for entry in refs] == [0, 1]
    assert all(entry["decision_id"] == decision["decision_id"] for entry in refs)
    outlined = ctx.validate(response([target([entry["rule_id"] for entry in selected])]))
    assert targets(outlined)[0]["rule_refs"] == refs


async def test_form_without_original_template_is_saved_with_specific_missing_source_issue(tmp_path):
    _, _, ctx = await context(tmp_path)
    outlined = ctx.validate(response([target([next(iter(ctx.rules))], kind="form")]))
    form = targets(outlined)[0]
    assert form["source_refs"] == []
    assert any(issue["code"] == "outline_template_missing" and issue["target_id"] == form["target_id"]
               for issue in outlined["issues"])


async def test_source_catalog_does_not_claim_unread_background_was_read(tmp_path):
    _, _, ctx = await context(tmp_path, documents=[("main", "main", [
        ("负责人应持A证。", "obligation"), ("偏离表须按本格式填写。", None)])])
    read_ids = {entry["range_id"] for view in ctx.payload["source_views"]
                for key in ("body", "supporting") for entry in view.get(key, [])}
    catalog_ids = {entry["range_id"] for entry in ctx.payload["source_catalog"]}
    unread = catalog_ids - read_ids
    assert unread, "Unrelated native units must remain an availability catalog rather than full reread"
    outlined = ctx.validate(response([target([next(iter(ctx.rules))], kind="form", range_ids=[next(iter(unread))])]))
    form = targets(outlined)[0]
    assert len(form["source_refs"]) == 1
    assert any(issue["code"] == "outline_source_not_read" and issue["target_id"] == form["target_id"]
               for issue in outlined["issues"])


def test_template_loading_returns_the_complete_unchanged_native_table(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    original_ref = artifacts.put_bytes(b"synthetic-original")
    native = source([{"text": "项目背景说明。"}, {"grid": grid(rows=5), "heading": "人员偏离表"}])
    native = SourceIndex({**native.document, "original_ref": original_ref}, native.raw)
    raw_ref = artifacts.put_json(native.raw)
    index_ref = artifacts.put_json(native.descriptor(raw_ref))
    reference = {"source_index_ref": index_ref, "unit_id": "unit:1"}
    loaded = load_outline_source(artifacts, reference)
    assert loaded["document"] == native.document
    assert loaded["unit"] == native.raw["units"][1]
    assert loaded["unit"]["grid"] == grid(rows=5)
    assert loaded["unit"]["locator"] == native.raw["units"][1]["locator"]


async def test_model_payload_contains_semantics_without_cas_or_physical_metadata(tmp_path):
    artifacts, _, ctx = await context(tmp_path)
    before = {path: path.read_bytes() for path in artifacts.root.rglob("*") if path.is_file()}
    forbidden = {"artifact_ref", "source_index_ref", "original_ref", "raw_ref", "sha256", "relative_path",
                 "checklist_digest", "authoring_input", "run_id", "source_version", "domain_revision",
                 "locator", "physical_locator", "physical_path", "usage"}

    def inspect(value):
        if isinstance(value, dict):
            assert not forbidden.intersection(value)
            for child in value.values():
                inspect(child)
        elif isinstance(value, list):
            for child in value:
                inspect(child)

    inspect(ctx.payload)
    assert {entry["item"]["kind"] for entry in ctx.payload["rules"]} == {
        "obligation", "scoring_rule", "consequence_rule", "submission_requirement"}
    ctx.validate(response([target([next(iter(ctx.rules))])]))
    assert {path: path.read_bytes() for path in artifacts.root.rglob("*") if path.is_file()} == before


def test_unverified_relationships_use_the_same_wire_identity_as_relations():
    payload = {"schema_version": 2, "relations": [{"relation_id": "candidate:relation:unverified"}],
               "unverified_relation_ids": ["candidate:relation:unverified"]}
    wire = WirePayload("outline", payload)
    assert wire.payload["relations"][0]["relation_id"] == "l1"
    assert wire.payload["unverified_relation_ids"] == ["l1"]


@pytest.mark.parametrize("capacity,reserve,error", [(100, 32, ModelBudgetError),
                                                    (131072, 0, ModelConfigurationError)])
async def test_unsendable_outline_never_calls_executor_or_writes_artifacts(tmp_path, monkeypatch, capacity, reserve, error):
    artifacts, value, _ = await context(tmp_path)
    before = {p for p in artifacts.root.rglob("*") if p.is_file()}
    calls = []

    async def confirmed(*args, **kwargs):
        return value

    async def execute(*args):
        calls.append(args)
        pytest.fail("An unadmitted outline must not reach the executor")

    monkeypatch.setattr("docreader.tender.authoring.read_authoring_input", confirmed)
    budget = RequestBudget(context_tokens=capacity, model="synthetic", prompts={}, output_reserve=reserve)
    with pytest.raises(error):
        await organize_outline(None, artifacts, execute, budget, **IDENTITY, checklist_digest=value["checklist_digest"])
    assert calls == [] and {p for p in artifacts.root.rglob("*") if p.is_file()} == before


async def test_organizer_uses_the_execution_prompt_snapshot(tmp_path, monkeypatch):
    artifacts, value, _ = await context(tmp_path)
    calls = []

    async def confirmed(*args, **kwargs):
        return value

    async def execute(request, validate):
        calls.append(request)
        assert request["messages"][0]["content"] == "本次执行固定的编制提示"
        return validate({"schema_version": 2, "chapters": [], "findings": []})

    monkeypatch.setattr("docreader.tender.authoring.read_authoring_input", confirmed)
    budget = RequestBudget(context_tokens=131072, model="synthetic", output_reserve=32768,
                           prompts={"outline": "本次执行固定的编制提示"})
    reference = await organize_outline(None, artifacts, execute, budget, **IDENTITY,
                                      checklist_digest=value["checklist_digest"])
    assert len(calls) == 1 and artifacts.read_json(reference)["authoring_input"] == value
