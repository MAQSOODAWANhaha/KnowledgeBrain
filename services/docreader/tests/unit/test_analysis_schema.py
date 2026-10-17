"""Version 2 keeps source facts optional while enforcing the wire contract."""

from copy import deepcopy

import pytest
from pydantic import ValidationError

from docreader.tender.schema import (
    AnalysisExtraction, AnalysisExtractionResponse, AnalysisVerification, AssociationResult,
    Candidate, CatalogEntry, Decision, Issue, ItemDisposition, ItemDispositionResponse,
    ItemRef, LocateResult, TenderItem,
)


def evidence():
    return [{"range_id": "r1", "quote": "每份业绩2分，最高6分。"}]


def item():
    return {"item_id": "p1:i1", "kind": "scoring_rule", "text": "每份业绩2分，最高6分。",
            "evidence": evidence()}


def extraction():
    return {"schema_version": 2, "items": [item()], "dispositions": [{
        "range_id": "r1", "status": "items", "reason": "该段规定业绩计分及上限。",
        "item_ids": ["p1:i1"],
    }], "relations": [], "issues": []}


def review():
    return {"schema_version": 2, "reviewed_range_ids": ["r1"],
            "reviewed_item_ids": ["p1:i1"], "reviewed_relation_ids": [],
            "reviewed_decision_ids": ["a1:d1"], "accepted_decision_ids": ["a1:d1"],
            "findings": []}


def decision():
    return {"decision_id": "a1:d1", "issue_ids": ["q1"],
            "reason": "补遗取消上限，原有每份业绩2分继续有效。", "evidence": evidence(),
            "amendment": {"target_item_id": "p1:i1", "effect": "delete",
                          "affected_scope": "最高6分的计分上限", "retained_rules": [{
                              "text": "每份业绩2分。", "scope": "本项业绩评分", "evidence": evidence(),
                          }], "replacement_item_ids": []}}


def test_minimal_item_has_only_four_source_backed_fields():
    parsed = TenderItem.model_validate(item())
    assert parsed.model_dump(exclude_unset=True) == item()
    assert TenderItem.model_validate_json(parsed.model_dump_json(exclude_unset=True)) == parsed


@pytest.mark.parametrize("field,value", [
    ("domains", []), ("subject", None), ("subject", "  "), ("scope", "\n"),
    ("conditions", []), ("exceptions", [" "]), ("numbers", []),
    ("scoring", {}), ("scoring", None), ("scoring", {"method": " "}),
    ("scoring", {"points": None}), ("submission", {"fields": []}),
])
def test_optional_item_information_cannot_be_null_empty_or_placeholder(field, value):
    with pytest.raises(ValidationError):
        TenderItem.model_validate({**item(), field: value})


@pytest.mark.parametrize("kind,detail,value", [
    ("obligation", "scoring", {"points": 2}),
    ("scoring_rule", "consequence", {"effect": "否决投标"}),
    ("consequence_rule", "submission", {"documents": ["证书"]}),
    ("submission_requirement", "scoring", {"points": 0}),
])
def test_business_details_belong_only_to_the_corresponding_kind(kind, detail, value):
    with pytest.raises(ValidationError):
        TenderItem.model_validate({**item(), "kind": kind, detail: value})


def test_zero_and_negative_score_deltas_are_source_facts_not_missing_values():
    for scoring in ({"points": 0, "max_points": 0}, {"points": -2, "method": "每缺一项扣2分，最多扣6分"}):
        parsed = TenderItem.model_validate({**item(), "scoring": scoring})
        assert parsed.scoring.model_dump(exclude_unset=True) == scoring


@pytest.mark.parametrize("value", [True, False, "2", float("nan"), float("inf"), -float("inf")])
def test_score_numbers_are_strict_and_finite(value):
    with pytest.raises(ValidationError):
        TenderItem.model_validate({**item(), "scoring": {"points": value}})


def test_score_upper_bound_is_nonnegative_and_does_not_store_deduction_limit():
    with pytest.raises(ValidationError):
        TenderItem.model_validate({**item(), "scoring": {"max_points": -6}})


@pytest.mark.parametrize("value", [True, False, "", " ", float("nan"), float("inf"), -float("inf")])
def test_declared_quantities_cannot_hide_boolean_or_nonfinite_numbers(value):
    with pytest.raises(ValidationError):
        TenderItem.model_validate({**item(), "numbers": [{"value": value, "unit": "分", "quote": "2分"}]})


