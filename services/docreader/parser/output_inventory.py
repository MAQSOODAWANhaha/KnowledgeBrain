"""Lossless export inventory profile of the shared DocReader service.

Text/grids use the existing parser DTOs. The versioned metadata sidecar records
physical carriers and explicit omissions; it never substitutes for source text.
"""
import base64
import hashlib
import json
import re
from importlib.metadata import version
from collections import Counter
from io import BytesIO
from pathlib import Path
from types import SimpleNamespace

from docx import Document as DocxDocument
from docx.oxml import parse_xml
from docx.oxml.ns import qn
from docx.table import Table
from docx.text.paragraph import Paragraph

from docreader.models.document import (
    AttachmentLocator, Document, DocumentLocator, ParagraphImageParent,
    StructuredSourceUnit, StructuredSourceUnitKind, TableCellImageParent,
)
from docreader.parser.docx_parser import (
    _docx_package_image_payloads, _docx_table_anchors, _drawing_units,
)

PROFILE = "output_inventory_v1"


def _entry(unit, part, ordinal, kind, *, bookmarks=None, fields=None, reason=None,
           heading_level=None, field_region=None):
    return {
        "unit_key": unit.key, "part": part, "ordinal": ordinal, "kind": kind,
        "bookmarks": bookmarks or [], "fields": fields or [],
        "heading_level": heading_level, "field_region": field_region,
        "status": "not_checked" if reason else "extracted", "reason": reason,
    }


def _style_names(doc):
    """styleId → `w:name`. Editors renumber `w:styleId` (Word writes plain digits
    after a round trip), so an outline level may only be read from `w:name`."""
    names = {}
    for style in doc.styles.element.iter(qn("w:style")):
        identity = style.get(qn("w:styleId"))
        label = style.find(qn("w:name"))
        if identity and label is not None:
            names[identity] = label.get(qn("w:val")) or ""
    return names


def _heading_level(child, style_names):
    properties = child.find(qn("w:pPr"))
    reference = properties.find(qn("w:pStyle")) if properties is not None else None
    identity = reference.get(qn("w:val")) if reference is not None else None
    match = re.match(r"^heading\s+(\d+)$", (style_names.get(identity or "") or "").strip(), re.IGNORECASE)
    if match is None:
        return None
    try:
        return max(1, int(match.group(1)))
    except ValueError:
        return None


def _field_region(child, stack):
    """Classify a story child against enclosing field regions.

    A table of contents spans several paragraphs: `fldChar begin` opens it, the
    entries follow as ordinary paragraphs, `fldChar end` closes it. Those entries
    repeat chapter titles verbatim, so a reader that cannot see the region reads
    every chapter twice. `stack` carries the open regions across children.
    """
    region = stack[-1] if stack else None
    for node in child.iter(qn("w:fldChar"), qn("w:instrText"), qn("w:fldSimple")):
        if node.tag == qn("w:fldChar"):
            kind = node.get(qn("w:fldCharType"), "")
            if kind == "begin":
                stack.append("field")
                region = region or "field"
            elif kind == "end" and stack:
                region = region or stack[-1]
                stack.pop()
        elif node.tag == qn("w:instrText"):
            if (node.text or "").strip().upper().startswith("TOC"):
                if stack:
                    stack[-1] = "toc"
                region = "toc"
        elif node.get(qn("w:instr"), "").strip().upper().startswith("TOC"):
            region = "toc"
        else:
            region = region or "field"
    return region


