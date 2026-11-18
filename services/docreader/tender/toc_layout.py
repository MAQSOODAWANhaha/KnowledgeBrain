"""Calculate generated TOC caches from measured, independently checked PDF pages."""

from copy import deepcopy
import hashlib
from html import unescape
from io import BytesIO
import math
import re
from zipfile import BadZipFile, ZipFile

from lxml import etree as ET
from pypdf import PdfReader
import pypdfium2 as pdfium


MAX_DOCX_BYTES = 50 * 1024 * 1024
MAX_PDF_BYTES = 50 * 1024 * 1024
MAX_XML_BYTES = 100 * 1024 * 1024
MAX_TOC_ENTRIES = 2000
MAX_PDF_PAGES = 2000
MAX_PDF_LINKS = 20000
MAX_ZIP_MEMBERS = 512
MAX_RENDERS = 4
CONTRACT = 'toc_layout_v1'
W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'


def _sha(value):
    return hashlib.sha256(value).hexdigest()


def _compact(value):
    return re.sub(r'\s+', '', value or '')


class LayoutError(ValueError):
    def __init__(self, code, detail, *, candidate_docx=None, receipt=None):
        super().__init__(detail)
        self.code, self.candidate_docx = code, candidate_docx
        self.receipt = receipt or {}


def _word(data):
    if not isinstance(data, bytes) or len(data) > MAX_DOCX_BYTES:
        raise LayoutError('TOC_DOCX_LIMIT', '目录布局候选文件超过大小限制。')
    try:
        with ZipFile(BytesIO(data)) as archive:
            members = archive.infolist()
            if (len(members) > MAX_ZIP_MEMBERS or len({m.filename for m in members}) != len(members)
                    or sum(m.file_size for m in members) > MAX_XML_BYTES
                    or any(m.flag_bits & 1 for m in members)):
                raise LayoutError('TOC_PACKAGE_LIMIT', '目录布局候选包超过展开限制或成员不唯一。')
            xml = archive.read('word/document.xml')
        if b'<!DOCTYPE' in xml or b'<!ENTITY' in xml:
            raise LayoutError('TOC_XML_INVALID', '目录布局不接受外部实体声明。')
        root = ET.fromstring(xml, ET.XMLParser(resolve_entities=False, no_network=True))
        if root.tag != W + 'document' or root.find(W + 'body') is None:
            raise LayoutError('TOC_DOCX_INVALID', '目录布局候选缺少Word正文。')
    except LayoutError:
        raise
    except (BadZipFile, KeyError, OSError, ValueError, ET.XMLSyntaxError):
        raise LayoutError('TOC_DOCX_INVALID', '目录布局候选文件无法读取。') from None
    entries = []
    bookmarks = {}
    for bookmark in root.iter(W + 'bookmarkStart'):
        name = bookmark.get(W + 'name')
        bookmarks[name] = bookmarks.get(name, 0) + 1
    for paragraph in root.iter(W + 'p'):
        for link in paragraph.findall(W + 'hyperlink'):
            anchor = link.get(W + 'anchor', '')
            if not anchor.startswith('kb_c_'):
                continue
            if bookmarks.get(anchor) != 1:
                raise LayoutError('TOC_BOOKMARK_INVALID', '目录目标书签缺失或不唯一。')
            pattern = r'\s*PAGEREF\s+' + re.escape(anchor) + r'\s+\\h\s*'
            fields = [f for f in paragraph.findall(W + 'fldSimple') if re.fullmatch(pattern, f.get(W + 'instr', ''))]
            if len(fields) != 1 or len(list(fields[0].iter(W + 't'))) != 1:
                raise LayoutError('TOC_CACHE_INVALID', '目录项需要唯一的页码域及缓存。')
            label = ''.join(t.text or '' for t in link.iter(W + 't'))
            if not label.strip():
                raise LayoutError('TOC_LABEL_INVALID', '目录项没有可核对的标题。')
            entries.append({'anchor': anchor, 'label': label,
                'cache': next(fields[0].iter(W + 't')).text or ''})
    references = [f for f in root.iter(W + 'fldSimple')
        if re.match(r'\s*PAGEREF\s+kb_c_', f.get(W + 'instr', ''))]
    if len(references) != len(entries) or len({e['anchor'] for e in entries}) != len(entries):
        raise LayoutError('TOC_ENTRIES_INVALID', '目录链接、书签及页码域未能一一对应。')
    if len(entries) > MAX_TOC_ENTRIES:
        raise LayoutError('TOC_ENTRIES_LIMIT', '目录条目超过布局核验限制。')
    if entries and (len(list(root.iter(W + 'sectPr'))) != 1 or list(root.iter(W + 'pgNumType'))):
        raise LayoutError('TOC_PAGE_POLICY_UNSUPPORTED', '当前自动目录只支持每份文件独立连续页号；请在Word更新域并确认保存。')
    return xml, root, entries


