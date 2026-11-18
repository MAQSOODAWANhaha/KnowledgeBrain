"""Manifest role validation is stable across supported Python Enum versions."""

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.evidence.registration import register_manifest
from docreader.tender.status import DocumentRole


@pytest.mark.parametrize('role', [role.value for role in DocumentRole] + list(DocumentRole))
def test_registration_accepts_string_values_and_enum_members(tmp_path, role):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    ref = artifacts.put_bytes(b'synthetic original')
    registered = register_manifest({'documents': [{
        'document_id': 'source', 'document_version': '1', 'role': role, 'original_ref': ref,
    }]}, artifacts)

    assert registered['issues'] == []
    assert registered['documents'][0]['role'] == role
    assert registered['documents'][0]['original_ref'] == ref


def test_registration_defaults_to_main(tmp_path):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    ref = artifacts.put_bytes(b'synthetic original')
    registered = register_manifest({'documents': [{
        'document_id': 'source', 'document_version': '1', 'original_ref': ref,
    }]}, artifacts)

    assert registered['issues'] == []
    assert registered['documents'][0]['role'] == DocumentRole.MAIN


@pytest.mark.parametrize('role', ['unknown', '', None, 1, [], {}])
def test_registration_reports_invalid_roles_including_unhashable_values(tmp_path, role):
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    ref = artifacts.put_bytes(b'synthetic original')
    registered = register_manifest({'documents': [{
        'document_id': 'source', 'document_version': '1', 'role': role, 'original_ref': ref,
    }]}, artifacts)

    assert [issue['code'] for issue in registered['issues']] == ['invalid_document_role']
    assert [issue['code'] for issue in registered['documents'][0]['issues']] == ['invalid_document_role']
    assert registered['documents'][0]['original_ref'] == ref
