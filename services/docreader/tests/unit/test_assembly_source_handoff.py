"""Assembly context sources do not imply response duties or template instances."""

from copy import deepcopy
import json

import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.recovery import _fragments
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import _OutlineContext
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.schema import OutlineAssemblyTarget
from test_structural_packing import source


BODY = ["原要求甲。", "原要求乙。"]
CONDITION = "适用条件：只适用已声明的范围。其余范围不要求。"
QUOTES = ["只适用已声明的范围。", "其余范围不要求。"]
UNRELATED = "另一事项的独立规定。"
UNBOUND = "未定位原文仍保留供阅读。"


def context_fixture(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    native = source([{"text": text, "heading": f"来源{number}"}
                     for number, text in enumerate([*BODY, CONDITION, UNRELATED])])
    native = SourceIndex({**native.document, "original_ref": artifacts.put_bytes(b"synthetic original")}, native.raw)
    index_ref = artifacts.put_json(native.descriptor(artifacts.put_json(native.raw)))
    items = [{"item_id": f"item-{number}", "text": text, "evidence": [
        {"range_id": native.ranges[number]["range_id"], "quote": text}]} for number, text in enumerate(BODY)]
    items.append({"item_id": "condition", "text": CONDITION, "evidence": [
        *({"range_id": native.ranges[2]["range_id"], "quote": quote} for quote in QUOTES),
        {"range_id": "unavailable-range", "quote": UNBOUND}]})
    snapshot = {"schema_version": 2, "run_id": "source-handoff", "source_version": 1, "domain_revision": 1,
        "frozen_ref": artifacts.put_json({"documents": [
            {**native.document, "role": "main", "source_index_ref": index_ref}]}),
        "extract_refs": [artifacts.put_json({"items": items})]}
    return artifacts, _OutlineContext(artifacts, snapshot)


async def organize(tmp_path, choose=None, *, compact=False):
    artifacts, context = context_fixture(tmp_path)
    calls = []
    budget = RequestBudget(model="fixture", context_tokens=131072)
    original_fits = pipeline._fits

    async def execute(task, request, validate):
        payload = json.loads(request["messages"][1]["content"][0]["text"])
        calls.append(deepcopy(payload))
        if payload.get("operation") == "assemble":
            candidate = next(t for c in payload["chapters"] for t in c["targets"])
            target = {"kind": "narrative", "purpose": "按原要求及已声明适用范围应答。",
                      "target_ids": [candidate["target_id"]]}
            sources = payload["submission_sources"]
            if isinstance(sources, dict):
                sources = [{"source_id": key, **value} for key, value in sources.items()]
            if choose is not None:
                target["source_ids"] = choose(sources)
            response = {"chapters": [{"title": "技术响应", "targets": [target], "children": []}]}
            before = deepcopy((payload, response))
            try:
                return validate(response)
            except ModelContractError as error:
                assert _fragments(response, error.errors) == [
                    {"path": ["chapters", 0, "targets", 0, "source_ids", 0], "value": target["source_ids"][0]}]
                raise
            finally:
                assert (payload, response) == before
        aliases = {entry["text"]: entry["range_id"] for entry in payload["sources"]}
        rules = {entry["item"]["text"]: entry["rule_id"] for entry in payload["rules"]}
        return validate({"schema_version": 2, "chapters": [{"title": "技术响应", "children": [],
            "targets": [{"kind": "narrative", "purpose": "原两个要求。", "rule_ids": [rules[text] for text in BODY],
                         "range_ids": [aliases[text] for text in BODY], "support_origins": ["tender"]}]}],
            "constraint_rule_ids": [rules[CONDITION]], "constraint_range_ids": [aliases[CONDITION], aliases[UNRELATED]],
            "findings": []})

    # Exercise the same source IDs after the production lossless projection too.
    with pytest.MonkeyPatch.context() as patch:
        if compact:
            patch.setattr(pipeline, "_fits", lambda b, task, payload, **kw:
                False if payload.get("operation") == "assemble" else original_fits(b, task, payload, **kw))
        outline = await pipeline._organize(artifacts, context, budget, execute, [])
    return artifacts, context, outline, calls


@pytest.mark.parametrize("mode", ["omitted", "source", "quotes_and_source"])
@pytest.mark.parametrize("compact", [False, True])
async def test_selected_context_reaches_writing_without_changing_two_original_rules(tmp_path, mode, compact):
    def choose(sources):
        texts = {CONDITION} if mode == "source" else {CONDITION, *QUOTES}
        ids = [entry["source_id"] for entry in sources if entry["text"] in texts]
        return [*ids, ids[0]]  # Identical source references still load once.

    artifacts, context, outline, calls = await organize(tmp_path, None if mode == "omitted" else choose, compact=compact)
    task = pipeline.plan_writing_tasks(outline)[0]
    loaded, _, issues = pipeline._load_task(artifacts, context, task, {"response": {"shared_arrangements": []}}, [])
    target = task["targets"][0]
    assert [ref["item_ref"]["item_id"] for ref in target["rule_refs"]] == ["item-0", "item-1"]
    assert target["template_refs"] == [] and target["support_origins"] == ["tender"]
    assert len(target["source_refs"]) == (2 if mode == "omitted" else 3)
    texts = [entry["text"] for entry in loaded["supports"]]
    assert texts.count(CONDITION) == (0 if mode == "omitted" else 1)
    assert UNRELATED not in texts and UNBOUND not in texts
    assert all(text in texts for text in BODY) and not issues
    assert len(calls) == 2


@pytest.mark.parametrize("choice,code", [("unknown", "outline_unknown_source"),
                                        ("unbound", "outline_source_unbindable")])
async def test_invalid_context_source_reports_only_its_leaf(tmp_path, choice, code):
    def choose(sources):
        if choice == "unknown":
            return ["not-provided"]
        entry = next(value for value in sources if value["text"] == UNBOUND)
        assert entry["bindable"] is False
        return [entry["source_id"]]

    with pytest.raises(ModelContractError) as error:
        await organize(tmp_path, choose)
    assert error.value.errors == [{"path": ["chapters", 0, "targets", 0, "source_ids", 0],
        "code": code, "detail": ("Select sources supplied in this assembly request." if choice == "unknown"
                                 else "This source has no frozen reference; select a bindable source or leave it unresolved.")}]


def test_source_map_keeps_quotes_and_full_source_separate_without_mutating_shared_data(tmp_path):
    _, context = context_fixture(tmp_path)
    before = deepcopy((context.payload, context.sources))
    reference = context.rules["rule-3"]
    condition_source = context.sources[context.payload["rules"][2]["item"]["evidence"][0]["range_id"]][0]
    bindings = {}
    payload = pipeline._constraint_context(context, [reference], [condition_source], source_bindings=bindings)
    by_text = {source["text"]: source for source in payload["submission_sources"]}
    assert set(by_text) == {*QUOTES, CONDITION, UNBOUND}
    assert by_text[UNBOUND]["bindable"] is False and by_text[UNBOUND]["source_id"] not in bindings
    assert all("bindable" not in by_text[text] for text in [*QUOTES, CONDITION])
    assert all(bindings[by_text[text]["source_id"]] == [condition_source] for text in [*QUOTES, CONDITION])
    bindings[by_text[CONDITION]["source_id"]][0]["unit_id"] = "changed-local-copy"
    assert (context.payload, context.sources) == before


def test_context_source_alone_does_not_create_a_response_position():
    with pytest.raises(ValidationError, match="a position requires a candidate"):
        OutlineAssemblyTarget(kind="narrative", purpose="仅有上下文没有应答位置。", source_ids=["s1"])
