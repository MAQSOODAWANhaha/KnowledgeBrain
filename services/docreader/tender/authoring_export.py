"""Assemble editable drafts and original evidence without making missing facts a gate."""

from __future__ import annotations

from collections import defaultdict
from io import BytesIO
import json
import mimetypes
import re
from zipfile import ZIP_DEFLATED, ZipFile

from docx import Document
from docx.enum.table import WD_CELL_VERTICAL_ALIGNMENT
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Mm, Pt
from PIL import Image, ImageOps

from docreader.tender.authoring_templates import load_template_part, template_part_key
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.word_navigation import add_bookmark


DOCX_MEDIA_TYPE = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
PENDING_ANALYSIS_NOTE = "分析整理（待确认）：摘要及适用范围仍需结合原文和有效补遗核对。"


def _hard_paragraph_break(previous, following):
    """A PDF line wrap is not a new paragraph. A finished sentence, a citation, or a new item is."""
    if following.startswith("《") or re.match(r"^(?:\d+[.、．]|[（(]\d+[）)]|[一二三四五六七八九十]+、)", following):
        return True
    return previous.endswith(("。", "！", "？", "；", "：", ":", "》"))


def _display_line(line):
    return re.sub(r"^( {0,3})#{1,6}[ \t]+", r"\1", line)


def _flow_lines(lines):
    paragraphs, buffer = [], []
    def flush():
        if buffer:
            paragraphs.append("".join(buffer))
            buffer.clear()
    for line in lines:
        visible = line.strip()
        if not visible:
            flush()
            continue
        if buffer and _hard_paragraph_break(buffer[-1].strip(), visible):
            flush()
        buffer.append(line[:len(line) - len(line.lstrip())] + visible)
        if visible.endswith(("。", "！", "？", "；")):
            flush()
    flush()
    return paragraphs


def _paragraphs_from_pieces(pieces):
    """Join adjacent slices of one source range unless the source marks a real paragraph."""
    paragraphs, buffer, previous = [], [], None

    def flush():
        if buffer:
            paragraphs.extend(_flow_lines(buffer))
            buffer.clear()

    for piece in pieces:
        if previous is not None:
            same = previous["range_id"] == piece["range_id"] and piece["start"] >= previous["end"]
            cut = any(offset in previous["breaks"] for offset in range(previous["end"], piece["start"] + 1))
            if not same or cut:
                flush()
        cursor = piece["start"]
        for index, line in enumerate(piece["text"].split("\n")):
            if index:
                if cursor in piece["breaks"]:
                    flush()
                cursor += 1
            cleaned = _display_line(line)
            if cleaned.strip():
                buffer.append(cleaned)
            elif buffer:
                flush()
            cursor += len(line)
        previous = piece
    flush()
    return [item for item in paragraphs if item]


def _template_paragraphs(text, breaks, start=0):
    """A whole form keeps its line breaks. Only a recorded paragraph boundary starts a new paragraph."""
    if not breaks:
        shown = re.sub(r"(?m)^( {0,3})#{1,6}[ \t]+", r"\1", text)
        return [shown] if shown.strip() else []
    chunks, chunk = [], []
    for index, char in enumerate(text):
        if char == "\n" and start + index in breaks:
            chunks.append("".join(chunk))
            chunk = []
        else:
            chunk.append(char)
    chunks.append("".join(chunk))
    shown = []
    for chunk in chunks:
        value = re.sub(r"(?m)^( {0,3})#{1,6}[ \t]+", r"\1", chunk)
        if value.strip():
            shown.append(value)
    return shown


def _display_title(title):
    """Drop a source appendix locator when a business title remains. Do not invent one."""
    match = re.match(r"技术附表[A-Za-z0-9]+\s+(\S.*)", title or "")
    return match.group(1) if match else title


def _source_key(reference):
    return reference["source_index_ref"]["sha256"], reference["unit_id"]


