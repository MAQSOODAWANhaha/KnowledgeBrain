"""Bounded source review contracts; synthetic replies do not measure model quality."""

from copy import deepcopy
from functools import partial
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm import ModelInfrastructureError
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelConfigurationError
from docreader.tender.checks.analysis import _Analysis, assign_issues, build_catalog, current_items, verify_analysis
from docreader.tender.evidence import planner
from docreader.tender.evidence.freeze import plan_sources
from docreader.tender.evidence.references import EvidenceRepository, SourceIndex, fragment
from docreader.tender.evidence.view import resolve_evidence_quote, supplied_evidence_texts
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


def review_texts(model):
    return [source["text"] for payload in model.calls if payload["operation"] == "review"
            for view in payload["source_views"] for key in ("body", "supporting")
            for source in view.get(key, []) if source.get("text")]


def only_review(model):
    assert {payload["operation"] for payload in model.calls} <= {"review"}


def reference_candidate(payload):
    if not payload["issues"]:
        return []
    return [{"candidate_id": payload["work_unit_id"] + ":c1",
             "issue_ids": [entry["issue_id"] for entry in payload["issues"]],
             "range_ids": range_ids(payload["source_views"]),
             "item_ids": [entry["item_id"] for entry in payload["items"]], "reason": "核对原文适用条件。"}]


def all_source_candidate(payload):
    if not payload["work_unit_id"].startswith("discovery-"):
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
    assert artifacts.read_json(result["review_plan_ref"])["schema_version"] == 2
    assert result["locate_plan_ref"] is None and result["association_plan_ref"] is None
    assert result["joint_plan_ref"] is None
    assert len(result["operation_refs"]) == len(model.calls)
    assert all("source_version" not in payload and "domain_revision" not in payload for payload in model.calls)


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
    views = [{"body":[],"supporting":[],key:[row]}]

    assert resolve_evidence_quote(original["range_id"], "证书", context.sources, supplied_evidence_texts(views, context.sources)) is not None
    assert resolve_evidence_quote(original["range_id"], original["text"], context.sources, supplied_evidence_texts(views, context.sources)) is None
    row["text"] = original["text"]
    assert resolve_evidence_quote(original["range_id"], original["text"], context.sources, supplied_evidence_texts(views, context.sources)) is not None


@pytest.mark.asyncio
async def test_table_alias_cannot_reintroduce_an_unsent_canonical_row(tmp_path):
    data = await scenario(tmp_path, documents=[("main","main",[("姓名 | 证书", "obligation")])])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    original = context.indices["main"].ranges[0]
    alias = fragment(original,0,original["end"]-1)
    context.sources[alias["range_id"]] = alias
    row = {"range_id":original["range_id"],"cells":[{"column":0,"text":"姓名"},{"column":1,"text":"证书"}]}
    views = [{"body":[row],"supporting":[],"source_aliases":[{"range_id":alias["range_id"],
        "segments":[{"range_id":original["range_id"],"start":alias["start"],"end":alias["end"]}]}]}]
    texts = supplied_evidence_texts(views, context.sources)

    assert resolve_evidence_quote(alias["range_id"], "姓名", context.sources, texts) is not None
    assert resolve_evidence_quote(alias["range_id"], alias["text"], context.sources, texts) is None
    assert resolve_evidence_quote(alias["range_id"], "证书", context.sources, texts) is None


