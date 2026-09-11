"""Bounded original-page views, sharing the PDF parser's renderer and lock."""
import hashlib
import io
import warnings
from importlib.metadata import version

from PIL import Image, ImageOps

from docreader.config import CONFIG
from docreader.parser.concurrency import parser_worker_limit
from docreader.parser.image_identity import image_pixel_identity
from docreader.parser.pdf_parser import _PDFIUM_LOCK, _render_page_to_jpeg
from docreader.proto.docreader_pb2 import SourceViewResponse


class UnsupportedSourceView(ValueError):
    pass


def source_view(request):
    raw = request.file_content
    if not raw or len(raw) > CONFIG.grpc_max_file_size_mb:
        raise ValueError("source size outside service limits")
    if hashlib.sha256(raw).hexdigest() != request.source_sha256:
        raise ValueError("original digest mismatch")
    if not 0 < request.max_edge <= CONFIG.pdf_render_max_edge:
        raise ValueError("image edge outside service limits")
    if not 0 < request.max_image_bytes <= CONFIG.grpc_max_file_size_mb:
        raise ValueError("image byte budget outside service limits")

    if request.media_type == "application/pdf":
        import pypdfium2 as pdfium

        # Same lock order as PDFParser. Rendering must never race its native
        # text/table extraction in the gRPC thread pool.
        with _PDFIUM_LOCK, parser_worker_limit("pdf_render", CONFIG.pdf_render_max_workers):
            with pdfium.PdfDocument(raw) as document:
                if request.page_ordinal >= len(document):
                    raise ValueError("physical page outside original")
                page = document[request.page_ordinal]
                try:
                    encoded = _render_page_to_jpeg(
                        page, CONFIG.pdf_render_dpi / 72, CONFIG.pdf_jpeg_quality, request.max_edge
                    )
                finally:
                    page.close()
        renderer = f"docreader-source-view-v1/pdfium/{version('pypdfium2')}/pillow/{version('Pillow')}"
    elif request.media_type in {"image/png", "image/jpeg", "image/webp"}:
        if request.page_ordinal != 0:
            raise ValueError("standalone image has no additional pages")
        with warnings.catch_warnings():
            warnings.simplefilter("error", Image.DecompressionBombWarning)
            with Image.open(io.BytesIO(raw)) as original:
                _, _, media_type = image_pixel_identity(raw)
                if media_type != request.media_type:
                    raise ValueError("image media type disagrees with original")
                oriented = ImageOps.exif_transpose(original)
                oriented.thumbnail((request.max_edge, request.max_edge))
                with Image.new("RGB", oriented.size, "white") as rgb, oriented.convert("RGBA") as rgba:
                    rgb.paste(rgba, mask=rgba.getchannel("A"))
                    buffer = io.BytesIO()
                    rgb.save(buffer, format="JPEG", quality=CONFIG.pdf_jpeg_quality, optimize=True)
                    encoded = buffer.getvalue()
                oriented.close()
        renderer = f"docreader-source-view-v1/pillow/{version('Pillow')}"
    else:
        raise UnsupportedSourceView("original-page views support PDF and uploaded images; Office layout is not a physical-page source")
    if len(encoded) > request.max_image_bytes:
        raise ValueError("rendered image exceeds requested byte budget")
    with Image.open(io.BytesIO(encoded)) as image:
        width, height = image.size
    return SourceViewResponse(
        image_data=encoded, media_type="image/jpeg", image_sha256=hashlib.sha256(encoded).hexdigest(),
        source_sha256=request.source_sha256, page_ordinal=request.page_ordinal,
        width=width, height=height, renderer=renderer,
    )
