"""TOC page references remain explicit until calculated, with no floating break paragraph."""

from io import BytesIO

from docx import Document
from docx.oxml.ns import qn

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.word_navigation import bookmark_name

from test_authoring_layout import native_part


def chapter(identity, title, *, role="body", targets=(), children=()):
    return {"chapter_id": identity, "title": title, "targets": list(targets), "children": list(children),
            "presentation_role": role, "is_volume": False}


def word(artifacts, outline):
    result = export_authoring_document(artifacts, outline, [])
    return Document(BytesIO(artifacts.read_bytes(result["docx_ref"])))


def test_each_toc_page_reference_uses_its_real_bookmark_and_never_a_fake_page_one(tmp_path):
    document = word(ArtifactStore(tmp_path), {"chapters": [
        chapter("one", "Technical", children=[chapter("two", "Service")]),
        chapter("content", "Source excerpt", role="content")]})
    fields = [node for node in document.element.iter(qn("w:fldSimple"))
              if node.get(qn("w:instr"), "").strip().startswith("PAGEREF")]
    assert [node.get(qn("w:instr")).strip() for node in fields] == [
        f'PAGEREF {bookmark_name(identity)} \\h' for identity in ("one", "two")]
    assert ["".join(text.text or "" for text in node.iter(qn("w:t"))) for node in fields] == ["待计算", "待计算"]
    assert all(node.get(qn("w:dirty")) in {"true", "1"} for node in fields)
    names = {node.get(qn("w:name")) for node in document.element.iter(qn("w:bookmarkStart"))}
    assert all(bookmark_name(identity) in names for identity in ("one", "two"))


def _source(text):
    return {"target_id": "pending", "kind": "form", "purpose": text, "rule_refs": [],
            "source_refs": [], "manual_fill": True, "support_origins": ["tender"]}


def _catalog_frozen(artifacts):
    literal = artifacts.put_json({"pages": [{"text_stream": (
        "第六章 投标文件格式 ................................................................... 85\n"
        "一、 投标书 ......................................................................... 87\n"
        "二、 开标一览表 ..................................................................... 90\n"
        "三、 投标分项报价表 ................................................................. 91\n"
        "四、 商务条款响应/偏离表 ............................................................ 92\n"
        "五、 投标保证金证明及退款申请函 ..................................................... 93\n"
        "六、 单位负责人身份证明书 ........................................................... 96\n"
        "七、 单位负责人授权委托书 ........................................................... 97\n"
        "八、 资格证明文件 ................................................................... 98\n"
        "九、 投标人根据评标办法需要提供的其他商务文件 ...................................... 104\n"
        "十、 技术条款响应/偏离表 ........................................................... 112\n"
        "十一、 项目服务方案 ................................................................ 113\n"
        "十二、 实施团队人员情况 ............................................................ 115\n"
        "十三、 投标人根据评标办法需要提供的其他商务及技术文件 .......................... 117\n"
    )}]})
    raw = artifacts.put_json({"literal_source_ref": literal})
    return artifacts.put_json({"schema_version": 2, "documents": [{
        "document_id": "main", "role": "main", "raw_ref": raw}]})


