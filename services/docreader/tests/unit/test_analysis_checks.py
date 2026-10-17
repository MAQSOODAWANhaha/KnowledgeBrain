"""Bounded source review contracts; synthetic replies do not measure model quality."""

from copy import deepcopy
from functools import partial
from itertools import combinations
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm import ModelInfrastructureError, ModelSemanticError
from docreader.llm.budget import RequestBudget
from docreader.tender.checks.analysis import _Analysis, assign_issues, build_catalog, current_items, verify_analysis
from docreader.tender.evidence.freeze import plan_sources
from docreader.tender.evidence.references import EvidenceRepository, SourceIndex, fragment
from docreader.tender.requirements import extract_items, item_payload
from test_structural_packing import source


IDENTITY = {"run_id": "run-analysis", "source_version": 1, "domain_revision": 1}


def budget(capacity=100000):
    return RequestBudget(context_tokens=capacity, output_reserve=0,
                         prompts={"extract": "Analyze source data.", "verify": "Review source data independently."})


def range_ids(views):
    ids = {entry["range_id"] for view in views for key in ("body", "supporting", "source_aliases")
           for entry in view.get(key, [])}
    ids.update(identity for view in views for template in view.get("blank_table_templates", [])
               for row in template["rows"] for identity in row["range_ids"])
    return sorted(ids)


class Receipts:
    def __init__(self, artifacts):
        self.artifacts, self.saved = artifacts, {}

    async def __call__(self, stage, producer, *, scope="", inputs=None, model=False):
        key = (stage, scope, json.dumps(inputs, sort_keys=True))
        if key not in self.saved:
            self.saved[key] = self.artifacts.put_json(await producer())
        return self.saved[key]


class Model:
    def __init__(self, *, locate=None, associate=None, accept=True, mutate=None):
        self.locate, self.associate, self.accept, self.mutate = locate, associate, accept, mutate
        self.calls = []

    async def call(self, task, payload, images=()):
        assert task == "verify"
        self.calls.append(deepcopy(payload))
        operation = payload["operation"]
        if operation == "locate":
            value = {"schema_version": 2, "candidates": self.locate(payload) if self.locate else [], "findings": []}
        elif operation == "associate":
            value = (self.associate(payload) if self.associate else
                     {"schema_version": 2, "relations": [], "decisions": [], "findings": []})
        else:
            decisions = [entry["decision_id"] for entry in payload["decisions"]]
            value = {"schema_version": 2, "accepted_decision_ids": decisions if self.accept else [], "findings": []}
        if self.mutate:
            self.mutate(payload, value)
        return value


class ExtractModel:
    def __init__(self, result):
        self.result = result

    async def call(self, task, payload, images=()):
        assert task == "extract"
        return deepcopy(self.result)


async def scenario(tmp_path, *, documents=None, source_issues=(), analysis_issue=False, sections=None,
                   native_setup=None):
    artifacts = ArtifactStore(tmp_path)
    documents = documents or [
        ("main", "main", [("负责人应持A证。", "obligation"),
                           ("技术方案完整得5分。", "scoring_rule"),
                           ("未附A证复印件则否决投标。", "consequence_rule"),
                           ("人员表须填写姓名并附证书。", "submission_requirement")]),
        ("annex", "attachment", [("售后响应不超过2小时。", "obligation")]),
    ]
    frozen_docs, kinds = [], {}
    for ordinal, (document_id, role, facts) in enumerate(documents):
        index = source([{"text": text, "section": sections[number] if sections else f"s{number}",
                         "heading": f"章节{sections[number] if sections else number}"}
                        for number, (text, _) in enumerate(facts)], document_id=document_id)
        original = artifacts.put_bytes((document_id + " native source").encode(), "text/plain")
        index = SourceIndex({**index.document, "original_ref": original}, index.raw)
        if native_setup:
            native_setup(index)
            index = SourceIndex(index.document, index.raw)
        kinds.update({entry["range_id"]: kind for entry, (_, kind) in zip(index.ranges, facts, strict=True)})
        raw_ref = artifacts.put_json(index.raw)
        source_ref = artifacts.put_json(index.descriptor(raw_ref))
        frozen_docs.append({**index.document, "role": role, "ordinal": ordinal,
                            "file_name": f"{document_id}.txt", "raw_ref": raw_ref,
                            "source_index_ref": source_ref, "status": "complete", "issues": [],
                            "native_range_count": len(index.ranges)})
    count = sum(document["native_range_count"] for document in frozen_docs)
    frozen = {"schema_version": 2, "documents": frozen_docs, "issues": list(source_issues),
              "native_range_count": count, "stats": {"document_count": len(documents),
                  "native_range_count": count, "image_count": 0, "ocr_request_count": 0, "duration_seconds": 0.0}}
    source_problems = assign_issues(source_issues, "source")
    builder = partial(item_payload, identity=IDENTITY, issues=source_problems)
    plan = artifacts.read_json(await plan_sources(frozen, artifacts, budget=budget(), payload_builder=builder))
    repository, extracted = EvidenceRepository(artifacts), []
    for number, descriptor in enumerate(plan["packs"]):
        pack = repository.load_pack(descriptor)
        items, dispositions = [], []
        for entry in pack["body"]:
            ids = []
            if kinds[entry["range_id"]] is not None:
                item_id = f"{pack['pack_id']}:i{len(items) + 1}"
                ids.append(item_id)
                items.append({"item_id": item_id, "kind": kinds[entry["range_id"]], "text": entry["text"],
                              "evidence": [{"range_id": entry["range_id"], "quote": entry["text"]}]})
            dispositions.append({"range_id": entry["range_id"], "status": "items" if ids else "non_item",
                                 "reason": "原文规定本事项。" if ids else "本段仅说明修订动作。", "item_ids": ids})
        problems = ([{"code": "condition_unclear", "detail": "需核对本事项适用条件。", "blocking": True,
                      "range_id": items[0]["evidence"][0]["range_id"], "item_ids": [items[0]["item_id"]]}]
                    if analysis_issue and number == 0 else [])
        result = await extract_items(pack, ExtractModel({"schema_version": 2, "items": items,
            "dispositions": dispositions, "relations": [], "issues": problems}),
            identity=IDENTITY, issues=source_problems)
        extracted.append({"pack_id": pack["pack_id"], "artifact_ref": artifacts.put_json(result)})
    return artifacts, plan, {"schema_version": 2, "packs": extracted}, Receipts(artifacts)