@pytest.mark.asyncio
async def test_text_alias_can_quote_across_contiguous_sent_segments(tmp_path):
    data = await scenario(tmp_path, documents=[("main","main",[("完整人员条件原文。", "obligation")])])
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    original = context.indices["main"].ranges[0]
    middle = len(original["text"])//2
    first,second = fragment(original,0,middle),fragment(original,middle,original["end"])
    views = [{"body":[first,second],"supporting":[],"source_aliases":[{"range_id":original["range_id"],
        "segments":[{"range_id":entry["range_id"],"start":entry["start"],"end":entry["end"]}
                    for entry in [first,second]]}]}]
    assert resolve_evidence_quote(original["range_id"], original["text"], context.sources, supplied_evidence_texts(views, context.sources)) is not None


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
    if not payload["work_unit_id"].startswith("discovery-"):
        return []
    return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [], "range_ids": [], "item_ids": [],
             "page_refs": [{"document_id": document, "page_ordinal": page}], "reason": "核对明确物理PDF页原文。"}]


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
    model = Model()
    result = await run(data,model)
    only_review(model)
    assert "图片原文。" in review_texts(model)
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
async def test_physical_page_candidate_loads_complete_page_for_association(tmp_path):
    data = await scenario(tmp_path, documents=[("main","main",[
        ("首物理页原文。", "obligation"),("第二物理页甲要求。", "obligation"),
        ("第二物理页补充说明。",None)])],
        native_setup=partial(physical_pdf,unit_pages=(0,1,1)))
    catalog = build_catalog(data[1],data[0])
    assert catalog[0]["page_count"] == 2
    model = Model()
    result = await run(data,model)
    only_review(model)
    assert {"第二物理页甲要求。","第二物理页补充说明。"} <= set(review_texts(model))
    assert result["blocking"] is False


@pytest.mark.asyncio
@pytest.mark.parametrize("null_field", ["section_refs","page_refs"])
async def test_unused_optional_candidate_selectors_normalize_explicit_null(tmp_path,null_field):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("第二页。",None)])],
                          native_setup=physical_pdf)

    def candidate(payload):
        select = page_candidate if null_field == "section_refs" else all_source_candidate
        return [{**entry,null_field:None} for entry in select(payload)]

    result = await run(data,Model(locate=candidate))
    assert result['blocking'] is False


@pytest.mark.asyncio
@pytest.mark.parametrize("page,document", [(2,"main"),(0,"absent")])
async def test_page_candidates_reject_out_of_range_or_unuploaded_document(tmp_path,page,document):
    data = await scenario(tmp_path,documents=[("main","main",[("第一页。","obligation"),("第二页。",None)])],
                          native_setup=physical_pdf)
    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["schema_version"] == 2


@pytest.mark.asyncio
async def test_page_candidate_requires_a_complete_pdf_page_contract(tmp_path):
    data = await scenario(tmp_path)
    assert all("page_count" not in document for document in build_catalog(data[1],data[0]))
    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["schema_version"] == 2


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
    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["association_plan_ref"] is None


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
    only_review(model)
    assert all("catalog" not in payload for payload in model.calls)


@pytest.mark.asyncio
async def test_section_candidate_loads_every_original_range_and_non_item_content(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("人员应持A证。", "obligation"), ("本节说明证书审核口径。", None), ("另一章节。", None)])],
        sections=["same", "same", "other"])

    def candidate(payload):
        if not payload["work_unit_id"].startswith("discovery-"):
            return []
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [], "range_ids": [], "item_ids": [],
                 "section_refs": [{"document_id": "main", "section_id": "same"}],
                 "reason": "联合补读本节原文和口径说明。"}]

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert {"人员应持A证。", "本节说明证书审核口径。"} <= set(review_texts(model))
    assert result["blocking"] is False


@pytest.mark.asyncio
@pytest.mark.parametrize("document,section", [("other", "s0"), ("main", "absent"), ("main", None)])
async def test_section_candidates_must_use_catalog_identities_supplied_to_locate(tmp_path, document, section):
    data = await scenario(tmp_path)
    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["schema_version"] == 2


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

    model = Model()
    await run(data, model)
    only_review(model)
    assert "未在发现请求提供的描述原文。" in review_texts(model)


@pytest.mark.asyncio
async def test_complete_section_candidate_is_not_split_to_fit_association_budget(tmp_path):
    data = await scenario(tmp_path, documents=[("main", "main", [
        ("人员应持A证。", "obligation"), ("本节说明证书审核口径。", None)])], sections=[None, None])

    def candidate(payload):
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": [], "item_ids": [], "section_refs": [{"document_id": "main", "section_id": None}],
                 "reason": "补读完整无标题章节。"}]

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert {"人员应持A证。", "本节说明证书审核口径。"} <= set(review_texts(model))
    assert result["association_plan_ref"] is None and result["blocking"] is False


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
async def test_only_an_independently_accepted_source_decision_closes_a_model_issue(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)
    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["blocking"] is True
    assert result["accepted_decision_ids"] == []
    assert any(problem["code"] == "condition_unclear" for problem in result["issues"])


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

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["accepted_decision_ids"] == []


