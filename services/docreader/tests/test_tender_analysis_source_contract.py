"""Check manually verified source anchors through the shared service entrypoint.

This is parser/source preservation coverage, not an LLM extraction evaluation.
"""
import hashlib
import json
import re
from pathlib import Path

import pytest

from docreader.main import DocReaderServicer
from docreader.proto.docreader_pb2 import ReadRequest

REPO = Path(__file__).resolve().parents[3]
EXPECTATIONS = REPO / "crates/bidding/tests/fixtures/tender-analysis-golden-v1.json"
SOURCE = REPO / json.loads(EXPECTATIONS.read_text())["source"]


@pytest.mark.skipif(not SOURCE.is_file(), reason="private real tender sample is not available")
def test_shared_service_preserves_cybersecurity_tender_anchors():
    fixture = json.loads(EXPECTATIONS.read_text())
    source = REPO / fixture["source"]
    raw = source.read_bytes()
    assert hashlib.sha256(raw).hexdigest() == fixture["sha256"], "recheck expectations for changed original"
    parsed, _ = DocReaderServicer()._parse_request(
        ReadRequest(file_name=source.name, file_type=source.suffix.lstrip("."), file_content=raw)
    )
    by_page = {}
    for unit in parsed.structured_source_units:
        ordinal = getattr(unit.locator, "page_ordinal", None)
        if ordinal is None:
            continue
        by_page.setdefault(ordinal + 1, []).append(unit.text or "")
        for cell in getattr(unit.locator, "cells", []) or []:
            by_page[ordinal + 1].append(cell.text or "")
        grid = getattr(unit, "grid", None)
        if grid is not None:
            for cell in grid.cells:
                by_page[ordinal + 1].append(cell.text or "")
    compact = lambda text: re.sub(r"\s+", "", text)
    for case in fixture["cases"]:
        for anchor in case["evidence"]:
            page = compact("\n".join(by_page.get(anchor["page"], [])))
            assert compact(anchor["text"]) in page, (case["id"], anchor)