def _plain_picture_is_backed(drawing, owner, image_refs):
    """Only remove a redundant carrier when it is exactly a retained picture.

    Mixed graphics, linked images, crop/effect/rotation carriers remain separate
    omissions. A nearby successfully decoded picture is never enough.
    """
    data = drawing.xpath(".//*[local-name()='graphicData']")
    picture_uri = "http://schemas.openxmlformats.org/drawingml/2006/picture"
    if len(data) != 1 or data[0].get("uri") != picture_uri:
        return False
    if len(data[0]) != 1 or data[0][0].tag != f"{{{picture_uri}}}pic":
        return False
    blips = drawing.xpath(".//*[local-name()='blip']")
    if len(blips) != 1 or len(blips[0]):
        return False
    if drawing.xpath(".//*[local-name()='txbxContent' or local-name()='effectLst' or local-name()='effectDag']"):
        return False
    for rect in drawing.xpath(".//*[local-name()='srcRect']"):
        if any(value != "0" for value in rect.attrib.values()):
            return False
    for transform in drawing.xpath(".//*[local-name()='xfrm']"):
        if any(transform.get(name, "0") not in {"0", "false", "off"} for name in ("rot", "flipH", "flipV")):
            return False
    relation = blips[0].get(qn("r:embed"))
    part = owner.part.related_parts.get(relation) if relation else None
    raw = getattr(part, "blob", None)
    if not isinstance(raw, bytes) or not raw:
        return False
    extension = Path(str(getattr(part, "partname", ""))).suffix.lower() or ".bin"
    return f"images/{hashlib.sha256(raw).hexdigest()}{extension}" in image_refs


