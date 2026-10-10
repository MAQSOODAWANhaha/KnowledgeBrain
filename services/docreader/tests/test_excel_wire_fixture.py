"""Keep the synthetic cross-language replay bytes tied to the real producer."""
from pathlib import Path
import runpy


def test_excel_wire_fixture_matches_current_python_producer():
    root = Path(__file__).resolve().parents[3]
    producer = runpy.run_path(str(root / "services/docreader/scripts/generate_excel_wire_fixture.py"))
    actual = producer["response_bytes"]()
    expected = (root / "crates/docparser/tests/fixtures/python-excel-wire.pb").read_bytes()
    assert actual == expected