def _pdf_links(reader):
    by_id = {(p.indirect_reference.idnum, p.indirect_reference.generation): i
        for i, p in enumerate(reader.pages, 1)}
    raw, annotations = [], 0
    for ordinal, page in enumerate(reader.pages, 1):
        local = []
        for reference in page.get('/Annots', []):
            annotations += 1
            if annotations > MAX_PDF_LINKS:
                raise LayoutError('TOC_PDF_LINKS_LIMIT', '目录PDF链接数量超过核验限制。')
            annotation = reference.get_object()
            if annotation.get('/Subtype') != '/Link':
                continue
            destination = annotation.get('/Dest')
            action = annotation.get('/A')
            action = action.get_object() if hasattr(action, 'get_object') else action
            if destination is None and action and action.get('/S') == '/GoTo':
                destination = action.get('/D')
            if destination is None:
                continue
            destination = destination.get_object() if hasattr(destination, 'get_object') else destination
            if isinstance(destination, str):
                named = reader.named_destinations.get(destination)
                if named is None:
                    raise LayoutError('TOC_PDF_DESTINATION_INVALID', '目录PDF目标不存在。')
                key, physical = ('named', destination), reader.get_destination_page_number(named) + 1
                resolved = named.dest_array
            else:
                target = destination[0]
                physical = by_id[(target.idnum, target.generation)]
                key = (target.idnum, target.generation, *(str(v) for v in destination[1:]))
                resolved = destination
            if len(resolved) < 4 or str(resolved[1]) != '/XYZ':
                raise LayoutError('TOC_PDF_DESTINATION_UNSUPPORTED', '目录目标坐标策略尚不支持自动核验。')
            try:
                top = float(resolved[3])
            except (TypeError, ValueError):
                raise LayoutError('TOC_PDF_DESTINATION_UNSUPPORTED', '目录目标需要明确的垂直坐标。') from None
            target_page = reader.pages[physical - 1]
            if (not math.isfinite(top) or not 0 < top <= float(target_page.mediabox.height)
                    or target_page.rotation % 360 or target_page.cropbox != target_page.mediabox
                    or float(target_page.get('/UserUnit', 1)) != 1):
                raise LayoutError('TOC_PDF_DESTINATION_UNSUPPORTED', '目录目标页框、旋转或坐标策略尚需核定。')
            rectangle = tuple(float(v) for v in annotation['/Rect'])
            if len(rectangle) != 4 or not all(math.isfinite(v) for v in rectangle):
                raise LayoutError('TOC_PDF_RECTANGLE_INVALID', '目录PDF链接区域无效。')
            local.append({'source_page': ordinal, 'page': physical, 'key': key, 'rectangle': rectangle, 'top': top})
        raw.extend(sorted(local, key=lambda item: (-item['rectangle'][3], item['rectangle'][0])))
    grouped = []
    for link in raw:
        # Equal physical pages alone are never evidence that two links are fragments.
        if grouped and grouped[-1]['key'] == link['key']:
            grouped[-1]['fragments'].append(link)
        else:
            grouped.append({'key': link['key'], 'page': link['page'], 'top': link['top'], 'fragments': [link]})
    return grouped