async def run(data, model, admission=None):
    artifacts, plan, items, save = data
    return await verify_analysis(plan, items, artifacts, model, admission or budget(), identity=IDENTITY, save=save)


def reference_candidate(payload):
    if not payload["issues"]:
        return []
    return [{"candidate_id": payload["work_unit_id"] + ":c1",
             "issue_ids": [entry["issue_id"] for entry in payload["issues"]],
             "range_ids": range_ids(payload["source_views"]),
             "item_ids": [entry["item_id"] for entry in payload["items"]], "reason": "核对原文适用条件。"}]


def all_source_candidate(payload):
    if payload["work_unit_id"] != "discovery-1":
        return []
    return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
             "range_ids": [], "section_refs": [
                 {"document_id": document["document_id"], "section_id": section["section_id"]}
                 for document in payload["catalog"] for section in document["sections"]],
             "item_ids": [entry["item_id"] for entry in payload["items"]], "reason": "核对跨文档关联。"}]


def resolution(payload):
    sources = [source for view in payload["source_views"] for key in ("body", "supporting") for source in view[key]]
    original = next((problem["range_id"] for problem in payload["issues"] if problem.get("range_id")), None)
    source = next((source for source in sources if source["range_id"] == original), sources[0])
    return {"schema_version": 2, "relations": [], "decisions": [{
        "decision_id": payload["work_unit_id"] + ":d1",
        "issue_ids": [problem["issue_id"] for problem in payload["issues"]],
        "reason": "依据完整原文说明适用范围。",
        "evidence": [{"range_id": source["range_id"], "quote": source["text"]}],
    }], "findings": []}


@pytest.mark.asyncio
async def test_complete_source_analysis_keeps_four_business_kinds_and_original_artifacts(tmp_path):
    data = await scenario(tmp_path)
    artifacts, plan, items, _ = data
    before = deepcopy(items)
    model = Model()
    result = await run(data, model)
    assert result["schema_version"] == 2 and result["blocking"] is False and result["issues"] == []
    assert result["counts"]["items"] == 5 and result["counts"]["ranges"] == plan["range_count"]
    projection = current_items(artifacts, result)
    assert {entry["item"]["kind"] for entry in projection} == {
        "obligation", "scoring_rule", "consequence_rule", "submission_requirement"}
    assert items == before
    assert all(artifacts.read_json(result[key])["schema_version"] == 2 for key in (
        "review_plan_ref", "locate_plan_ref", "association_plan_ref", "joint_plan_ref"))
    assert len(result["operation_refs"]) == len(model.calls)
    assert all(payload["source_version"] == 1 and payload["domain_revision"] == 1 for payload in model.calls)


