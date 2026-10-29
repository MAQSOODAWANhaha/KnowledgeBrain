"""Outline mappings preserve effective rules and frozen formats without re-extraction."""

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.request import ModelContractError
from docreader.llm.wire import WirePayload
from docreader.tender.authoring import _OutlineContext, load_outline_source
from docreader.tender.authoring_templates import prepare_template_parts
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
            "extract_refs": [], "issues": [],
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


async def test_search_intents_bind_rules_and_actual_sources_in_the_shared_outline_context(tmp_path):
    _, _, ctx = await context(tmp_path)
    rule_id, range_id = next(iter(ctx.rules)), next(iter(ctx.sources))
    raw = response([target([rule_id], range_ids=[range_id])])
    raw["chapters"][0]["targets"][0]["support_origins"] = ["tender", "knowledge"]
    raw["search_intents"] = [{"query": "角色权限管理", "for_ids": [rule_id, range_id, rule_id]}]
    outlined = ctx.validate(raw)
    intent = outlined["search_intents"][0]
    assert intent["rule_refs"] == [ctx.rules[rule_id]]
    assert all(ref in intent["source_refs"] for ref in ctx.sources[range_id])
    assert range_id in intent["range_ids"]
    assert targets(outlined)[0]["support_origins"] == ["tender", "knowledge"]
    assert AuthoringOutline.model_validate(outlined).model_dump(exclude_unset=True) == outlined
    for invalid in ("unknown-rule", "unknown-range"):
        with pytest.raises(ModelContractError):
            ctx.validate({**raw, "search_intents": [{"query": "角色权限管理", "for_ids": [invalid]}]})


async def test_source_only_query_keeps_its_actual_range_without_inventing_rule_identity(tmp_path):
    _, _, ctx = await context(tmp_path)
    ctx.rules = {}
    ctx.payload["rules"] = []
    range_id = next(iter(ctx.sources))
    raw = response([target(range_ids=[range_id])])
    raw["search_intents"] = [{"query": "培训服务", "for_ids": [range_id]}]
    outlined = ctx.validate(raw)
    assert outlined["search_intents"] == [{"query": "培训服务", "rule_refs": [],
        "source_refs": ctx.sources[range_id], "range_ids": [range_id]}]


@pytest.mark.parametrize("source_read", [True, False])
async def test_source_supported_reference_position_does_not_require_unrelated_rule_binding(tmp_path, source_read):
    _, _, ctx = await context(tmp_path)
    assert ctx.rules
    range_id = next(iter(ctx.sources))
    references = ctx.sources[range_id]
    keys = {(ref["source_index_ref"]["sha256"], ref["unit_id"]) for ref in references}
    if source_read:
        ctx.read_units.update(keys)
    else:
        ctx.read_units.difference_update(keys)
    raw = response([{**target(range_ids=[range_id]),
                     "purpose": "保留原文规定的参见位置，由全局组装关联实际应答。"}])

    outlined = ctx.validate(raw)

    selected = targets(outlined)[0]
    assert selected["source_refs"] == references
    assert selected["rule_refs"] == []
    assert not any(issue["code"] == "outline_target_unbound" for issue in outlined["issues"])
    assert any(issue["code"] == "outline_source_not_read" for issue in outlined["issues"]) is not source_read
    # Selecting a source is not a semantic disposition of any existing rule.
    unassigned = {json.dumps(ref, sort_keys=True) for issue in outlined["issues"]
                  if issue["code"] == "outline_rule_unassigned" for ref in issue["rule_refs"]}
    assert unassigned == {json.dumps(ref, sort_keys=True) for ref in ctx.rules.values()}


async def test_native_template_position_is_grounded_without_invented_rule_binding(tmp_path):
    artifacts, _, ctx = await context(tmp_path)
    _, parts = prepare_template_parts(artifacts, ctx)
    assert parts and ctx.rules
    part_id = parts[0]["part_id"]
    raw = response([{**target(kind="form"), "template_part_ids": [part_id]}])

    outlined = ctx.validate(raw)

    selected = targets(outlined)[0]
    assert selected["rule_refs"] == selected["source_refs"] == []
    assert selected["template_refs"] == [ctx.template_parts[part_id]]
    assert not any(issue["code"] in {"outline_target_unbound", "outline_template_missing"}
                   for issue in outlined["issues"])
    assert load_outline_source(artifacts, selected["template_refs"][0]["source_ref"])["unit"]


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
    assert [issue["target_id"] for issue in outlined["issues"] if issue["code"] == "outline_target_unbound"] == [
        targets(outlined)[0]["target_id"]]
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


async def test_source_only_snapshot_supplies_original_text_without_waiting_for_extraction(tmp_path):
    artifacts, value, original = await context(tmp_path)
    value.update(items_ref=None, verification_ref=None, confirmation_ref=None, checklist_digest=None)
    available = _OutlineContext(artifacts, value)
    assert available.payload["rules"] == []
    assert available.payload["rule_origin"] == "source_only"
    assert original.payload["rule_origin"] == "verified_projection"
    assert available.payload["source_views"]
    assert available.read_units.issuperset(original.read_units)
    assert available.validate(response([target()]))["authoring_input"]["items_ref"] is None


async def test_missing_item_artifact_becomes_diagnostic_and_keeps_available_sources(tmp_path):
    artifacts, value, _ = await context(tmp_path)
    verification = artifacts.read_json(value["verification_ref"])
    reference = verification["item_refs"][0]["artifact_ref"]
    artifacts.path(reference).unlink()
    available = _OutlineContext(artifacts, value)
    assert available.sources
    assert any(issue["code"] == "authoring_projection_unavailable" for issue in available.payload["issues"])
    assert available.validate(response())["issues"]