def _docx(content):
    doc = DocxDocument(BytesIO(content))
    style_names = _style_names(doc)
    units, entries = [], []
    image_ordinal = 0
    parts = {str(part.partname): part for part in doc.part.package.parts}
    stories = [(doc.part, doc.element.body)]
    used_parts = {str(doc.part.partname)}

    def unsupported(part, ordinal, reason):
        unit = StructuredSourceUnit(
            key=f"carrier:{len(units)}", ordinal=len(units),
            kind=StructuredSourceUnitKind.ATTACHMENT_REGION, text="",
            locator=AttachmentLocator(part_name=part, relationship_type="output_carrier"),
        )
        units.append(unit)
        entries.append(_entry(unit, part, ordinal, "not_checked", reason=reason))

    # Resolve inheritance without creating missing header/footer definitions.
    # A first/even story becomes applicable only when that section enables it.
    references = {}
    used_references = []
    even_pages = doc.settings.odd_and_even_pages_header_footer
    for section in doc.element.xpath(".//w:sectPr"):
        for reference in section:
            if reference.tag in {qn("w:headerReference"), qn("w:footerReference")}:
                references[(reference.tag, reference.get(qn("w:type"), "default"))] = reference.get(qn("r:id"))
        enabled = {"default"}
        title_page = section.find(qn("w:titlePg"))
        if title_page is not None and title_page.get(qn("w:val"), "true") not in {"0", "false", "off"}:
            enabled.add("first")
        if even_pages:
            enabled.add("even")
        used_references.extend(relation for (_, story_type), relation in references.items() if story_type in enabled)
    for relation in dict.fromkeys(used_references):
        part = doc.part.related_parts.get(relation)
        if part is None:
            unsupported(str(doc.part.partname), len(entries), f"missing story relationship: {relation}")
            continue
        name = str(part.partname)
        if name not in used_parts:
            used_parts.add(name)
            stories.append((part, parse_xml(part.blob)))

    for kind in ("footnote", "endnote"):
        referenced = {node.get(qn("w:id")) for _, root in stories for node in root.iter(qn(f"w:{kind}Reference"))}
        if not referenced:
            continue
        candidates = [part for rel in doc.part.rels.values()
                      if rel.reltype.endswith(f"/{kind}s") and not rel.is_external
                      for part in [rel.target_part]]
        found = set()
        for part in candidates:
            used_parts.add(str(part.partname))
            root = parse_xml(part.blob)
            for note in root:
                identity = note.get(qn("w:id"))
                if identity in referenced:
                    found.add(identity)
                    stories.append((part, note))
        for missing in sorted(referenced - found):
            unsupported(str(doc.part.partname), len(entries), f"missing {kind}: {missing}")

    part_ordinals = {}
    for part, story in stories:
        name = str(part.partname)
        owner = SimpleNamespace(part=part)
        # Bookmarks that wrap a whole paragraph sit as siblings of `w:p`. Word
        # normalizes them inline, but a freshly compiled document still has them
        # outside, so they belong to the next carrier rather than to nothing.
        block_bookmarks = []
        open_fields = []
        for child in story:
            if child.tag in {qn("w:bookmarkStart"), qn("w:bookmarkEnd")}:
                bookmark = child.get(qn("w:name"))
                if bookmark:
                    block_bookmarks.append(bookmark)
                continue
            ordinal = part_ordinals.get(name, 0)
            part_ordinals[name] = ordinal + 1
            bookmarks = block_bookmarks + [
                node.get(qn("w:name")) for node in child.iter(qn("w:bookmarkStart"))
                if node.get(qn("w:name"))
            ]
            block_bookmarks = []
            fields = [node.text or "" for node in child.iter(qn("w:instrText"))]
            fields += [node.get(qn("w:instr"), "") for node in child.iter(qn("w:fldSimple"))]
            field_region = _field_region(child, open_fields)
            section = len(units)
            locator = DocumentLocator(section_ordinal=section, heading_path="")
            grid = None
            anchors = []
            heading_level = None
            if child.tag == qn("w:p"):
                text = Paragraph(child, owner).text
                heading_level = _heading_level(child, style_names)
                kind = "paragraphs"
            elif child.tag == qn("w:tbl"):
                try:
                    grid, anchors = _docx_table_anchors(Table(child, owner))
                except (ValueError, TypeError) as error:
                    unsupported(name, ordinal, f"unsupported table geometry: {error}")
                    continue
                locator.table_ordinal = ordinal
                text, kind = "", "table"
            elif child.tag == qn("w:sectPr"):
                text, kind = "", "section"
            else:
                unsupported(name, ordinal, f"unsupported story carrier: {child.tag}")
                continue
            unit = StructuredSourceUnit(
                key=f"story:{len(units)}", ordinal=len(units),
                kind=StructuredSourceUnitKind.TABLE_REGION if grid else StructuredSourceUnitKind.SECTION,
                text=text, locator=locator, grid=grid,
            )
            units.append(unit)
            entries.append(_entry(unit, name, ordinal, kind, bookmarks=bookmarks, fields=fields,
                                  heading_level=heading_level, field_region=field_region))
            # Preserve image bytes and their established typed parent identity.
            before = len(units)
            if grid:
                for row, column, cell in anchors:
                    image_ordinal = _drawing_units(cell, owner, units, image_ordinal,
                        TableCellImageParent(section_ordinal=section, table_ordinal=ordinal,
                                             row_ordinal=row, cell_ordinal=column))
            else:
                image_ordinal = _drawing_units(child, owner, units, image_ordinal,
                    ParagraphImageParent(section_ordinal=section, paragraph_ordinal=ordinal))
            extracted_image_refs = {image.locator.original_ref for image in units[before:]}
            for image in units[before:]:
                entries.append(_entry(image, name, ordinal, "image", reason="image requires visual review"))
            # Never silently treat unsupported XML carriers as ordinary text.
            unsupported_tags = {qn(f"w:{tag}") for tag in (
                "drawing", "pict", "object", "txbxContent", "altChunk", "sdt", "del", "ins",
            )}
            for descendant in child.iter():
                if descendant.tag in unsupported_tags:
                    if descendant.tag == qn("w:drawing") and _plain_picture_is_backed(descendant, owner, extracted_image_refs):
                        continue
                    unsupported(name, ordinal, f"visual or unsupported nested carrier: {descendant.tag}")
                elif descendant.tag == qn("w:tbl") and descendant is not child:
                    unsupported(name, ordinal, "nested table requires independent geometry review")
            if fields:
                unsupported(name, ordinal, "field instruction/result consistency requires review")
        if block_bookmarks:
            unsupported(name, part_ordinals.get(name, 0),
                        f"block bookmarks carry no following content: {block_bookmarks}")

    bookmark_counts = Counter(name for entry in entries for name in entry["bookmarks"])
    for entry in list(entries):
        duplicate = [name for name in entry["bookmarks"] if bookmark_counts[name] > 1]
        if duplicate:
            unsupported(entry["part"], entry["ordinal"], f"duplicate bookmarks: {duplicate}")

    # Other content-bearing relationship parts are explicit omissions, not a
    # generic scan of every XML part (styles and settings are not body text).
    for part in parts.values():
        for relation in part.rels.values():
            leaf = relation.reltype.rsplit("/", 1)[-1]
            if leaf not in {"aFChunk", "oleObject", "package", "chart", "diagramData", "comments"}:
                continue
            target = relation.target_ref if relation.is_external else str(relation.target_part.partname)
            unsupported(str(part.partname), len(entries), f"unsupported {leaf} relationship: {target}")

    return Document(content="\n".join(unit.text for unit in units),
                    images=_docx_package_image_payloads(content), structured_source_units=units), entries