@pytest.mark.asyncio
@pytest.mark.parametrize("complete", [False, True])
async def test_review_views_keep_document_native_order_and_fragment_character_order(tmp_path, complete):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    expected = {}
    selected = []
    for document_id, index in context.indices.items():
        expected[document_id] = [source["range_id"] for source in index.ranges]
        selected.extend(reversed(expected[document_id]))
    views = context.views(reversed(selected), complete=complete)
    assert [view["document_id"] for view in views] == [document["document_id"] for document in data[1]["documents"]]
    assert {view["document_id"]:[source["range_id"] for source in view["body"]] for view in views} == expected
    native = context.indices["main"].ranges[0]
    middle = max(1, len(native["text"])//2)
    first, second = fragment(native, 0, middle), fragment(native, middle, native["end"])
    context.sources.update({entry["range_id"]:entry for entry in [second, first]})
    view = context.views([second["range_id"],first["range_id"]], complete=False)[0]
    assert [source["range_id"] for source in view["body"]] == [first["range_id"],second["range_id"]]
    # Pack context can arrive in any receipt order; supporting keeps native order too.
    pack = context.owners[native["range_id"]]
    pack["supporting"] = list(reversed(context.indices["main"].ranges[1:]))
    view = context.views([native["range_id"]], complete=complete)[0]
    assert [source["range_id"] for source in view["supporting"]] == expected["main"][1:]


@pytest.mark.asyncio
@pytest.mark.parametrize("key", ["body", "supporting"])
async def test_table_evidence_uses_only_sent_cell_text_or_explicit_row_text(tmp_path, key):
    data = await scenario(tmp_path, documents=[("main","main",[("姓名 | 证书", "obligation")])])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    original = context.indices["main"].ranges[0]
    row = {"range_id":original["range_id"],"cells":[{"column":0,"text":"姓名"},{"column":1,"text":"证书"}]}
    view = {"body":[],"supporting":[],key:[row]}
    payload = context.payload("associate", "associate-1", source_views=[view])

    def cited(quote):
        return [{"evidence":[{"range_id":original["range_id"],"quote":quote}]}]

    context.evidence(cited("证书"),payload)
    with pytest.raises(ModelSemanticError,match="exact supplied source substring"):
        context.evidence(cited(original["text"]),payload)
    row["text"] = original["text"]
    context.evidence(cited(original["text"]),payload)


@pytest.mark.asyncio
async def test_table_alias_cannot_reintroduce_an_unsent_canonical_row(tmp_path):
    data = await scenario(tmp_path, documents=[("main","main",[("姓名 | 证书", "obligation")])])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    original = context.indices["main"].ranges[0]
    alias = fragment(original,0,original["end"]-1)
    context.sources[alias["range_id"]] = alias
    row = {"range_id":original["range_id"],"cells":[{"column":0,"text":"姓名"},{"column":1,"text":"证书"}]}
    view = {"body":[row],"supporting":[],"source_aliases":[{"range_id":alias["range_id"],
        "segments":[{"range_id":original["range_id"],"start":alias["start"],"end":alias["end"]}]}]}
    payload = context.payload("associate", "associate-1", source_views=[view])

    def cited(quote):
        return [{"evidence":[{"range_id":alias["range_id"],"quote":quote}]}]

    context.evidence(cited("姓名"),payload)
    with pytest.raises(ModelSemanticError,match="exact supplied source substring"):
        context.evidence(cited(alias["text"]),payload)
    with pytest.raises(ModelSemanticError,match="exact supplied source substring"):
        context.evidence(cited("证书"),payload)


@pytest.mark.asyncio
async def test_text_alias_can_quote_across_contiguous_sent_segments(tmp_path):
    data = await scenario(tmp_path, documents=[("main","main",[("完整人员条件原文。", "obligation")])])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    original = context.indices["main"].ranges[0]
    middle = len(original["text"])//2
    first,second = fragment(original,0,middle),fragment(original,middle,original["end"])
    view = {"body":[first,second],"supporting":[],"source_aliases":[{"range_id":original["range_id"],
        "segments":[{"range_id":entry["range_id"],"start":entry["start"],"end":entry["end"]}
                    for entry in [first,second]]}]}
    payload = context.payload("associate", "associate-1", source_views=[view])
    context.evidence([{"evidence":[{"range_id":original["range_id"],"quote":original["text"]}]}],payload)


def physical_pdf(index, *, unit_pages=(0,1), page_count=2, partial_unit=None, missing_page=None):
    index.document["original_ref"]["media_type"] = "application/pdf"
    index.raw["metadata"] = {"page_count":page_count}
    for unit, identity, page in zip(index.raw["units"],index.raw["source_contract"]["units"],unit_pages,strict=True):
        # PDF logical section locators need not carry physical page positions.
        unit["locator"].pop("page_ordinal",None)
        identity["physical_locator"] = {"locator_kind":"page", "page_ordinal":page}
        identity["completeness"] = "partial" if identity["unit_id"] == partial_unit else "complete"
    index.raw["source_contract"]["page_manifest"] = [
        {"page_ordinal":page,"classification":"text",
         "unit_ids":[unit["unit_id"] for unit, position in zip(index.raw["units"],unit_pages,strict=True)
                     if position==page], "image_unit_ids":[]}
        for page in range(page_count) if page != missing_page]


def page_candidate(payload, page=1, document="main"):
    if payload["work_unit_id"] != "discovery-1":
        return []
    return [{"candidate_id":"discovery-1:c1","issue_ids":[],"range_ids":[],"item_ids":[],
             "page_refs":[{"document_id":document,"page_ordinal":page}], "reason":"核对明确物理PDF页原文。"}]


def pdf_image_page(index, *, missing=False, separate_list=False, status="complete", issues=()):
    physical_pdf(index)
    page = index.raw["source_contract"]["page_manifest"][1]
    page["image_unit_ids"] = ["missing:image" if missing else "unit:1"]
    if missing:
        return
    unit = index.raw["units"][1]
    unit["kind"] = "image_region"
    unit["locator"] = {"locator_kind":"image","image_key":"page-image"}
    index.raw["ocr"][unit["unit_id"]] = {"status":status,"text":unit["text"],"issues":list(issues)}
    if separate_list:
        page["unit_ids"].remove(unit["unit_id"])
        index.raw["source_contract"]["units"][1]["physical_locator"].pop("page_ordinal")


@pytest.mark.asyncio
async def test_page_image_manifest_loads_image_even_without_a_repeated_page_locator(tmp_path):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("图片原文。",None)])],
                          native_setup=partial(pdf_image_page,separate_list=True))
    model = Model(locate=page_candidate)
    result = await run(data,model)
    association = next(payload for payload in model.calls if payload["operation"]=="associate")
    assert association["source_views"][0]["body"][0]["text"] == "图片原文。"
    assert result["blocking"] is False


@pytest.mark.asyncio
@pytest.mark.parametrize("native_setup", [
    partial(pdf_image_page,missing=True), partial(pdf_image_page,status="unresolved"),
    partial(pdf_image_page,issues=[{"code":"ocr_incomplete","detail":"图片内容未完整辨认。","blocking":True}]),
], ids=["missing-image","unfinished-ocr","ocr-issue"])
async def test_page_image_missing_or_incomplete_never_reaches_association(tmp_path,native_setup):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("图片原文。",None)])],
                          native_setup=native_setup)
    model = Model(locate=page_candidate)
    result = await run(data,model)
    assert result["blocking"] is True
    assert any(problem["code"]=="analysis_page_source_unavailable" for problem in result["issues"])
    assert not any(payload["operation"]=="associate" for payload in model.calls)


@pytest.mark.asyncio
async def test_physical_page_candidate_loads_complete_page_and_reuses_it_for_joint_review(tmp_path):
    data = await scenario(tmp_path, documents=[("main","main",[
        ("首物理页原文。", "obligation"),("第二物理页甲要求。", "obligation"),
        ("第二物理页补充说明。",None)])],
        native_setup=partial(physical_pdf,unit_pages=(0,1,1)))
    catalog = build_catalog(data[1],data[0])
    assert catalog[0]["page_count"] == 2
    model = Model(locate=page_candidate)
    result = await run(data,model)
    associated = next(payload for payload in model.calls if payload["operation"] == "associate")
    assert [source["text"] for view in associated["source_views"] for source in view["body"]] == [
        "第二物理页甲要求。","第二物理页补充说明。"]
    joint = next(payload for payload in model.calls if payload["work_unit_id"].startswith("joint-"))
    assert joint["source_views"] == associated["source_views"]
    assert len(associated["dispositions"]) == 2 and result["blocking"] is False


@pytest.mark.asyncio
@pytest.mark.parametrize("null_field", ["section_refs","page_refs"])
async def test_unused_optional_candidate_selectors_reject_explicit_null(tmp_path,null_field):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("第二页。",None)])],
                          native_setup=physical_pdf)

    def candidate(payload):
        select = page_candidate if null_field == "section_refs" else all_source_candidate
        return [{**entry,null_field:None} for entry in select(payload)]

    with pytest.raises(ModelSemanticError,match="host contract"):
        await run(data,Model(locate=candidate))


