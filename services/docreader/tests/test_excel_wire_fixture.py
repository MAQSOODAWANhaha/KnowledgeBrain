"""Keep the synthetic cross-language replay bytes tied to the real producer."""
from pathlib import Path
import runpy


def test_excel_wire_fixture_matches_current_python_producer():
    root = Path(__file__).resolve().parents[3]
    producer = runpy.run_path(str(root / "services/docreader/scripts/generate_excel_wire_fixture.py"))
    actual = producer["response_bytes"]()
    expected = (root / "crates/docparser/tests/fixtures/python-excel-wire.pb").read_bytes()
    assert actual == expected


def test_exact_native_grid_retains_its_own_formula_and_display_metadata():
    from docreader.proto.docreader_pb2 import ReadResponse
    import json
    root = Path(__file__).resolve().parents[3]
    producer = runpy.run_path(str(root / "services/docreader/scripts/generate_excel_wire_fixture.py"))
    response = ReadResponse.FromString(producer["response_bytes"]())
    contract = json.loads(response.metadata["source_contract"])
    unit = next(unit for unit in contract["units"] if unit["unit_id"] == "sheet:0:used")
    cells = unit["physical_locator"]["cells"]
    assert next(cell for cell in cells if cell["address"] == "A1")["display_text"] == "15%"
    formula = next(cell for cell in cells if cell["address"] == "D1")
    assert formula["formula"] == "=C1*(1+A1)"
    assert formula["display_incomplete_reason"] == "formula_cached_value_missing"
