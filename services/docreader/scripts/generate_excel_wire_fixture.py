"""Generate a synthetic XLSX through the real parser and production protobuf encoder."""
import argparse
from datetime import datetime
from io import BytesIO
from pathlib import Path
import sys
from lxml import etree as ET
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from openpyxl import Workbook
from docreader.parser.excel_parser import ExcelParser
from docreader.main import _structured_units_to_proto
from docreader.proto.docreader_pb2 import ReadResponse


def response_bytes():
    wb = Workbook()
    wb.properties.created = datetime(2000, 1, 1)
    wb.properties.modified = datetime(2000, 1, 1)
    sheet = wb.active
    sheet.title = "报价"
    for address, value, fmt in [
        ("A1", .15, "0%"), ("B1", datetime(2026, 1, 2), "yyyy-mm-dd"),
        ("C1", 1234.5, '#,##0.00"元"'), ("D1", "=C1*(1+A1)", "0.00"),
        ("E1", "='参数'!A1*2", '0.00"元"'), ("F1", 12, "[Red]0;[Blue]-0"),
        ("G1", "=1/0", "General"), ("H1", "普通文字", "General"),
    ]:
        sheet[address] = value
        sheet[address].number_format = fmt
    wb.create_sheet("参数")["A1"] = 21
    raw = BytesIO()
    wb.save(raw)
    wb.close()
    out = BytesIO()
    # Supply explicit fixture caches as Excel would; never evaluate formulas.
    ns = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
    with zipfile.ZipFile(BytesIO(raw.getvalue())) as source, zipfile.ZipFile(out, "w") as target:
        for name in source.namelist():
            data = source.read(name)
            if name == "xl/worksheets/sheet1.xml":
                root = ET.fromstring(data)
                for cell in root.iter(ns + "c"):
                    if cell.attrib["r"] in {"E1", "G1"}:
                        value = cell.find(ns + "v")
                        if value is None:
                            value = ET.SubElement(cell, ns + "v")
                        value.text = "42" if cell.attrib["r"] == "E1" else "#DIV/0!"
                        if cell.attrib["r"] == "G1":
                            cell.set("t", "e")
                data = ET.tostring(root)
            if name == "docProps/core.xml":
                root = ET.fromstring(data)
                for child in root:
                    if child.tag.rsplit("}", 1)[-1] in {"created", "modified"}:
                        child.text = "2000-01-01T00:00:00Z"
                data = ET.tostring(root)
            info = zipfile.ZipInfo(name, (2000, 1, 1, 0, 0, 0))
            target.writestr(info, data)
    parsed = ExcelParser(file_name="synthetic.xlsx", file_type="xlsx").parse(out.getvalue())
    response = ReadResponse(markdown_content=parsed.content,
                            metadata={k: str(v) for k, v in parsed.metadata.items()},
                            structured_source_units=_structured_units_to_proto(parsed.structured_source_units))
    return response.SerializeToString(deterministic=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_bytes(response_bytes())
