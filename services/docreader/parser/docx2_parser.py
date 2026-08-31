import logging

from docreader.models.document import Document
from docreader.parser.chain_parser import FirstParser
from docreader.parser.docx_parser import DocxParser, _docx_structured_units
from docreader.parser.markitdown_parser import MarkitdownParser

logger = logging.getLogger(__name__)


class Docx2Parser(FirstParser):
    # Tender parsing requires typed source units; MarkItDown only provides
    # markdown, so the structure-preserving parser must be the primary path.
    _parser_cls = (DocxParser, MarkitdownParser)

    def parse_into_text(self, content: bytes) -> Document:
        document = super().parse_into_text(content)
        if document.is_valid() and not document.structured_source_units:
            try:
                document.structured_source_units = _docx_structured_units(content)
            except Exception:
                logger.exception(
                    "failed to attach DOCX structured source units after markitdown"
                )
        return document


if __name__ == "__main__":
    logging.basicConfig(level=logging.DEBUG)

    your_file = "/path/to/your/file.docx"
    parser = Docx2Parser(separators=[".", "?", "!", "。", "？", "！"])
    try:
        with open(your_file, "rb") as f:
            content = f.read()
    except OSError as error:
        raise SystemExit(f"cannot read {your_file}: {error}") from error

    document = parser.parse(content)
    for cc in document.chunks:
        logger.info(f"chunk: {cc}")

        # document = parser.parse_into_text(content)
        # logger.info(f"docx content: {document.content}")
        # logger.info(f"find images {document.images.keys()}")
