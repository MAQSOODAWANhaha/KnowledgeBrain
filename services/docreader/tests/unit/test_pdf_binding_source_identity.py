"""A frozen PDF glyph proof cannot be reused as evidence for another source."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import _normalize_layout_symbols
from test_pdf_anfu_symbols import SOURCE_SHA, proof


@pytest.mark.parametrize('problem', ['other_document', 'missing_source', 'string_binding', 'list_binding'])
def test_layout_keeps_the_original_symbol_when_pdf_binding_is_not_its_source(problem):
    item = proof()
    if problem == 'other_document':
        item['pdf_glyph_binding']['source_document_sha256'] = 'b' * 64
    elif problem == 'missing_source':
        item['pdf_glyph_binding'].pop('source_document_sha256')
    elif problem == 'string_binding':
        item['pdf_glyph_binding'] = 'not a binding'
    else:
        item['pdf_glyph_binding'] = []
    raw = {'source_contract': {'document_revision': SOURCE_SHA, 'glyph_normalizations': [item]}}
    original = deepcopy(raw)
    pages, issues = _normalize_layout_symbols(['\uf06c服务条款'], raw)
    assert pages == ['\uf06c服务条款'] and len(issues) == 1
    assert raw == original


def test_layout_accepts_the_exact_pdf_binding_for_its_original_revision():
    raw = {'source_contract': {'document_revision': SOURCE_SHA, 'glyph_normalizations': [proof()]}}
    pages, issues = _normalize_layout_symbols(['\uf06c服务条款'], raw)
    assert pages == ['●服务条款'] and not issues
