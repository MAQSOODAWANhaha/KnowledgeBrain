"""Fail-closed PDF symbol decoding from an actual embedded font's cmap.

A PUA code point alone carries no Unicode meaning. Verified checkbox glyph names
and explicitly audited embedded font programs are normalized. All other private
glyphs remain verbatim and make their native carrier explicitly incomplete.
"""
import ctypes
import hashlib
from io import BytesIO
import re
import unicodedata


# BUG-178: SymbolMT's generic uniFxxx names do not identify their meaning.
# This exact embedded program was checked against the original visual formula.
_MATH_FONT_SHA256 = "fe2dc455591187a1f55d2476fdb8cf1f2c62d6f5ff2c7867a06465c271ddf188"
_MATH_SYMBOLS = {0xF03D: ("uniF03D", "="), 0xF0B4: ("uniF0B4", "×")}
_AUDITED_SYMBOLS = {
    ("symbolmt", _MATH_FONT_SHA256): _MATH_SYMBOLS,
    ("wingdings-regular", "fa49cf8fbc5f5f9589030eaeae9dd205f750baf282eafe44135486311b2bc268"):
        {0xF06C: ("uniF06C", "●")},
}
_PDF_FONT_SHA256 = "9baff418df77a3d6013f8f3214f85e7a2c20de4dbae9a41768f820f0e6da3a53"
_PDF_ENCODING_SHA256 = "2d004858e0ab7f5c9d37ccf7c60e588d514465e675afc5ea0669b7b543cdd09f"
_PDF_GLYPHS = {0xF06C: (122, "glyph00122", "●"), 0xF0D8: (190, "glyph00190", "➢")}


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


