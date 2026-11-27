"""Project-assigned source metadata alongside the immutable uploaded bytes."""

from docreader.integrations.web import WebError, _digest, _uuid
from docreader.tender.status import DocumentRole


async def project_sources(bridge, project_id, *, connection=None):
    async with bridge.store.transaction(connection) as conn:
        row = await (await conn.execute(
            "SELECT s.settings, COALESCE(s.revision, 0) AS revision, (SELECT COALESCE(jsonb_agg(jsonb_build_object("
            "'document_id', w.document_id, 'original_ref', w.original_ref, 'file_name', d.file_name) "
            "ORDER BY w.created_at, w.document_id), '[]'::jsonb) "
            "FROM tender_document_sources w JOIN tender_documents d "
            "ON d.id = w.document_id AND d.project_id = w.project_id WHERE w.project_id = p.id) AS documents "
            "FROM tender_projects p LEFT JOIN tender_project_source_settings s ON s.project_id = p.id "
            "WHERE p.id = %s",
            (project_id,),
        )).fetchone()
    if row is None:
        raise WebError("PROJECT_NOT_FOUND", "项目不存在或无权访问。", 404)
    settings = row["settings"] if row["settings"] is not None else {
        "documents": [], "expected_document_ids": [],
    }
    return row["documents"], settings, row["revision"]


def configured_documents(rows, settings):
    saved = {document["document_id"]: document for document in settings["documents"]}
    documents = []
    for row in rows:
        document_id = row["document_id"]
        documents.append({
            "document_id": document_id,
            "file_name": row["file_name"],
            "role": DocumentRole.MAIN,
            "replaces": None,
            "modification_scope": None,
            **saved.get(document_id, {}),
            "document_version": row["original_ref"]["sha256"],
            "original_ref": row["original_ref"],
        })
    return documents


def settings_view(rows, settings, revision):
    documents = configured_documents(rows, settings)
    return {
        "source_settings": {
            "version": _digest({"documents": documents, "settings": settings, "revision": revision}),
            "expected_document_ids": settings["expected_document_ids"],
            "documents": [
                {key: item[key] for key in ("document_id", "file_name", "role", "replaces", "modification_scope")}
                for item in documents
            ],
        }
    }


def submitted_settings(payload, rows):
    if set(payload) - {"action", "project_id", "owner_user_id", "request_key", "expected_version",
                       "documents", "expected_document_ids"}:
        raise WebError("INVALID_SOURCE_SETTINGS", "来源设置包含不支持的字段。")
    ids = {row["document_id"] for row in rows}
    documents = payload.get("documents")
    if not isinstance(documents, list):
        raise WebError("INVALID_SOURCE_SETTINGS", "请提供当前全部项目文件的来源设置。")
    selected = {}
    for entry in documents:
        if not isinstance(entry, dict) or set(entry) != {"document_id", "role", "replaces", "modification_scope"}:
            raise WebError("INVALID_SOURCE_SETTINGS", "文件来源字段不完整或包含额外内容。")
        document_id = _uuid(entry["document_id"])
        replacement = _uuid(entry["replaces"]) if entry["replaces"] is not None else None
        scope = entry["modification_scope"]
        if (document_id not in ids or document_id in selected
                or entry["role"] not in tuple(DocumentRole)
                or (replacement is not None and (replacement not in ids or replacement == document_id))
                or (scope is not None and (not isinstance(scope, str) or len(scope) > 2000 or "\0" in scope))):
            raise WebError("INVALID_SOURCE_SETTINGS", "请核对文件角色、同项目替代文件和适用范围。")
        selected[document_id] = {
            "document_id": document_id, "role": entry["role"], "replaces": replacement,
            "modification_scope": scope.strip() or None if scope is not None else None,
        }
    expected = payload.get("expected_document_ids")
    if (selected.keys() != ids or not isinstance(expected, list) or len(expected) > 100
            or any(not isinstance(value, str) or not value.strip() or len(value) > 256 or "\0" in value
                   for value in expected)):
        raise WebError("INVALID_SOURCE_SETTINGS", "请登记全部上传文件及有效的必要附件标识。")
    expected = sorted({value.strip() for value in expected})
    return {"documents": [selected[key] for key in sorted(selected)], "expected_document_ids": expected}
