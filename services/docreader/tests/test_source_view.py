"""Exercise the shared authenticated RPC, independent of an LLM provider."""
import hashlib
import io
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import grpc
import pytest
from PIL import Image
from pypdf import PdfWriter

from docreader.auth import AuthInterceptor
from docreader.config import CONFIG
from docreader.main import DocReaderServicer
from docreader.proto.docreader_pb2 import SourceViewRequest
from docreader.proto.docreader_pb2_grpc import DocReaderStub, add_DocReaderServicer_to_server


@pytest.fixture
def rpc(monkeypatch):
    import secrets

    token = secrets.token_hex()
    monkeypatch.setenv("GRPC_AUTH_TOKEN", token)
    server = grpc.server(ThreadPoolExecutor(max_workers=2), interceptors=[AuthInterceptor()])
    add_DocReaderServicer_to_server(DocReaderServicer(), server)
    port = server.add_insecure_port("127.0.0.1:0")
    server.start()
    channel = grpc.insecure_channel(f"127.0.0.1:{port}")
    try:
        yield DocReaderStub(channel), [("authorization", f"Bearer {token}")]
    finally:
        channel.close()
        server.stop(0).wait()


def request(raw, media_type, **overrides):
    return SourceViewRequest(**dict(
        file_content=raw, media_type=media_type, source_sha256=hashlib.sha256(raw).hexdigest(),
        page_ordinal=0, max_edge=min(CONFIG.pdf_render_max_edge, 1000),
        max_image_bytes=min(CONFIG.grpc_max_file_size_mb, 1_000_000),
    ) | overrides)


def pdf():
    document = PdfWriter()
    document.add_blank_page(width=72, height=144)
    document.add_blank_page(width=144, height=72)
    buffer = io.BytesIO()
    document.write(buffer)
    return buffer.getvalue()


def test_rpc_renders_requested_physical_page_with_verifiable_identity(rpc):
    client, metadata = rpc
    raw = pdf()
    first = client.SourceView(request(raw, "application/pdf"), metadata=metadata)
    second = client.SourceView(request(raw, "application/pdf", page_ordinal=1), metadata=metadata)
    assert first.height > first.width
    assert second.width > second.height
    assert first.image_sha256 != second.image_sha256
    assert second.page_ordinal == 1
    assert second.source_sha256 == hashlib.sha256(raw).hexdigest()
    assert second.image_sha256 == hashlib.sha256(second.image_data).hexdigest()


def test_rpc_preserves_uploaded_transparent_image_on_white(rpc):
    client, metadata = rpc
    buffer = io.BytesIO()
    Image.new("RGBA", (80, 40), (0, 0, 0, 0)).save(buffer, "PNG")
    view = client.SourceView(request(buffer.getvalue(), "image/png"), metadata=metadata)
    assert (view.width, view.height) == (80, 40)
    with Image.open(io.BytesIO(view.image_data)) as image:
        assert all(channel > 250 for channel in image.getpixel((40, 20)))


@pytest.mark.parametrize("overrides", [
    {"source_sha256": "0" * 64}, {"page_ordinal": 2}, {"max_edge": 0},
    {"max_edge": CONFIG.pdf_render_max_edge + 1}, {"max_image_bytes": 1},
])
def test_rpc_rejects_changed_original_page_and_budgets(rpc, overrides):
    client, metadata = rpc
    with pytest.raises(grpc.RpcError) as error:
        client.SourceView(request(pdf(), "application/pdf", **overrides), metadata=metadata)
    assert error.value.code() == grpc.StatusCode.INVALID_ARGUMENT


def test_rpc_requires_existing_service_authentication(rpc):
    client, _ = rpc
    with pytest.raises(grpc.RpcError) as error:
        client.SourceView(request(pdf(), "application/pdf"))
    assert error.value.code() == grpc.StatusCode.UNAUTHENTICATED


def test_office_pages_are_explicitly_unsupported(rpc):
    client, metadata = rpc
    with pytest.raises(grpc.RpcError) as error:
        client.SourceView(request(b"office", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"), metadata=metadata)
    assert error.value.code() == grpc.StatusCode.UNIMPLEMENTED


REAL_PDF = Path(__file__).resolve().parents[3] / "testdata/bid/BiddingFile.pdf"


@pytest.mark.skipif(not REAL_PDF.exists(), reason="private real tender sample unavailable")
def test_real_tender_original_view(rpc):
    client, metadata = rpc
    # Physical page 70 contains both G.1/G.2 and the table notes; printed page is 62.
    view = client.SourceView(request(REAL_PDF.read_bytes(), "application/pdf", page_ordinal=69), metadata=metadata)
    assert view.page_ordinal == 69
    assert view.height > view.width > 0
    assert view.image_sha256 == hashlib.sha256(view.image_data).hexdigest()
