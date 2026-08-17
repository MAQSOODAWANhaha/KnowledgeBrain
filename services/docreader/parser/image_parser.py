import base64
import logging

from docreader.models.document import Document
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

        return Document(content=text, images=images)
