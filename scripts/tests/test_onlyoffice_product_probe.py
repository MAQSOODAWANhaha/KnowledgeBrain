"""Acceptance must retain actual analysis lineage and both exported files."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
import onlyoffice_product_probe as probe


class ExportIdentityTests(unittest.TestCase):
    def setUp(self):
        self.docx = b"saved docx identity fixture"
        self.sha = hashlib.sha256(self.docx).hexdigest()
        self.basis = dict(document_set_id="documents", document_set_sha256="d" * 64,
                          requirement_set_id="analysis", requirement_set_sha256="a" * 64)
        self.current = dict(project_id="project", workspace_id="workspace", round_id="round",
                            version_id="version", docx_sha256=self.sha, round_basis=self.basis,
                            editor=dict(pending_save_id=None, save_error=None))
        self.ticket = dict(project_id="project", workspace="workspace", expected=dict(
            round_id="round", version_id="version", docx_sha256=self.sha, **self.basis))

    def download(self, path):
        assert path.endswith("/download"), "outline probe must not require a retired composition report"
        return self.docx

    def test_attach_requires_actual_saved_docx_and_basis(self):
        self.assertEqual(probe.verify_attached_identity(self.ticket, self.current, self.docx, self.download), self.current)
        for key in self.basis:
            changed = copy.deepcopy(self.current)
            changed["round_basis"][key] = "fixture replacement"
            with self.subTest(key=key), self.assertRaises(AssertionError):
                probe.verify_attached_identity(self.ticket, changed, self.docx, self.download)
        changed = copy.deepcopy(self.ticket)
        changed["expected"]["docx_sha256"] = "forged locally"
        with self.assertRaises(AssertionError):
            probe.verify_attached_identity(changed, self.current, self.docx, self.download)
        with self.assertRaises(AssertionError):
            probe.verify_attached_identity(self.ticket, self.current, self.docx + b"edited", self.download)

    def test_report_and_pdf_cannot_come_from_another_version(self):
        outputs = dict(docx=self.docx, pdf=b"%PDF-1.7 identity fixture")
        identities = {kind: dict(artifact_id=kind, sha256=hashlib.sha256(data).hexdigest(), byte_length=len(data))
                      for kind, data in outputs.items()}
        source = dict(round_id="round", version_id="version", docx_sha256=self.sha)
        package = dict(source=source, outputs=identities, assessment_report_sha256="c" * 64)
        report = dict(schema_version=2, source=source, outputs=identities, content_sha256="c" * 64,
                      checks=[dict(id="semantic", status="not_checked", detail="fixture")])
        probe.verify_export_package(package, report, outputs, self.current, self.docx)
        wrong = copy.deepcopy(report)
        wrong["source"]["version_id"] = "later-version"
        with self.assertRaises(AssertionError):
            probe.verify_export_package(package, wrong, outputs, self.current, self.docx)
        with self.assertRaises(AssertionError):
            probe.verify_export_package(package, report, dict(outputs, pdf=b"%PDF-different"), self.current, self.docx)


if __name__ == "__main__":
    unittest.main()