def _measure(docx, pdf):
    _, _, entries = _word(docx)
    if not isinstance(pdf, bytes) or len(pdf) > MAX_PDF_BYTES:
        raise LayoutError('TOC_PDF_LIMIT', '目录PDF超过布局核验大小限制。')
    try:
        reader = PdfReader(BytesIO(pdf))
        count = len(reader.pages)
        if reader.is_encrypted or not 0 < count <= MAX_PDF_PAGES:
            raise LayoutError('TOC_PDF_PAGES_LIMIT', '目录PDF页数或加密状态不支持自动核验。')
        labels = reader.page_labels
        if labels != [str(i) for i in range(1, count + 1)]:
            raise LayoutError('TOC_PAGE_POLICY_UNSUPPORTED', '目录PDF不使用独立连续阿拉伯页号。')
        links = _pdf_links(reader)
        if len(entries) != len(links):
            raise LayoutError('TOC_PDF_LINKS_INVALID', '目录条目与PDF实际目标数量不一致。')
        producer = str((reader.metadata or {}).get('/Producer', ''))
        if len(producer) > 512:
            raise LayoutError('TOC_ENGINE_INVALID', 'PDF布局引擎标识无效。')
        measured = []
        from docreader.parser.pdf_parser import _PDFIUM_LOCK
        with _PDFIUM_LOCK, pdfium.PdfDocument(pdf) as rendered:
            if len(rendered) != count:
                raise LayoutError('TOC_PDF_PAGES_INVALID', 'PDF页数未能独立核对。')
            for entry, link in zip(entries, links):
                text, display = '', ''
                for ordinal, fragment in enumerate(link['fragments']):
                    page = rendered[fragment['source_page'] - 1]
                    textpage = page.get_textpage()
                    try:
                        text += textpage.get_text_bounded(*fragment['rectangle'])
                        if ordinal == len(link['fragments']) - 1:
                            _left, bottom, right, top = fragment['rectangle']
                            # The generated PAGEREF follows the hyperlink at a right tab
                            # with DOTS leader, in the final link fragment's line band.
                            display = textpage.get_text_bounded(right, bottom, page.get_width(), top)
                    finally:
                        textpage.close()
                        page.close()
                if _compact(text) != _compact(entry['label']):
                    raise LayoutError('TOC_PDF_LABEL_MISMATCH', '目录标题与PDF实际链接文字不一致。')
                physical = link['page']
                if not 1 <= physical <= count:
                    raise LayoutError('TOC_PDF_DESTINATION_INVALID', '目录目标页超出实际PDF范围。')
                lower = max((other['top'] for other in links
                    if other['page'] == physical and other['top'] < link['top']), default=0)
                page = rendered[physical - 1]
                textpage = page.get_textpage()
                try:
                    target_text = _compact(textpage.get_text_bounded(0, lower, page.get_width(), link['top']))
                finally:
                    textpage.close()
                    page.close()
                title = re.sub(r'^\d+(?:\.\d+)*\s+', '', entry['label'])
                if _compact(title) not in target_text:
                    raise LayoutError('TOC_PDF_TITLE_MISMATCH', '目录目标区域未能核对对应正文标题。')
                measured.append({**entry, 'page': labels[physical - 1],
                    'display_cache': _compact(display).lstrip('.'),
                    'destination_top': link['top'], 'destination_band_bottom': lower})
        return {'pdf_sha256': _sha(pdf), 'pdf_pages': count, 'pdf_producer': producer,
            'entries': measured, 'caches_match': all(e['cache'] == e['page'] for e in measured),
            'display_caches_match': all(e['display_cache'] == e['page'] for e in measured)}
    except LayoutError:
        raise
    except Exception:
        raise LayoutError('TOC_PDF_INVALID', '目录PDF无法安全读取或核对。') from None


def _neutral_xml(root):
    root = deepcopy(root)
    for field in root.iter(W + 'fldSimple'):
        if re.match(r'\s*PAGEREF\s+kb_c_', field.get(W + 'instr', '')):
            next(field.iter(W + 't')).text = '__verified_page_cache__'
    return ET.tostring(root, method='c14n')


def _patch(docx, measurement):
    original_xml, original_root, _ = _word(docx)
    try:
        xml = original_xml.decode('utf-8')
    except UnicodeDecodeError:
        raise LayoutError('TOC_CACHE_FORMAT_UNSUPPORTED', '目录候选缓存格式不支持自动回填。') from None
    for entry in measurement['entries']:
        pattern = (r'(<w:fldSimple\b[^>]*\bw:instr="PAGEREF ' + re.escape(entry['anchor'])
            + r' \\h"[^>]*>.*?<w:t(?:\s[^>]*)?>)([^<]*)(</w:t>.*?</w:fldSimple>)')
        matches = list(re.finditer(pattern, xml, re.S))
        if len(matches) != 1 or unescape(matches[0].group(2)) != entry['cache']:
            raise LayoutError('TOC_CACHE_FORMAT_UNSUPPORTED', '目录缓存字节位置缺失或不唯一。')
        xml = re.sub(pattern, lambda m: m.group(1) + entry['page'] + m.group(3), xml, count=1, flags=re.S)
    output = BytesIO()
    with ZipFile(BytesIO(docx)) as original, ZipFile(output, 'w') as derived:
        derived.comment = original.comment
        for info in original.infolist():
            derived.writestr(info, xml.encode('utf-8') if info.filename == 'word/document.xml' else original.read(info))
    result = output.getvalue()
    _, result_root, _ = _word(result)
    if _neutral_xml(original_root) != _neutral_xml(result_root):
        raise LayoutError('TOC_BODY_CHANGED', '目录回填改变了页码缓存以外的XML，候选未发布。')
    with ZipFile(BytesIO(docx)) as original, ZipFile(BytesIO(result)) as derived:
        if original.namelist() != derived.namelist() or any(original.read(n) != derived.read(n)
                for n in original.namelist() if n != 'word/document.xml'):
            raise LayoutError('TOC_PACKAGE_CHANGED', '目录回填改变了其他包成员，候选未发布。')
    return result