@pytest.mark.asyncio
async def test_joint_blocking_findings_prevent_acceptance_even_if_model_lists_the_decision(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)

    def finding(payload, value):
        if payload["work_unit_id"].startswith("joint-"):
            value["findings"] = [{"code": "condition_unclear", "detail": "仍未解释适用范围。", "blocking": False}]

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["blocking"] is True and result["accepted_decision_ids"] == []
    assert result["resolved_issue_ids"] == []


@pytest.mark.asyncio
async def test_joint_skips_omission_targets_and_preserves_complete_review_context(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)
    artifacts, plan, index, _ = data
    extracted = artifacts.read_json(index["packs"][0]["artifact_ref"])
    first, second = extracted["items"][:2]
    original_relation = {"relation_id": "original:l1", "from_item_id": first["item_id"],
        "to_item_id": second["item_id"], "relation": "评分规则涉及人员资质。",
        "evidence": deepcopy(first["evidence"])}
    extracted["relations"].append(original_relation)
    index["packs"][0]["artifact_ref"] = artifacts.put_json(extracted)
    plan["scope_decisions"] = [{"range_id": first["evidence"][0]["range_id"],
        "decision": "local_only", "target_range_ids": [], "rationale": "该条件仅限定本项人员资质。",
        "evidence": deepcopy(first["evidence"])}]
    proposed = []

    def candidate(payload):
        candidates = reference_candidate(payload)
        for entry in candidates:
            entry["section_refs"] = [{"document_id": document["document_id"], "section_id": section["section_id"]}
                for document in payload["catalog"] if document["document_id"] == "main"
                for section in document["sections"]]
        return candidates

    def associated(payload):
        value = resolution(payload)
        relation = {**deepcopy(original_relation), "relation_id": payload["work_unit_id"] + ":l1"}
        value["relations"] = [relation]
        value["decisions"][0]["relation_ids"] = [relation["relation_id"]]
        value["findings"] = [{"code": "source_formatting_note", "detail": "原文换行不影响本项含义。",
                              "blocking": False, "range_id": first["evidence"][0]["range_id"]}]
        proposed.append(deepcopy(value))
        return value

    model = Model()
    result = await run(data, model)
    only_review(model)
    source_reviews = [p for p in model.calls if p["operation"] == "review"]
    expected_dispositions = {d["range_id"]: d for descriptor in index["packs"]
        for d in artifacts.read_json(descriptor["artifact_ref"])["dispositions"]}
    reviewed_dispositions = {d["range_id"]: d for p in source_reviews for d in p["dispositions"]}
    assert reviewed_dispositions == expected_dispositions
    assert result["accepted_decision_ids"] == []


@pytest.mark.asyncio
@pytest.mark.parametrize("facts,reason,detail,amendment", [
    (["仅代理商须提供证明。", "制造商不适用该要求。"], "全部投标人均须提供证明。", "提案扩大了仅适用于代理商的条件。", False),
    (["未提交证明应否决投标。", "制造商可以承诺函替代证明。"], "没有提交证明的一律否决。", "提案遗漏制造商的替代证明例外。", False),
    (["交付期限30天，保修期2年。", "补遗仅将交付期限改为60天，保修期不变。"],
     "补遗将原要求整体替换。", "提案未保留补遗没有修改的2年保修规则。", True),
])
async def test_joint_without_omission_targets_still_preserves_and_reports_counterevidence(tmp_path, facts, reason, detail, amendment):
    data = await scenario(tmp_path, documents=[("main", "main", [(text, "obligation") for text in facts])],
                          analysis_issue=True)

    def associated(payload):
        value = resolution(payload)
        value["decisions"][0]["reason"] = reason
        if amendment:
            original, replacement = payload["items"]
            value["decisions"][0].update(issue_ids=[], evidence=[*deepcopy(original["evidence"]),
                *deepcopy(replacement["evidence"])], amendment={"target_item_id": original["item_id"],
                    "effect": "replace", "affected_scope": "交付期限", "retained_rules": [],
                    "replacement_item_ids": [replacement["item_id"]]})
        return value

    def counterevidence(payload, value):
        if payload["operation"] == "locate":
            for candidate in value["candidates"]:
                candidate["section_refs"] = [{"document_id": document["document_id"], "section_id": section["section_id"]}
                    for document in payload["catalog"] for section in document["sections"]]
        elif payload["work_unit_id"].startswith("joint-"):
            visible = [entry for view in payload["source_views"] for key in ("body", "supporting") for entry in view[key]]
            assert {entry["text"] for entry in visible} == set(facts)
            contrary = next(entry for entry in visible if entry["text"] == facts[1])
            value["findings"] = [{"code": "condition_unclear", "detail": detail, "blocking": True,
                                  "range_id": contrary["range_id"]}]

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert set(facts) <= set(review_texts(model))
    assert result["accepted_decision_ids"] == []
    if amendment:
        assert {entry["item"]["text"] for entry in current_items(data[0], result)} == set(facts)


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

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["accepted_decision_ids"] == []
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

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["joint_plan_ref"] is None


