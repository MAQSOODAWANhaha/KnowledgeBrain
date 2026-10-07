"""Fold parser units into one document tree.

Chapters nest by ``heading_path``. A table, image, form or sheet row hangs on
the chapter or sheet that owns it. Scanned pages stay image leaves; this does
not invent text for them.
"""

import re
from typing import Optional

from pydantic import BaseModel, Field

from docreader.models.document import (
    AttachmentLocator,
    DocumentLocator,
    ImageLocator,
    PageTableLocator,
    SpreadsheetLocator,
    StructuredSourceUnit,
    StructuredSourceUnitKind,
)

_ATX = re.compile(r"^(#{1,6})[ \t]+(.+?)\s*#*\s*$")


class DocumentNode(BaseModel):
    kind: str
    key: str = ""
    keys: list[str] = Field(default_factory=list)
    section_ordinal: Optional[int] = None
    heading_path: str = ""
    title: str = ""
    children: list["DocumentNode"] = Field(default_factory=list)


def build_document_tree(
    units: list[StructuredSourceUnit],
    content: str = "",
) -> DocumentNode:
    root = DocumentNode(kind="document")
    ordered = sorted(units, key=lambda unit: unit.ordinal)
    if not ordered:
        if content.strip():
            _sections_from_markdown(root, content)
        return root

    current: Optional[DocumentNode] = None
    sheet: Optional[DocumentNode] = None
    for unit in ordered:
        if _is_sheet(unit):
            sheet = DocumentNode(
                kind="sheet",
                key=unit.key,
                keys=[unit.key],
                title=_sheet_title(unit),
            )
            root.children.append(sheet)
            current = None
            continue
        if unit.kind is StructuredSourceUnitKind.SECTION and isinstance(
            unit.locator, DocumentLocator
        ):
            current = _ensure_section(
                root, unit.locator.heading_path, unit.locator.section_ordinal
            )
            if unit.key not in current.keys:
                current.keys.append(unit.key)
            sheet = None
            continue
        parent = _owner(root, current, sheet, unit)
        parent.children.append(
            DocumentNode(
                kind=_leaf_kind(unit),
                key=unit.key,
                keys=[unit.key],
                title=_leaf_title(unit),
            )
        )
    return root


def _is_sheet(unit: StructuredSourceUnit) -> bool:
    return (
        unit.kind is StructuredSourceUnitKind.SECTION
        and isinstance(unit.locator, SpreadsheetLocator)
        and ":row:" not in unit.key
        and ":table:" not in unit.key
        and not unit.key.endswith(":used")
    )


def _sheet_title(unit: StructuredSourceUnit) -> str:
    if isinstance(unit.locator, SpreadsheetLocator) and unit.locator.sheet_name:
        return unit.locator.sheet_name
    return unit.text


def _leaf_kind(unit: StructuredSourceUnit) -> str:
    if unit.kind is StructuredSourceUnitKind.TABLE_ROW:
        return "row"
    if unit.kind is StructuredSourceUnitKind.TABLE_REGION:
        return "table"
    if unit.kind is StructuredSourceUnitKind.FORM_REGION:
        return "form"
    if unit.kind is StructuredSourceUnitKind.IMAGE_REGION:
        return "image"
    if unit.kind is StructuredSourceUnitKind.ATTACHMENT_REGION:
        return "attachment"
    return "text"


def _leaf_title(unit: StructuredSourceUnit) -> str:
    if isinstance(unit.locator, SpreadsheetLocator):
        return unit.locator.region.a1_range
    if isinstance(unit.locator, ImageLocator):
        return unit.locator.original_ref
    if isinstance(unit.locator, AttachmentLocator):
        return unit.locator.part_name
    if isinstance(unit.locator, PageTableLocator):
        return f"page:{unit.locator.page_ordinal}:table:{unit.locator.table_ordinal}"
    return ""


def _owner(
    root: DocumentNode,
    current: Optional[DocumentNode],
    sheet: Optional[DocumentNode],
    unit: StructuredSourceUnit,
) -> DocumentNode:
    locator = unit.locator
    if isinstance(locator, DocumentLocator):
        return _ensure_section(root, locator.heading_path, locator.section_ordinal)
    if isinstance(locator, SpreadsheetLocator) and sheet is not None:
        return sheet
    if isinstance(locator, ImageLocator) and locator.compound_parent is not None:
        parent = locator.compound_parent
        return _section_by_ordinal(root, parent.section_ordinal) or current or root
    return current or root


def _ensure_section(
    root: DocumentNode, heading_path: str, section_ordinal: int
) -> DocumentNode:
    parts = [part for part in heading_path.split(" > ") if part]
    if not parts:
        found = _section_by_ordinal(root, section_ordinal)
        if found is not None and found.heading_path == "":
            return found
        node = DocumentNode(
            kind="section",
            section_ordinal=section_ordinal,
            heading_path="",
            title="",
        )
        root.children.append(node)
        return node
    parent = root
    path: list[str] = []
    node: Optional[DocumentNode] = None
    for index, part in enumerate(parts):
        path.append(part)
        full = " > ".join(path)
        node = next(
            (
                child
                for child in parent.children
                if child.kind == "section" and child.heading_path == full
            ),
            None,
        )
        if node is None:
            node = DocumentNode(
                kind="section",
                heading_path=full,
                title=part,
                section_ordinal=section_ordinal if index == len(parts) - 1 else None,
            )
            parent.children.append(node)
        parent = node
    assert node is not None
    if node.section_ordinal is None:
        node.section_ordinal = section_ordinal
    return node


def _section_by_ordinal(root: DocumentNode, section_ordinal: int) -> Optional[DocumentNode]:
    found: list[DocumentNode] = []

    def walk(node: DocumentNode) -> None:
        if node.kind == "section" and node.section_ordinal == section_ordinal:
            found.append(node)
        for child in node.children:
            walk(child)

    walk(root)
    return found[-1] if found else None


def _sections_from_markdown(root: DocumentNode, content: str) -> None:
    current = root
    buf: list[str] = []

    def flush() -> None:
        nonlocal buf
        text = "\n".join(buf).strip()
        buf = []
        if not text:
            return
        parent = current if current.kind == "section" else root
        parent.children.append(DocumentNode(kind="text", title=text))

    for line in content.splitlines():
        match = _ATX.match(line.strip())
        if match is None:
            if line.strip():
                buf.append(line)
            continue
        flush()
        level = len(match.group(1))
        title = match.group(2).strip()
        parent = root
        # Walk existing section depth so a new heading of level N sits under the
        # nearest open ancestor shorter than N.
        ancestors = _open_ancestors(root)
        if level > 1 and len(ancestors) >= level - 1:
            parent = ancestors[level - 2]
        elif level > 1 and ancestors:
            parent = ancestors[-1]
        heading = title if parent is root or not parent.heading_path else f"{parent.heading_path} > {title}"
        current = DocumentNode(kind="section", heading_path=heading, title=title)
        parent.children.append(current)
    flush()


def _open_ancestors(root: DocumentNode) -> list[DocumentNode]:
    """Rightmost section spine, root excluded."""
    spine: list[DocumentNode] = []
    node = root
    while True:
        sections = [child for child in node.children if child.kind == "section"]
        if not sections:
            return spine
        spine.append(sections[-1])
        node = sections[-1]