def finalize_toc(docx_bytes, render_pdf):
    """Return cache-only final bytes; renderer returns (complete PDF bytes, identity)."""
    candidate = docx_bytes
    receipt = {'contract': CONTRACT, 'source_docx_sha256': _sha(docx_bytes), 'render_count': 0, 'rounds': []}
    try:
        _, _, entries = _word(candidate)
        if not entries:
            return candidate, {**receipt, 'status': 'no_toc', 'docx_sha256': _sha(candidate), 'entries': []}
        seen = {_sha(candidate)}
        engine = None
        for _ in range(MAX_RENDERS):
            receipt['render_count'] += 1
            try:
                pdf, identity = render_pdf(candidate)
            except Exception:
                raise LayoutError('TOC_RENDER_FAILED', '目录布局转换失败；候选保留，可重试或在Word更新域并确认保存。') from None
            if not isinstance(identity, str) or not identity.strip() or len(identity) > 512:
                raise LayoutError('TOC_ENGINE_INVALID', '目录布局引擎标识无效。')
            measurement = _measure(candidate, pdf)
            actual_engine = {'renderer_identity': identity, 'pdf_producer': measurement['pdf_producer']}
            if engine is not None and engine != actual_engine:
                raise LayoutError('TOC_ENGINE_CHANGED', '目录回填期间布局引擎改变，请重新生成布局。')
            engine = actual_engine
            receipt.update(engine=engine, pdf_sha256=measurement['pdf_sha256'],
                pdf_pages=measurement['pdf_pages'], entries=measurement['entries'])
            receipt['rounds'].append({'docx_sha256': _sha(candidate), 'pdf_sha256': measurement['pdf_sha256'],
                'pages': [e['page'] for e in measurement['entries']], 'caches_match': measurement['caches_match'],
                'display_caches_match': measurement['display_caches_match']})
            if measurement['caches_match']:
                if not measurement['display_caches_match']:
                    raise LayoutError('TOC_PDF_CACHE_MISMATCH', 'PDF实际显示的目录页码未与Word缓存及目的页一致。')
                return candidate, {**receipt, 'status': 'fixed', 'docx_sha256': _sha(candidate),
                    'cache_only_preserved': True, 'pdf_cache_display_verified': True}
            if receipt['render_count'] == MAX_RENDERS:
                break
            following = _patch(candidate, measurement)
            if _sha(following) in seen:
                raise LayoutError('TOC_LAYOUT_CYCLE', '目录分页发生循环；候选保留，请在Word更新域并确认保存。')
            seen.add(_sha(following))
            candidate = following
        raise LayoutError('TOC_LAYOUT_NOT_CONVERGED', '目录页码在限定轮次内未稳定；候选保留，请重试或在Word更新域并确认保存。')
    except LayoutError as error:
        error.candidate_docx = candidate
        error.receipt = {**receipt, 'status': 'pending', 'docx_sha256': _sha(candidate), 'error_code': error.code}
        raise


def verify_toc(docx_bytes, pdf_bytes):
    """Independently reject wrong numeric caches instead of quietly repairing them."""
    measurement = _measure(docx_bytes, pdf_bytes)
    if not measurement['caches_match']:
        raise LayoutError('TOC_CACHE_MISMATCH', '目录页码缓存与实际PDF目的页不一致。')
    if not measurement['display_caches_match']:
        raise LayoutError('TOC_PDF_CACHE_MISMATCH', 'PDF实际显示的目录页码未与Word缓存及目的页一致。')
    return measurement
