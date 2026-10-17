"""Synthetic corpus validity is separate from model quality and human acceptance."""

from copy import deepcopy
import importlib.util
import json
from pathlib import Path

import pytest

from docreader.tender.prompts import get_prompt
from docreader.tender.schema import Decision, SourceView, TenderItem


PATH = Path(__file__).parents[1] / "acceptance/check_semantic_cases.py"
SPEC = importlib.util.spec_from_file_location("semantic_cases", PATH)
evaluation = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(evaluation)


def corpus():
    return json.loads(evaluation.CORPUS.read_bytes())


def test_v2_cases_use_current_source_item_and_decision_contracts():
    value = corpus()
    assert value["schema_version"] == 2
    result = evaluation.check_corpus(value)
    assert result == {"review_cases": 32, "cross_document_cases": 2, "decision_cases": 2}
    for case in value["cases"]:
        payload = case["payload"]
        assert payload["schema_version"] == 2 and payload["operation"] == "review"
        assert "requirements" not in payload
        for view in payload["source_views"]:
            SourceView.model_validate(view)
        for item in payload["items"]:
            TenderItem.model_validate(item)
        for decision in payload["decisions"]:
            Decision.model_validate(decision)
    assert not value["human_review_completed"] and not value["model_evaluation_completed"]
    assert not value["semantic_acceptance"]


def test_export_isolated_v2_inputs_with_budget_and_no_answer_leak(tmp_path):
    root = tmp_path / "export"
    assert evaluation.prepare_evaluation(root, evaluation.CORPUS.read_bytes(), get_prompt("verify").encode()) == 32
    manifest = json.loads((root / "review/manifest.json").read_text())
    assert manifest["schema_version"] == 2 and not manifest["semantic_acceptance"]
    assert manifest["budget"]["planned_requests"] == 32
    for entry in manifest["entries"]:
        payload = json.loads((root / entry["payload_path"]).read_text())
        assert not set(payload) & {"case_id", "expected", "category", "provenance"}
        assert entry["budget"]["fits"]
        assert payload["work_unit_id"] == "w1" and "request_ref" in payload
        assert json.loads((root / entry["reference_mapping_path"]).read_text())["w"]
    assert all(r["status"] == "not_run" and r["adjudication"] is None
               for r in json.loads((root / "review/records.json").read_text()))
    with pytest.raises(FileExistsError):
        evaluation.prepare_evaluation(root, evaluation.CORPUS.read_bytes(), get_prompt("verify").encode())


def test_table_quotes_remain_visible_in_literal_cells_and_amendments_keep_retained_rules():
    value = corpus()
    tables = [r for c in value["cases"] for v in c["payload"]["source_views"]
              for k in ("body", "supporting") for r in v[k] if r.get("cells")]
    assert tables
    assert all("text" in cell and "start" not in cell and "end" not in cell for r in tables for cell in r["cells"])
    amendment = [c for c in value["cases"] if c["payload"]["decisions"]]
    assert len(amendment) == 2
    correct = next(c for c in amendment if c["expected"]["verdict"] == "consistent")
    wrong = next(c for c in amendment if c["expected"]["verdict"] == "incorrect")
    assert correct["payload"]["items"] == wrong["payload"]["items"]
    assert correct["payload"]["decisions"][0]["amendment"]["retained_rules"]
    assert not wrong["payload"]["decisions"][0]["amendment"]["retained_rules"]


@pytest.mark.parametrize("damage", ["duplicate_range", "quote", "item", "annotation", "answer_leak"])
def test_corpus_rejects_invalid_current_contracts(damage):
    value = deepcopy(corpus())
    payload = value["cases"][0]["payload"]
    if damage == "duplicate_range":
        payload["source_views"][0]["body"] *= 2
    elif damage == "quote":
        payload["items"][0]["evidence"][0]["quote"] = "原文没有的引文。"
    elif damage == "item":
        payload["items"][0]["applicable"] = True
    elif damage == "annotation":
        value["human_review_completed"] = True
    else:
        payload["expected"] = value["cases"][0]["expected"]
    with pytest.raises(ValueError):
        evaluation.check_corpus(value)


def test_wrong_prompt_rejected_before_export(tmp_path):
    root = tmp_path / "export"
    with pytest.raises(ValueError):
        evaluation.prepare_evaluation(root, evaluation.CORPUS.read_bytes(), b"old v1 prompt")
    assert not root.exists()
