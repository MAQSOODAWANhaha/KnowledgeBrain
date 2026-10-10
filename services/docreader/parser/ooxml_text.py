"""Deterministic OOXML visible text shared by extraction and export readback.

Runs are formatting boundaries, never word boundaries. Paragraphs and nested
rows are separated by newlines; nested cells by tabs. Field instructions,
deleted revisions and drawing text are excluded (those carriers require their
own structural/visual review); displayed field results are ordinary w:t text.
"""
from docx.oxml.ns import qn


def inline_text(node) -> str:
    if node.tag == qn("w:t"):
        return node.text or ""
    separators = {"w:tab": "\t", "w:br": "\n", "w:cr": "\n",
                  "w:noBreakHyphen": "\u2011", "w:softHyphen": "\u00ad"}
    if node.tag in {qn(tag) for tag in separators}:
        return next(value for tag, value in separators.items() if node.tag == qn(tag))
    if node.tag in {qn(tag) for tag in (
        "w:del", "w:moveFrom", "w:instrText", "w:drawing", "w:pict", "w:object",
    )}:
        return ""
    return "".join(inline_text(child) for child in node)


def cell_text(cell) -> str:
    blocks = []
    for child in cell:
        if child.tag == qn("w:p"):
            blocks.append(inline_text(child))
        elif child.tag == qn("w:tbl"):
            blocks.append("\n".join(
                "\t".join(cell_text(tc) for tc in row if tc.tag == qn("w:tc"))
                for row in child if row.tag == qn("w:tr")
            ))
        elif child.tag in {qn("w:sdt"), qn("w:sdtContent"), qn("w:customXml")}:
            blocks.append(cell_text(child))
    # No strip(): leading/trailing tabs and empty paragraphs can be form blanks.
    return "\n".join(blocks)
