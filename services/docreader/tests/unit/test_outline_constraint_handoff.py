"""Visible frozen context can be handed to assembly without claiming it was read."""

from copy import deepcopy

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import validate_outline_references
from docreader.tender.schema import OutlineResponse
from docreader.tests.submission_responses import assemble_independent_candidates
from test_assembly_source_handoff import BODY, CONDITION, QUOTES, context_fixture
from test_authoring_pipeline_boundaries import payload


def local_context(tmp_path, monkeypatch):
    artifacts, context = context_fixture(tmp_path)
    rule = context.payload["rules"][0]
    actual = rule["item"]["evidence"][0]["range_id"]
    external = context.payload["rules"][2]["item"]["evidence"][0]["range_id"]
    rule["item"]["evidence"].append({"range_id": external, "quote": QUOTES[0]})
    monkeypatch.setattr(pipeline, "_source_records", lambda *args: [{
        "source": {"range_id": actual, "text": BODY[0], "kind": "section", "role": "main"},
        "rules": [rule],
    }])
    return artifacts, context, actual, external


def source_response(value):
    return {"schema_version": 2, "chapters": [{"title": "技术响应", "targets": [{
        "kind": "narrative", "purpose": "按实际条件应答。",
        "rule_ids": [value["rules"][0]["rule_id"]],
        "range_ids": [value["sources"][0]["range_id"]],
    }]}], "constraint_range_ids": [value["rules"][0]["item"]["evidence"][1]["range_id"]],
        "findings": []}


async def test_visible_evidence_handoff_loads_full_source_without_marking_it_read(tmp_path, monkeypatch):
    artifacts, context, actual, external = local_context(tmp_path, monkeypatch)
    before = deepcopy((context.payload, context.sources))
    calls = []

    async def execute(task, request, validate):
        value = payload(request)
        calls.append(value)
        if value.get("operation") == "assemble":
            assert [entry["text"] for entry in value["submission_sources"]] == [CONDITION]
            assert QUOTES[0] != CONDITION
            return validate(assemble_independent_candidates(value))
        assert {entry["range_id"] for entry in value["sources"]}.isdisjoint(
            {value["rules"][0]["item"]["evidence"][1]["range_id"]})
        return validate(source_response(value))

    gaps = []
    outline = await pipeline._organize(artifacts, context, RequestBudget(
        model="fixture", context_tokens=131072), execute, gaps)
    assert len(calls) == 2 and not gaps
    assert outline["submission_refs"] == context.sources[external]
    assert context.read_units == {(ref["source_index_ref"]["sha256"], ref["unit_id"])
                                  for ref in context.sources[actual]}
    assert all((ref["source_index_ref"]["sha256"], ref["unit_id"]) not in context.read_units
               for ref in context.sources[external])
    assert context.payload == before[0]
    assert all(context.sources[identity] == refs for identity, refs in before[1].items())
    assert all(not target["template_refs"] for chapter in outline["chapters"]
               for target in chapter["targets"])


@pytest.mark.parametrize("identity", ["hidden-frozen", "unknown", "visible-unbindable"])
async def test_source_handoff_rejects_hidden_and_unknown_references(tmp_path, monkeypatch, identity):
    artifacts, context, _, external = local_context(tmp_path, monkeypatch)
    hidden = next(key for key, refs in context.sources.items()
                  if key != external and refs[0]["unit_id"] == "unit:3")
    if identity == "visible-unbindable":
        context.payload["rules"][0]["item"]["evidence"].append({
            "range_id": "unavailable-range", "quote": "尚无冻结定位的证据。"})

    async def execute(task, request, validate):
        value = payload(request)
        response = source_response(value)
        selected = (hidden if identity == "hidden-frozen" else
                    value["rules"][0]["item"]["evidence"][2]["range_id"]
                    if identity == "visible-unbindable" else "not-provided")
        response["constraint_range_ids"] = [selected]
        with pytest.raises(ModelContractError) as error:
            validate(response)
        assert error.value.errors[0]["path"] == ["constraint_range_ids", 0]
        raise AssertionError("invalid reference rejected")

    with pytest.raises(AssertionError, match="invalid reference rejected"):
        await pipeline._organize(artifacts, context, RequestBudget(
            model="fixture", context_tokens=131072), execute, [])


async def test_handoff_does_not_authorize_unread_response_table_conditions(tmp_path, monkeypatch):
    artifacts, context, _, _ = local_context(tmp_path, monkeypatch)

    async def execute(task, request, validate):
        value = payload(request)
        response = source_response(value)
        response["chapters"][0]["targets"][0]["response_table"] = {
            "context_range_ids": response["constraint_range_ids"],
            "rows": [{"range_ids": [value["sources"][0]["range_id"]]}],
        }
        with pytest.raises(ModelContractError) as error:
            validate(response)
        assert error.value.errors[0]["path"] == [
            "chapters", 0, "targets", 0, "response_table", "context_range_ids", 0]
        raise AssertionError("unread common condition rejected")

    with pytest.raises(AssertionError, match="unread common condition rejected"):
        await pipeline._organize(artifacts, context, RequestBudget(
            model="fixture", context_tokens=131072), execute, [])


def test_validator_preserves_default_and_explicit_constraint_domains():
    response = OutlineResponse(schema_version=2, chapters=[], findings=[],
                               constraint_range_ids=["visible"]).model_dump()
    options = {"rule_ids": [], "source_ids": {"actual", "visible"}, "template_part_ids": []}
    validate_outline_references(response, **options)
    with pytest.raises(ModelContractError) as error:
        validate_outline_references(response, **options, constraint_source_ids={"actual"})
    assert error.value.errors[0]["path"] == ["constraint_range_ids", 0]
    validate_outline_references(response, **options, constraint_source_ids={"actual", "visible"},
                                context_source_ids={"actual"})