def parse_output_inventory(file_name, file_type, content):
    media = file_type.lower().lstrip(".")
    if media == "docx":
        document, entries = _docx(content)
        parser = f"docreader-output-inventory-v1/python-docx/{version('python-docx')}"
        config = {"profile": PROFILE, "preserve_story_occurrences": True}
    elif media == "pdf":
        from docreader.parser.pdf_parser import PDFParser
        document = PDFParser(file_name=file_name, file_type="pdf", output_inventory=True).parse_into_text(content)
        entries = []
        for unit in document.structured_source_units:
            page = unit.locator.page_ordinal
            image = unit.kind == StructuredSourceUnitKind.IMAGE_REGION
            entries.append(_entry(unit, f"pdf:page:{page + 1}", unit.ordinal,
                                  "image" if image else "table" if unit.grid else "pdf_page",
                                  reason="image requires visual review" if image else None))
        parser = f"docreader-output-inventory-v1/pdfium/{version('pypdfium2')}"
        config = {"profile": PROFILE, "preserve_all_pages": True, "preserve_page_images": True, "text_cleaning": False}
    else:
        raise ValueError("output inventory supports exact DOCX and PDF bytes only")
    # Freeze the actual implementation and dependency identities, not just a
    # manually incremented profile label. Config records all extraction knobs.
    from docreader.config import CONFIG
    from docreader.parser import pdf_parser
    implementation = hashlib.sha256()
    for filename in ("output_inventory.py", "docx_parser.py", "pdf_parser.py", "pdf_tables.py"):
        implementation.update(Path(__file__).with_name(filename).read_bytes())
    parser += f"/implementation/{implementation.hexdigest()}/lxml/{version('lxml')}/pillow/{version('Pillow')}"
    config["settings"] = {} if media == "docx" else {
        "pdf_render_dpi": CONFIG.pdf_render_dpi, "pdf_jpeg_quality": CONFIG.pdf_jpeg_quality,
        "pdf_render_max_edge": CONFIG.pdf_render_max_edge,
        "layout_ordering": pdf_parser.LAYOUT_ORDERING,
        "extract_embedded_images": pdf_parser.EXTRACT_EMBEDDED_IMAGES,
    }
    image_sha256 = {ref: hashlib.sha256(base64.b64decode(data)).hexdigest()
                    for ref, data in (document.images or {}).items()}
    manifest = {
        "schema_version": 1, "profile": PROFILE,
        "file_sha256": hashlib.sha256(content).hexdigest(), "parser": parser,
        "config": config, "image_sha256": image_sha256, "page_count": document.metadata.get("page_count"), "units": entries,
    }
    document.metadata["output_inventory_manifest"] = json.dumps(manifest, ensure_ascii=False, separators=(",", ":"))
    return document