@pytest.mark.asyncio
async def test_relation_only_association_skips_joint_and_keeps_relations_unverified(tmp_path):
    data = await scenario(tmp_path)

    def related(payload):
        citation = deepcopy(payload["items"][0]["evidence"][0])
        return {"schema_version": 2, "decisions": [], "findings": [], "relations": [{
            "relation_id": payload["work_unit_id"] + ":l1", "from_item_id": payload["items"][0]["item_id"],
            "to_item_id": payload["items"][1]["item_id"], "relation": "评分规则针对该资格要求。",
            "evidence": [citation]}]}

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["joint_plan_ref"] is None and result["blocking"] is False


@pytest.mark.asyncio
async def test_untraceable_decision_cannot_close_existing_issue_and_joint_receives_pending_content(tmp_path):
    data = await scenario(tmp_path, analysis_issue=True)

    def unsupported(payload):
        value = resolution(payload)
        value["decisions"][0]["evidence"][0]["quote"] = "不存在的决议引文"
        return value

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["blocking"] is True and result["accepted_decision_ids"] == []
    assert result["resolved_issue_ids"] == [] and data[0].read_json(result["decisions_ref"]) == []
    assert any(p["code"] == "condition_unclear" for p in result["issues"])


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
async def test_review_owns_overlapping_omission_targets_once_and_preserves_supplements(tmp_path, monkeypatch):
    monkeypatch.setattr(planner, "MAX_EXTRACT_BODY_RANGES", 1)
    artifacts, plan, items, _ = await scenario(tmp_path, sections=["same-section"] * 4)
    assert sum(pack["document_id"] == "main" for pack in plan["packs"]) > 1
    context = _Analysis(plan, items, artifacts, IDENTITY, budget())
    values = list(context.items.values())
    first, last = values[0], values[-1]
    first_range, last_range = (item["evidence"][0]["range_id"] for item in (first, last))
    relation = {"relation_id": "review-test:l1", "from_item_id": first["item_id"], "to_item_id": last["item_id"],
                "relation": "跨文档条件", "evidence": [*first["evidence"], *last["evidence"]]}
    context.relations[relation["relation_id"]] = relation
    decision = {"range_id": first_range, "decision": "local_only", "target_range_ids": [],
                "rationale": "仅适用于本处。", "evidence": first["evidence"]}
    context.plan["scope_decisions"] = [decision]

    requests = context.review_requests()

    targeted = [entry["range_id"] for payload in requests for entry in payload["dispositions"]]
    assert len(targeted) == len(set(targeted)) and set(targeted) == set(context.owners)
    main = [payload for payload in requests if payload["dispositions"]]
    assert len(main) == 2
    related, = [payload for payload in requests if relation in payload["relations"]]
    assert not related["dispositions"]
    assert {first_range, last_range} <= set(range_ids(related["source_views"]))
    scope, = [payload for payload in requests if payload["work_unit_id"].startswith("scope-review-")]
    assert not scope["dispositions"] and decision in scope["scope_decisions"]
    assert first_range in range_ids(scope["source_views"])


