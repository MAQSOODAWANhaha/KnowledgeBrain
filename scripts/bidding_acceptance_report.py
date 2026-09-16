#!/usr/bin/env python3
"""Local evidence index for explicit independent local review; never sends inputs to a model."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re

from onlyoffice_product_probe import verify_attached_identity, verify_export_package


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fingerprint(path):
    data = path.read_bytes()
    return {"path": str(path.resolve()), "sha256": sha(data), "byte_length": len(data)}


def read_json(path):
    return json.loads(path.read_text())


def collect(probe, composition, matrix):
    """Require completed attached-probe files, retaining all their byte identities."""
    names = ["attached-identity.json", "initial.docx", "result.json", "run.exit",
             "finalized.docx", "finalized-after-reopen.docx", "finalized-pdf-source.docx",
             "finalized-same-version.pdf", "finalized-export-package.json",
             "finalized-final-report.json", "finalized-pdf-result.json", "finalized-pdf-readback.json"]
    files = {name: fingerprint(probe / name) for name in names}
    files["composition_manifest"] = fingerprint(composition)
    files["acceptance_matrix"] = fingerprint(matrix)
    attached = read_json(probe / "attached-identity.json")
    result = read_json(probe / "result.json")
    package = read_json(probe / "finalized-export-package.json")
    technical = read_json(probe / "finalized-final-report.json")
    binding = {"source": package["source"], "outputs": package["outputs"],
               "analysis_sha256": attached["expected"]["analysis_sha256"],
               "initial_version_id": attached["expected"]["version_id"],
               "evidence_sha256": {name: item["sha256"] for name, item in files.items()}}
    errors = []
    try:
        ticket = dict(attached, workspace=attached["workspace_id"])
        original = (probe / "initial.docx").read_bytes()
        verify_attached_identity(ticket, attached["initial"], original,
                                 lambda path: composition.read_bytes() if path.endswith("composition-report") else original)
        docx = (probe / "finalized.docx").read_bytes()
        verify_export_package(package, technical, {
            "docx": (probe / "finalized-pdf-source.docx").read_bytes(),
            "pdf": (probe / "finalized-same-version.pdf").read_bytes()}, result["final"], docx)
        assert (probe / "run.exit").read_text().strip() == "0", "probe did not finish successfully"
        assert docx == (probe / "finalized-after-reopen.docx").read_bytes(), "reopened DOCX differs"
        for key in ("round_id", "document_set_id", "document_set_sha256", "requirement_set_id", "requirement_set_sha256"):
            assert package["source"][key] == attached["expected"][key], f"source identity changed: {key}"
        assert result["mode"] == "finalize_only" and result["analysis_identity_preserved"] is True
        assert result["native_toc_saved_and_reopened"] is True and result["probe_edits_inserted"] is False
        pdf = read_json(probe / "finalized-pdf-result.json")
        assert result["same_version_pdf"] == pdf, "PDF receipts differ"
        assert pdf["version_id"] == package["source"]["version_id"]
        assert pdf["docx_sha256"] == sha(docx)
        assert pdf["pdf_sha256"] == package["outputs"]["pdf"]["sha256"]
        assert pdf["product_export_flow_tested"] is True and pdf["analysis_identity_preserved"] is True
        assert not any(check["status"] == "fail" for check in technical["checks"]), "technical check failed"
    except (AssertionError, KeyError, TypeError) as error:
        errors.append(str(error) or "probe identity assertion failed")
    criteria = []
    for line in matrix.read_text().splitlines():
        if re.match(r"\|\s*[A-Z]\d+\s*\|", line):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            criteria.append(dict(zip(("id", "source", "expectation", "carrier"), cells)))
    if not criteria or len({c["id"] for c in criteria}) != len(criteria):
        raise ValueError("matrix must contain distinct numbered criteria")
    return binding, files, technical, criteria, errors


def summarize(probe, composition, matrix, observations):
    binding, files, technical, criteria, errors = collect(probe, composition, matrix)
    manual = read_json(observations) if observations else {}
    if observations:
        files["review_observations"] = fingerprint(observations)
    if manual and manual.get("binding") != binding:
        errors.append("review observations belong to different files, matrix or versions")
    items = manual.get("items", [])
    ids = [item.get("id") for item in items]
    if len(set(ids)) != len(ids) or set(ids) - {c["id"] for c in criteria}:
        errors.append("duplicate or unknown review criterion")
    reviews = {item.get("id"): item for item in items}
    matrix_rows = []
    for criterion in criteria:
        item = reviews.get(criterion["id"], {})
        status = item.get("status", "not_checked")
        if status not in ("pass", "fail", "not_checked"):
            errors.append(f"{criterion['id']}: invalid review status")
            status = "fail"
        gaps = []
        if status == "pass":
            for field in ("detail", "candidate_versions", "source_evidence", "docx_location", "pdf_location", "evidence"):
                if not item.get(field):
                    gaps.append(field)
            if not manual.get("reviewer") or not manual.get("reviewed_at"):
                gaps.append("reviewer/reviewed_at")
            if gaps:
                status = "not_checked"
        for evidence in item.get("evidence", []):
            try:
                path = Path(evidence["path"])
                if not path.is_absolute():
                    path = observations.parent / path
                if fingerprint(path)["sha256"] != evidence["sha256"]:
                    raise ValueError("evidence hash changed")
            except (OSError, KeyError, TypeError, ValueError) as error:
                errors.append(f"{criterion['id']}: invalid evidence: {error}")
                status = "fail"
        matrix_rows.append({**criterion, "status": status, "missing_observations": gaps, "review_observation": item})
    # Empty lists are valid for these three distinct categories; absence is not.
    required = ("source_open_items", "deferred_bidder_fields", "extraction_or_composition_errors", "metrics", "provenance")
    missing = [key for key in required if key not in manual]
    for key in required[:3]:
        if key in manual and not isinstance(manual[key], list):
            errors.append(f"{key} must be a list")
    for key in ("metrics", "provenance"):
        if key in manual and not manual[key]:
            missing.append(key)
    if manual.get("extraction_or_composition_errors"):
        errors.append("unresolved extraction or composition errors")
    statuses = [row["status"] for row in matrix_rows]
    status = "fail" if errors or "fail" in statuses else "not_checked" if missing or "not_checked" in statuses else "pass"
    return {"schema_version": 1, "created_at": datetime.now(timezone.utc).isoformat(),
            "scope": "independent local review evidence; no model calls; no product approval mutation",
            "acceptance_status": status, "binding": binding, "files": files,
            "identity_errors": errors, "missing_summary_fields": missing,
            "technical_report": technical, "technical_report_status_unchanged": technical.get("status"),
            "reviewer": manual.get("reviewer"), "reviewed_at": manual.get("reviewed_at"),
            "items": matrix_rows, "summary": {key: manual.get(key) for key in required}}


def write_report(report, output):
    output.mkdir(parents=True, exist_ok=False)
    data = json.dumps(report, ensure_ascii=False, indent=2).encode() + b"\n"
    (output / "report.json").write_bytes(data)
    lines = ["# Local bid acceptance evidence", "", f"Independent local acceptance: **{report['acceptance_status']}**.",
             f"Product technical report status, unchanged: **{report['technical_report_status_unchanged']}**.",
             f"Reviewer: {report['reviewer']}; reviewed at: {report['reviewed_at']}.",
             "This separate independent local review does not change product approval or complete model acceptance for another document.", "",
             "## Final identities", "", "```json", json.dumps(report["binding"], ensure_ascii=False, indent=2), "```", "",
             "## Per-item review", ""]
    for item in report["items"]:
        lines += [f"### {item['id']} — {item['status']}", "", f"Source: {item['source']}",
                  item.get("expectation", ""), "", "```json",
                  json.dumps(item["review_observation"], ensure_ascii=False, indent=2), "```", "",
                  "Missing observations: " + ", ".join(item["missing_observations"]), ""]
    lines += ["## Original technical checks (unchanged)", "", "```json",
              json.dumps(report["technical_report"]["checks"], ensure_ascii=False, indent=2), "```", "",
              "## Gaps and summary", "", "```json", json.dumps({
        "identity_errors": report["identity_errors"], "missing_fields": report["missing_summary_fields"],
        **report["summary"]}, ensure_ascii=False, indent=2), "```", ""]
    (output / "report.md").write_text("\n".join(lines))
    manifest = {"schema_version": 1, "binding": report["binding"],
                "reports": {name: fingerprint(output / name) for name in ("report.json", "report.md")}}
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe-dir", type=Path, required=True)
    parser.add_argument("--composition-manifest", type=Path, required=True)
    parser.add_argument("--matrix", type=Path, required=True)
    parser.add_argument("--observations", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="new directory; never overwrite prior acceptance")
    args = parser.parse_args()
    try:
        report = summarize(args.probe_dir, args.composition_manifest, args.matrix, args.observations)
        write_report(report, args.output)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f"Not accepted; required final evidence unavailable or invalid: {error}\n")
    print(f"acceptance_status={report['acceptance_status']}; technical_report_status={report['technical_report_status_unchanged']}")
    return 0 if report["acceptance_status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