def test_source_number_strings_and_zero_remain_explicit_source_facts():
    for value in (0, "0", "0.25", "三", "三十", "一千万元"):
        parsed = TenderItem.model_validate({**item(), "numbers": [{
            "value": value, "unit": "分", "quote": "2分",
        }]})
        assert parsed.numbers[0].value == value


def test_v2_rejects_old_fields_and_does_not_convert_old_extraction():
    with pytest.raises(ValidationError):
        TenderItem.model_validate({**item(), "requirement_id": "old"})
    with pytest.raises(ValidationError):
        AnalysisExtraction.model_validate({"requirements": [], "dispositions": [], "issues": []})


@pytest.mark.parametrize("field", ["schema_version", "items", "dispositions", "relations", "issues"])
def test_extraction_requires_all_protocol_collections(field):
    payload = extraction()
    payload.pop(field)
    with pytest.raises(ValidationError):
        AnalysisExtraction.model_validate(payload)


def test_model_disposition_omits_host_derived_ids_without_relaxing_canonical_contract():
    declaration = {"range_id": "r1", "status": "items", "reason": "该段规定业绩计分及上限。"}
    assert ItemDispositionResponse.model_validate(declaration).model_dump(exclude_unset=True) == declaration
    with pytest.raises(ValidationError):
        ItemDisposition.model_validate(declaration)


@pytest.mark.parametrize("status,ids", [
    ("items", []), ("non_item", ["p1:i1"]), ("unresolved", ["p1:i1", "p1:i1"]),
])
def test_redundant_model_ids_are_retained_for_host_diagnostics(status, ids):
    declaration = {"range_id": "r1", "status": status, "reason": "原文处置待核对。", "item_ids": ids}
    assert ItemDispositionResponse.model_validate(declaration).model_dump(exclude_unset=True) == declaration
    with pytest.raises(ValidationError):
        ItemDisposition.model_validate(declaration)


@pytest.mark.parametrize("field", ["schema_version", "items", "dispositions", "relations", "issues"])
def test_model_response_keeps_required_envelope_fields(field):
    payload = extraction()
    payload.pop(field)
    with pytest.raises(ValidationError):
        AnalysisExtractionResponse.model_validate(payload)


@pytest.mark.parametrize("field", ["items", "dispositions", "relations", "issues"])
@pytest.mark.parametrize("invalid", [None, {}, "[]", False])
def test_model_response_rejects_non_array_collections(field, invalid):
    payload = extraction()
    payload[field] = invalid
    with pytest.raises(ValidationError):
        AnalysisExtractionResponse.model_validate(payload)


def test_model_response_preserves_local_defects_for_host_normalization():
    payload = extraction()
    payload["items"][0]["numbers"] = [{"value": 2, "unit": "分"}]
    payload["dispositions"][0].pop("item_ids")
    payload["relations"] = [None, {"relation_id": "p1:l1"}]
    payload["issues"] = [{"code": "unclear"}]
    parsed = AnalysisExtractionResponse.model_validate(payload)
    assert parsed.model_dump(exclude_unset=True) == payload
    with pytest.raises(ValidationError):
        AnalysisExtraction.model_validate(payload)


@pytest.mark.parametrize("version", [1, "2", True])
def test_model_response_does_not_accept_old_or_coerced_versions(version):
    payload = {**extraction(), "schema_version": version}
    with pytest.raises(ValidationError):
        AnalysisExtractionResponse.model_validate(payload)


def test_partial_range_preserves_items_and_has_a_separate_unresolved_issue():
    payload = extraction()
    payload["issues"] = [{"code": "reference_unresolved", "detail": "详见第五章的目标尚未读取。",
                          "blocking": True, "range_id": "r1", "item_ids": ["p1:i1"]}]
    parsed = AnalysisExtraction.model_validate(payload)
    assert parsed.dispositions[0].item_ids == ["p1:i1"]
    assert parsed.issues[0].item_ids == ["p1:i1"]


@pytest.mark.parametrize("field", ["code", "detail", "blocking"])
def test_v2_issue_fields_are_explicit_without_changing_historical_defaults(field):
    finding = {"code": "reference_unresolved", "detail": "第五章目标尚未读取。", "blocking": True}
    finding.pop(field)
    payload = extraction()
    payload["issues"] = [finding]
    with pytest.raises(ValidationError):
        AnalysisExtraction.model_validate(payload)
    assert Issue(code="old_issue", detail="Legacy diagnostic.").blocking is True