def _requirement_key(reference):
    return json.dumps({key: value for key, value in reference.items() if value is not None},
                      ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _requirement_value(value, *, parent_label=None):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(_requirement_value(item) for item in value)
    if isinstance(value, dict):
        labels = {"value": "数值", "unit": "单位", "quote": "原文", "method": "评分方法",
                  "points": "分值", "max_points": "最高分", "effect": "后果", "trigger": "触发条件",
                  "fields": "填写字段", "documents": "提交材料"}
        values = []
        for index, (field, item) in enumerate(value.items()):
            label = labels.get(field, "待核内容")
            prefix = "" if index == 0 and label == parent_label else f"{label}："
            values.append(prefix + _requirement_value(item))
        return "\n".join(values)
    if isinstance(value, bool):
        return "是" if value else "否"
    return "待核" if value is None else str(value)


def _requirement_text(rule):
    values = [_requirement_value(rule["text"])]
    labels = {"subject": "主体", "scope": "适用范围",
              "conditions": "条件", "exceptions": "例外", "numbers": "量化事实", "scoring": "评分细则",
              "consequence": "后果", "submission": "提交要求"}
    for field, label in labels.items():
        value = rule.get(field)
        if value is None or value == "" or value == [] or value == {}:
            continue
        if ((field in {"subject", "scope"} and not isinstance(value, str))
                or (field in {"conditions", "exceptions"} and (
                    not isinstance(value, list) or any(not isinstance(item, str) for item in value)))
                or (field == "numbers" and not isinstance(value, list))
                or (field in {"scoring", "consequence", "submission"} and not isinstance(value, dict))):
            label += "（待核）"
        values.append(f"{label}：{_requirement_value(value, parent_label=label)}")
    return "\n".join(values)


def _walk(chapters, prefix=()):
    for ordinal, chapter in enumerate(chapters, 1):
        path = (*prefix, ordinal)
        yield chapter, path
        yield from _walk(chapter.get("children", []), path)


def _body_entries(entries):
    notes = [path for chapter, path in entries if chapter.get("presentation_role") == "submission_note"]
    return [(chapter, path) for chapter, path in entries
            if not any(path[:len(parent)] == parent for parent in notes)]


def _display_paths(entries):
    paths, identities, counts = {}, {}, defaultdict(int)
    for chapter, path in _body_entries(entries):
        parent = paths.get(path[:-1], ())
        if chapter.get("presentation_role", "body") == "cover":
            paths[path] = parent
            continue
        counts[parent] += 1
        paths[path] = (*parent, counts[parent])
        identities[chapter["chapter_id"]] = paths[path]
    return identities


def _page_width(document):
    section = document.sections[-1]
    return section.page_width - section.left_margin - section.right_margin


def _table_layout(document, table, widths=None, header_count=0):
    available = _page_width(document)
    widths = widths or [available / len(table.columns)] * len(table.columns)
    factor = min(1, available / sum(widths))
    widths = [int(width * factor) for width in widths]
    table.autofit = False
    for column, width in zip(table.columns, widths):
        column.width = width
    for number, row in enumerate(table.rows):
        if number < header_count:
            header = OxmlElement("w:tblHeader")
            header.set(qn("w:val"), "true")
            row._tr.get_or_add_trPr().append(header)
        for cell, width in zip(row.cells, widths):
            cell.width = width
            cell.vertical_alignment = WD_CELL_VERTICAL_ALIGNMENT.TOP
    return widths


def _toc(document):
    document.add_paragraph("目录", style="Title")
    paragraph = document.add_paragraph()
    for kind in ("begin", "instruction", "separate", "end"):
        element = OxmlElement("w:instrText" if kind == "instruction" else "w:fldChar")
        if kind == "instruction":
            element.set(qn("xml:space"), "preserve")
            element.text = ' TOC \\o "1-9" \\h \\z \\u '
        else:
            element.set(qn("w:fldCharType"), kind)
            if kind == "begin":
                element.set(qn("w:dirty"), "true")
        paragraph.add_run()._r.append(element)
        if kind == "separate":
            paragraph.add_run("请在文字处理软件中更新目录以显示页码。")
    settings = document.settings.element
    update = settings.find(qn("w:updateFields"))
    if update is None:
        update = OxmlElement("w:updateFields")
        settings.append(update)
    update.set(qn("w:val"), "true")


def _submission_notes(outline, requirements, gaps, artifacts=None):
    entries = list(_walk(outline.get("chapters", [])))
    rules = {_requirement_key(row["rule_ref"]): row for row in (requirements or {}).get("rules", [])}
    assigned = {_requirement_key(ref) for chapter, _ in entries for target in chapter.get("targets", [])
                for ref in target.get("rule_refs", [])}
    constraints = outline.get("constraint_rule_refs", [])
    assigned.update(_requirement_key(ref) for ref in constraints)
    assigned.update(_requirement_key(ref) for issue in outline.get("issues", [])
                    if issue["code"] in {"outline_target_excluded", "outline_rule_excluded"}
                    for ref in issue.get("rule_refs", []))
    lines = ["# 提交要求与待办", "", "以下内容来自本次固定分析和编制结果，供人工整理提交；不表示已完成履约或企业应答。", ""]
    if requirements is not None and not requirements.get("authoring_input", {}).get("confirmation_ref"):
        lines.extend([PENDING_ANALYSIS_NOTE, ""])

    def append_rules(title, references):
        lines.extend(["## " + title, ""])
        references = {_requirement_key(ref): ref for ref in references}
        for number, reference in enumerate(references.values(), 1):
            rule = rules.get(_requirement_key(reference), {}).get("rule")
            if not isinstance(rule, dict) or not rule.get("text"):
                lines.extend([f"{number}. 具体要求待核对，正式引用保留在大纲和要求底稿中。", ""])
                continue
            lines.extend([f"{number}. " + _requirement_text(rule).replace("\n", "\n   "), ""])
            evidence = rule.get("evidence") or []
            if not isinstance(evidence, list):
                evidence = [evidence]
            for item in evidence:
                quote = _requirement_value(item.get("quote", item) if isinstance(item, dict) else item)
                lines.extend(["> " + quote.replace("\n", "\n> "), ""])

    append_rules("已知提交与编制约束", constraints)
    body_paths = {path for _, path in _body_entries(entries)}
    for chapter, path in entries:
        if path not in body_paths:
            lines.extend(["## 提交待办：" + chapter["title"], ""])
            for target in chapter.get("targets", []):
                lines.extend(["- " + target["purpose"], ""])
                if artifacts is not None:
                    from docreader.tender.authoring_excerpts import load_source_excerpt, whole_source_excerpts
                    excerpts = list(target.get("source_excerpts", []))
                    precise = {_source_key(ref["source_ref"]) for ref in excerpts}
                    try:
                        for reference in target.get("source_refs", []):
                            if _source_key(reference) not in precise:
                                excerpts.extend(whole_source_excerpts(artifacts, reference))
                        for excerpt in excerpts:
                            text = load_source_excerpt(artifacts, excerpt)["text"]
                            lines.extend(["> " + text.replace("\n", "\n> "), ""])
                    except (OSError, ValueError, KeyError):
                        lines.extend(["原文暂不可读，来源引用保留在本次大纲中，待人工核对。", ""])
    paths = _display_paths(entries)
    for chapter, _ in entries:
        references = [ref for target in chapter.get("targets", []) for ref in target.get("rule_refs", [])]
        if references:
            location = " ".join(part for part in (
                ".".join(map(str, paths.get(chapter["chapter_id"], ()))), chapter["title"]) if part)
            label = "封面填写要求：" if chapter.get("presentation_role", "body") == "cover" else "章节要求："
            append_rules(label + location, references)
    for issue in outline.get("issues", []):
        if issue["code"] in {"outline_target_excluded", "outline_rule_excluded"}:
            excluded_target = issue["code"] == "outline_target_excluded"
            append_rules("已排除候选及关联要求" if excluded_target else "已明确排除的要求",
                         issue.get("rule_refs", []))
            if excluded_target:
                lines.extend(["此记录仅排除该候选；关联要求在其他响应位置的安排仍保留。", ""])
            if issue.get("detail"):
                lines.extend(["排除理由：" + issue["detail"], ""])
    append_rules("尚未安排的要求", [row["rule_ref"] for key, row in rules.items() if key not in assigned])
    lines.extend(["## 人工填写位置", ""])
    for chapter, _ in entries:
        for target in chapter.get("targets", []):
            if target.get("manual_fill"):
                lines.extend([f"- {chapter['title']}：{target['purpose']}", ""])
    lines.extend(["## 当前核对意见与待办", "", "各项保持原核对含义；不统一视为资料缺失，也不阻止草稿导出。", ""])
    for issue in gaps:
        lines.extend(["- " + issue["detail"].replace("\n", "\n  "), ""])
    return "\n".join(lines)


def _image(document, content):
    """Fit real pixels into the page without cropping or changing their aspect ratio."""
    with Image.open(BytesIO(content)) as original:
        oriented = ImageOps.exif_transpose(original)
        try:
            output = BytesIO()
            oriented.save(output, format="PNG")
            section = document.sections[-1]
            width = section.page_width - section.left_margin - section.right_margin
            height = section.page_height - section.top_margin - section.bottom_margin - Mm(20)
            ratio = min(width / oriented.width, height / oriented.height)
            document.add_picture(BytesIO(output.getvalue()),
                                 width=int(oriented.width * ratio), height=int(oriented.height * ratio))
        finally:
            oriented.close()


def _pdf(document, path):
    import pypdfium2 as pdfium
    from docreader.parser.pdf_parser import _PDFIUM_LOCK, _render_page_to_jpeg

    # PDFium is process-global; use the same lock as ingest/source-view rendering.
    with _PDFIUM_LOCK, pdfium.PdfDocument(str(path)) as original:
        if not len(original):
            raise ValueError("attachment PDF has no pages")
        for ordinal in range(len(original)):
            page = original[ordinal]
            try:
                encoded = _render_page_to_jpeg(page, 150 / 72, 90, 2400)
            finally:
                page.close()
            _image(document, encoded)


def _block(document, block, *, width_ratios=None):
    if block.get("text"):
        document.add_paragraph(block["text"])
    rows = ([block["headers"]] if block.get("headers") else []) + block.get("rows", [])
    if not rows or not any(rows):
        return
    table = document.add_table(rows=len(rows), cols=max(map(len, rows)))
    table.style = "Table Grid"
    widths = ([_page_width(document) * ratio / sum(width_ratios) for ratio in width_ratios]
              if width_ratios else None)
    _table_layout(document, table, widths, 1 if block.get("headers") else 0)
    for row_index, values in enumerate(rows):
        for column_index, text in enumerate(values):
            table.cell(row_index, column_index).text = text


class _Assembler:
    def __init__(self, artifacts, writing_units, review_issues, entries=()):
        self.artifacts = artifacts
        body_targets = {target['target_id'] for chapter, _ in _body_entries(entries)
                        for target in chapter.get('targets', [])}
        self.note_targets = {target['target_id'] for chapter, _ in entries
                             for target in chapter.get('targets', [])} - body_targets
        self.units = defaultdict(list)
        for unit in writing_units:
            if unit.get('target_ids') and set(unit['target_ids']).issubset(self.note_targets):
                continue
            self.units[unit["chapter_id"]].append(unit)
        self.gaps = list(review_issues or [])
        self.pending_rules = []
        self._running_headers = {}
        self._response_context = None
        self.attachments = {}
        self.template_parts = {}
        self.repository = EvidenceRepository(artifacts)
        self.display_paths = _display_paths(entries)
        self.locations = {target["target_id"]: " ".join(part for part in (
            ".".join(map(str, self.display_paths.get(chapter["chapter_id"], ()))), chapter["title"]) if part)
            for chapter, _ in _body_entries(entries) for target in chapter.get("targets", [])}

    def _running_header(self, source_ref, text):
        stripped = text.strip()
        if not stripped or "\n" in stripped or len(stripped) > 80:
            return False
        index_ref = source_ref["source_index_ref"]
        digest = index_ref["sha256"]
        if digest not in self._running_headers:
            counts = {}
            try:
                index = self.artifacts.read_json(index_ref)
                raw = self.artifacts.read_json(index["raw_ref"])
                for unit in raw.get("units") or []:
                    value = (unit.get("text") or "").strip()
                    if value and "\n" not in value and len(value) <= 80:
                        counts[value] = counts.get(value, 0) + 1
            except (OSError, ValueError, KeyError, TypeError):
                counts = {}
            self._running_headers[digest] = {value for value, count in counts.items() if count >= 3}
        return stripped in self._running_headers[digest]

    def _selected_grid(self, excerpt, loaded):
        ranges = [item for item in loaded.get("ranges") or [] if item.get("row_start") is not None]
        if not ranges:
            return None
        try:
            index = self.repository.source_index(excerpt["source_ref"]["source_index_ref"])
        except (OSError, ValueError, KeyError, TypeError):
            return None
        unit = next((item for item in index.raw["units"]
                     if item["unit_id"] == excerpt["source_ref"]["unit_id"] and item.get("grid")), None)
        if unit is None:
            return None
        return unit["grid"], {item["row_start"] for item in ranges}

    def _grid_rows(self, grid, selected):
        if not selected:
            return None
        chosen = set(selected)
        if 0 not in chosen and any(row > 0 for row in chosen):
            chosen.add(0)
        order = [row for row in range(grid["row_count"]) if row in chosen]
        remap = {row: index for index, row in enumerate(order)}
        cells = []
        for cell in grid["cells"]:
            start = cell["row"]
            if start not in remap:
                continue
            covered = [remap[start + offset] for offset in range(cell.get("row_span", 1)) if start + offset in remap]
            if not covered:
                continue
            cells.append({**cell, "row": remap[start], "row_span": max(covered) - min(covered) + 1})
        return {"row_count": len(order), "column_count": grid["column_count"],
                "cells": cells, **({"widths_mm": grid["widths_mm"]} if grid.get("widths_mm") else {})}

    def _source_index(self, source_ref):
        try:
            return self.repository.source_index(source_ref["source_index_ref"])
        except (OSError, ValueError, KeyError, TypeError):
            return None

    def _prose_pieces(self, excerpt, loaded):
        index = self._source_index(excerpt["source_ref"])
        pieces = []
        for item in loaded.get("ranges") or []:
            source_id = item.get("source_range_id") or item.get("range_id")
            base = index.by_id.get(source_id) if index is not None else None
            pieces.append({"range_id": source_id, "start": item.get("start", 0), "end": item.get("end", 0),
                           "text": item.get("text") or "", "breaks": set((base or {}).get("paragraph_breaks") or [])})
        if not pieces and loaded.get("text"):
            pieces.append({"range_id": None, "start": 0, "end": len(loaded["text"]),
                           "text": loaded["text"], "breaks": set()})
        return pieces

    def _template_breaks(self, part, unit):
        # Selector text is already joined for display. Offsets belong to the frozen range, not that join.
        if part.get("selectors") or not unit.get("unit_id"):
            return set(), 0
        index = self._source_index(part["source_ref"])
        text = unit.get("text") or ""
        matched = next((row for row in (index.ranges if index is not None else [])
                        if row.get("unit_id") == unit.get("unit_id") and row.get("row_start") is None
                        and row.get("text") == text), None)
        if matched is None:
            return set(), 0
        return set(matched.get("paragraph_breaks") or []), matched.get("start", 0)

    def _write_prose(self, document, pieces):
        written = False
        for paragraph in _paragraphs_from_pieces(pieces):
            document.add_paragraph(paragraph)
            written = True
        return written

    def _show_bound_requirements(self, document, target, chapter_id):
        from docreader.tender.authoring_excerpts import load_source_excerpt
        if (target.get("template_refs") or (target.get("response_table") or {}).get("rows")
                or not target.get("source_excerpts") or not target.get("rule_refs")):
            return False
        shown = False
        pending = None
        prose = []
        def flush_prose():
            nonlocal shown
            if prose and self._write_prose(document, prose):
                shown = True
            prose.clear()
        def flush():
            nonlocal pending, shown
            if pending is None:
                return
            grid, rows, _key = pending
            table = self._grid_rows(grid, rows)
            if table is not None:
                self.table(document, table, [], chapter_id, target["target_id"])
                shown = True
            pending = None
        for excerpt in target["source_excerpts"]:
            try:
                loaded = load_source_excerpt(self.artifacts, excerpt, repository=self.repository)
            except (OSError, ValueError, KeyError):
                flush()
                flush_prose()
                continue
            selected = self._selected_grid(excerpt, loaded)
            if selected is not None:
                flush_prose()
                grid, rows = selected
                key = excerpt["source_ref"]["unit_id"]
                if pending is not None and pending[2] == key:
                    pending[1].update(rows)
                else:
                    flush()
                    pending = [grid, set(rows), key]
                continue
            flush()
            prose.extend(self._prose_pieces(excerpt, loaded))
        flush()
        flush_prose()
        if not shown:
            return False
        self.gap("export_bound_requirement_answer_blank", "已展示绑定的招标要求，企业应答留空。", chapter_id, target["target_id"])
        return True

    def gap(self, code, detail, chapter_id, target_id=None):
        value = {"code": code, "detail": detail, "chapter_id": chapter_id}
        if target_id is not None:
            value["target_id"] = target_id
        self.gaps.append(value)

    def load_template(self, part):
        key = template_part_key(part)
        if key not in self.template_parts:
            self.template_parts[key] = load_template_part(self.artifacts, part, repository=self.repository)
        return self.template_parts[key]

    def template(self, document, part, fills, chapter_id, target_id):
        try:
            loaded = self.load_template(part)
            unit = loaded["unit"]
            grid = unit.get("grid")
            header = bool(unit.get("text")) and self._running_header(part["source_ref"], unit["text"])
            if header and not grid:
                self.gap("export_running_header_omitted", "已省略在多页独立重复出现的源页眉。", chapter_id, target_id)
                return
            if unit.get("text") and not header and not grid:
                # Keep a whole form's own line breaks. Split only where the source recorded a paragraph.
                breaks, start = self._template_breaks(part, unit)
                for paragraph in _template_paragraphs(unit["text"], breaks, start):
                    document.add_paragraph(paragraph)
            if grid:
                count = part.get("header_row_count")
                if count is None:
                    rows = {cell["row"] for cell in unit.get("header_cells", [])}
                    count = 0
                    while count in rows:
                        count += 1
                self.table(document, grid, fills, chapter_id, target_id, header_count=count)
            elif loaded.get("image_ref"):
                _image(document, self.artifacts.read_bytes(loaded["image_ref"]))
                self.gap("export_template_not_editable", "该原格式以真实图片保留，填写位置仍待补。",
                         chapter_id, target_id)
            elif loaded.get("spreadsheet_cells") or unit.get("locator", {}).get("cells"):
                cells = list(loaded.get("spreadsheet_cells", {}).values()) or unit["locator"]["cells"]
                rows = defaultdict(dict)
                for cell in cells:
                    rows[cell["row"]][cell["column"]] = cell.get("text", "")
                columns = sorted({cell["column"] for cell in cells})
                _block(document, {"kind": "table", "rows": [
                    [rows[row].get(column, "") for column in columns] for row in sorted(rows)]})
                self.gap("export_template_structure_incomplete", "仅恢复已有原生单元格，需核对原表合并及填写位置。",
                         chapter_id, target_id)
            elif not unit.get("text"):
                document.add_paragraph("（原格式待补）")
                self.gap("export_template_unrendered", "原格式缺少可渲染的文字、网格或图片。", chapter_id, target_id)
            if fills and not grid:
                self.gap("export_cell_fill_unplaced", "该来源没有可填写的原生网格，填充值未写入原格式。",
                         chapter_id, target_id)
            self.gap("export_template_layout_unverified", "已保留可读取的原文和网格；字体、分页和原版式仍需核对。",
                     chapter_id, target_id)
        except Exception as exc:
            document.add_paragraph("（原格式待补）")
            self.gap("export_source_unavailable", f"原格式暂不可用：{type(exc).__name__}。", chapter_id, target_id)

    def table(self, document, grid, fills, chapter_id, target_id, *, header_count=0):
        table = document.add_table(rows=grid["row_count"], cols=grid["column_count"])
        table.style = "Table Grid"
        widths = _table_layout(document, table,
            [Mm(width) for width in grid["widths_mm"]] if grid.get("widths_mm") else None, header_count)
        choices = defaultdict(set)
        for fill in fills:
            choices[(fill["row"], fill["column"])].add(fill["text"])
        anchors = {(cell["row"], cell["column"]) for cell in grid["cells"]}
        for position in choices.keys() - anchors:
            self.gap("export_cell_fill_unplaced", f"填充值不对应原网格锚点：{position}。", chapter_id, target_id)
        for cell in grid["cells"]:
            row, column = cell["row"], cell["column"]
            target = table.cell(row, column)
            row_span, col_span = cell.get("row_span", 1), cell.get("col_span", 1)
            if row_span > 1 or col_span > 1:
                target = target.merge(table.cell(row + row_span - 1, column + col_span - 1))
            target.width = sum(widths[column:column + col_span])
            text = cell.get("text", "")
            values = choices.get((row, column), set())
            if text.strip():
                if values - {text}:
                    self.gap("export_fixed_cell_preserved", f"原固定文字已保留，未覆盖单元格({row}, {column})。",
                             chapter_id, target_id)
            elif len(values) > 1:
                self.gap("export_conflicting_cell_fills", f"单元格({row}, {column})有冲突填值，保留原空白。",
                         chapter_id, target_id)
            elif values:
                text = next(iter(values))
            target.text = text

    def _native_row(self, excerpt, loaded):
        selected = self._selected_grid(excerpt, loaded)
        if not selected:
            return None
        grid, rows = selected
        if len(rows) != 1:
            return None
        row = next(iter(rows))
        width = grid["column_count"]
        values = [""] * width
        for cell in grid["cells"]:
            if cell["row"] == row and cell["column"] < width:
                values[cell["column"]] = cell.get("text") or ""
        return {"unit_id": excerpt["source_ref"]["unit_id"], "row": row, "values": values, "grid": grid,
                "text": "\n".join(value for value in values if value.strip())}

    def _describe_excerpt(self, excerpt):
        from docreader.tender.authoring_excerpts import load_source_excerpt
        try:
            loaded = load_source_excerpt(self.artifacts, excerpt, repository=self.repository)
        except (OSError, ValueError, KeyError):
            return {"kind": "prose", "text": "（原文要求待核对，来源引用已保留）", "missing": True}
        native = self._native_row(excerpt, loaded)
        if native:
            return {"kind": "grid", **native}
        return {"kind": "prose", "text": re.sub(r"(?m)^( {0,3})#{1,6}[ \t]+", r"\1", loaded["text"])}

    def _row_answer(self, choices, row_id, chapter_id, target_id):
        values = choices.get(row_id, set())
        if len(values) > 1:
            self.gap("export_conflicting_row_fills", "该要求行存在冲突应答，保留完整要求及空白响应。",
                     chapter_id, target_id)
            return ""
        return next(iter(values), "")

    def _write_prose_response(self, document, entries):
        table = document.add_table(rows=len(entries) + 1, cols=2)
        table.style = "Table Grid"
        _table_layout(document, table, [_page_width(document) * 0.65, _page_width(document) * 0.35], 1)
        table.cell(0, 0).text, table.cell(0, 1).text = "招标要求", "投标响应"
        for ordinal, (text, answer) in enumerate(entries, 1):
            table.cell(ordinal, 0).text = text
            if answer:
                table.cell(ordinal, 1).text = answer

    def _write_native_response(self, document, grid, entries):
        """Draw selected native rows, plus one blank answer column. Do not reprint the reading string."""
        body = list(entries)
        if 0 not in {row for row, _, _ in body} and any(row > 0 for row, _, _ in body):
            header = [""] * grid["column_count"]
            for cell in grid["cells"]:
                if cell["row"] == 0 and cell["column"] < grid["column_count"]:
                    header[cell["column"]] = cell.get("text") or ""
            body.insert(0, (0, header, None))
        columns = grid["column_count"] + 1
        table = document.add_table(rows=len(body), cols=columns)
        table.style = "Table Grid"
        _table_layout(document, table, None, 1 if body and body[0][0] == 0 else 0)
        for ordinal, (source_row, values, answer) in enumerate(body):
            for column, text in enumerate(values):
                table.cell(ordinal, column).text = text
            table.cell(ordinal, columns - 1).text = "投标响应" if source_row == 0 and answer is None else (answer or "")

    def response_table(self, document, target, units, chapter_id):
        rows = target.get("response_table", {}).get("rows", [])
        if not rows:
            return False
        bound_context = target["response_table"].get("context_source_excerpts", [])
        context_texts = []
        for excerpt in bound_context:
            item = self._describe_excerpt(excerpt)
            if item.get("missing"):
                context_texts.append("（原表共同条件待核对，来源引用已保留）")
                self.gap("export_response_source_unavailable", "该表部分共同条件不可读，保留来源和待核位置。",
                         chapter_id, target["target_id"])
            elif item["kind"] == "prose" and item["text"].strip():
                context_texts.append(item["text"])
        if not context_texts and not bound_context and self._response_context:
            context_texts = list(self._response_context)
            self.gap("export_response_context_continued",
                     "本表未单独绑定共同条件，已沿用上一张响应表已绑定的条件展示。", chapter_id, target["target_id"])
        if context_texts:
            self._response_context = list(context_texts)
            document.add_paragraph("\n".join(context_texts))
        choices = defaultdict(set)
        row_ids = {row["row_id"] for row in rows}
        for unit in units:
            for fill in unit.get("row_fills", []):
                if fill["row_id"] not in row_ids:
                    continue
                if target.get("manual_fill", False):
                    self.gap("export_manual_content_omitted", "人工填写响应表未采用自动填值。",
                             chapter_id, target["target_id"])
                elif fill.get("text", "").strip() and not fill.get("material_ids"):
                    self.gap("export_row_fill_unsupported", "该行应答没有知识依据，保留要求和空白响应。",
                             chapter_id, target["target_id"])
                else:
                    choices[fill["row_id"]].add(fill["text"])
        described = []
        for row in rows:
            parts = []
            for excerpt in row["source_excerpts"]:
                item = self._describe_excerpt(excerpt)
                if item.get("missing"):
                    self.gap("export_response_source_unavailable", "该行部分原文不可读，保留来源和待核位置。",
                             chapter_id, target["target_id"])
                parts.append(item)
            described.append((row, parts, self._row_answer(choices, row["row_id"], chapter_id, target["target_id"])))
        index = 0
        while index < len(described):
            _, parts, answer = described[index]
            grids = [part for part in parts if part["kind"] == "grid"]
            if not grids:
                prose_rows = []
                while index < len(described):
                    row, parts, answer = described[index]
                    if any(part["kind"] == "grid" for part in parts):
                        break
                    prose_rows.append(("\n".join(part["text"] for part in parts), answer))
                    index += 1
                if prose_rows:
                    self._write_prose_response(document, prose_rows)
                continue
            unit_id = grids[0]["unit_id"]
            if len({part["unit_id"] for part in grids}) != 1:
                self._write_split_native_row(document, parts, answer)
                index += 1
                continue
            grid = grids[0]["grid"]
            native_rows = []
            while index < len(described):
                row, parts, answer = described[index]
                grids = [part for part in parts if part["kind"] == "grid"]
                if not grids or len({part["unit_id"] for part in grids}) != 1 or grids[0]["unit_id"] != unit_id:
                    break
                prose = "\n".join(part["text"] for part in parts if part["kind"] == "prose" and part["text"].strip())
                for ordinal, native in enumerate(grids):
                    values = list(native["values"])
                    if prose and ordinal == len(grids) - 1:
                        values[-1] = "\n".join(part for part in (values[-1], prose) if part.strip())
                    native_rows.append((native["row"], values, answer if ordinal == 0 else ""))
                index += 1
            self._write_native_response(document, grid, native_rows)
        return True

    def _write_split_native_row(self, document, parts, answer):
        prose = "\n".join(part["text"] for part in parts if part["kind"] == "prose" and part["text"].strip())
        grids = [part for part in parts if part["kind"] == "grid"]
        for ordinal, native in enumerate(grids):
            values = list(native["values"])
            if prose and ordinal == len(grids) - 1:
                values[-1] = "\n".join(part for part in (values[-1], prose) if part.strip())
            self._write_native_response(document, native["grid"],
                                        [(native["row"], values, answer if ordinal == 0 else "")])

    def attachment(self, document, attachment, chapter_id):
        reference = attachment["artifact_ref"]
        try:
            path = self.artifacts.path(reference)
        except Exception as exc:
            self.gap("export_attachment_unavailable", f"{attachment['title']}原件不可用：{type(exc).__name__}。", chapter_id)
            document.add_paragraph(f"{attachment['title']}：待补原件。")
            return
        key = reference["sha256"]
        if key not in self.attachments:
            extension = mimetypes.guess_extension(reference["media_type"]) or ".bin"
            self.attachments[key] = {**attachment,
                "package_path": f"attachments/{len(self.attachments) + 1:03d}-{key[:12]}{extension}",
                "embedded": False, "chapter_ids": [], "occurrences": []}
        manifest = self.attachments[key]
        if chapter_id not in manifest["chapter_ids"]:
            manifest["chapter_ids"].append(chapter_id)
        occurrence = {"material_id": attachment["material_id"], "title": attachment["title"], "chapter_id": chapter_id}
        if occurrence not in manifest["occurrences"]:
            manifest["occurrences"].append(occurrence)
        document.add_paragraph(attachment["title"])
        try:
            media_type = reference["media_type"]
            if media_type.startswith("image/"):
                _image(document, self.artifacts.read_bytes(reference))
            elif media_type == "application/pdf":
                _pdf(document, path)
            else:
                document.add_paragraph("（真实原件随交付包附送）")
                self.gap("export_attachment_not_embedded", f"{attachment['title']}原件已附送，尚未嵌入正文。", chapter_id)
                return
            manifest["embedded"] = True
        except Exception as exc:
            document.add_paragraph("（真实原件随交付包附送）")
            self.gap("export_attachment_not_embedded", f"{attachment['title']}未完整嵌入：{type(exc).__name__}；原件仍随包附送。",
                     chapter_id)

    def writing_unit(self, document, unit, chapter_id, blocked_forms=(), manual_forms=(), response_tables=(),
                    templated_forms=(), shown_requirements=()):
        blocks = []
        for block in unit.get("blocks", []):
            target_ids = set(block.get("target_ids") or unit.get("target_ids", []))
            if target_ids and target_ids.issubset(self.note_targets):
                continue
            if target_ids and target_ids.issubset(blocked_forms):
                manual = target_ids.issubset(manual_forms)
                self.gap("export_manual_content_omitted" if manual else "export_form_content_unplaced",
                         "人工填写位置未采用生成正文。" if manual else
                         "该原格式没有可填写网格，未另行追加生成声明；补充说明须安排独立正文位置。", chapter_id)
            else:
                blocks.append(block)
        for block in blocks:
            _block(document, block)
            if block["kind"] == "paragraph" and (block.get("headers") or block.get("rows")):
                self.gap("block_kind_mismatch", "段落块同时携带表格字段，已完整保留段落及表格内容。", chapter_id)
        for attachment in unit.get("attachments", []):
            self.attachment(document, attachment, chapter_id)
        for gap in unit.get("gaps", []):
            self.gap("authoring_content_gap", gap, chapter_id)
        has_blocks = any(block.get("text", "").strip() or block.get("headers") or block.get("rows") for block in blocks)
        targets = set(unit.get("target_ids") or [])
        table_only = bool(targets) and targets.issubset(response_tables)
        form_already_rendered = bool(targets) and targets.issubset(templated_forms)
        requirement_shown = bool(targets) and targets.issubset(shown_requirements)
        if not table_only and not form_already_rendered and not requirement_shown and (unit["status"] == "placeholder"
                               or not (has_blocks or unit.get("cell_fills") or unit.get("attachments"))):
            document.add_paragraph("（本部分内容待补）")
            self.gap("export_content_pending", "写作任务尚未形成完整内容，保留当前位置。", chapter_id)

    def chapter(self, document, chapter, path, *, heading=True):
        chapter_id = chapter["chapter_id"]
        chapter_start = len(document.element.body) - 1
        if heading:
            path = self.display_paths.get(chapter_id, path)
            paragraph = document.add_heading(f"{'.'.join(map(str, path))} {_display_title(chapter['title'])}", level=min(len(path), 9))
            add_bookmark(paragraph._p, chapter_id)
        units = self.units.get(chapter_id, [])
        targets = chapter.get("targets", [])
        reference_only = {target['target_id'] for target in targets
            if target['kind'] == 'narrative' and not target.get('manual_fill', False)
            and not any(target.get(field) for field in ('rule_refs', 'source_refs', 'source_excerpts',
                                                       'template_refs', 'response_table'))
            and target.get('related_target_ids')
            and all(identity in self.locations and identity != target['target_id']
                    for identity in target['related_target_ids'])}
        forms, grid_forms = {}, set()
        form_ids = {target["target_id"] for target in targets if target["kind"] == "form"}
        manual_forms = {target["target_id"] for target in targets
                        if target["kind"] == "form" and target.get("manual_fill", False)}
        for target in targets:
            if target["kind"] != "form" and not target.get("template_refs"):
                continue
            selected = forms[target["target_id"]] = {}
            for part in target.get("template_refs", []):
                try:
                    selected[template_part_key(part)] = part
                except (KeyError, TypeError, ValueError):
                    self.gap("export_source_unavailable", "原格式选择无效，保留待补位置。",
                             chapter_id, target["target_id"])
            for part in selected.values():
                try:
                    if self.load_template(part)["unit"].get("grid"):
                        grid_forms.add(target["target_id"])
                except Exception:
                    # Rendering below records the unavailable part without substituting its whole unit.
                    continue
        blocked_forms = (form_ids - grid_forms) | manual_forms
        templated_forms = {identity for identity, parts in forms.items() if parts}
        row_owners = {row["row_id"]: target["target_id"] for target in targets
                      for row in (target.get("response_table") or {}).get("rows", [])}
        for unit in units:
            allowed = {_source_key(part["source_ref"]) for target_id in unit.get("target_ids", [])
                       if target_id in form_ids for part in forms.get(target_id, {}).values()}
            for fill in unit.get("cell_fills", []):
                if _source_key(fill["source_ref"]) not in allowed:
                    self.gap("export_cell_fill_unplaced", "填值来源不属于本写作任务响应位置的原格式。", chapter_id)
            for fill in unit.get("row_fills", []):
                if row_owners.get(fill["row_id"]) not in unit.get("target_ids", []):
                    self.gap("export_row_fill_unplaced", "行应答无法绑定当前任务的实际响应行，要求保持不变。", chapter_id)
        rendered, unit_anchors = set(), {}
        response_tables = {target["target_id"] for target in targets
                           if (target.get("response_table") or {}).get("rows")}
        shown_requirements = set()
        for target in targets:
            start_index = len(document.element.body) - 1
            target_id = target["target_id"]
            if self._show_bound_requirements(document, target, chapter_id):
                shown_requirements.add(target_id)
            related = []
            for identity in dict.fromkeys(target.get("related_target_ids", [])):
                if identity in self.locations and identity != target_id:
                    related.append(self.locations[identity])
                else:
                    self.gap("export_related_target_unavailable", "相关响应位置暂不能定位，原关系保留待核。",
                             chapter_id, target_id)
            if related:
                document.add_paragraph("参见 " + "、".join(dict.fromkeys(related)))
            linked = [(ordinal, unit) for ordinal, unit in enumerate(units) if target_id in unit.get("target_ids", [])]
            if target_id in response_tables:
                self.response_table(document, target, [unit for _, unit in linked], chapter_id)
            if target_id in forms:
                if target["kind"] == "form" and target.get("subject"):
                    document.add_paragraph(target["subject"], style="Caption")
                for part in forms[target_id].values():
                    key = _source_key(part["source_ref"])
                    fills = [fill for _, unit in linked for fill in unit.get("cell_fills", [])
                             if target_id in form_ids and _source_key(fill["source_ref"]) == key]
                    if target_id in manual_forms and fills:
                        fills = []
                        self.gap("export_manual_content_omitted", "人工填写表格未采用自动填值。", chapter_id, target_id)
                    self.template(document, part, fills, chapter_id, target_id)
                if target["kind"] == "form" and not forms[target_id] and target_id not in response_tables:
                    document.add_paragraph("（原格式待补）")
                    self.gap("export_template_missing", "该填写位置尚无可用原格式。", chapter_id, target_id)
            for ordinal, unit in linked:
                if ordinal not in rendered:
                    has_content = (any(block.get('text', '').strip() or block.get('headers') or block.get('rows')
                                       for block in unit.get('blocks', []))
                                   or unit.get('cell_fills') or unit.get('row_fills') or unit.get('attachments'))
                    if set(unit.get('target_ids', [])).issubset(reference_only) and not has_content:
                        for gap in unit.get('gaps', []):
                            self.gap('authoring_content_gap', gap, chapter_id)
                        rendered.add(ordinal)
                        continue
                    unit_start = len(document.element.body) - 1
                    self.writing_unit(document, unit, chapter_id, blocked_forms, manual_forms,
                                      response_tables, templated_forms, shown_requirements)
                    if len(document.element.body) - 1 > unit_start:
                        unit_anchors[ordinal] = document.element.body[unit_start]
                    rendered.add(ordinal)
            if (not linked and target_id not in reference_only and target_id not in response_tables
                    and not forms.get(target_id) and target_id not in shown_requirements):
                document.add_paragraph("（本部分内容待补）")
                self.gap("export_content_pending", "响应位置尚无写作内容。", chapter_id, target_id)
            if len(document.element.body) - 1 > start_index:
                add_bookmark(document.element.body[start_index], target_id, "target")
            else:
                for ordinal, _ in linked:
                    if ordinal in unit_anchors:
                        add_bookmark(unit_anchors[ordinal], target_id, "target")
                        break
        for ordinal, unit in enumerate(units):
            if ordinal not in rendered:
                self.writing_unit(document, unit, chapter_id, blocked_forms, manual_forms,
                                  response_tables, templated_forms, shown_requirements)
                self.gap("export_unassigned_content", "写作内容未绑定当前章节的响应位置，已保留在章节末尾。", chapter_id)
        if not heading and len(document.element.body) - 1 > chapter_start:
            add_bookmark(document.element.body[chapter_start], chapter_id)

    def render(self, entries, title, *, include_pending=False):
        entries = _body_entries(entries)
        document = Document()
        section = document.sections[0]
        section.page_width, section.page_height = Mm(210), Mm(297)
        section.left_margin = section.right_margin = Mm(25)
        section.top_margin = section.bottom_margin = Mm(22)
        normal = document.styles["Normal"]
        normal.font.name, normal.font.size = "宋体", Pt(11)
        normal.element.get_or_add_rPr().rFonts.set(qn("w:eastAsia"), "宋体")
        volumes = [path for chapter, path in entries if chapter.get("is_volume")
                   and chapter.get("presentation_role", "body") != "cover"]
        scope = (entries[0][1] if entries and entries[0][0].get("is_volume")
                 and entries[0][0].get("presentation_role", "body") != "cover"
                 and all(path[:len(entries[0][1])] == entries[0][1] for _, path in entries) else ())
        covers = defaultdict(list)
        for chapter, path in entries:
            if chapter.get("presentation_role", "body") == "cover":
                owner = next((volume for volume in reversed(volumes)
                              if len(volume) < len(path) and path[:len(volume)] == volume), ())
                covers[owner].append((chapter, path))

        def render_covers(owner):
            for chapter, path in covers[owner]:
                self.chapter(document, chapter, path, heading=False)
                document.add_page_break()

        if covers[scope]:
            render_covers(scope)
        else:
            document.add_heading(title, 0)
        _toc(document)
        document.add_page_break()
        for chapter, path in entries:
            if chapter.get("presentation_role", "body") == "cover":
                continue
            if chapter.get("is_volume"):
                self._response_context = None
            if chapter.get("is_volume") and path != scope:
                render_covers(path)
            self.chapter(document, chapter, path)
        if include_pending and self.pending_rules:
            document.add_heading("尚未归入响应表", 1)
            for pending in self.pending_rules:
                document.add_paragraph(pending)
                document.add_paragraph("")
        field = OxmlElement("w:fldSimple")
        field.set(qn("w:instr"), "PAGE")
        # Word rejects a simple field that has no result run.
        run = OxmlElement("w:r")
        text = OxmlElement("w:t")
        text.text = "1"
        run.append(text)
        field.append(run)
        section.footer.paragraphs[0]._p.append(field)
        output = BytesIO()
        document.save(output)
        return self.artifacts.put_bytes(output.getvalue(), DOCX_MEDIA_TYPE)


def export_authoring_document(artifacts, outline: dict, writing_units: list[dict],
                              review_issues: list[dict] | None = None, *, requirements: dict | None = None,
                              coverage: dict | None = None) -> dict:
    """Return immutable DOCX/ZIP references; source and business gaps remain deliverable."""
    entries = list(_walk(outline.get("chapters", [])))
    assembler = _Assembler(artifacts, writing_units, review_issues, entries)
    if requirements is not None:
        rules = {_requirement_key(entry["rule_ref"]): entry.get("rule") for entry in requirements.get("rules", [])}
        locations = [(ref, chapter["chapter_id"], target["target_id"]) for chapter, _ in entries
                     for target in chapter.get("targets", []) for ref in target.get("rule_refs", [])]
        located = {_requirement_key(ref) for ref, _, _ in locations}
        locations.extend((entry["rule_ref"], None, None) for entry in requirements.get("rules", [])
                         if _requirement_key(entry["rule_ref"]) not in located)
        for reference, chapter_id, target_id in locations:
            rule = rules.get(_requirement_key(reference))
            if not isinstance(rule, dict) or not rule.get("text"):
                assembler.gap("export_requirement_unavailable", "已保留要求位置及引用，具体内容尚待核对。",
                              chapter_id, target_id)
                continue
            evidence = rule.get("evidence")
            if not evidence or not isinstance(evidence, list) or any(
                    not isinstance(item, dict) or not isinstance(item.get("quote"), str) for item in evidence):
                assembler.gap("export_requirement_evidence_unverified", "已有原文依据结构需核对，原值保留在底稿中。",
                              chapter_id, target_id)
        assigned = {_requirement_key(ref) for chapter, _ in entries
                    for target in chapter.get("targets", []) for ref in target.get("rule_refs", [])}
        assigned.update(_requirement_key(ref) for ref in outline.get("constraint_rule_refs", []))
        assigned.update(_requirement_key(ref) for issue in outline.get("issues", [])
                        if issue["code"] in {"outline_target_excluded", "outline_rule_excluded"}
                        for ref in issue.get("rule_refs", []))
        for entry in requirements.get("rules", []):
            if _requirement_key(entry["rule_ref"]) not in assigned:
                text = (entry.get("rule") or {}).get("text")
                if isinstance(text, str) and text.strip():
                    assembler.pending_rules.append(text)
                assembler.gap("export_requirement_unassigned", "底稿中有尚未安排正文或交付位置的要求，完整内容保留在底稿中。", None)
    chapter_ids = {chapter["chapter_id"] for chapter, _ in entries}
    for chapter_id in assembler.units.keys() - chapter_ids:
        assembler.gap("export_unassigned_content", "内容的章节已不在当前大纲中，保留在补充内容中。", chapter_id)
        entries.append(({"chapter_id": chapter_id, "title": "补充内容", "targets": []}, (len(entries) + 1,)))
    docx_ref = assembler.render(entries, "投标文件", include_pending=True)
    volumes = []
    publish_volumes = not any(issue.get("code") == "outline_assembly_unavailable"
                              for issue in outline.get("issues", []))
    for chapter, path in _body_entries(entries):
        if publish_volumes and chapter.get("is_volume"):
            subtree = [(value, value_path) for value, value_path in entries if value_path[:len(path)] == path]
            reference = assembler.render(subtree, chapter["title"])
            volumes.append({"chapter_id": chapter["chapter_id"], "title": chapter["title"],
                "artifact_ref": reference, "package_path": f"volumes/{len(volumes) + 1:03d}.docx"})
    # Repeated uses in a main file and a volume share a single diagnostic entry.
    gaps = list({json.dumps(gap, ensure_ascii=False, sort_keys=True): gap for gap in assembler.gaps}.values())
    attachments = list(assembler.attachments.values())
    manifest = {"schema_version": 2, "document": {"artifact_ref": docx_ref, "package_path": "投标文件.docx"},
                "volumes": volumes, "attachments": attachments}
    requirement_files = []
    if requirements is not None:
        manifest["requirements"] = {"artifact_ref": artifacts.put_json(requirements), "package_path": "招标要求底稿.json"}
        requirement_files.append(manifest["requirements"])
    notes = artifacts.put_bytes(_submission_notes(outline, requirements, gaps, artifacts).encode(), "text/markdown")
    manifest["submission_notes"] = {"artifact_ref": notes, "package_path": "提交要求与待办.md"}
    coverage_file = None
    if coverage is not None:
        coverage_ref = artifacts.put_json(coverage)
        coverage_file = {"artifact_ref": coverage_ref, "package_path": "覆盖结算.json"}
        manifest["coverage"] = coverage_file
    output = BytesIO()
    with ZipFile(output, "w", compression=ZIP_DEFLATED) as archive:
        for entry in [manifest["document"], *volumes, *attachments, *requirement_files, manifest["submission_notes"],
                      *([coverage_file] if coverage_file else [])]:
            archive.write(artifacts.path(entry["artifact_ref"]), entry["package_path"])
        archive.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2))
        archive.writestr("待补事项.json", json.dumps(gaps, ensure_ascii=False, indent=2))
    return {"docx_ref": docx_ref, "package_ref": artifacts.put_bytes(output.getvalue(), "application/zip"),
            "attachments": [{key: attachment[key] for key in ("material_id", "title", "artifact_ref")}
                            for attachment in attachments], "gaps": gaps}
