"""Fail-closed PDF symbol decoding from an actual embedded font's cmap.

A PUA code point alone carries no Unicode meaning. Only the verified Wingdings
2 checkbox glyph names below are normalized. All other private glyphs remain
verbatim and make their native carrier explicitly incomplete.
"""
import ctypes
import hashlib
from io import BytesIO
import re
import unicodedata


def is_private_symbol(char):
    return len(char) == 1 and unicodedata.category(char) == "Co"


def verified_checkbox(font_name, codepoint, glyph_names):
    family = re.sub(r"^[A-Z]{6}\+", "", font_name).split(",", 1)[0]
    if family.replace(" ", "").casefold() != "wingdings2":
        return None
    expected = {0xF052: ("boxcheck", "☑"), 0xF0A3: ("box1", "□")}.get(codepoint)
    if expected is None or set(glyph_names) != {expected[0]}:
        return None
    return expected


def _embedded_font(raw, textpage, char_index, cache):
    obj = raw.FPDFText_GetTextObject(textpage, char_index)
    font = raw.FPDFTextObj_GetFont(obj) if obj else None
    if not font or raw.FPDFFont_GetIsEmbedded(font) != 1:
        return None
    identity = ctypes.cast(font, ctypes.c_void_p).value
    if identity in cache:
        return cache[identity]
    size = raw.FPDFFont_GetBaseFontName(font, None, 0)
    if size <= 1 or size > 4096:
        cache[identity] = None
        return None
    name = ctypes.create_string_buffer(size)
    raw.FPDFFont_GetBaseFontName(font, name, size)
    font_name = name.value.decode("utf-8", "replace")
    data_size = ctypes.c_size_t()
    if not raw.FPDFFont_GetFontData(font, None, 0, ctypes.byref(data_size)) or not 0 < data_size.value <= 16 * 1024 * 1024:
        cache[identity] = None
        return None
    data = (ctypes.c_ubyte * data_size.value)()
    if not raw.FPDFFont_GetFontData(font, data, len(data), ctypes.byref(data_size)):
        cache[identity] = None
        return None
    from fontTools.ttLib import TTFont
    payload = bytes(data)
    try:
        with TTFont(BytesIO(payload), lazy=False) as embedded:
            cmap = {}
            for table in embedded["cmap"].tables:
                for codepoint, glyph in table.cmap.items():
                    if codepoint in {0xF052, 0xF0A3}:
                        cmap.setdefault(codepoint, set()).add(glyph)
    except Exception:
        cache[identity] = None
        return None
    cache[identity] = (font_name, hashlib.sha256(payload).hexdigest(), cmap)
    return cache[identity]


def collect_page_symbols(page, page_ordinal, raw, cache):
    """Read local glyph/font evidence without modifying any text or PDF bytes."""
    result = []
    textpage = page.get_textpage()
    try:
        width, height = page.get_size()
        for index in range(textpage.count_chars()):
            char = chr(raw.FPDFText_GetUnicode(textpage, index))
            if not is_private_symbol(char):
                continue
            item = {"page_ordinal": page_ordinal, "char_index": index, "raw_symbol": char}
            try:
                font = _embedded_font(raw, textpage, index, cache)
                mapping = verified_checkbox(font[0], ord(char), font[2].get(ord(char), ())) if font else None
                if mapping:
                    left, bottom, right, top = textpage.get_charbox(index)
                    if not (0 <= left <= right <= width and 0 <= bottom <= top <= height):
                        raise ValueError("symbol is outside the physical page")
                    item.update({"normalized_symbol": mapping[1], "glyph_name": mapping[0],
                                 "font_name": font[0], "font_sha256": font[1],
                                 "left": left, "bottom": bottom, "right": right, "top": top,
                                 "page_width": width, "page_height": height})
            except Exception:
                # Keep the original symbol. Missing/malformed font evidence is
                # not permission to use a font-independent replacement table.
                pass
            result.append(item)
    finally:
        textpage.close()
    return result


def normalize_pdf_units(units, page_symbols):
    """Apply only page-unambiguous verified symbols, preserving provenance."""
    by_page = {}
    for item in page_symbols:
        by_page.setdefault(item["page_ordinal"], {}).setdefault(item["raw_symbol"], []).append(item)
    accepted = []
    maps = {}
    for page, symbols in by_page.items():
        maps[page] = {}
        for char, items in symbols.items():
            normalized = {item.get("normalized_symbol") for item in items}
            if len(normalized) == 1 and None not in normalized:
                maps[page][char] = next(iter(normalized))
                accepted.extend(items)
    for unit in units:
        locator = unit.physical_locator or unit.locator
        page = getattr(locator, "page_ordinal", None)
        replacements = maps.get(page, {})
        def normalize(text):
            return "".join(replacements.get(char, char) for char in text)
        unit.text = normalize(unit.text)
        if unit.grid is not None:
            for cell in unit.grid.cells:
                cell.text = normalize(cell.text)
        if hasattr(unit.locator, "heading_path"):
            unit.locator.heading_path = normalize(unit.locator.heading_path)
        remaining = unit.text + "".join(cell.text for cell in unit.grid.cells) if unit.grid else unit.text
        if any(is_private_symbol(char) for char in remaining):
            unit.source_issues.append("unresolved_private_use_symbol_requires_visual_review")
    return accepted