@pytest.mark.parametrize("field", ["range_id", "item_ids"])
@pytest.mark.parametrize("model,collection,payload", [
    (AnalysisExtraction, "issues", extraction()),
    (AnalysisVerification, "findings", review()),
    (LocateResult, "findings", {"schema_version": 2, "candidates": [], "findings": []}),
    (AssociationResult, "findings", {"schema_version": 2, "relations": [], "decisions": [], "findings": []}),
])
def test_v2_output_issues_omit_unknown_locations_instead_of_null(model, collection, payload, field):
    payload = deepcopy(payload)
    payload[collection] = [{"code": "reference_unresolved", "detail": "目标未定位。",
                            "blocking": True, field: None}]
    with pytest.raises(ValidationError):
        model.model_validate(payload)


@pytest.mark.parametrize("status,ids", [("items", []), ("non_item", ["p1:i1"]), ("unresolved", ["p1:i1"])])
def test_disposition_status_matches_its_item_references(status, ids):
    with pytest.raises(ValidationError):
        ItemDisposition(range_id="r1", status=status, reason="有来源依据的处置", item_ids=ids)


def test_empty_protocol_collections_and_a_plain_blank_row_are_valid():
    payload = {"schema_version": 2, "items": [], "dispositions": [{
        "range_id": "blank", "status": "non_item", "reason": "字段要求由表头表达，本行无新增独立要求。",
        "item_ids": [],
    }], "relations": [], "issues": []}
    assert AnalysisExtraction.model_validate(payload).items == []


def test_partial_deletion_retains_original_facts_without_inheriting_old_score_cap():
    parsed = Decision.model_validate(decision())
    retained = parsed.amendment.retained_rules[0].model_dump()
    assert set(retained) == {"text", "scope", "evidence"}
    assert "scoring" not in retained


def test_pure_deletion_needs_no_fabricated_replacement_or_issue():
    payload = decision()
    payload["issue_ids"] = []
    payload["amendment"]["retained_rules"] = []
    assert Decision.model_validate(payload).amendment.replacement_item_ids == []


@pytest.mark.parametrize("effect,replacements", [("replace", []), ("delete", ["p2:i1"])])
def test_amendment_effect_requires_exact_replacement_shape(effect, replacements):
    payload = decision()
    payload["amendment"].update(effect=effect, replacement_item_ids=replacements)
    with pytest.raises(ValidationError):
        Decision.model_validate(payload)


@pytest.mark.parametrize("field", list(decision()["amendment"]))
def test_amendment_effect_does_not_guess_missing_retained_or_replacement_content(field):
    payload = decision()
    payload["amendment"].pop(field)
    with pytest.raises(ValidationError):
        Decision.model_validate(payload)


def test_non_amendment_decision_must_identify_the_problem_it_proposes_to_resolve():
    payload = decision()
    payload.pop("amendment")
    payload["issue_ids"] = []
    with pytest.raises(ValidationError):
        Decision.model_validate(payload)


def test_review_explicitly_accepts_only_reviewed_decisions():
    assert AnalysisVerification.model_validate(review()).accepted_decision_ids == ["a1:d1"]
    payload = review()
    payload["accepted_decision_ids"] = ["absent"]
    with pytest.raises(ValidationError):
        AnalysisVerification.model_validate(payload)
    payload = review()
    payload["reviewed_item_ids"] *= 2
    with pytest.raises(ValidationError):
        AnalysisVerification.model_validate(payload)


@pytest.mark.parametrize("field", list(review()))
def test_review_requires_every_explicit_coverage_collection(field):
    payload = review()
    payload.pop(field)
    with pytest.raises(ValidationError):
        AnalysisVerification.model_validate(payload)


def test_candidate_must_select_actual_identifiable_input_for_later_host_resolution():
    payload = {"candidate_id": "d1:c1", "issue_ids": [], "range_ids": [],
               "item_ids": ["p1:i1"], "reason": "与补遗的交付期限需联合核对。"}
    assert Candidate.model_validate(payload).item_ids == ["p1:i1"]
    payload["item_ids"] = []
    with pytest.raises(ValidationError):
        Candidate.model_validate(payload)


