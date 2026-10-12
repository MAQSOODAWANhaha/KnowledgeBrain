"""Chapter boundaries shared by PDF and DOCX.

Knowledge chunks markdown on ATX headings. Bidding reads the same boundary as
``heading_path``. A page break is not a chapter, and a Word Normal paragraph
that says 「第一章」 is still a chapter.
"""

import re
from typing import Optional

_ZH_CHAPTER = re.compile(
    r"^第[ \t]*[0-9一二三四五六七八九十百千零〇]+[ \t]*(卷|章|节|節|篇|部分)(?:[ \t]*[:：.．]?.*)?$"
)
_EN_CHAPTER = re.compile(
    r"^(Chapter|Part|Section|Kapitel|Teil|Abschnitt)[ \t]+(?:[0-9]+|[IVX]{1,5})"
    r"(?:[ \t]*[:：.．][ \t]*|[ \t]+)\S.*$",
    re.IGNORECASE,
)
_NUMBERED_TITLE = re.compile(
    r"^(?:[0-9]+(?:\.[0-9]+){1,3}|(?:[0-9]+|[IVX]{1,5})\.)[ \t]+\S.*$"
)
_CN_ENUM = re.compile(r"^[一二三四五六七八九十百千]+、\S.*$")
_CN_PAREN = re.compile(r"^[（(][一二三四五六七八九十百千0-9]+[）)]\S.*$")
_HEADING_PUNCT = "。.!！?？,，;；:："


# Leader width is layout-dependent, not semantic; long TOC rows are common.
_INDEX_ENTRY = re.compile(r"(?:[.．…·⋅‐‑―─][ \t]*){2,}(?:[0-9]{1,5}|[ivxlcdmIVXLCDM]+)[ \t]*$")
_INDEX_PREFIX = re.compile(r"(?:[.．…·⋅‐‑―─][ \t]*){2,}$")


def is_index_entry(title: str) -> bool:
    return bool(_INDEX_ENTRY.search(title.strip()))


def is_index_entry_prefix(title: str) -> bool:
    return bool(_INDEX_PREFIX.search(title.strip()))


def explicit_heading_kind(title: str) -> Optional[str]:
    if is_index_entry(title):
        return None
    if zh := _ZH_CHAPTER.match(title.strip()):
        label = zh.group(1)
        return "volume" if label in {"卷", "篇", "部分"} else "chapter" if label == "章" else "section"
    if en := _EN_CHAPTER.match(title.strip()):
        label = en.group(1).lower()
        return "volume" if label in {"part", "teil"} else "chapter" if label in {"chapter", "kapitel"} else "section"
    return None


def subordinate_heading_level(title: str, level: int) -> int:
    """Relative depth below an explicit chapter (Chinese lists start at 2)."""
    if _CN_ENUM.match(title) or _CN_PAREN.match(title):
        return max(1, level - 1)
    return level


def structural_heading(
    line: str,
    allow_numbered: bool = True,
    allow_deep_numbers: bool = True,
) -> Optional[tuple[int, str]]:
    """Return ``(level, title)`` when this line opens a chapter or clause."""
    raw = line.strip()
    if not raw or raw.startswith("#") or is_index_entry(raw) or is_index_entry_prefix(raw):
        return None
    if raw[-1] in _HEADING_PUNCT:
        return None
    if zh := _ZH_CHAPTER.match(raw):
        level = 2 if zh.group(1) in {"节", "節"} else 1
        return level, raw
    if en := _EN_CHAPTER.match(raw):
        level = 2 if en.group(1).lower() in {"section", "abschnitt"} else 1
        return level, raw
    if _CN_ENUM.match(raw):
        return 2, raw
    if _CN_PAREN.match(raw):
        return 3, raw
    if allow_numbered and _NUMBERED_TITLE.match(raw):
        token = raw.split()[0].rstrip(".")
        if not allow_deep_numbers and "." in token:
            return None
        return min(token.count(".") + 1, 6), raw
    return None


def outline_flags(texts: list[str]) -> tuple[bool, bool]:
    """``(allow_numbered, allow_deep_numbers)``.

    A price schedule full of ``1.1`` rows does not become a chapter tree.
    ``第一章`` and ``一、`` stay chapters either way.
    """
    nonempty = 0
    deep = 0
    for text in texts:
        for line in text.splitlines():
            raw = line.strip()
            if not raw or is_index_entry(raw) or is_index_entry_prefix(raw):
                continue
            nonempty += 1
            if raw[-1] in _HEADING_PUNCT or not _NUMBERED_TITLE.match(raw):
                continue
            token = raw.split()[0].rstrip(".")
            if "." in token:
                deep += 1
    flooded = nonempty > 0 and deep * 5 > nonempty * 2
    return True, not flooded


def numbered_headings_flood(texts: list[str]) -> bool:
    """True when deep decimal rows should not open sections."""
    return not outline_flags(texts)[1]


def promote_structural_headings(
    text: str,
    allow_numbered: bool,
    allow_deep_numbers: bool = True,
) -> str:
    if not text:
        return text
    lines = []
    for line in text.splitlines():
        heading = structural_heading(line, allow_numbered, allow_deep_numbers)
        if heading is None:
            lines.append(line)
        else:
            level, title = heading
            lines.append(f"{'#' * level} {title}")
    return "\n".join(lines)