async def test_reviewed_cross_section_condition_scopes_only_its_source_document(tmp_path):
    artifacts, plan, items, _ = await scenario(tmp_path, documents=[
        ("main", "main", [("前附表：不要求履约保证金。", "obligation"),
                          ("合同章：按照招标文件规定提交履约保证金。", "obligation")]),
        ("annex", "attachment", [("报价应包括运输费。", "obligation")]),
    ])
    base = _Analysis(plan, items, artifacts, IDENTITY, budget())
    condition, target = base.indices["main"].ranges
    unrelated, = base.indices["annex"].ranges
    decision = {"range_id": condition["range_id"], "decision": "bind",
                "target_range_ids": [target["range_id"]], "rationale": "前附表适用于此合同条款。",
                "evidence": [{"range_id": condition["range_id"], "quote": condition["text"]}]}
    plan["scope_decisions"] = [decision]
    scope = _Analysis(plan, items, artifacts, IDENTITY, budget())

    request = scope.review_payload([target["range_id"]])
    assert condition["range_id"] in range_ids(request["source_views"])
    assert [row["range_id"] for row in request["dispositions"]] == [target["range_id"]]
    # BUG-299: a request that reads the decision's ranges also reads the human conclusion.
    assert request["scope_decisions"] == [decision]
    annex = scope.review_payload([unrelated["range_id"]])
    assert condition["range_id"] not in range_ids(annex["source_views"])
    assert "scope_decisions" not in annex
    requests = scope.review_requests()
    targeted = [row["range_id"] for payload in requests for row in payload["dispositions"]]
    assert sorted(targeted) == sorted(scope.review_targets)
    supplement, = [payload for payload in requests if payload["work_unit_id"].startswith("scope-review-")]
    assert supplement["scope_decisions"] == [decision] and not supplement["dispositions"]


@pytest.mark.asyncio
@pytest.mark.parametrize("case", ["unchecked", "checked", "ambiguous", "other_page", "outside_table", "over_budget"])
async def test_review_carries_unique_pdf_choice_row_as_support_without_owning_its_target(tmp_path, case):
    clause = "送样须事先备案。"
    options = "☑提供\n□不提供" if case == "checked" else "☑不提供\n□提供"
    choice = f"{options}\n{clause}"
    other = clause if case == "ambiguous" else "同页其他表项。"

    def native_setup(index):
        index.document["original_ref"]["media_type"] = "application/pdf"
        table = {"locator_kind": "page_table", "page_ordinal": 0, "table_ordinal": 0,
                 "left": 5, "right": 90, "bottom": 10, "top": 95}
        literal = {"locator_kind": "page", "page_ordinal": int(case == "other_page"),
                   "left": 1 if case == "outside_table" else 20, "right": 80, "bottom": 20, "top": 25}
        for number, (unit, identity) in enumerate(zip(index.raw["units"], index.raw["source_contract"]["units"], strict=True)):
            locator = literal if number == 1 else table
            unit["locator"] = identity["physical_locator"] = locator
            if number == 1:
                unit["text_basis"] = "pdf_literal"
            else:
                unit.update(kind="table_region", grid={"row_count": 1, "column_count": 1, "cells": [
                    {"row": 0, "column": 0, "row_span": 1, "col_span": 1, "text": unit["text"]},
                ]})

    kind = "obligation" if case == "checked" else None
    data = await scenario(tmp_path, documents=[("main", "main", [(choice, None), (clause, kind), (other, None)])],
                          native_setup=native_setup)
    index = EvidenceRepository(data[0]).source_index(data[1]["documents"][0]["source_index_ref"])
    choice_id, clause_id, other_id = (row["range_id"] for row in index.ranges)
    model = Model()
    admission = OperationBudget(lambda payload: len(payload["dispositions"]) > 1
                                or case == "over_budget" and any(view["supporting"] for view in payload["source_views"]))
    result = await run(data, model, admission)

    requested = {p["dispositions"][0]["range_id"]: p for p in model.calls}
    if case == "over_budget":
        assert result["hard_blocking"] and result["coverage"]["missing"]["range"] == [clause_id]
        assert any(issue["code"] == "analysis_budget_exceeded" for issue in result["issues"])
        assert len(model.calls) == 2 and requested.keys() == {choice_id, other_id}
        return

    assert not result["coverage"]["missing"]
    assert len(model.calls) == 3 and requested.keys() == {choice_id, clause_id, other_id}
    clause_request = requested[clause_id]
    view, = clause_request["source_views"]
    assert {e["range_id"] for e in view["body"]} == {clause_id}
    assert requested[clause_id]["dispositions"][0]["status"] == ("items" if kind else "non_item")
    if case in {"unchecked", "checked"}:
        row, = view["supporting"]
        assert row["range_id"] == choice_id and row["cells"][0]["text"] == choice
    else:
        assert view["supporting"] == []
    assert other_id not in range_ids(clause_request["source_views"])