@pytest.mark.asyncio
@pytest.mark.parametrize("page,document", [(2,"main"),(0,"absent")])
async def test_page_candidates_reject_out_of_range_or_unuploaded_document(tmp_path,page,document):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("第二页。",None)])],
                          native_setup=physical_pdf)
    with pytest.raises(ModelSemanticError,match="host contract"):
        await run(data,Model(locate=partial(page_candidate,page=page,document=document)))


@pytest.mark.asyncio
async def test_page_candidate_requires_a_complete_pdf_page_contract(tmp_path):
    data = await scenario(tmp_path)
    assert all("page_count" not in document for document in build_catalog(data[1],data[0]))
    with pytest.raises(ModelSemanticError,match="host contract"):
        await run(data,Model(locate=page_candidate))


@pytest.mark.asyncio
async def test_missing_page_manifest_is_not_advertised_for_page_selection(tmp_path):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("第二页。",None)])],
                          native_setup=partial(physical_pdf,missing_page=1))
    assert "page_count" not in build_catalog(data[1],data[0])[0]


@pytest.mark.asyncio
@pytest.mark.parametrize("native_setup", [partial(physical_pdf,partial_unit="unit:1"),
                                         partial(physical_pdf,unit_pages=(0,0))], ids=["partial","missing"])
async def test_missing_or_partial_physical_page_never_reaches_association_or_joint_model(tmp_path,native_setup):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("不完整第二页。",None)])],
                          native_setup=native_setup)
    model = Model(locate=page_candidate)
    result = await run(data,model)
    assert result["blocking"] is True
    assert any(problem["code"]=="analysis_page_source_unavailable" for problem in result["issues"])
    assert not any(payload["operation"]=="associate" for payload in model.calls)


@pytest.mark.asyncio
async def test_complete_page_candidate_is_indivisible_when_its_budget_does_not_fit(tmp_path):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("第二页。",None)])],
                          native_setup=physical_pdf)
    model = Model(locate=page_candidate)
    result = await run(data,model,OperationBudget(lambda payload:payload["operation"]=="associate"))
    requests = data[0].read_json(result["association_plan_ref"])["requests"]
    assert len(requests)==1 and requests[0]["budget"]["fits"] is False
    assert result["blocking"] is True and not any(payload["operation"]=="associate" for payload in model.calls)


@pytest.mark.asyncio
async def test_catalog_contains_document_identity_once_and_only_locate_receives_it(tmp_path):
    data = await scenario(tmp_path)
    catalog = build_catalog(data[1], data[0])
    assert len(catalog) == 2
    assert {entry["document_id"] for entry in catalog} == {"main", "annex"}
    assert sum(len(entry["sections"]) for entry in catalog) == 5
    assert all(set(section) == {"section_id", "heading_path"}
               for entry in catalog for section in entry["sections"])
    model = Model()
    await run(data, model)
    assert all(("catalog" in payload) == (payload["operation"] == "locate") for payload in model.calls)


@pytest.mark.asyncio
async def test_section_candidate_loads_every_original_range_and_non_item_content(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("人员应持A证。", "obligation"), ("本节说明证书审核口径。", None), ("另一章节。", None)])],
        sections=["same", "same", "other"])

    def candidate(payload):
        if payload["work_unit_id"] != "discovery-1":
            return []
        return [{"candidate_id": "discovery-1:c1", "issue_ids": [], "range_ids": [], "item_ids": [],
                 "section_refs": [{"document_id": "main", "section_id": "same"}],
                 "reason": "联合补读本节原文和口径说明。"}]

    model = Model(locate=candidate)
    result = await run(data, model)
    associated = next(payload for payload in model.calls if payload["operation"] == "associate")
    assert {source["text"] for view in associated["source_views"] for source in view["body"]} == {
        "人员应持A证。", "本节说明证书审核口径。"}
    assert len(associated["items"]) == 1
    assert len(associated["dispositions"]) == 2
    joint = next(payload for payload in model.calls if payload["work_unit_id"].startswith("joint-"))
    assert associated["source_views"] == joint["source_views"]
    assert result["blocking"] is False


@pytest.mark.asyncio
@pytest.mark.parametrize("document,section", [("other", "s0"), ("main", "absent"), ("main", None)])
async def test_section_candidates_must_use_catalog_identities_supplied_to_locate(tmp_path, document, section):
    data = await scenario(tmp_path)

    def candidate(payload):
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": [], "item_ids": [],
                 "section_refs": [{"document_id": document, "section_id": section}],
                 "reason": "尝试补读章节。"}]

    with pytest.raises(ModelSemanticError, match="host contract"):
        await run(data, Model(locate=candidate))


@pytest.mark.asyncio
async def test_locate_cannot_guess_an_existing_but_unsupplied_range_id(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("人员应持A证。", "obligation"), ("未在发现请求提供的描述原文。", None)])])
    repository = EvidenceRepository(data[0])
    hidden = next(source["range_id"] for source in repository.source_index(
        data[1]["documents"][0]["source_index_ref"]).ranges if source["text"].startswith("未在"))

    def candidate(payload):
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": [hidden], "item_ids": [], "reason": "猜测未装载原文身份。"}]

    with pytest.raises(ModelSemanticError, match="host contract"):
        await run(data, Model(locate=candidate))


@pytest.mark.asyncio
async def test_complete_section_candidate_is_not_split_to_fit_association_budget(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("人员应持A证。", "obligation"), ("本节说明证书审核口径。", None)])], sections=[None, None])

    def candidate(payload):
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": [], "item_ids": [], "section_refs": [{"document_id": "main", "section_id": None}],
                 "reason": "补读完整无标题章节。"}]

    model = Model(locate=candidate)
    result = await run(data, model, OperationBudget(lambda payload: payload["operation"] == "associate"))
    requests = data[0].read_json(result["association_plan_ref"])["requests"]
    assert len(requests) == 1 and len(requests[0]["range_ids"]) == 2
    assert result["blocking"] is True
    assert not any(payload["operation"] == "associate" for payload in model.calls)


