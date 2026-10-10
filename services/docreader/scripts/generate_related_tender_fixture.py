"""Synthetic related native PDF and XLSX, encoded by production wire adapters."""
from io import BytesIO
from pathlib import Path
import json
import runpy
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
from docreader.parser.pdf_parser import PDFParser
from docreader.main import _structured_units_to_proto
from docreader.proto.docreader_pb2 import ReadResponse


def records():
    writer = PdfWriter()
    page = writer.add_blank_page(width=612, height=792)
    font = writer._add_object(DictionaryObject({NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'), NameObject('/BaseFont'): NameObject('/Helvetica')}))
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
    stream = DecodedStreamObject()
    lines = [
        'Tender requirements: pricing schedule is provided in synthetic.xlsx.',
        'Use the quoted percentage in cell A1 and preserve the currency unit.',
        'The response must include the prescribed pricing schedule and its totals.',
        'An unavailable formula result must remain explicitly unresolved.',
    ]
    stream.set_data(('BT /F1 12 Tf 40 720 Td ' + ' 0 -24 Td '.join(f'({line}) Tj' for line in lines) + ' ET').encode('ascii'))
    page[NameObject('/Contents')] = writer._add_object(stream)
    raw = BytesIO()
    writer.write(raw)
    parsed = PDFParser(file_name='main.pdf', file_type='pdf').parse(raw.getvalue())
    assert not parsed.images, 'synthetic native PDF unexpectedly requires OCR'
    pdf = ReadResponse(markdown_content=parsed.content,
        metadata={k: str(v) for k, v in parsed.metadata.items()},
        structured_source_units=_structured_units_to_proto(parsed.structured_source_units))
    excel = runpy.run_path(str(Path(__file__).with_name('generate_excel_wire_fixture.py')))['response_bytes']()
    return {'main_pdf': pdf.SerializeToString(deterministic=True).hex(), 'pricing_xlsx': excel.hex()}


if __name__ == '__main__':
    Path(sys.argv[1]).write_text(json.dumps(records(), sort_keys=True) + '\n')