def verified_symbol(font_name, codepoint, glyph_names, font_sha256=None, *, glyph_binding=None):
    if (not isinstance(font_name, str) or type(codepoint) is not int
            or not isinstance(glyph_names, (list, tuple, set, frozenset))
            or any(not isinstance(name, str) for name in glyph_names)):
        return None
    checkbox = verified_checkbox(font_name, codepoint, glyph_names)
    if checkbox:
        return checkbox
    if not isinstance(font_sha256, str):
        return None
    family = re.sub(r"^[A-Z]{6}\+", "", font_name).split(",", 1)[0]
    family = family.replace(" ", "").casefold()
    if family == "wingdings-regular" and font_sha256 == _PDF_FONT_SHA256:
        glyph = _PDF_GLYPHS.get(codepoint)
        if glyph is None or not isinstance(glyph_binding, dict):
            return None
        index, name, unicode = glyph
        required = {"font_sha256": font_sha256, "encoding": "Identity-H", "cid_to_gid_map": "Identity",
                    "to_unicode_sha256": _PDF_ENCODING_SHA256, "source_code": index, "glyph_index": index,
                    "unicode_codepoint": codepoint, "glyph_name": name}
        source_sha = glyph_binding.get("source_document_sha256")
        if (not isinstance(source_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", source_sha)
                or any(glyph_binding.get(key) != value for key, value in required.items())
                or any(type(glyph_binding.get(key)) is not int
                       for key in ("source_code", "glyph_index", "unicode_codepoint"))
                or set(glyph_names) != {name}):
            return None
        return name, unicode
    expected = _AUDITED_SYMBOLS.get((family, font_sha256), {}).get(codepoint)
    if expected is None or set(glyph_names) != {expected[0]}:
        return None
    return expected


def pdf_glyph_bindings(content):
    """Match audited PDF font resources, including the encoding absent from its TTF.

    A second encoding for the same program is ambiguous without object-level
    text ownership. Keep that entire program unresolved rather than guessing.
    """
    from pypdf import PdfReader

    def obj(value):
        return value.get_object() if hasattr(value, "get_object") else value

    def fonts(resources, seen):
        resources = obj(resources)
        if not resources or id(resources) in seen:
            return
        seen.add(id(resources))
        yield from obj(resources.get("/Font", {})).values()
        for reference in obj(resources.get("/XObject", {})).values():
            form = obj(reference)
            if form.get("/Subtype") == "/Form":
                yield from fonts(form.get("/Resources"), seen)

    pages, invalid = {}, set()
    try:
        reader = PdfReader(BytesIO(content))
        for number, page in enumerate(reader.pages):
            for reference in fonts(page.get("/Resources"), set()):
                font = obj(reference)
                family = re.sub(r"^[A-Z]{6}\+", "", str(font.get("/BaseFont", "")).lstrip("/"))
                if family.replace(" ", "").casefold() != "wingdings-regular":
                    continue
                descendants = obj(font.get("/DescendantFonts", []))
                if len(descendants) != 1:
                    continue
                descendant = obj(descendants[0])
                descriptor = obj(descendant.get("/FontDescriptor", {}))
                program = obj(descriptor.get("/FontFile2"))
                if program is None or hashlib.sha256(program.get_data()).hexdigest() != _PDF_FONT_SHA256:
                    continue
                pages.setdefault(number, set()).add(_PDF_FONT_SHA256)
                encoding = str(obj(font.get("/Encoding", ""))).lstrip("/")
                cid_map = str(obj(descendant.get("/CIDToGIDMap", ""))).lstrip("/")
                unicode_map = obj(font.get("/ToUnicode"))
                unicode_sha = hashlib.sha256(unicode_map.get_data()).hexdigest() if unicode_map is not None else None
                if (encoding, cid_map, unicode_sha) != ("Identity-H", "Identity", _PDF_ENCODING_SHA256):
                    invalid.add(_PDF_FONT_SHA256)
    except Exception:
        # No trusted binding was established; the collector preserves the PUA.
        return {}
    source_sha = hashlib.sha256(content).hexdigest()
    return {number: {fingerprint: {codepoint: {
        "source_document_sha256": source_sha, "font_sha256": fingerprint,
        "encoding": "Identity-H", "cid_to_gid_map": "Identity", "to_unicode_sha256": _PDF_ENCODING_SHA256,
        "source_code": index, "glyph_index": index, "unicode_codepoint": codepoint, "glyph_name": name}
        for codepoint, (index, name, _) in _PDF_GLYPHS.items()}
        for fingerprint in fingerprints if fingerprint not in invalid}
        for number, fingerprints in pages.items()}


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
            for table in embedded["cmap"].tables if "cmap" in embedded else []:
                for codepoint, glyph in table.cmap.items():
                    if codepoint in {0xF052, 0xF0A3, 0xF03D, 0xF0B4, 0xF06C}:
                        cmap.setdefault(codepoint, set()).add(glyph)
    except Exception:
        cache[identity] = None
        return None
    cache[identity] = (font_name, hashlib.sha256(payload).hexdigest(), cmap)
    return cache[identity]


def collect_page_symbols(page, page_ordinal, raw, cache, *, font_bindings=None):
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
                binding = (font_bindings or {}).get(page_ordinal, {}).get(font[1], {}).get(ord(char)) if font else None
                glyphs = font[2].get(ord(char), ()) if font else ()
                if not glyphs and isinstance(binding, dict):
                    glyphs = [binding.get("glyph_name")]
                mapping = verified_symbol(font[0], ord(char), glyphs, font[1], glyph_binding=binding) if font else None
                if mapping and font:
                    left, bottom, right, top = textpage.get_charbox(index)
                    if not (0 <= left <= right <= width and 0 <= bottom <= top <= height):
                        raise ValueError("symbol is outside the physical page")
                    item.update({"normalized_symbol": mapping[1], "glyph_name": mapping[0],
                                 "font_name": font[0], "font_sha256": font[1],
                                 "left": left, "bottom": bottom, "right": right, "top": top,
                                 "page_width": width, "page_height": height})
                    if binding is not None:
                        item["pdf_glyph_binding"] = dict(binding)
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
            verified = []
            for item in items:
                mapping = verified_symbol(item.get("font_name"), ord(char), [item.get("glyph_name")],
                                          item.get("font_sha256"), glyph_binding=item.get("pdf_glyph_binding"))
                meaning = item.get("normalized_symbol") if mapping and mapping[1] == item.get("normalized_symbol") else None
                if meaning:
                    verified.append(meaning)
            if not verified:
                continue
            # Keep each glyph. A unanimous page may replace the character; a mixed page must not.
            accepted.extend(items)
            if len(verified) == len(items) and len(set(verified)) == 1:
                maps[page][char] = verified[0]
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