def test_first_level_toc_copies_the_tender_catalog_numerals_and_wording(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    outline = {
        "authoring_input": {"frozen_ref": _catalog_frozen(artifacts)},
        "chapters": [
            chapter("letter", "投标书", targets=[_source("一、投标书")]),
            chapter("bond-way", "投标保证金采用汇款形式", targets=[_source("5-2 投标保证金采用汇款形式")]),
            chapter("bond", "投标保证金证明", targets=[_source("五、投标保证金证明")]),
            chapter("other", "其他技术文件", targets=[_source("十三、投标人根据评标办法认为需要提供的其他技术文件")]),
        ],
    }
    document = word(artifacts, outline)
    toc = []
    seen_heading = False
    for paragraph in document.paragraphs:
        style = paragraph.style.name if paragraph.style is not None else ""
        if str(style).startswith("Heading"):
            seen_heading = True
        if not seen_heading and style == "Normal" and paragraph.text.strip():
            toc.append(paragraph.text.split("\t")[0])
    assert toc == [
        "一、 投标书",
        "二、 开标一览表",
        "三、 投标分项报价表",
        "四、 商务条款响应/偏离表",
        "五、 投标保证金证明及退款申请函",
        "六、 单位负责人身份证明书",
        "七、 单位负责人授权委托书",
        "八、 资格证明文件",
        "九、 投标人根据评标办法需要提供的其他商务文件",
        "十、 技术条款响应/偏离表",
        "十一、 项目服务方案",
        "十二、 实施团队人员情况",
        "十三、 投标人根据评标办法需要提供的其他商务及技术文件",
    ]
    headings = [paragraph.text for paragraph in document.paragraphs if paragraph.style and str(paragraph.style.name).startswith("Heading")]
    assert headings[0] == "一、 投标书"
    assert "五、 投标保证金证明及退款申请函" in headings
    assert "十三、 投标人根据评标办法需要提供的其他商务及技术文件" in headings
    assert "5-2 投标保证金采用汇款形式" not in toc
    assert not any(item[:2].isdigit() for item in toc)


def test_bug338_multilevel_toc_headings_preserve_source_numbering_and_dedup_body_heading(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    outline = {
        "authoring_input": {"frozen_ref": _catalog_frozen(artifacts)},
        "chapters": [
            chapter("ch1", "一、投标书", targets=[_source("一、投标书\n致：招标人")], children=[
                chapter("ch1.1", "廉洁投标承诺书", targets=[_source("廉洁投标承诺书\n我方承诺")]),
            ]),
            chapter("pledge", "独立承诺", targets=[_source("独立承诺\n郑重承诺")]),
            chapter("ch5", "五、投标保证金证明", targets=[_source("五、投标保证金证明")], children=[
                chapter("ch5.1", "5-1 投标保证金采用电子保函形式（通宝函）",
                        targets=[_source("5-1 投标保证金采用电子保函形式（通宝函）\n保函正文")]),
            ]),
            chapter("ch11", "十一、项目服务方案", targets=[_source("十一、项目服务方案\n编制指引")], children=[
                chapter("ch11.1", "1.2 工作要求", children=[
                    chapter("ch11.1.1", "1.2.1.2 常态化驻场安全服务", targets=[{
                        "target_id": "t-svc", "kind": "narrative", "purpose": "驻场安全方案",
                        "rule_refs": [], "source_refs": [], "manual_fill": False, "support_origins": ["knowledge"],
                    }]),
                ]),
            ]),
            chapter("ch12", "十二、实施团队人员情况", targets=[_source("十二、实施团队人员情况")], children=[
                chapter("ch12.1", "（一）实施团队人员情况表", targets=[_source("（一）实施团队人员情况表\n表前说明")]),
            ]),
            chapter("extra", "无关联关系承诺函", targets=[_source("无关联关系承诺函\n致：采购人")]),
        ],
    }
    document = word(artifacts, outline)
    toc_rows = []
    seen_heading = False
    for paragraph in document.paragraphs:
        style = paragraph.style.name if paragraph.style is not None else ""
        if str(style).startswith("Heading"):
            seen_heading = True
        if not seen_heading and style == "Normal" and paragraph.text.strip():
            indent = paragraph.paragraph_format.left_indent
            toc_rows.append((paragraph.text.split("\t")[0], round(indent.mm) if indent else 0))

    assert ("一、 投标书", 0) in toc_rows
    assert ("廉洁投标承诺书", 4) in toc_rows
    assert ("独立承诺", 0) in toc_rows
    assert toc_rows.index(("一、 投标书", 0)) < toc_rows.index(("独立承诺", 0)) < toc_rows.index(("五、 投标保证金证明及退款申请函", 0))
    assert ("五、 投标保证金证明及退款申请函", 0) in toc_rows
    assert ("5-1 投标保证金采用电子保函形式（通宝函）", 4) in toc_rows
    assert ("十一、 项目服务方案", 0) in toc_rows
    assert ("1.2 工作要求", 4) in toc_rows
    assert ("1.2.1.2 常态化驻场安全服务", 8) in toc_rows
    assert ("十二、 实施团队人员情况", 0) in toc_rows
    assert ("（一）实施团队人员情况表", 4) in toc_rows
    assert toc_rows[-1] == ("无关联关系承诺函", 0)

    headings = [
        (paragraph.style.name, paragraph.text, paragraph._p.find(f"{qn('w:pPr')}/{qn('w:numPr')}") is not None)
        for paragraph in document.paragraphs
        if paragraph.style is not None and str(paragraph.style.name).startswith("Heading")
    ]
    assert ("Heading 1", "一、 投标书", False) in headings
    assert ("Heading 2", "廉洁投标承诺书", False) in headings
    assert ("Heading 2", "5-1 投标保证金采用电子保函形式（通宝函）", False) in headings
    assert ("Heading 2", "1.2 工作要求", False) in headings
    assert ("Heading 3", "1.2.1.2 常态化驻场安全服务", False) in headings
    assert ("Heading 2", "（一）实施团队人员情况表", False) in headings
    # No duplicate heading line right after any Heading, and no bogus "（原格式待补）" under containers.
    texts = [paragraph.text.strip() for paragraph in document.paragraphs]
    assert "五、投标保证金证明" not in texts
    assert "5-1 投标保证金采用电子保函形式（通宝函）" in texts
    assert texts.count("5-1 投标保证金采用电子保函形式（通宝函）") == 2  # once in TOC, once in Heading 2
    assert "（原格式待补）" not in texts


def test_toc_body_boundary_is_on_the_real_first_heading_not_an_empty_break_paragraph(tmp_path):
    document = word(ArtifactStore(tmp_path), {"chapters": [chapter("one", "Actual body")]})
    anchor = next(node for node in document.element.iter(qn("w:bookmarkStart"))
                  if node.get(qn("w:name")) == bookmark_name("one"))
    first = anchor.getparent()
    assert first.find(f"{qn('w:pPr')}/{qn('w:pageBreakBefore')}") is not None
    assert not [node for node in document.element.iter(qn("w:br")) if node.get(qn("w:type")) == "page"]
    assert not [paragraph for paragraph in document.paragraphs if not paragraph.text]


def test_unnumbered_source_content_keeps_its_bookmark_and_stable_page_start(tmp_path):
    target = {"target_id": "source:1", "kind": "form", "purpose": "Actual original statement.\nSignature: ____",
        "rule_refs": [], "source_refs": [], "manual_fill": True, "support_origins": ["tender"]}
    document = word(ArtifactStore(tmp_path), {"chapters": [
        chapter("source", "Original", role="content", targets=[target]), chapter("later", "Later chapter")]})
    anchor = next(node for node in document.element.iter(qn("w:bookmarkStart"))
                  if node.get(qn("w:name")) == bookmark_name("source"))
    first = anchor.getparent()
    assert first.find(f"{qn('w:pPr')}/{qn('w:pageBreakBefore')}") is not None
    assert "".join(node.text or "" for node in first.iter(qn("w:t"))) == "Actual original statement."
    assert [paragraph.text for paragraph in document.paragraphs].count("Signature: ____") == 1


def test_original_table_at_body_start_keeps_both_anchors_without_a_break_carrier(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    part, _ = native_part(artifacts)
    target = {"target_id": "source:1", "kind": "form", "purpose": "Original table", "template_refs": [part],
        "rule_refs": [], "source_refs": [], "manual_fill": True}
    document = word(artifacts, {"chapters": [
        chapter("source", "Original", role="content", targets=[target]), chapter("later", "Later chapter")]})
    first = document.tables[0].cell(0, 0).paragraphs[0]._p
    assert first.find(f"{qn('w:pPr')}/{qn('w:pageBreakBefore')}") is not None
    names = [node.get(qn("w:name")) for node in first.iter(qn("w:bookmarkStart"))]
    assert names.count(bookmark_name("source")) == names.count(bookmark_name("source:1", "target")) == 1
    assert not [paragraph for paragraph in document.paragraphs if not paragraph.text]
