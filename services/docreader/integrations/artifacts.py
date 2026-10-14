"""Immutable, content-addressed files shared by the explicit Python processes."""

from __future__ import annotations

import hashlib
import json
import mimetypes
import os
import re
import tempfile
from pathlib import Path
from typing import Any, BinaryIO


class ArtifactIntegrityError(ValueError):
    """An artifact reference or the file it identifies has been modified."""


class ArtifactStore:
    def __init__(self, root: str | Path):
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)

    def _write(self, source: BinaryIO, media_type: str) -> dict[str, Any]:
        digest = hashlib.sha256()
        size = 0
        fd, temporary = tempfile.mkstemp(prefix=".artifact-", dir=self.root)
        try:
            with os.fdopen(fd, "wb") as output:
                while chunk := source.read(1024 * 1024):
                    digest.update(chunk)
                    size += len(chunk)
                    output.write(chunk)
                output.flush()
                os.fsync(output.fileno())
            sha256 = digest.hexdigest()
            relative_path = f"{sha256[:2]}/{sha256}"
            target = self.root / relative_path
            target.parent.mkdir(exist_ok=True)
            # link is atomic and cannot overwrite a file another process installed.
            os.chmod(temporary, 0o444)
            try:
                os.link(temporary, target)
                for directory in (target.parent, self.root):
                    directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
                    try:
                        os.fsync(directory_fd)
                    finally:
                        os.close(directory_fd)
            except FileExistsError:
                pass
            ref = {
                "sha256": sha256,
                "relative_path": relative_path,
                "size": size,
                "media_type": media_type,
            }
            self.path(ref)  # Also reject a corrupted pre-existing content address.
            return ref
        finally:
            Path(temporary).unlink(missing_ok=True)

    def put_bytes(self, data: bytes, media_type: str = "application/octet-stream") -> dict[str, Any]:
        from io import BytesIO

        return self._write(BytesIO(data), media_type)

    def put_json(self, value: Any) -> dict[str, Any]:
        payload = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)
        return self.put_bytes(payload.encode("utf-8"), "application/json")

    def import_file(self, path: str | Path) -> dict[str, Any]:
        source = Path(path)
        media_type = mimetypes.guess_type(source.name)[0] or "application/octet-stream"
        with source.open("rb") as stream:
            return self._write(stream, media_type)

    def path(self, ref: dict[str, Any]) -> Path:
        digest = ref.get("sha256", "")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ArtifactIntegrityError("invalid content digest")
        relative_path = f"{digest[:2]}/{digest}"
        if ref.get("relative_path") != relative_path:
            raise ArtifactIntegrityError("invalid content path")
        path = self.root / relative_path
        if path.resolve().parent != (self.root / digest[:2]):
            raise ArtifactIntegrityError("artifact path escapes storage root")
        if not path.is_file() or path.stat().st_size != ref.get("size"):
            raise ArtifactIntegrityError("artifact is missing or its size changed")
        with path.open("rb") as stream:
            actual = hashlib.file_digest(stream, "sha256").hexdigest()
        if actual != digest:
            raise ArtifactIntegrityError("artifact content changed")
        return path

    def read_bytes(self, ref: dict[str, Any]) -> bytes:
        data = self.path(ref).read_bytes()
        if hashlib.sha256(data).hexdigest() != ref["sha256"]:
            raise ArtifactIntegrityError("artifact changed during read")
        return data

    def read_json(self, ref: dict[str, Any]) -> Any:
        return json.loads(self.read_bytes(ref))
