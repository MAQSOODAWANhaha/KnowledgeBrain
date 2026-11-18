"""Chapter guidance selects documents from a flat knowledge catalog."""

from docreader.tender.guidance import (
    allowed_material_ids,
    bind_slot,
    chapter_guide,
    resolve_utterance,
    tasks_for_chapters,
)


def catalog():
    library = [
        "企业联系方式.md",
        "模拟验收CCRC信息系统安全集成三级证书.png",
        "模拟验收质量管理体系认证证书.png",
        "模拟验收营业执照.png",
        "质量管理体系认证证书.pdf",
        "营业执照.pdf",
        "职业健康安全管理体系认证证书.pdf",
        "环境管理体系认证证书.pdf",
        "法定代表人身份证复印件.pdf",
        "企业资信等级证书.pdf",
        "企业基本信息.md",
        "业绩合同-测试电网边界安全改造项目.pdf",
        "售后服务制度-测试.md",
    ]
    product = [
        "电器电子产品有害物质限制使用评价证书-NGFW-X3000.pdf",
        "商用密码产品认证证书-NGFW-X3000.pdf",
        "产品技术白皮书.md",
        "中国国家强制性产品认证证书-NGFW-X3000.pdf",
        "WEB攻击防护功能证明-NGFW-X3000.pdf",
    ]
    documents = [{"document_id": title, "title": title, "folder": "", "kind": "library"} for title in library]
    documents.extend({"document_id": title, "title": title, "folder": "", "kind": "product"} for title in product)
    return documents


def rule(item_id):
    return {"item_ref": {"artifact_ref": {"sha256": "a" * 64}, "item_id": item_id}}


def target(target_id, *, kind, purpose, rules, manual=False, origins=None, subject=None):
    return {
        "target_id": target_id,
        "kind": kind,
        "purpose": purpose,
        "rule_refs": [rule(item) for item in rules],
        "manual_fill": manual,
        "support_origins": origins or ["knowledge"],
        "subject": subject,
    }


def chapter(chapter_id, title, targets):
    return {"chapter_id": chapter_id, "title": title, "targets": targets}


def qualification():
    return chapter("qual", "资格证明", [
        target("license", kind="attachment", purpose="营业执照", rules=["license"]),
        target("certs", kind="attachment", purpose="资质证书", rules=["certs"]),
    ])


def performance():
    return chapter("perf", "类似业绩", [
        target("cases", kind="attachment", purpose="近三年类似业绩", rules=["cases"]),
    ])


def technical():
    return chapter("tech", "技术方案", [
        target("method", kind="narrative", purpose="产品功能与组网", rules=["method"]),
        target("price", kind="form", purpose="投标报价", rules=["price"], manual=True),
        target("background", kind="narrative", purpose="项目背景", rules=["background"], origins=["tender"]),
    ])


def titles(slot):
    return [document["title"] for document in slot["candidates"]]


def library_titles():
    return [document["title"] for document in catalog() if document["kind"] == "library"]


def test_filename_does_not_filter_company_or_performance_documents():
    guide = {slot["key"]: slot for slot in chapter_guide(qualification(), catalog(), {}, hints=[
        {"document_id": "营业执照.pdf"},
        {"document_id": "not-in-catalog"},
    ])}

    assert titles(guide["proof:license"]) == library_titles()
    assert "企业基本信息.md" in titles(guide["proof:license"])
    assert guide["proof:license"]["hints"] == ["营业执照.pdf"]
    assert titles(guide["proof:certs"]) == library_titles()
    assert titles(chapter_guide(performance(), catalog(), {})[0]) == library_titles()
    assert guide["proof:license"]["selected_document_ids"] == []


def test_service_chapter_keeps_documents_whose_names_do_not_mention_service():
    chapter_service = chapter("service", "售后服务", [
        target("sla", kind="narrative", purpose="售后服务", rules=["sla"]),
    ])
    slot = chapter_guide(chapter_service, catalog(), {})[0]

    assert slot["class"] == "service"
    assert titles(slot) == library_titles()
    assert "营业执照.pdf" in titles(slot)
    assert slot["selected_document_ids"] == []


def test_product_chapter_uses_the_product_library_and_skips_manual_or_tender_slots():
    slots = {slot["class"]: slot for slot in chapter_guide(technical(), catalog(), {})}

    assert titles(slots["product"]) == [
        "电器电子产品有害物质限制使用评价证书-NGFW-X3000.pdf",
        "商用密码产品认证证书-NGFW-X3000.pdf",
        "产品技术白皮书.md",
        "中国国家强制性产品认证证书-NGFW-X3000.pdf",
        "WEB攻击防护功能证明-NGFW-X3000.pdf",
    ]
    assert "price" not in slots
    assert "background" not in slots