@pytest.mark.asyncio
@pytest.mark.parametrize(("case", "target_is_row", "matched"), [
    ("row", True, True),
    ("literal", False, True),
    ("joined", True, True),
    ("duplicate", True, False),
    ("outside", True, False),
    ("other_page", True, False),
    ("different_number", True, False),
    ("over_budget", True, True),
])
async def test_review_carries_items_from_a_unique_same_pdf_table_projection(tmp_path, case, target_is_row, matched):
    cell_texts = ["3", "须提供 2 份证明。"] if case == "joined" else ["须提供 2 份证明。"]
    literal = "3 须提供2份证明。" if case == "joined" else "须提供2份证明。"
    if case == "different_number":
        cell_texts = ["须提供1 23份证明。"]
        literal = "须提供12 3份证明。"
    other = cell_texts if case == "duplicate" else ["独立的其他要求。"]

    def native_setup(index):
        index.document["original_ref"]["media_type"] = "application/pdf"
        table = {"locator_kind": "page_table", "page_ordinal": 0, "table_ordinal": 0,
                 "left": 5, "right": 90, "bottom": 10, "top": 95}
        line = {"locator_kind": "page", "page_ordinal": int(case == "other_page"),
                "left": 1 if case == "outside" else 20, "right": 80, "bottom": 20, "top": 25}
        for number, (unit, identity) in enumerate(zip(index.raw["units"], index.raw["source_contract"]["units"], strict=True)):
            unit["locator"] = identity["physical_locator"] = line if number == 1 else table
            if number == 1:
                unit["text_basis"] = "pdf_literal"
                continue
            texts = cell_texts if number == 0 else other
            unit.update(kind="table_region", grid={"row_count": 1, "column_count": len(texts), "cells": [
                {"row": 0, "column": column, "row_span": 1, "col_span": 1, "text": text}
                for column, text in enumerate(texts)]})

    row_kind, line_kind = (None, "obligation") if target_is_row else ("obligation", None)
    data = await scenario(tmp_path, documents=[("main", "main", [(" | ".join(cell_texts), row_kind),
                          (literal, line_kind), (" | ".join(other), None)])], native_setup=native_setup)
    artifacts, plan, items, _ = data
    index = EvidenceRepository(artifacts).source_index(plan["documents"][0]["source_index_ref"])
    row_id, line_id, other_id = (source["range_id"] for source in index.ranges)
    target, alternate = (row_id, line_id) if target_is_row else (line_id, row_id)
    admission = OperationBudget(lambda payload: len(payload["dispositions"]) > 1
                                or case == "over_budget" and {target, alternate} <= set(range_ids(payload["source_views"]))
                                and any(d["range_id"] == target for d in payload["dispositions"]))
    model = Model()
    result = await run(data, model, admission)
    requests = {payload["dispositions"][0]["range_id"]: payload for payload in model.calls}
    if case == "over_budget":
        assert result["coverage"]["missing"]["range"] == [target]
        assert any(issue["code"] == "analysis_budget_exceeded" for issue in result["issues"])
        assert requests.keys() == {alternate, other_id}
        return

    assert not result["coverage"]["missing"]
    assert requests.keys() == {target, alternate, other_id}
    payload = requests[target]
    assert payload["dispositions"][0]["status"] == "non_item"
    assert {e["range_id"] for item in payload["items"] for e in item["evidence"]} == ({alternate} if matched else set())
    assert range_ids(payload["source_views"]) == sorted({target, alternate} if matched else {target})
    supplement = _Analysis(plan, items, artifacts, IDENTITY, admission).review_payload([target], check_omissions=False)
    assert not supplement["dispositions"] and not supplement["items"]


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

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["accepted_decision_ids"] == []
    assert data[0].read_json(result["decisions_ref"]) == []


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

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["accepted_decision_ids"] == []
    assert len(current_items(data[0], result)) == 5


