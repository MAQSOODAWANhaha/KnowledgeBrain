from concurrent.futures import ThreadPoolExecutor

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore


def test_content_addressed_json_and_concurrent_writes(tmp_path):
    store = ArtifactStore(tmp_path)
    value = {"标题": "招标文件", "rows": [["", "value"], ["", ""]]}
    with ThreadPoolExecutor(max_workers=4) as pool:
        refs = list(pool.map(store.put_json, [value] * 8))
    assert all(ref == refs[0] for ref in refs)
    assert store.read_json(refs[0]) == value
    assert len(list(tmp_path.glob("*/*"))) == 1
    assert not list(tmp_path.glob(".artifact-*"))


def test_import_is_detached_from_mutable_source(tmp_path):
    source = tmp_path / "source.txt"
    source.write_text("original")
    store = ArtifactStore(tmp_path / "artifacts")
    ref = store.import_file(source)
    source.write_text("changed")
    assert store.read_bytes(ref) == b"original"
    assert ref["media_type"] == "text/plain"


def test_reference_escape_and_content_corruption_rejected(tmp_path):
    store = ArtifactStore(tmp_path)
    ref = store.put_bytes(b"1234")
    with pytest.raises(ArtifactIntegrityError, match="path"):
        store.read_bytes({**ref, "relative_path": "../secret"})
    path = store.path(ref)
    path.chmod(0o644)
    path.write_bytes(b"5678")
    with pytest.raises(ArtifactIntegrityError, match="content changed"):
        store.read_bytes(ref)
    with pytest.raises(ArtifactIntegrityError, match="content changed"):
        store.put_bytes(b"1234")


def test_missing_file_and_nonfinite_json_rejected(tmp_path):
    store = ArtifactStore(tmp_path)
    ref = store.put_bytes(b"data")
    store.path(ref).unlink()
    with pytest.raises(ArtifactIntegrityError, match="missing"):
        store.read_bytes(ref)
    with pytest.raises(ValueError):
        store.put_json({"invalid": float("nan")})
