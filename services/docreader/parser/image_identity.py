"""Pixel identity for freeze locators: decoded size and a canonical MIME."""

from __future__ import annotations

import io

from PIL import Image, ImageOps

_MEDIA_BY_FORMAT = {
    "PNG": "image/png",
    "JPEG": "image/jpeg",
    "JPG": "image/jpeg",
    "WEBP": "image/webp",
}


def image_pixel_identity(raw: bytes) -> tuple[int, int, str]:
    """Return width, height, and canonical MIME after EXIF orientation.

    Stored bytes are not rewritten. Locator dimensions follow the visual
    orientation so inverted-tag JPEGs do not fail later decode/view checks.
    """
    with Image.open(io.BytesIO(raw)) as image:
        image.load()
        oriented = ImageOps.exif_transpose(image)
        try:
            width, height = oriented.size
            fmt = (oriented.format or image.format or "").upper()
        finally:
            if oriented is not image:
                oriented.close()
    media = _MEDIA_BY_FORMAT.get(fmt)
    if media is None:
        raise ValueError(f"unsupported image format {fmt!r}")
    return width, height, media