@pytest.mark.asyncio
@pytest.mark.parametrize("field", ["reviewed_range_ids", "reviewed_item_ids"])
async def test_model_cannot_override_host_source_or_item_coverage(tmp_path, field):
    data = await scenario(tmp_path)

    def omit(payload, value):
        if payload["operation"] == "review":
            value[field] = []

    result = await run(data, Model(mutate=omit))
    assert not any(issue["code"] == "analysis_review_incomplete" for issue in result["issues"])


@pytest.mark.asyncio
async def test_source_finding_survives_localization_without_an_accepted_decision(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)
    model = Model()
    result = await run(data, model)
    assert result["blocking"] is True and result["resolved_issue_ids"] == []
    assert any(problem["code"] == "condition_unclear" for problem in result["issues"])
    assert not any(payload["operation"] == "associate" for payload in model.calls)


@pytest.mark.asyncio
@pytest.mark.parametrize("accept", [False, True])
async def test_only_an_independently_accepted_source_decision_closes_a_model_issue(tmp_path, accept):
    data = await scenario(tmp_path, analysis_issue=True)
    model = Model(locate=reference_candidate, associate=resolution, accept=accept)
    result = await run(data, model)
    assert result["blocking"] is not accept
    assert bool(result["resolved_issue_ids"]) is accept
    assert bool(result["accepted_decision_ids"]) is accept
    assert any(payload["operation"] == "review" and payload["decisions"] for payload in model.calls)
    assert any(problem["code"] == "condition_unclear" for problem in result["all_issues"])
    assert any(problem["code"] == "condition_unclear" for problem in result["issues"]) is not accept


@pytest.mark.asyncio
@pytest.mark.parametrize("field", ["reviewed_relation_ids", "reviewed_decision_ids"])
async def test_model_cannot_override_host_joint_review_coverage(tmp_path, field):
    data = await scenario(tmp_path, analysis_issue=True)

    def candidate(payload):
        candidates = reference_candidate(payload)
        if candidates:
            candidates[0]["section_refs"] = [
                {"document_id": document["document_id"], "section_id": section["section_id"]}
                for document in payload["catalog"] if document["document_id"] == "main"
                for section in document["sections"][:2]]
        return candidates

    def associated(payload):
        value = resolution(payload)
        source = payload["source_views"][0]["body"][0]
        relation_id = payload["work_unit_id"] + ":l1"
        value["relations"] = [{"relation_id": relation_id,
            "from_item_id": payload["items"][0]["item_id"], "to_item_id": payload["items"][1]["item_id"],
            "relation": "本评分规则与人员资质相关联。",
            "evidence": [{"range_id": source["range_id"], "quote": source["text"]}]}]
        value["decisions"][0]["relation_ids"] = [relation_id]
        return value

    def omit(payload, value):
        if payload["work_unit_id"].startswith("joint-"):
            value[field] = []
            value["accepted_decision_ids"] = []

    result = await run(data, Model(locate=candidate, associate=associated, mutate=omit))
    assert not any(issue["code"] == "analysis_review_incomplete" for issue in result["issues"])
    assert not result["accepted_decision_ids"]


@pytest.mark.asyncio
async def test_joint_blocking_findings_prevent_acceptance_even_if_model_lists_the_decision(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)

    def finding(payload, value):
        if payload["work_unit_id"].startswith("joint-"):
            value["findings"] = [{"code": "condition_unclear", "detail": "仍未解释适用范围。", "blocking": False}]

    result = await run(data, Model(locate=reference_candidate, associate=resolution, mutate=finding))
    assert result["blocking"] is True and result["accepted_decision_ids"] == []
    assert result["resolved_issue_ids"] == []
    assert any(problem["code"] == "decision_not_accepted" for problem in result["issues"])


@pytest.mark.asyncio
async def test_accepted_decision_cannot_erase_a_host_source_blocker(tmp_path):
    host = {"code": "source_partial", "detail": "必要附件仍不可读。", "blocking": True}
    data = await scenario(tmp_path, source_issues=[host])

    def candidate(payload):
        if not payload["issues"]:
            return []
        return [{"candidate_id": payload["work_unit_id"] + ":c1",
                 "issue_ids": [problem["issue_id"] for problem in payload["issues"]],
                 "range_ids": [], "item_ids": [], "section_refs": [{
                     "document_id": payload["catalog"][0]["document_id"],
                     "section_id": payload["catalog"][0]["sections"][0]["section_id"]}],
                 "reason": "申请核对来源问题。"}]

    result = await run(data, Model(locate=candidate, associate=resolution))
    assert result["blocking"] is True and result["resolved_issue_ids"] == []
    assert result["accepted_decision_ids"] == []
    assert any(problem["code"] == "source_partial" for problem in result["issues"])