def test_ambiguous_purpose_lists_the_whole_catalog_instead_of_guessing():
    mixed = chapter("mixed", "技术服务", [
        target("plan", kind="narrative", purpose="技术服务方案", rules=["plan"]),
    ])
    slot = chapter_guide(mixed, catalog(), {})[0]

    assert slot["class"] == "unresolved"
    assert len(slot["candidates"]) == len(catalog())
    assert slot["selected_document_ids"] == []


def test_two_licenses_are_not_auto_bound_and_project_scope_is_reused():
    guide = {slot["key"]: slot for slot in chapter_guide(qualification(), catalog(), {})}
    assert guide["proof:license"]["selected_document_ids"] == []

    bindings = bind_slot({}, guide["proof:license"], ["营业执照.pdf"], catalog(), scope="project", chapter_id="qual")
    bindings = bind_slot(bindings, guide["proof:certs"], ["不存在.pdf"], catalog(), scope="chapter", chapter_id="qual")
    rebound = {slot["key"]: slot for slot in chapter_guide(qualification(), catalog(), bindings)}

    assert rebound["proof:license"]["selected_document_ids"] == ["营业执照.pdf"]
    assert rebound["proof:license"]["inherited"] is True
    assert rebound["proof:certs"]["selected_document_ids"] == []


def test_chapter_local_performance_does_not_appear_in_another_chapter():
    guide = chapter_guide(performance(), catalog(), {})[0]
    bindings = bind_slot({}, guide, ["业绩合同-测试电网边界安全改造项目.pdf"], catalog(), scope="chapter", chapter_id="perf")
    other = chapter("other", "另一标段业绩", [
        target("other-cases", kind="attachment", purpose="类似业绩", rules=["cases"]),
    ])

    assert chapter_guide(other, catalog(), bindings)[0]["selected_document_ids"] == []


def test_content_identifies_a_document_whose_filename_is_misleading():
    documents = [
        {"document_id": "trap", "title": "营业执照.pdf", "folder": "", "kind": "library",
         "text": "会议纪要。下周讨论日程，没有证照信息。"},
        {"document_id": "body", "title": "记录002.md", "folder": "", "kind": "library",
         "text": "测试企业。统一社会信用代码 TEST-NOT-REAL-0000。类型：营业执照。"},
        {"document_id": "other", "title": "售后服务制度-测试.md", "folder": "", "kind": "library",
         "text": "售后响应时间：4小时。"},
    ]
    slot = chapter_guide(chapter("qual", "资格", [
        target("license", kind="attachment", purpose="营业执照", rules=["license"]),
    ]), documents, {})[0]

    assert [item["document_id"] for item in slot["candidates"]] == ["trap", "body", "other"]
    assert slot["content_document_ids"] == ["body"]
    assert slot["selected_document_ids"] == []


def test_filename_utterance_does_not_select_and_low_score_does_not_drop_or_adopt():
    slots = chapter_guide(qualification(), catalog(), {}) + chapter_guide(performance(), catalog(), {})
    scores = {"企业基本信息.md": 0.9, "业绩合同-测试电网边界安全改造项目.pdf": 0.049}
    ranked = chapter_guide(performance(), catalog(), {}, scores=scores)[0]

    assert resolve_utterance("资质用营业执照.pdf", slots) is None
    assert titles(ranked)[0] == "企业基本信息.md"
    assert "业绩合同-测试电网边界安全改造项目.pdf" in titles(ranked)
    assert len(ranked["candidates"]) == len(library_titles())
    assert ranked["selected_document_ids"] == []


def test_write_uses_only_documents_bound_to_the_requested_chapter():
    slots = {slot["key"]: slot for slot in chapter_guide(qualification(), catalog(), {})}
    bindings = bind_slot({}, slots["proof:license"], ["营业执照.pdf"], catalog(), scope="project", chapter_id="qual")
    tasks = [
        {"task_id": "qual-task", "chapter_id": "qual", "targets": qualification()["targets"]},
        {"task_id": "tech-task", "chapter_id": "tech", "targets": technical()["targets"]},
    ]
    materials = [
        {"material_id": "mat-license", "document_id": "营业执照.pdf"},
        {"material_id": "mat-case", "document_id": "业绩合同-测试电网边界安全改造项目.pdf"},
        {"material_id": "mat-paper", "document_id": "产品技术白皮书.md"},
    ]

    assert [task["task_id"] for task in tasks_for_chapters(tasks, ["qual"])] == ["qual-task"]
    assert allowed_material_ids(tasks[0], chapter_guide(qualification(), catalog(), bindings), bindings, materials) == [
        "mat-license"]
    assert allowed_material_ids(tasks[1], chapter_guide(technical(), catalog(), bindings), bindings, materials) == []