def test_locate_and_associate_results_cannot_masquerade_as_independent_acceptance():
    assert LocateResult(schema_version=2, candidates=[], findings=[]).candidates == []
    associated = AssociationResult(schema_version=2, relations=[], decisions=[decision()], findings=[])
    with pytest.raises(ValidationError):
        AnalysisVerification.model_validate(associated.model_dump(exclude_unset=True))


def test_catalog_keeps_document_identity_and_null_unheaded_section():
    entry = {"document_id": "d1", "document_version": "v1", "role": "main",
             "sections": [{"section_id": None, "heading_path": ""},
                          {"section_id": "s1", "heading_path": "相同标题"},
                          {"section_id": "s2", "heading_path": "相同标题"}]}
    assert CatalogEntry.model_validate(entry).sections[0].section_id is None
    for field, value in (("range_ids", ["r1"]), ("section_id", None)):
        with pytest.raises(ValidationError):
            CatalogEntry.model_validate({**entry, field: value})


def test_section_candidate_is_identifiable_without_inventing_unloaded_range_ids():
    payload = {"candidate_id": "d1:c1", "issue_ids": [], "range_ids": [], "item_ids": [],
               "section_refs": [{"document_id": "d1", "section_id": None}], "reason": "补读无标题正文。"}
    assert Candidate.model_validate(payload).section_refs[0].section_id is None


def test_physical_page_candidate_needs_no_invented_range_identity():
    payload = {"candidate_id": "d1:c1", "issue_ids": [], "range_ids": [], "item_ids": [],
               "page_refs": [{"document_id": "d1", "page_ordinal": 0}], "reason": "补读物理首页。"}
    assert Candidate.model_validate(payload).page_refs[0].page_ordinal == 0


@pytest.mark.parametrize("refs", [None, [], [{"document_id": "d1", "page_ordinal": -1}],
    [{"document_id": "d1", "page_ordinal": True}], [{"document_id": "d1", "page_ordinal": "1"}],
    [{"document_id": "d1", "page_ordinal": 0}, {"document_id": "d1", "page_ordinal": 0}]])
def test_page_candidate_rejects_empty_duplicate_or_nonphysical_indices(refs):
    with pytest.raises(ValidationError):
        Candidate.model_validate({"candidate_id": "d1:c1", "issue_ids": [], "range_ids": [],
            "item_ids": [], "page_refs": refs, "reason": "补读物理页。"})


@pytest.mark.parametrize("page_count", [0, -1, True, "106", None])
def test_catalog_page_count_must_be_positive_or_omitted(page_count):
    with pytest.raises(ValidationError):
        CatalogEntry.model_validate({"document_id": "d1", "document_version": "v1", "role": "main",
                                     "sections": [], "page_count": page_count})


@pytest.mark.parametrize("refs", [None, [], [{"document_id": "d1"}],
    [{"document_id": "d1", "section_id": "s1"}, {"document_id": "d1", "section_id": "s1"}]])
def test_section_candidate_omits_empty_fields_and_rejects_duplicate_or_incomplete_refs(refs):
    payload = {"candidate_id": "d1:c1", "issue_ids": [], "range_ids": [], "item_ids": ["i1"],
               "section_refs": refs, "reason": "补读章节。"}
    with pytest.raises(ValidationError):
        Candidate.model_validate(payload)


def test_catalog_does_not_allow_duplicate_section_identities():
    with pytest.raises(ValidationError):
        CatalogEntry.model_validate({"document_id": "d1", "document_version": "v1", "role": "main",
            "sections": [{"section_id": None, "heading_path": ""},
                         {"section_id": None, "heading_path": "另一个标题"}]})


def test_item_reference_preserves_artifact_and_item_identity():
    artifact = {"sha256": "a" * 64, "relative_path": "aa/" + "a" * 64,
                "size": 12, "media_type": "application/json"}
    parsed = ItemRef(artifact_ref=artifact, item_id="p1:i1")
    assert ItemRef.model_validate_json(parsed.model_dump_json()) == parsed


@pytest.mark.parametrize("schema_version", [1, "2", 2.0, True])
def test_schema_version_is_an_explicit_strict_integer_two(schema_version):
    payload = deepcopy(extraction())
    payload["schema_version"] = schema_version
    with pytest.raises(ValidationError):
        AnalysisExtraction.model_validate(payload)
