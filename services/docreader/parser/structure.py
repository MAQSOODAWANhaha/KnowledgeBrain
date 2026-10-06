"""Chapter boundaries shared by PDF and DOCX.

Knowledge chunks markdown on ATX headings. Bidding reads the same boundary as
``heading_path``. A page break is not a chapter, and a Word Normal paragraph
that says 「第一章」 is still a chapter.
"""

import re
from typing import Optional

_ZH_CHAPTER = re.compile(
    r"^第[ \t]*[0-9一二三四五六七八九十百千零〇]+[ \t]*(章|节|節|篇|部分)(?:[ \t]*[:：.．]?.*)?$"
)
_EN_CHAPTER = re.compile(
    r"^(Chapter|Part|Section|Kapitel|Teil|Abschnitt)[ \t]+(?:[0-9]+|[IVX]{1,5})"
    r"(?:[ \t]*[:：.．][ \t]*|[ \t]+)\S.*$",
    re.IGNORECASE,
)
_NUMBERED_TITLE = re.compile(
    r"^(?:[0-9]+(?:\.[0-9]+){1,3}|(?:[0-9]+|[IVX]{1,5})\.)[ \t]+\S.*$"
)
_HEADING_PUNCT = "。.!！?？,，;；:："


def structural_heading(line: str, allow_numbered: bool = True) -> Optional[tuple[int, str]]:
    """Return ``(level, title)`` when this line opens a chapter or clause."""
    raw = line.strip()
    if not raw or raw.startswith("#") or len(raw) > 40:
        return None
    if raw[-1] in _HEADING_PUNCT:
        return None
    zh = _ZH_CHAPTER.match(raw)
    if zh:
        level = 2 if zh.group(1) in {"节", "節"} else 1
        return level, raw
    en = _EN_CHAPTER.match(raw)
    if en:
        level = 2 if en.group(1).lower() in {"section", "abschnitt"} else 1
        return level, raw
    if allow_numbered and _NUMBERED_TITLE.match(raw):
        token = raw.split()[0].rstrip(".")
        return min(token.count(".") + 1, 6), raw
    return None


def numbered_headings_flood(texts: list[str]) -> bool:
    """A price list of ``1.1`` rows is not a chapter tree."""
    nonempty = 0
    numbered = 0
    for text in texts:
        for line in text.splitlines():
            raw = line.strip()
            if not raw:
                continue
            nonempty += 1
            if (
                len(raw) <= 40
                and raw[-1] not in _HEADING_PUNCT
                and _NUMBERED_TITLE.match(raw)
            ):
                numbered += 1
    return nonempty > 0 and numbered * 5 > nonempty * 2


def promote_structural_headings(text: str, allow_numbered: bool) -> str:
    if not text:
        return text
    lines = []
    for line in text.splitlines():
        heading = structural_heading(line, allow_numbered)
        if heading is None:
            lines.append(line)
        else:
            level, title = heading
            lines.append(f"{'#' * level} {title}")
    return "\n".join(lines)