@pytest.mark.asyncio
async def test_localization_cannot_invent_a_source_identity(tmp_path):
    data = await scenario(tmp_path)

    def invented(payload):
        return [{"candidate_id": payload["work_unit_id"] + ":c1", "issue_ids": [],
                 "range_ids": ["not-a-frozen-range"], "item_ids": [], "reason": "尝试定位关联。"}]

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["schema_version"] == 2


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
    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["association_plan_ref"] is None and result["blocking"] is False


@pytest.mark.asyncio
async def test_shared_rule_does_not_merge_independent_candidates_into_an_oversized_request(tmp_path):
    data = await scenario(tmp_path)
    expected = []

    def candidates(payload):
        if not payload["work_unit_id"].startswith("discovery-"):
            return []
        common, first, second = payload["items"][:3]
        expected.extend([{common["item_id"], target["item_id"]} for target in (first, second)])
        return [{"candidate_id": payload["work_unit_id"] + f":c{number}", "issue_ids": [],
                 "range_ids": [entry["evidence"][0]["range_id"] for entry in (common, target)],
                 "item_ids": [common["item_id"], target["item_id"]], "reason": "分别核对公共规则的适用对象。"}
                for number, target in enumerate((first, second), start=1)]

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["association_plan_ref"] is None and result["blocking"] is False


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

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["blocking"] is True


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
async def test_successful_scope_context_does_not_cover_rejected_omission_target(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    item = next(iter(context.items.values()))
    target = item["evidence"][0]["range_id"]
    data[1]["scope_decisions"] = [{"range_id": target, "target_range_ids": [], "decision": "local_only",
                                   "rationale": "只适用于本处。", "evidence": item["evidence"]}]
    admission = OperationBudget(lambda payload: payload["work_unit_id"].startswith("review-"))
    model = Model()

    result = await run(data, model, admission)

    scope, = model.calls
    assert scope["work_unit_id"].startswith("scope-review-")
    assert target in range_ids(scope["source_views"])
    assert result["hard_blocking"] and target in result["coverage"]["missing"]["range"]


@pytest.mark.asyncio
async def test_recovery_reuses_completed_review_and_localization_receipts(tmp_path):
    data = await scenario(tmp_path)
    failures = [True]

    def fail_once(payload):
        if failures:
            failures.pop()
            raise ModelInfrastructureError("synthetic pre-send failure")
        return {"schema_version": 2, "relations": [], "decisions": [], "findings": []}

    model = Model()
    result = await run(data, model)
    only_review(model)
    assert result["blocking"] is False


@pytest.mark.asyncio
async def test_analysis_payloads_are_content_stable_across_host_revisions(tmp_path):
    data = await scenario(tmp_path)
    first = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    other = _Analysis(data[1], data[2], data[0], {**IDENTITY, "domain_revision": 2}, budget())
    left, right = first.review_requests(), other.review_requests()
    assert left == right
    assert all(payload["work_unit_id"].startswith("review-") for payload in left)
    assert all("source_version" not in payload and "domain_revision" not in payload for payload in left)
    assert all(len(payload["work_unit_id"].split("-", 1)[-1]) == 12 for payload in left)


@pytest.mark.asyncio
async def test_analysis_validate_rejects_retired_locate_and_associate_operations(tmp_path):
    data = await scenario(tmp_path)
    context = _Analysis(data[1], data[2], data[0], IDENTITY, budget())
    locate_payload = context.payload("locate", "locate-1")
    assert "catalog" not in locate_payload
    with pytest.raises(ModelConfigurationError, match="Unsupported verify operation: locate"):
        context.validate({"schema_version": 2, "candidates": [], "findings": []}, locate_payload)
    associate_payload = context.payload("associate", "associate-1")
    with pytest.raises(ModelConfigurationError, match="Unsupported verify operation: associate"):
        context.validate({"schema_version": 2, "relations": [], "decisions": [], "findings": []}, associate_payload)
