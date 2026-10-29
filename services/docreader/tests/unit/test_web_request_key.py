"""Cross-language identity fixtures shared with Rust tender_replay_tests."""
import pytest

from docreader.integrations.web import _request_key


@pytest.mark.parametrize("key,digest", [
    ("repeat-1", "de0e08ea7a97df492f08ad29cda78a85f8e9017b48b8bf92dd8e0aef8cd9e4d3"),
    ('repeat/"quoted"\\key-1', "6db44703d9875ca8055170a4f0d1de741a3a1acb8f7516fb92b391152547b843"),
])
def test_web_document_identity_matches_rust_golden(key, digest):
    assert _request_key({"project_id": "11111111-1111-4111-8111-111111111111", "request_key": key}) == digest