@pytest.mark.asyncio
@pytest.mark.parametrize("effect", ["replace", "delete"])
async def test_partial_amendment_projects_only_reviewed_retained_rules_and_keeps_originals(tmp_path, effect):
    documents = [
        ("main", "main", [("交付期限30天得2分，保修期限3年得3分，最高5分。", "scoring_rule")]),
        ("change", "amendment", [("交付期限改为20天得2分，保修条款不变。" if effect == "replace" else
                                    "删除交付期限评分，保修条款不变。", "scoring_rule" if effect == "replace" else None)]),
    ]
    data = await scenario(tmp_path, documents=documents)
    artifacts, _, items, _ = data
    original_pack = artifacts.read_json(items["packs"][0]["artifact_ref"])
    original_pack["items"][0]["scoring"] = {"max_points": 5}
    items["packs"][0]["artifact_ref"] = artifacts.put_json(original_pack)
    before = [artifacts.read_json(entry["artifact_ref"]) for entry in items["packs"]]

    def review_deletion(payload, value):
        # A deletion adds no item. Full-source review raises its unresolved
        # target; reference localization retains the complete document catalog.
        if effect == "delete" and payload["work_unit_id"].startswith("review-"):
            for view in payload["source_views"]:
                if view["document_id"] == "change":
                    value["findings"].append({"code": "amendment_target_unresolved", "blocking": True,
                        "range_id": view["body"][0]["range_id"], "detail": "删除交付期限评分，需核对原评分条款。"})

    def locate(payload):
        if effect != "delete":
            return all_source_candidate(payload)
        if not payload["issues"]:
            return []
        candidates = all_source_candidate({**payload, "work_unit_id": "discovery-1"})
        candidates[0].update(candidate_id=payload["work_unit_id"] + ":c1",
                             issue_ids=[p["issue_id"] for p in payload["issues"]])
        return candidates

    def amendment(payload):
        target = payload["items"][0]
        original = target["evidence"][0]
        changed = next(view["body"][0] for view in payload["source_views"] if view["document_id"] == "change")
        return {"schema_version": 2, "relations": [], "findings": [], "decisions": [{
            "decision_id": payload["work_unit_id"] + ":d1",
            "issue_ids": [p["issue_id"] for p in payload["issues"]],
            "reason": "补遗仅修改交付条款并明确保留保修条款。",
            "evidence": [original, {"range_id": changed["range_id"], "quote": changed["text"]}],
            "amendment": {"target_item_id": target["item_id"], "effect": effect,
                "affected_scope": "交付期限评分", "retained_rules": [{"text": "保修期限3年得3分。", "scope": "原保修评分条款",
                    "evidence": [{"range_id": original["range_id"], "quote": "保修期限3年得3分"}]}],
                "replacement_item_ids": [payload["items"][1]["item_id"]] if effect == "replace" else []},
        }]}

    result = await run(data, Model(locate=locate, associate=amendment, mutate=review_deletion))
    assert result["blocking"] is False and len(result["accepted_decision_ids"]) == 1
    projection = current_items(artifacts, result)
    retained = [entry["effective_rule"] for entry in projection if "effective_rule" in entry]
    assert [entry["text"] for entry in retained] == ["保修期限3年得3分。"]
    assert set(retained[0]) == {"text", "scope", "evidence"}
    assert len(projection) == (2 if effect == "replace" else 1)
    assert [artifacts.read_json(entry["artifact_ref"]) for entry in items["packs"]] == before


@pytest.mark.asyncio
@pytest.mark.parametrize("invalid_evidence", ["invented_quote", "missing_quote", "empty", "null"])
async def test_association_keeps_valid_siblings_and_sends_untraceable_declarations_to_joint_review(tmp_path, invalid_evidence):
    data = await scenario(tmp_path)
    returned = []

    def unsupported(payload):
        citation = deepcopy(payload["items"][0]["evidence"][0])
        evidence = {"invented_quote": [{**citation, "quote": "原文没有这句"}],
                    "missing_quote": [{"range_id": citation["range_id"]}], "empty": [], "null": None}[invalid_evidence]
        valid = {"relation_id": payload["work_unit_id"] + ":l2",
                 "from_item_id": payload["items"][0]["item_id"],
                 "to_item_id": payload["items"][1]["item_id"], "relation": "有效的来源关联。",
                 "evidence": [citation]}
        value = {"schema_version": 2, "decisions": [], "findings": [], "relations": [{
            "relation_id": payload["work_unit_id"] + ":l1", "from_item_id": payload["items"][0]["item_id"],
            "to_item_id": payload["items"][1]["item_id"], "relation": "需要核对的适用条件关联。",
            "evidence": evidence,
        }, valid]}
        returned.append((value, deepcopy(value)))
        return value

    model = Model(locate=all_source_candidate, associate=unsupported)
    result = await run(data, model)
    assert result["blocking"] is True and result["coverage"]["missing"] == {}
    assert [r["relation"] for r in data[0].read_json(result["relations_ref"])] == ["有效的来源关联。"]
    problem = next(p for p in result["issues"] if p["code"] == "analysis_association_unresolved")
    assert "relations[0]" in problem["detail"] and "需要核对的适用条件关联。" in problem["detail"]
    joint = next(p for p in model.calls if p["work_unit_id"].startswith("joint-"))
    assert any(p["code"] == "analysis_association_unresolved" for p in joint["issues"])
    assert set(problem["item_ids"]) <= {item["item_id"] for item in joint["items"]}
    assert all(value == original for value, original in returned)


@pytest.mark.asyncio
async def test_association_restores_linebreaks_in_relations_decisions_and_retained_rules_without_mutating_raw(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [("负责人应持\nA证。", "obligation"),
                                                               ("评分\n条款。", "scoring_rule")])], analysis_issue=True)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    items = list(context.items.values())
    sources = [e["range_id"] for item in items for e in item["evidence"]]
    payload = context.payload("associate", "associate-1", source_views=context.views(sources),
                              items=items, issues=context.problems)
    citation = {**items[0]["evidence"][0], "quote": items[0]["evidence"][0]["quote"].replace("\n", "")}
    raw = {"schema_version": 2, "relations": [{"relation_id": "associate-1:l1",
        "from_item_id": items[0]["item_id"], "to_item_id": items[1]["item_id"], "relation": "两事项有关联。",
        "evidence": [citation]}], "decisions": [{"decision_id": "associate-1:d1",
        "issue_ids": [p["issue_id"] for p in context.problems], "reason": "核对来源并保留原条款。",
        "relation_ids": ["associate-1:l1"], "evidence": [citation], "amendment": {
            "target_item_id": items[0]["item_id"], "effect": "delete", "affected_scope": "部分原条款",
            "replacement_item_ids": [], "retained_rules": [{"text": items[0]["text"], "scope": "原条款保留部分",
                                                             "evidence": [citation]}]}}], "findings": []}
    before = deepcopy(raw)
    result = context.validate(raw, payload)
    assert raw == before and result["findings"] == []
    assert {d["path"] for d in result["diagnostics"]} == {
        "relations[0].evidence[0].quote", "decisions[0].evidence[0].quote",
        "decisions[0].amendment.retained_rules[0].evidence[0].quote"}
    assert all(d["code"] == "evidence_quote_restored" and d["start"] == 0
               and d["end"] == len(items[0]["evidence"][0]["quote"]) for d in result["diagnostics"])
    assert result["decisions"][0]["evidence"][0]["quote"] == items[0]["evidence"][0]["quote"]


