"""Word navigation is a projection of the saved file, never a semantic rewrite."""

from collections import Counter
from copy import deepcopy
from hashlib import sha256
from io import BytesIO
import re
from zipfile import ZipFile

from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from lxml import etree


def bookmark_name(identity: str, kind: str = "chapter") -> str:
    return ("kb_c_" if kind == "chapter" else "kb_t_") + sha256(identity.encode()).hexdigest()[:28]


def add_bookmark(element, identity: str, kind: str = "chapter") -> None:
    """Anchor existing text or a table cell without adding visible paragraphs."""
    paragraph = element if element.tag == qn("w:p") else next(element.iter(qn("w:p")), None)
    if paragraph is None:
        return
    root = paragraph.getroottree().getroot()
    name = bookmark_name(identity, kind)
    if any(item.get(qn("w:name")) == name for item in root.iter(qn("w:bookmarkStart"))):
        return
    used = [int(item.get(qn("w:id"))) for item in root.iter(qn("w:bookmarkStart"))
            if (item.get(qn("w:id")) or "").isdigit()]
    number = str(max(used, default=0) + 1)
    start, end = OxmlElement("w:bookmarkStart"), OxmlElement("w:bookmarkEnd")
    start.set(qn("w:id"), number)
    start.set(qn("w:name"), name)
    end.set(qn("w:id"), number)
    paragraph.insert(1 if len(paragraph) and paragraph[0].tag == qn("w:pPr") else 0, start)
    paragraph.append(end)


def read_docx_outline(data: bytes, known_chapters: list | None = None) -> dict:
    """Bind saved headings by exact stable bookmark, including renamed/moved ones."""
    parser = etree.XMLParser(resolve_entities=False, no_network=True)
    with ZipFile(BytesIO(data)) as archive:
        for name in ("word/document.xml", "word/styles.xml"):
            if name in archive.namelist() and archive.getinfo(name).file_size > 64 * 1024 * 1024:
                raise ValueError("Word XML exceeds navigation input limit")
        root = etree.fromstring(archive.read("word/document.xml"), parser)
        styles_root = etree.fromstring(archive.read("word/styles.xml"), parser) if "word/styles.xml" in archive.namelist() else None
    styles = {style.get(qn("w:styleId")): style for style in styles_root.iter(qn("w:style"))} if styles_root is not None else {}

    def level(paragraph):
        properties = paragraph.find(qn("w:pPr"))
        direct = properties.find(qn("w:outlineLvl")) if properties is not None else None
        if direct is not None and (direct.get(qn("w:val")) or "").isdigit():
            value = int(direct.get(qn("w:val")))
            return value + 1 if value < 9 else None
        style = properties.find(qn("w:pStyle")) if properties is not None else None
        key, visited = style.get(qn("w:val")) if style is not None else "", set()
        fallback = None
        while key and key not in visited:
            visited.add(key)
            match = re.fullmatch(r"Heading\s*([1-9])", key, re.IGNORECASE)
            if match and fallback is None:
                fallback = int(match[1])
            definition = styles.get(key)
            if definition is None:
                break
            outline = definition.find(f"{qn('w:pPr')}/{qn('w:outlineLvl')}")
            if outline is not None and (outline.get(qn("w:val")) or "").isdigit():
                value = int(outline.get(qn("w:val")))
                return value + 1 if value < 9 else None
            parent = definition.find(qn("w:basedOn"))
            key = parent.get(qn("w:val")) if parent is not None else ""
        return fallback

    known, depths = {}, {}
    def register(chapters, depth=1):
        for chapter in chapters:
            if chapter.get("presentation_role") == "submission_note":
                continue
            name = bookmark_name(chapter["chapter_id"])
            known[name], depths[name] = chapter, depth
            register(chapter.get("children", []), depth + 1)
    register(known_chapters or [])
    bookmarks = [item.get(qn("w:name")) for item in root.iter(qn("w:bookmarkStart"))]
    counts = Counter(bookmarks)
    chapters, stack, bound = [], [], set()
    for ordinal, paragraph in enumerate(root.iter(qn("w:p"))):
        names = [item.get(qn("w:name")) for item in paragraph.iter(qn("w:bookmarkStart"))]
        candidates = [name for name in names if name in known and counts[name] == 1]
        anchor = candidates[0] if len(candidates) == 1 else None
        cover = bool(anchor and known[anchor].get("presentation_role") == "cover")
        depth = depths[anchor] if cover else level(paragraph)
        if depth is None:
            continue
        chapter = deepcopy(known[anchor]) if anchor else {"chapter_id": f"manual:{ordinal}", "targets": [], "manual": True}
        chapter.update(children=[], bookmark=anchor)
        # Only text runs are display text; field instructions are not heading words.
        if not cover:
            chapter["title"] = "".join(item.text or "" for item in paragraph.iter(qn("w:t")))
        if anchor:
            bound.add(anchor)
        for target in chapter.get("targets", []):
            target_anchor = bookmark_name(target["target_id"], "target")
            target["bookmark"] = target_anchor if counts[target_anchor] == 1 else None
        while stack and stack[-1][0] >= depth:
            stack.pop()
        (stack[-1][1]["children"] if stack else chapters).append(chapter)
        stack.append((depth, chapter))
    return {"chapters": chapters, "bookmarks": [name for name, count in counts.items() if name and count == 1],
            "missing_chapter_ids": [chapter["chapter_id"] for name, chapter in known.items() if name not in bound]}
