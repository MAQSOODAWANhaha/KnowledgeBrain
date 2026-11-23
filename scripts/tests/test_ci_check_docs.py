"""Unit tests (including Red->Green negative fixtures) for scripts/ci_check_docs.py."""
from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import ci_check_docs  # noqa: E402


def _make_minimal_valid_repo(root: pathlib.Path) -> None:
    (root / "CLAUDE.md").write_text("@AGENTS.md\n@LESSONS.md\n", encoding="utf-8")
    (root / "AGENTS.md").write_text("# Agents\n", encoding="utf-8")
    (root / "LESSONS.md").write_text(
        "<!-- reviewed-through: abc1234 (2026-11-23) -->\n# Lessons\n",
        encoding="utf-8",
    )
    (root / "docs").mkdir()
    (root / "docs" / "code-standards.md").write_text("# Standards\n", encoding="utf-8")
    (root / "docs" / "README.md").write_text(
        "# Docs\n- [Standards](code-standards.md)\n- [Lessons](../LESSONS.md)\n",
        encoding="utf-8",
    )
    (root / "scripts").mkdir()
    (root / "scripts" / "ok.py").write_text("print('ok')\n", encoding="utf-8")


class CiCheckDocsTest(unittest.TestCase):
    def test_minimal_valid_repo_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            _make_minimal_valid_repo(root)
            self.assertEqual(ci_check_docs.check_repo(root), [])

    def test_catches_broken_relative_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            _make_minimal_valid_repo(root)
            (root / "AGENTS.md").write_text("See [missing](docs/missing.md)\n", encoding="utf-8")
            errors = ci_check_docs.check_repo(root)
            self.assertTrue(any("broken relative link" in e for e in errors), errors)

    def test_catches_missing_backtick_repo_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            _make_minimal_valid_repo(root)
            (root / "docs" / "code-standards.md").write_text(
                "Run `scripts/does_not_exist.py`.\n", encoding="utf-8"
            )
            errors = ci_check_docs.check_repo(root)
            self.assertTrue(any("does not exist" in e for e in errors), errors)

    def test_catches_forbidden_tmp_path_in_core_doc(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            _make_minimal_valid_repo(root)
            (root / "docs" / "code-standards.md").write_text(
                "Save output to /tmp/evidence-dir\n", encoding="utf-8"
            )
            errors = ci_check_docs.check_repo(root)
            self.assertTrue(any("forbidden /tmp path" in e for e in errors), errors)

    def test_catches_missing_claude_import_and_lessons_cursor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            _make_minimal_valid_repo(root)
            (root / "CLAUDE.md").write_text("@AGENTS.md\n", encoding="utf-8")
            (root / "LESSONS.md").write_text("# No cursor\n", encoding="utf-8")
            errors = ci_check_docs.check_repo(root)
            self.assertTrue(any("@LESSONS.md" in e for e in errors), errors)
            self.assertTrue(any("reviewed-through" in e for e in errors), errors)

    def test_catches_obsolete_docreader_port_env_var(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            _make_minimal_valid_repo(root)
            (root / "docs" / "code-standards.md").write_text(
                "Set `DOCREADER_PORT=50051`.\n", encoding="utf-8"
            )
            errors = ci_check_docs.check_repo(root)
            self.assertTrue(any("DOCREADER_PORT" in e for e in errors), errors)

    def test_real_repository_passes(self) -> None:
        self.assertEqual(ci_check_docs.check_repo(REPO_ROOT), [])


if __name__ == "__main__":
    unittest.main()
