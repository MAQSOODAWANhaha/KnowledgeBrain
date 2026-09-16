"""Offline identity/review regressions; byte fixtures are not document acceptance."""
import copy
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).parents[1]))
import bidding_acceptance_report as report


@pytest.fixture
def bundle(tmp_path):
    probe = tmp_path / "probe"
    probe.mkdir()
    def put(name, value):
        (probe / name).write_text(json.dumps(value))
    initial = b"initial synthetic identity fixture"
    final = b"saved synthetic identity fixture"
    pdf = b"%PDF-identity fixture only"
    basis = dict(document_set_id="documents", document_set_sha256="d" * 64,
                 requirement_set_id="requirements", requirement_set_sha256="r" * 64)
    current = dict(project_id="project", workspace_id="workspace", round_id="round",
                   version_id="initial", docx_sha256=report.sha(initial), round_basis=basis,
                   editor=dict(pending_save_id=None, save_error=None))
    composition = tmp_path / "composition.json"
    composition.write_text(json.dumps(dict(status="reviewed_template", docx_sha256=report.sha(initial), analysis_sha256="a" * 64)))
    expected = dict(basis, round_id="round", version_id="initial", docx_sha256=report.sha(initial),
                    analysis_sha256="a" * 64, composition_manifest_sha256=report.sha(composition.read_bytes()))
    put("attached-identity.json", dict(project_id="project", workspace_id="workspace", expected=expected, initial=current))
    source = dict(basis, round_id="round", version_id="final", docx_sha256=report.sha(final))
    outputs = {kind: dict(artifact_id=kind, sha256=report.sha(data), byte_length=len(data)) for kind, data in (("docx", final), ("pdf", pdf))}
    package = dict(source=source, outputs=outputs, assessment_report_sha256="t" * 64)
    technical = dict(schema_version=2, status="needs_review", source=source, outputs=outputs,
                     content_sha256="t" * 64, checks=[dict(id="semantics", status="not_checked")])
    receipt = dict(version_id="final", docx_sha256=report.sha(final), pdf_sha256=report.sha(pdf),
                   product_export_flow_tested=True, analysis_identity_preserved=True)
    put("finalized-export-package.json", package)
    put("finalized-final-report.json", technical)
    put("finalized-pdf-result.json", receipt)
    put("finalized-pdf-readback.json", {"content": "existing unified DocReader evidence fixture"})
    put("result.json", dict(final=source, same_version_pdf=receipt, mode="finalize_only",
                             analysis_identity_preserved=True, native_toc_saved_and_reopened=True, probe_edits_inserted=False))
    (probe / "initial.docx").write_bytes(initial)
    for name in ("finalized.docx", "finalized-after-reopen.docx", "finalized-pdf-source.docx"):
        (probe / name).write_bytes(final)
    (probe / "finalized-same-version.pdf").write_bytes(pdf)
    (probe / "run.exit").write_text("0\n")
    matrix = tmp_path / "matrix.md"
    matrix.write_text("| M01 | source1 | first observation | actual files |\n| M02 | source2 | second observation | actual files |\n")
    observation = tmp_path / "observations.json"
    binding = report.collect(probe, composition, matrix)[0]
    evidence = tmp_path / "review-note.txt"
    evidence.write_text("Independent review observation fixture, not actual review.")
    manual = dict(binding=binding, reviewer="test agent / independent local reviewer", reviewed_at="2026-09-16T00:00:00Z",
                  source_open_items=[], deferred_bidder_fields=["fixture blank"], extraction_or_composition_errors=[],
                  metrics={"model_calls": "not measured in offline fixture"}, provenance={"engine": "fixture"},
                  items=[dict(id=i, status="pass", detail="explicit observation", candidate_versions=["id@version"],
                              source_evidence="section", docx_location="heading/table", pdf_location="page/region",
                              evidence=[dict(path="review-note.txt", sha256=report.sha(evidence.read_bytes()))]) for i in ("M01", "M02")])
    observation.write_text(json.dumps(manual))
    return probe, composition, matrix, observation, manual


def test_explicit_review_does_not_upgrade_technical_report(bundle, tmp_path):
    probe, composition, matrix, observations, _ = bundle
    result = report.summarize(probe, composition, matrix, observations)
    assert result["acceptance_status"] == "pass"
    assert result["reviewer"] == "test agent / independent local reviewer"
    assert "review_observations" in result["files"]
    assert "review_observation" in result["items"][0]
    assert result["technical_report_status_unchanged"] == "needs_review"
    assert result["technical_report"]["checks"][0]["status"] == "not_checked"
    target = tmp_path / "accepted"
    report.write_report(result, target)
    manifest = report.read_json(target / "manifest.json")
    assert manifest["reports"]["report.json"]["sha256"] == report.sha((target / "report.json").read_bytes())
    with pytest.raises(FileExistsError):
        report.write_report(result, target)


def test_absent_or_incomplete_observations_cannot_pass(bundle):
    probe, composition, matrix, observations, manual = bundle
    assert report.summarize(probe, composition, matrix, None)["acceptance_status"] == "not_checked"
    manual["items"].pop()
    manual["items"][0].pop("pdf_location")
    observations.write_text(json.dumps(manual))
    result = report.summarize(probe, composition, matrix, observations)
    assert result["acceptance_status"] == "not_checked"
    assert all(i["status"] == "not_checked" for i in result["items"])


@pytest.mark.parametrize("filename", ["finalized.docx", "finalized-same-version.pdf", "finalized-pdf-readback.json"])
def test_file_change_invalidates_old_review(bundle, filename):
    probe, composition, matrix, observations, _ = bundle
    with (probe / filename).open("ab") as out:
        out.write(b"changed")
    result = report.summarize(probe, composition, matrix, observations)
    assert result["acceptance_status"] == "fail"
    assert result["identity_errors"]


def test_wrong_analysis_identity_and_changed_evidence_fail(bundle):
    probe, composition, matrix, observations, manual = bundle
    manual["binding"]["analysis_sha256"] = "another analysis"
    manual["items"][0]["evidence"][0]["sha256"] = "stale evidence"
    observations.write_text(json.dumps(manual))
    result = report.summarize(probe, composition, matrix, observations)
    assert result["acceptance_status"] == "fail"
    assert len(result["identity_errors"]) == 2


def test_duplicate_reviews_and_explicit_errors_fail(bundle):
    probe, composition, matrix, observations, manual = bundle
    manual["items"].append(copy.deepcopy(manual["items"][0]))
    manual["extraction_or_composition_errors"] = ["fixed label omitted"]
    observations.write_text(json.dumps(manual))
    result = report.summarize(probe, composition, matrix, observations)
    assert result["acceptance_status"] == "fail"


def test_missing_final_files_do_not_create_an_acceptance(bundle):
    probe, composition, matrix, observations, _ = bundle
    (probe / "finalized.docx").unlink()
    with pytest.raises(FileNotFoundError):
        report.summarize(probe, composition, matrix, observations)