@pytest.mark.asyncio
async def test_untraceable_decision_cannot_close_existing_issue_and_joint_receives_pending_content(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)

    def unsupported(payload):
        value = resolution(payload)
        value["decisions"][0]["evidence"][0]["quote"] = "不存在的决议引文"
        return value

    model = Model(locate=reference_candidate, associate=unsupported)
    result = await run(data, model)
    assert result["blocking"] is True and result["accepted_decision_ids"] == []
    assert result["resolved_issue_ids"] == [] and data[0].read_json(result["decisions_ref"]) == []
    assert any(p["code"] == "condition_unclear" for p in result["issues"])
    joint = next(p for p in model.calls if p["work_unit_id"].startswith("joint-"))
    assert any("不存在的决议引文" in p["detail"] for p in joint["issues"])


@pytest.mark.asyncio
@pytest.mark.parametrize("invalid", ["quote", "missing_quote", "rejected_relation"])
async def test_association_keeps_valid_decision_when_another_declaration_cannot_be_traced(tmp_path, invalid):
    data = await scenario(tmp_path, analysis_issue=True)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    items = list(context.items.values())[:2]
    ranges = {e["range_id"] for item in items for e in item["evidence"]}
    payload = context.payload("associate", "associate-1", source_views=context.views(ranges),
                              items=items, issues=context.problems)
    raw = resolution(payload)
    valid = {**deepcopy(raw["decisions"][0]), "decision_id": "associate-1:d2"}
    if invalid == "quote":
        raw["decisions"][0]["evidence"][0]["quote"] = "原文无此引文"
    elif invalid == "missing_quote":
        del raw["decisions"][0]["evidence"][0]["quote"]
    else:
        raw["relations"] = [{"relation_id": "associate-1:l1", "from_item_id": items[0]["item_id"],
            "to_item_id": items[1]["item_id"], "relation": "证据尚不成立的关联。",
            "evidence": [{**items[0]["evidence"][0], "quote": "原文无此引文"}]}]
        raw["decisions"][0]["relation_ids"] = ["associate-1:l1"]
    raw["decisions"].append(valid)
    before = deepcopy(raw)
    result = context.validate(raw, payload)
    assert raw == before and [d["decision_id"] for d in result["decisions"]] == ["associate-1:d2"]
    assert result["relations"] == []
    assert any("decisions[0]" in finding["detail"] for finding in result["findings"])


