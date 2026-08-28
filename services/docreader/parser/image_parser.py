import base64
import logging
from io import BytesIO

from PIL import Image

from docreader.models.document import (
    Document,
    ImageLocator,
    StructuredSourceUnit,
    StructuredSourceUnitKind,
)
from docreader.parser.base_parser import BaseParser

logger = logging.getLogger(__name__)


class ImageParser(BaseParser):
    """Parser for standalone image files.

    Returns the image as a markdown reference with the raw image data
    in Document.images so that the Go-side ImageResolver (or main.py's
    _resolve_images) can handle storage upload.
    """

    def parse_into_text(self, content: bytes) -> Document:
        logger.info("Parsing image file=%s, size=%d bytes", self.file_name, len(content))

        ref_path = f"images/{self.file_name}"

        text = f"![{self.file_name}]({ref_path})"
        images = {ref_path: base64.b64encode(content).decode()}
        with Image.open(BytesIO(content)) as image:
            width, height = image.size
            media_type = Image.MIME.get(image.format or "", "application/octet-stream")

        unit = StructuredSourceUnit(
            key="image:0",
            ordinal=0,
            kind=StructuredSourceUnitKind.IMAGE_REGION,
            text="",
            locator=ImageLocator(
                original_ref=ref_path,
                width=width,
                height=height,
                media_type=media_type,
            ),
        )
        return Document(content=text, images=images, structured_source_units=[unit])