@pytest.mark.asyncio
async def test_review_payload_includes_other_items_named_by_a_local_source_issue(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    first, second = list(context.items.values())[:2]
    original = first["evidence"][0]["range_id"]
    context.problems.extend(assign_issues([{"code": "analysis_association_unresolved",
        "detail": "两个事项之间的来源关系待核对。", "blocking": True, "range_id": original,
        "item_ids": [first["item_id"], second["item_id"]]}], "test"))
    payload = context.review_payload([original], "review-1")
    assert {item["item_id"] for item in payload["items"]} == {first["item_id"], second["item_id"]}
    assert {e["range_id"] for item in (first, second) for e in item["evidence"]} <= set(range_ids(payload["source_views"]))
    assert [d["range_id"] for d in payload["dispositions"]] == [original]


@pytest.mark.asyncio
@pytest.mark.parametrize("missing", ["target", "replacement"])
async def test_amendment_decision_must_cite_target_and_each_replacement_source(tmp_path, missing):
    data = await scenario(tmp_path)

    def one_sided(payload):
        target, replacement = payload["items"][0], payload["items"][-1]
        cited = replacement if missing == "target" else target
        return {"schema_version": 2, "relations": [], "findings": [], "decisions": [{
            "decision_id": payload["work_unit_id"] + ":d1", "issue_ids": [],
            "reason": "拟按补遗替换条款。", "evidence": deepcopy(cited["evidence"]),
            "amendment": {"target_item_id": target["item_id"], "effect": "replace",
                "affected_scope": "当前事项", "retained_rules": [], "replacement_item_ids": [replacement["item_id"]]},
        }]}

    result = await run(data, Model(locate=all_source_candidate, associate=one_sided))
    assert result["blocking"] is True and result["accepted_decision_ids"] == []
    assert data[0].read_json(result["decisions_ref"]) == []
    assert any(p["code"] == "analysis_association_unresolved" for p in result["issues"])


@pytest.mark.asyncio
async def test_overlapping_amendment_effects_require_a_single_reviewed_net_decision(tmp_path):
    data = await scenario(tmp_path)

    def competing(payload):
        evidence = deepcopy(payload["items"][0]["evidence"])
        return {"schema_version": 2, "relations": [], "findings": [], "decisions": [{
            "decision_id": payload["work_unit_id"] + f":d{number}", "issue_ids": [],
            "reason": "核对补遗删除条款。", "evidence": evidence,
            "amendment": {"target_item_id": payload["items"][0]["item_id"], "effect": "delete",
                          "affected_scope": "当前事项", "retained_rules": [], "replacement_item_ids": []},
        } for number in (1, 2)]}

    result = await run(data, Model(locate=all_source_candidate, associate=competing))
    assert result["blocking"] is True and result["accepted_decision_ids"] == []
    assert any(problem["code"] == "amendment_effect_unresolved" for problem in result["issues"])
    assert len(current_items(data[0], result)) == 5


@pytest.mark.asyncio
async def test_localization_cannot_invent_a_source_identity(tmp_path):
    data = await scenario(tmp_path)

    def invented(payload):
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": ["not-a-frozen-range"], "item_ids": [], "reason": "尝试定位关联。"}]

    with pytest.raises(ModelSemanticError, match="host contract"):
        await run(data, Model(locate=invented))


@pytest.mark.asyncio
async def test_indivisible_budget_failure_is_blocking_and_sends_no_model_request(tmp_path):
    data = await scenario(tmp_path)
    model = Model()
    result = await run(data, model, budget(500))
    assert result["blocking"] is True and model.calls == []
    assert any(problem["code"] == "analysis_budget_exceeded" for problem in result["issues"])


class OperationBudget:
    def __init__(self, reject):
        self.original, self.reject = budget(), reject

    def spec(self):
        return {**self.original.spec(), "test_admission": "operation"}

    def measure(self, task, payload):
        measurement = self.original.measure(task, payload)
        return {**measurement, "fits": measurement["fits"] and not self.reject(payload)}


@pytest.mark.asyncio
async def test_connected_candidate_is_not_split_when_its_complete_source_exceeds_capacity(tmp_path):
    data = await scenario(tmp_path)
    model = Model(locate=all_source_candidate)
    admission = OperationBudget(lambda payload: payload["operation"] == "associate")
    result = await run(data, model, admission)
    plan = data[0].read_json(result["association_plan_ref"])
    assert len(plan["requests"]) == 1 and plan["requests"][0]["budget"]["fits"] is False
    assert result["blocking"] is True and result["hard_blocking"] is True
    assert not any(payload["operation"] == "associate" for payload in model.calls)


@pytest.mark.asyncio
async def test_shared_rule_does_not_merge_independent_candidates_into_an_oversized_request(tmp_path):
    data = await scenario(tmp_path)
    expected = []

    def candidates(payload):
        if payload["work_unit_id"] != "discovery-1":
            return []
        common, first, second = payload["items"][:3]
        expected.extend([{common["item_id"], target["item_id"]} for target in (first, second)])
        return [{"candidate_id": payload["work_unit_id"] + f":c{number}", "issue_ids": [],
                 "range_ids": [entry["evidence"][0]["range_id"] for entry in (common, target)],
                 "item_ids": [common["item_id"], target["item_id"]], "reason": "分别核对公共规则的适用对象。"}
                for number, target in enumerate((first, second), start=1)]

    model = Model(locate=candidates)
    admission = OperationBudget(lambda payload: payload["operation"] == "associate" and len(payload["items"]) > 2)
    result = await run(data, model, admission)
    association = data[0].read_json(result["association_plan_ref"])
    assert len(association["requests"]) == 2
    assert [{item["item_id"] for item in payload["items"]}
            for payload in model.calls if payload["operation"] == "associate"] == expected
    assert result["blocking"] is False


@pytest.mark.asyncio
async def test_candidates_for_the_same_issue_still_load_all_proposed_targets_together(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)

    def candidates(payload):
        if not payload["issues"]:
            return []
        problem = payload["issues"][0]
        targets = [{"document_id": document["document_id"], "section_id": section["section_id"]}
                   for document in payload["catalog"] if document["document_id"] == "main"
                   for section in document["sections"][1:3]]
        return [{"candidate_id": payload["work_unit_id"] + f":c{number}",
                 "issue_ids": [problem["issue_id"]], "range_ids": [problem["range_id"]], "section_refs": [target],
                 "item_ids": [item["item_id"] for item in payload["items"]], "reason": "同一未决引用的候选目标。"}
                for number, target in enumerate(targets, start=1)]

    model = Model(locate=candidates, associate=resolution)
    result = await run(data, model)
    associated = [payload for payload in model.calls if payload["operation"] == "associate"]
    assert len(associated) == 1 and len(associated[0]["candidates"]) == 2
    supplied = set(range_ids(associated[0]["source_views"]))
    assert {identity for candidate in associated[0]["candidates"] for identity in candidate["range_ids"]} <= supplied
    assert len(associated[0]["items"]) == 3
    assert result["blocking"] is False


@pytest.mark.asyncio
async def test_resolution_cannot_erase_an_unperformed_independent_source_review(tmp_path):
    data = await scenario(tmp_path)

    def candidate(payload):
        if not payload["issues"]:
            return []
        return [{"candidate_id": payload["work_unit_id"] + ":c1",
                 "issue_ids": [problem["issue_id"] for problem in payload["issues"]],
                 "range_ids": [], "item_ids": [], "section_refs": [{
                     "document_id": payload["catalog"][0]["document_id"],
                     "section_id": payload["catalog"][0]["sections"][0]["section_id"]}],
                 "reason": "核对本问题的关联原文。"}]

    admission = OperationBudget(lambda payload: payload["operation"] == "review"
                                and payload["work_unit_id"].startswith("review-"))
    result = await run(data, Model(locate=candidate, associate=resolution), admission)
    assert result["blocking"] is True
    assert any(problem["code"] == "analysis_budget_exceeded" for problem in result["issues"])


@pytest.mark.asyncio
async def test_discovery_capacity_blocks_preserve_every_item_pair_without_keyword_filters(tmp_path):
    data = await scenario(tmp_path)
    model = Model()
    admission = OperationBudget(lambda payload: payload["operation"] == "locate" and len(payload["items"]) > 2)
    result = await run(data, model, admission)
    groups = [{entry["item_id"] for entry in payload["items"]}
              for payload in model.calls if payload["operation"] == "locate"]
    identities = {entry["item_id"] for entry in result["item_refs"]}
    assert set.union(*groups) == identities
    assert all(any(set(pair) <= group for group in groups) for pair in combinations(identities, 2))
    assert result["blocking"] is False


@pytest.mark.asyncio
async def test_recovery_reuses_completed_review_and_localization_receipts(tmp_path):
    data = await scenario(tmp_path)
    failures = [True]

    def fail_once(payload):
        if failures:
            failures.pop()
            raise ModelInfrastructureError("synthetic pre-send failure")
        return {"schema_version": 2, "relations": [], "decisions": [], "findings": []}

    model = Model(locate=all_source_candidate, associate=fail_once)
    with pytest.raises(ModelInfrastructureError):
        await run(data, model)
    completed = [payload for payload in model.calls if payload["operation"] != "associate"]
    result = await run(data, model)
    assert result["blocking"] is False
    assert [payload for payload in model.calls if payload["operation"] != "associate"
            and payload["work_unit_id"].startswith(("review-", "discovery-"))] == completed
    assert sum(payload["operation"] == "associate" for payload in model.calls) == 2
