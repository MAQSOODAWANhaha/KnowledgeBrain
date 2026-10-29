"""Authenticated project adapter over existing analysis and authoring receipts.

The Rust API owns login/project authorization and checks it before this RPC. This
adapter checks ownership again, accepts uploaded bytes rather than caller paths,
and only serves artifacts reached through the project's persisted execution.
"""
from __future__ import annotations

import asyncio
import hashlib
import json
import logging
from uuid import UUID, uuid4

from psycopg.types.json import Jsonb

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.store import Conflict, Store
from docreader.jobs.settings import Settings
from docreader.tender.status import AuthoringExecutionStatus, AuthoringJobStatus, ProjectStatus, status_row

logger = logging.getLogger(__name__)


_INITIALIZED = set()


class WebError(ValueError):
    def __init__(self, code, message, status=400):
        super().__init__(message)
        self.code, self.status = code, status


def _digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False,
                                    separators=(",", ":"), allow_nan=False).encode()).hexdigest()


def _uuid(value):
    try:
        return str(UUID(value))
    except (ValueError, TypeError, AttributeError):
        raise WebError("INVALID_ID", "项目或用户标识不正确。") from None


def _request_key(payload):
    key = payload.get("request_key")
    if not isinstance(key, str) or not key.strip() or len(key) > 256:
        raise WebError("REQUEST_KEY_REQUIRED", "此操作需要有效的幂等请求标识。")
    return _digest({"project_id": payload["project_id"], "key": key})


def _identity(row):
    return {key: row[key] for key in ("run_id", "source_version", "domain_revision")}


def _operation(job):
    return job["payload"].get("operation", "document")


def _source_identities(documents):
    return sorted(_digest({key: document.get(key) for key in
                           ("document_id", "document_version", "role", "original_ref")})
                  for document in documents)


def _analysis_request(payload):
    if set(payload) - {"action", "project_id", "owner_user_id", "request_key"}:
        raise WebError("INVALID_ANALYSIS_OPTIONS", "开始分析仅接受当前项目资料，请移除不支持的请求字段。")


def _outline_request(payload):
    if "expected_revision_id" not in payload:
        raise WebError("OUTLINE_BASE_REQUIRED", "保存大纲需要明确当前修订；首次保存请传空修订。")
    expected = _uuid(payload["expected_revision_id"]) if payload["expected_revision_id"] is not None else None
    source_revision = payload.get("source_revision_id")
    source_execution = payload.get("source_execution_id")
    if (source_revision is None) == (source_execution is None):
        raise WebError("OUTLINE_SOURCE_REQUIRED", "请选择一个已保存修订或已完成任务作为大纲来源。")
    source_revision = _uuid(source_revision) if source_revision is not None else None
    if source_execution is not None and (not isinstance(source_execution, str) or not source_execution.strip()):
        raise WebError("OUTLINE_SOURCE_REQUIRED", "大纲来源任务标识不正确。")
    return {"expected_revision_id": expected, "source_revision_id": source_revision,
            "source_execution_id": source_execution, "chapters": payload.get("chapters")}


def _authoring_request(payload, *, outline_only=False):
    from docreader.tender.schema import AuthoringReviewRequest
    from docreader.integrations.web_knowledge import normalize_knowledge_selections

    request = {"previous_execution_id": payload.get("previous_execution_id"),
               "review_request": payload.get("review_request")}
    try:
        selections = normalize_knowledge_selections(payload.get("knowledge_selections", []))
    except ValueError as error:
        raise WebError("INVALID_KNOWLEDGE_SELECTION", str(error)) from None
    if outline_only and selections:
        raise WebError("INVALID_OUTLINE_OPTIONS", "知识资产仅用于正文编制，请在生成正文时选择。")
    if selections:
        request["knowledge_selections"] = selections
    revision_id, outline_sha256 = payload.get("outline_revision_id"), payload.get("outline_sha256")
    if (revision_id is None) != (outline_sha256 is None) or outline_only and revision_id is not None:
        raise WebError("OUTLINE_REVISION_REQUIRED", "正文生成需要同时指定保存的大纲修订和校验值。")
    if revision_id is not None:
        request.update(outline_revision_id=_uuid(revision_id), outline_sha256=outline_sha256)
    if request["review_request"] is not None:
        request["review_request"] = AuthoringReviewRequest.model_validate(request["review_request"]).model_dump()
    return request


class WebBridge:
    def __init__(self, settings):
        self.settings = settings
        self.store = Store(settings.dsn)
        self.authoring = AuthoringStore(settings.dsn)
        self.artifacts = ArtifactStore(settings.artifact_root)

    async def setup(self):
        if self.settings.dsn in _INITIALIZED:
            return
        async with self.store.transaction() as conn:
            if self.settings.dsn in _INITIALIZED:
                return
            await self.store.setup()
            await self.authoring.setup()
            await conn.execute("SELECT document_id FROM tender_document_sources LIMIT 0")
            await conn.execute("SELECT run_id FROM tender_project_analyses LIMIT 0")
            await conn.execute("SELECT execution_id FROM tender_authoring_jobs LIMIT 0")
            await conn.execute("SELECT revision_id FROM tender_outline_revisions LIMIT 0")
        _INITIALIZED.add(self.settings.dsn)

    async def authorize(self, payload, *, write=False):
        project_id, user_id = _uuid(payload.get("project_id")), _uuid(payload.get("owner_user_id"))
        async with self.store.transaction() as conn:
            row = status_row(await (await conn.execute(
                "SELECT id, status FROM tender_projects WHERE id = %s AND owner_user_id = %s",
                (project_id, user_id),
            )).fetchone(), ProjectStatus)
        if row is None:
            raise WebError("PROJECT_NOT_FOUND", "项目不存在或无权访问。", 404)
        if write and row["status"] != ProjectStatus.OPEN:
            raise WebError("PROJECT_ENDED", "项目已结束，仍可读取已有文件。", 409)
        return project_id

    async def lock_project(self, conn, payload):
        row = status_row(await (await conn.execute(
            "SELECT id,status FROM tender_projects WHERE id=%s AND owner_user_id=%s FOR UPDATE",
            (_uuid(payload.get("project_id")), _uuid(payload.get("owner_user_id"))),
        )).fetchone(), ProjectStatus)
        if row is None:
            raise WebError("PROJECT_NOT_FOUND", "项目不存在或无权访问。", 404)
        return row["status"] == ProjectStatus.OPEN

    async def documents(self, project_id, kind="tender"):
        async with self.store.transaction() as conn:
            return await (await conn.execute(
                "SELECT w.*, d.file_name, d.media_type FROM tender_document_sources w "
                "JOIN tender_documents d ON d.id = w.document_id AND d.project_id = w.project_id "
                "WHERE w.project_id = %s AND (%s::text IS NULL OR w.kind = %s) ORDER BY w.created_at, w.document_id",
                (project_id, kind, kind),
            )).fetchall()

    async def put_document(self, payload, blob):
        document_id, project_id = _uuid(payload.get("document_id")), payload["project_id"]
        role, kind = payload.get("role", "main"), payload.get("kind", "tender")
        if role not in {"main", "attachment", "clarification", "amendment"} or kind not in {"tender", "enterprise"}:
            raise WebError("INVALID_DOCUMENT_ROLE", "资料类别或来源角色不正确。")
        digest = hashlib.sha256(blob).hexdigest()
        async with self.store.transaction() as conn:
            project_open = await self.lock_project(conn, payload)
            row = await (await conn.execute(
                "SELECT file_name, media_type, content_sha256, byte_length FROM tender_documents "
                "WHERE id = %s AND project_id = %s", (document_id, project_id),
            )).fetchone()
            if row is None:
                raise WebError("DOCUMENT_NOT_FOUND", "文件尚未登记在本项目中。", 404)
            if not blob or digest != row["content_sha256"] or len(blob) != row["byte_length"]:
                raise WebError("DOCUMENT_IDENTITY_MISMATCH", "上传内容与已登记文件身份不一致。", 409)
            reference = {"sha256": digest, "relative_path": f"{digest[:2]}/{digest}",
                         "size": len(blob), "media_type": row["media_type"]}
            saved = await (await conn.execute("SELECT * FROM tender_document_sources WHERE document_id = %s",
                                              (document_id,))).fetchone()
            if saved is None:
                if not project_open:
                    raise WebError("PROJECT_ENDED", "项目已结束，仍可读取已有文件。", 409)
                reference = await asyncio.to_thread(self.artifacts.put_bytes, blob, row["media_type"])
                saved = await (await conn.execute(
                    "INSERT INTO tender_document_sources(document_id, project_id, kind, role, original_ref) "
                    "VALUES (%s,%s,%s,%s,%s) RETURNING *",
                    (document_id, project_id, kind, role, Jsonb(reference)),
                )).fetchone()
            if (str(saved["project_id"]) != project_id or saved["kind"] != kind
                    or saved["role"] != role or saved["original_ref"] != reference):
                raise Conflict("document already identifies different project content or role")
        return {"document": {"document_id": document_id, "file_name": row["file_name"], "kind": kind, "role": role}}

    async def manifest(self, project_id, kind="tender"):
        from docreader.tender.evidence import register_manifest

        documents = await self.documents(project_id, kind)
        if not documents and kind == "tender":
            raise WebError("TENDER_DOCUMENT_REQUIRED", "请先上传招标文件，再开始分析或编制。", 409)
        registered = await asyncio.to_thread(register_manifest, {"documents": [
            {"document_id": str(row["document_id"]), "document_version": row["original_ref"]["sha256"],
             "role": row["role"], "file_name": row["file_name"], "original_ref": row["original_ref"]}
            for row in documents]}, self.artifacts)
        return registered

    async def latest_run(self, project_id, *, source_only=False):
        async with self.store.transaction() as conn:
            row = await (await conn.execute(
                "SELECT run_id FROM tender_project_analyses WHERE project_id = %s AND source_only = %s "
                "ORDER BY created_at DESC, run_id DESC LIMIT 1", (project_id, source_only),
            )).fetchone()
        return await self.store.get_run(row["run_id"]) if row else None

    async def start_analysis(self, payload):
        from docreader.workflow.persistence import initialize_checkpoints
        from docreader.integrations.web_replay import replay_ended

        _analysis_request(payload)
        reference = await asyncio.to_thread(self.artifacts.put_json, await self.manifest(payload["project_id"]))
        key = _request_key(payload)
        await initialize_checkpoints(self.settings.dsn)
        async with self.store.transaction() as conn:
            if not await self.lock_project(conn, payload):
                return await replay_ended(self, payload)
            row = await self.store.submit(reference, f"web-analysis:{key}", connection=conn)
            await conn.execute(
                "INSERT INTO tender_project_analyses(run_id,project_id,request_key) VALUES(%s,%s,%s) "
                "ON CONFLICT(project_id,request_key) DO NOTHING", (row["run_id"], payload["project_id"], key),
            )
        return {"analysis": await self.analysis_view(await self.store.get_run(row["run_id"]), payload["project_id"])}

    async def sources_current(self, run, registered):
        frozen = [entry for entry in run["artifacts"] if entry["stage"] == "freeze" and not entry["scope"]
                  and entry["source_version"] == run["source_version"] and entry["domain_revision"] == 0]
        if len(frozen) != 1:
            return False
        source = await asyncio.to_thread(self.artifacts.read_json, frozen[0]["artifact_ref"])
        return _source_identities(source["documents"]) == _source_identities(registered["documents"])

    async def source_run(self, project_id, *, registered=None):
        """Initialize usable native sources without enqueueing an analysis call."""
        from docreader.tender.evidence import freeze_sources

        registered = registered if registered is not None else await self.manifest(project_id)
        manifest_ref = await asyncio.to_thread(self.artifacts.put_json, registered)
        key = "source-only:" + manifest_ref["sha256"]
        async with self.store.transaction() as conn:
            existing = await (await conn.execute(
                "SELECT run_id FROM tender_project_analyses WHERE project_id=%s AND request_key=%s", (project_id, key),
            )).fetchone()
        if existing:
            return await self.store.get_run(existing["run_id"])
        frozen_ref = await freeze_sources(registered, self.artifacts, ocr=None, plan=False)
        run_id = "web-source-" + _digest({"project_id": project_id, "manifest": manifest_ref})
        async with self.store.transaction() as conn:
            await conn.execute("INSERT INTO tender_analysis_runs(run_id,request_key) VALUES(%s,%s) ON CONFLICT DO NOTHING",
                               (run_id, run_id))
            await conn.execute(
                "INSERT INTO tender_analysis_source_versions(run_id,source_version,request_key,manifest_ref) VALUES(%s,1,%s,%s) "
                "ON CONFLICT DO NOTHING", (run_id, run_id, Jsonb(manifest_ref)),
            )
            await conn.execute(
                "INSERT INTO tender_analysis_revisions(run_id,source_version,domain_revision,request_key) "
                "VALUES(%s,1,1,%s) ON CONFLICT DO NOTHING", (run_id, run_id),
            )
            await conn.execute(
                "INSERT INTO tender_analysis_artifacts(run_id,source_version,domain_revision,stage,artifact_ref,epoch,metadata) "
                "VALUES(%s,1,0,'freeze',%s,0,%s) ON CONFLICT DO NOTHING",
                (run_id, Jsonb(frozen_ref), Jsonb({"source_only": True, "model_requests": 0})),
            )
            await conn.execute(
                "INSERT INTO tender_project_analyses(run_id,project_id,request_key,source_only) VALUES(%s,%s,%s,true) "
                "ON CONFLICT DO NOTHING", (run_id, project_id, key),
            )
        return await self.store.get_run(run_id)

    async def materials(self, project_id):
        from docreader.tender.evidence import EvidenceRepository, freeze_sources

        registered = await self.manifest(project_id, "enterprise")
        if not registered["documents"]:
            return []
        frozen = self.artifacts.read_json(await freeze_sources(registered, self.artifacts, ocr=None, plan=False))
        repository, materials = EvidenceRepository(self.artifacts), []
        for document in frozen["documents"]:
            if not document.get("source_index_ref"):
                continue
            index = repository.source_index(document["source_index_ref"])
            for unit in index.raw["units"]:
                text = unit.get("text", "")
                if unit.get("grid"):
                    text += "\n" + "\n".join(cell.get("text", "") for cell in unit["grid"].get("cells", []))
                if not text.strip():
                    continue
                materials.append({"material_id": "web-material-" + _digest({"document": document["document_id"], "unit": unit["unit_id"]}),
                    "title": document["file_name"], "text": text, "original_ref": document["original_ref"],
                    "locator": {"document_id": document["document_id"], "file_name": document["file_name"],
                                "source_ref": {"source_index_ref": document["source_index_ref"], "unit_id": unit["unit_id"]}},
                    "usage": "reference", "attachment": True})
        return materials

    async def job(self, project_id, execution_id):
        async with self.store.transaction() as conn:
            row = status_row(await (await conn.execute(
                "SELECT * FROM tender_authoring_jobs WHERE project_id=%s AND execution_id=%s",
                (project_id, execution_id),
            )).fetchone(), AuthoringJobStatus)
        if row is None:
            raise WebError("EXECUTION_NOT_FOUND", "编制任务不存在或不属于此项目。", 404)
        return row

    async def outline_sources_current(self, project_id, outline):
        reference = outline["authoring_input"].get("frozen_ref")
        if reference is None:
            return False
        frozen = await asyncio.to_thread(self.artifacts.read_json, reference)
        current = await self.manifest(project_id)
        return _source_identities(frozen["documents"]) == _source_identities(current["documents"])

    async def outline_revision(self, project_id, revision_id):
        async with self.store.transaction() as conn:
            row = await (await conn.execute("SELECT * FROM tender_outline_revisions "
                "WHERE project_id=%s AND revision_id=%s", (project_id, _uuid(revision_id)))).fetchone()
        if row is None:
            raise WebError("OUTLINE_REVISION_NOT_FOUND", "大纲修订不存在或不属于此项目。", 404)
        return row

    async def outline_draft(self, row):
        from docreader.tender.schema import AuthoringOutline

        outline = AuthoringOutline.model_validate(await asyncio.to_thread(self.artifacts.read_json, row["outline_ref"])).model_dump(exclude_unset=True)
        return {"revision_id": str(row["revision_id"]), "revision": row["revision"],
                "parent_id": str(row["parent_id"]) if row["parent_id"] else None,
                "execution_id": row["execution_id"], "sha256": row["outline_ref"]["sha256"], "outline": outline,
                "sources_current": await self.outline_sources_current(str(row["project_id"]), outline)}

    async def execution_outline(self, project_id, execution_id):
        from docreader.tender.schema import AuthoringOutline

        job = await self.job(project_id, execution_id)
        execution = await self.authoring.get_execution(job["execution_id"], missing_ok=True)
        if not execution or execution["status"] != AuthoringExecutionStatus.SUCCEEDED or not execution["result_ref"]:
            raise WebError("OUTLINE_NOT_READY", "此任务尚未保存完整大纲，请等待完成或恢复原任务。", 409)
        reference = execution["result_ref"]
        value = await asyncio.to_thread(self.artifacts.read_json, reference)
        if _operation(job) == "document":
            reference = value["outline_ref"]
            value = await asyncio.to_thread(self.artifacts.read_json, reference)
        outline = AuthoringOutline.model_validate(value).model_dump(exclude_unset=True)
        return reference, outline

    async def outline_view(self, project_id):
        async with self.store.transaction() as conn:
            head = await (await conn.execute("SELECT * FROM tender_outline_revisions WHERE project_id=%s "
                "ORDER BY revision DESC LIMIT 1", (project_id,))).fetchone()
            jobs = [status_row(entry, AuthoringJobStatus) for entry in await (await conn.execute("SELECT * FROM tender_authoring_jobs WHERE project_id=%s "
                "ORDER BY created_at DESC,execution_id DESC", (project_id,))).fetchall()]
        views = [await self.authoring_view(job) for job in jobs if _operation(job) == "outline"]
        completed = next((view for view in views if view["status"] == AuthoringExecutionStatus.SUCCEEDED), None)
        if completed is None:
            documents = [await self.authoring_view(job) for job in jobs if _operation(job) == "document"]
            completed = next((view for view in documents if view["status"] == AuthoringExecutionStatus.SUCCEEDED), None)
        candidate = None
        if completed:
            reference, outline = await self.execution_outline(project_id, completed["execution_id"])
            candidate = {"execution_id": completed["execution_id"], "sha256": reference["sha256"], "outline": outline}
        return {"draft": await self.outline_draft(head) if head else None,
                "candidate": candidate, "generation": views[0] if views else None}

    async def save_outline(self, payload):
        from docreader.tender.outline_edit import apply_outline_edit
        from docreader.integrations.web_outline import response_catalog
        from docreader.integrations.web_replay import replay_ended

        request = _outline_request(payload)
        expected, source_revision, source_execution = (request[key] for key in
            ("expected_revision_id", "source_revision_id", "source_execution_id"))
        key, digest = _request_key(payload), _digest(request)
        async with self.store.transaction() as conn:
            if not await self.lock_project(conn, payload):
                return await replay_ended(self, payload)
            saved = await (await conn.execute("SELECT * FROM tender_outline_revisions WHERE project_id=%s AND request_key=%s",
                (payload["project_id"], key))).fetchone()
            if saved:
                if saved["request_digest"] != digest:
                    raise Conflict("request key already identifies a different outline edit")
            else:
                head = await (await conn.execute("SELECT * FROM tender_outline_revisions WHERE project_id=%s "
                    "ORDER BY revision DESC LIMIT 1", (payload["project_id"],))).fetchone()
                if (str(head["revision_id"]) if head else None) != expected:
                    raise WebError("OUTLINE_REVISION_CONFLICT", "大纲已被另一页面保存，请读取最新修订后合并当前修改。", 409)
                if source_revision is not None:
                    source = await self.outline_revision(payload["project_id"], source_revision)
                    base = await asyncio.to_thread(self.artifacts.read_json, source["outline_ref"])
                    source_execution = source["execution_id"]
                else:
                    _, base = await self.execution_outline(payload["project_id"], source_execution)
                    source = None
                catalog = await response_catalog(self, payload["project_id"], base,
                                                  revision=source, execution_id=source_execution)
                outline = await asyncio.to_thread(apply_outline_edit, base, request["chapters"], target_catalog=catalog)
                reference = await asyncio.to_thread(self.artifacts.put_json, outline)
                saved = await (await conn.execute("INSERT INTO tender_outline_revisions "
                    "(revision_id,project_id,revision,parent_id,execution_id,outline_ref,request_key,request_digest) "
                    "VALUES(%s,%s,%s,%s,%s,%s,%s,%s) RETURNING *", (str(uuid4()), payload["project_id"],
                    head["revision"] + 1 if head else 1, expected, source_execution, Jsonb(reference), key, digest))).fetchone()
        return {"draft": await self.outline_draft(saved)}

    async def start_outline(self, payload):
        if any(payload.get(key) is not None for key in ("review_request", "outline_revision_id", "outline_sha256")):
            raise WebError("INVALID_OUTLINE_OPTIONS", "生成大纲时请使用当前招标资料；正文修订选项仅用于正文生成。")
        return await self.start_authoring(payload, outline_only=True)

    async def start_authoring(self, payload, *, outline_only=False):
        from docreader.llm.config import ModelConfig
        from docreader.integrations.web_replay import replay_ended

        key = _request_key(payload)
        execution_id = ("web-outline-" if outline_only else "web-document-") + key
        result_key = "generation" if outline_only else "authoring"
        request = _authoring_request(payload, outline_only=outline_only)
        selections = request.get("knowledge_selections", [])
        revision_id, outline_sha256 = payload.get("outline_revision_id"), payload.get("outline_sha256")
        try:
            existing = await self.job(payload["project_id"], execution_id)
        except WebError as error:
            if error.code != "EXECUTION_NOT_FOUND":
                raise
        else:
            if existing["payload"]["request"] != request:
                raise Conflict("request key already identifies different authoring options")
            return {result_key: await self.authoring_view(existing)}
        ModelConfig.from_env()  # Admission only, never sends a model request.
        registered = await self.manifest(payload["project_id"])
        outline_ref = None
        if revision_id is not None:
            revision = await self.outline_revision(payload["project_id"], request["outline_revision_id"])
            if revision["outline_ref"]["sha256"] != outline_sha256:
                raise WebError("OUTLINE_IDENTITY_MISMATCH", "大纲修订与校验值不一致，请重新读取已保存的大纲。", 409)
            outline_ref = revision["outline_ref"]
            outline = await asyncio.to_thread(self.artifacts.read_json, outline_ref)
            if not await self.outline_sources_current(payload["project_id"], outline):
                raise WebError("SOURCE_CHANGED_REQUIRES_NEW_OUTLINE", "招标资料已更新，请重新生成并保存大纲后生成正文；原稿仍保留。", 409)
            identity = _identity(outline["authoring_input"])
            run = await self.store.get_run(identity["run_id"])
        else:
            run = await self.latest_run(payload["project_id"])
            if run is None or not await self.sources_current(run, registered):
                run = await self.source_run(payload["project_id"], registered=registered)
            identity = _identity(run)
        if request["previous_execution_id"] is not None:
            previous = await self.job(payload["project_id"], request["previous_execution_id"])
            run = await self.store.get_run(previous["run_id"])
            if (_operation(previous) != ("outline" if outline_only else "document")
                    or (outline_only or outline_ref is not None) and run["run_id"] != identity["run_id"]):
                raise Conflict("authoring predecessor must match the operation and outline analysis run")
            if not await self.sources_current(run, registered):
                raise WebError("SOURCE_CHANGED_REQUIRES_NEW_DRAFT", "招标资料已更新，请生成新稿；旧执行仍可继续恢复。", 409)
            if outline_ref is None:
                identity = _identity(run)
            if request["review_request"] is not None:
                # Selection identities belong to this successful document, not
                # to a newly organized outline or the latest analysis revision.
                document = await self.result(payload["project_id"], previous["execution_id"])
                previous_outline = await asyncio.to_thread(self.artifacts.read_json, document["outline_ref"])
                from docreader.tender.authoring_pipeline import plan_writing_tasks
                targets = {target["target_id"] for task in plan_writing_tasks(previous_outline)
                           for target in task["targets"] if not target.get("manual_fill", False)}
                if set(request["review_request"]["target_ids"]) - targets:
                    raise WebError("REVIEW_TARGET_UNBOUND", "所选响应位置不属于该生成稿，或需要人工填写，请重新选择。", 409)
                if outline_ref is not None and outline_ref != document["outline_ref"]:
                    raise WebError("REVIEW_OUTLINE_CHANGED", "定向复核需沿用所选生成稿的大纲，请单独按新大纲生成正文。", 409)
                outline_ref = document["outline_ref"]
                identity = _identity(previous_outline["authoring_input"])
        data = {"identity": identity, "request": request, "operation": "outline" if outline_only else "document",
                "owner_user_id": payload["owner_user_id"],
                "materials": [] if outline_only else await self.materials(payload["project_id"])}
        if selections:
            data["knowledge_selections"] = selections
        if outline_ref is None:
            snapshot = await read_authoring_input(self.store, self.artifacts, **identity)
            data["authoring_input_ref"] = await asyncio.to_thread(self.artifacts.put_json, snapshot)
        if outline_ref is not None:
            data["outline_ref"] = outline_ref
        async with self.store.transaction() as conn:
            if not await self.lock_project(conn, payload):
                return await replay_ended(self, payload)
            await conn.execute(
                "INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload) "
                "VALUES(%s,%s,%s,%s) ON CONFLICT DO NOTHING",
                (execution_id, payload["project_id"], run["run_id"], Jsonb(data)),
            )
        saved = await self.job(payload["project_id"], execution_id)
        if saved["payload"]["request"] != request:
            raise Conflict("request key already identifies different authoring options")
        return {result_key: await self.authoring_view(saved)}

    async def update_job(self, payload, *, cancel=False):
        from docreader.integrations.web_replay import replay_ended

        job = await self.job(payload["project_id"], payload.get("execution_id"))
        execution = await self.authoring.get_execution(job["execution_id"], missing_ok=True)
        if cancel:
            if execution is not None:
                await self.authoring.cancel(job["execution_id"])
                if execution["status"] == AuthoringExecutionStatus.SUCCEEDED:
                    return {"authoring": await self.authoring_view(job)}
            async with self.store.transaction() as conn:
                await conn.execute("UPDATE tender_authoring_jobs SET status='cancelled',updated_at=clock_timestamp() "
                                   "WHERE execution_id=%s AND status<>'succeeded'", (job["execution_id"],))
        else:
            key = _request_key(payload)
            async with self.store.transaction() as conn:
                if not await self.lock_project(conn, payload):
                    return await replay_ended(self, payload)
                current = status_row(await (await conn.execute("SELECT * FROM tender_authoring_jobs WHERE execution_id=%s FOR UPDATE",
                                                   (job["execution_id"],))).fetchone(), AuthoringJobStatus)
                if key not in current["resume_keys"]:
                    if current["status"] == AuthoringJobStatus.CANCELLED or execution and execution["cancel_requested"]:
                        raise Conflict("cancelled document execution cannot be resumed")
                    if current["status"] != AuthoringJobStatus.SUCCEEDED:
                        await conn.execute("UPDATE tender_authoring_jobs SET status='pending',claim_until=NULL,error=NULL,"
                                           "resume_keys=%s,updated_at=clock_timestamp() WHERE execution_id=%s",
                                           (Jsonb([*current["resume_keys"], key]), job["execution_id"]))
        return {"authoring": await self.authoring_view(await self.job(payload["project_id"], job["execution_id"]))}

    async def analysis_view(self, run, project_id=None):
        from docreader.tender.authoring import _OutlineContext
        from docreader.integrations.web_progress import analysis_progress

        snapshot = await read_authoring_input(self.store, self.artifacts, **_identity(run))
        context = await asyncio.to_thread(_OutlineContext, self.artifacts, snapshot)
        verified = self.artifacts.read_json(snapshot["verification_ref"]) if snapshot["verification_ref"] else {}
        sources_current = None
        if project_id:
            submitted = await asyncio.to_thread(self.artifacts.read_json, run["manifest_ref"])
            current = await self.manifest(project_id)
            sources_current = _source_identities(submitted["documents"]) == _source_identities(current["documents"])
        items = []
        for entry in context.payload["rules"]:
            item = entry.get("item", entry.get("effective_rule", {}))
            reference = context.rules[entry["rule_id"]]
            items.append({**item, "item_id": item.get("item_id", reference["item_ref"]["item_id"]),
                          "rule_id": entry["rule_id"], "rule_ref": reference})
        return {**_identity(run), "status": run["status"], "confirmed": snapshot["confirmation_ref"] is not None,
                "sources_current": sources_current,
                "items": items, "rules": context.payload["rules"], "issues": context.payload["issues"],
                "summary": verified.get("summary", {}), "usage": _usage(run["usage"]),
                "rule_origin": context.rule_origin, "progress": analysis_progress(run)}

    async def authoring_view(self, job):
        from docreader.integrations.web_progress import authoring_progress

        job = status_row(job, AuthoringJobStatus)
        execution = await self.authoring.get_execution(job["execution_id"], missing_ok=True)
        status = execution["status"] if execution else job["status"]
        if (status == AuthoringExecutionStatus.FAILED
                and job["status"] in {AuthoringJobStatus.PENDING, AuthoringJobStatus.PUBLISHED}):
            status = job["status"]
        if job["status"] in {AuthoringJobStatus.FAILED, AuthoringJobStatus.CANCELLED} and status != AuthoringExecutionStatus.SUCCEEDED:
            status = job["status"]
        return {"execution_id": job["execution_id"], "status": status, "operation": _operation(job),
                "usage": _usage(execution["requests"]) if execution else None,
                "error": (execution.get("error") or job["error"] if execution else job["error"])
                         if status == AuthoringJobStatus.FAILED else None,
                "progress": authoring_progress(job, execution, status=status)}

    async def result(self, project_id, execution_id):
        job = await self.job(project_id, execution_id)
        if _operation(job) != "document":
            raise WebError("DOCUMENT_EXECUTION_REQUIRED", "大纲任务不提供 Word 或交付包，请从已保存大纲生成正文。", 409)
        execution = await self.authoring.get_execution(job["execution_id"], missing_ok=True)
        if not execution or execution["status"] != AuthoringExecutionStatus.SUCCEEDED or not execution["result_ref"]:
            raise WebError("ARTIFACT_NOT_READY", "此任务尚无已保存的完整编制产物。", 409)
        return await asyncio.to_thread(self.artifacts.read_json, execution["result_ref"])

    async def workbench(self, project_id, execution_id=None):
        run = await self.latest_run(project_id)
        async with self.store.transaction() as conn:
            jobs = [status_row(entry, AuthoringJobStatus) for entry in await (await conn.execute("SELECT * FROM tender_authoring_jobs WHERE project_id=%s "
                                            "ORDER BY created_at DESC,execution_id DESC", (project_id,))).fetchall()]
        views = [await self.authoring_view(job) for job in jobs if _operation(job) == "document"]
        completed = next((view for view in views if view["status"] == AuthoringExecutionStatus.SUCCEEDED), None)
        context_id = execution_id or (completed["execution_id"] if completed else None)
        value = {"analysis": await self.analysis_view(run, project_id) if run else None,
                 "authoring": views[0] if views else None, "executions": views,
                 "context_execution_id": context_id,
                 "documents": [{"id": str(row["document_id"]), "document_id": str(row["document_id"]),
                                "kind": row["kind"], "role": row["role"], "status": "uploaded"}
                               for row in await self.documents(project_id, kind=None)],
                 "outline": None, "requirements": None, "units": [], "gaps": [], "materials": [],
                 "artifacts": {"docx_available": False, "package_available": False}}
        if completed:
            value["artifacts"] = {"execution_id": completed["execution_id"], "docx_available": True, "package_available": True}
        if context_id:
            document = await self.result(project_id, context_id)
            basis = self.artifacts.read_json(document["basis_ref"]) if document.get("basis_ref") else {}
            materials = [{**material, "text": self.artifacts.read_bytes(material["content_ref"]).decode("utf-8")
                          if material.get("content_ref") else material["text"]}
                         for material in basis.get("materials", [])]
            value.update(outline=self.artifacts.read_json(document["outline_ref"]),
                         requirements=self.artifacts.read_json(document["requirements_ref"]) if document.get("requirements_ref") else None,
                         units=[self.artifacts.read_json(ref) for ref in document["unit_refs"]], gaps=document["gaps"],
                         materials=materials)
        else:
            active = next((view for view in views if view["status"] in {AuthoringJobStatus.PENDING, AuthoringJobStatus.PUBLISHED, AuthoringExecutionStatus.RUNNING}), None)
            execution = await self.authoring.get_execution(active["execution_id"], missing_ok=True) if active else None
            outlines = [receipt for receipt in execution["receipts"] if receipt["stage"] == "document-outline"
                        and not receipt["scope"]] if execution else []
            if len(outlines) == 1:
                value["outline"] = await asyncio.to_thread(self.artifacts.read_json, outlines[0]["artifact_ref"])
        return value


def _usage(requests):
    # The existing accounting keeps absent supplier usage distinct from zero.
    from docreader.tender.document_execution import _usage as summarize

    return summarize([{**row, "metadata": {**row["metadata"], "logical_request_digest":
        row["metadata"].get("logical_request_digest", row["metadata"].get("request_digest", row["request_id"]))}}
        for row in requests])


async def handle_command(payload, blob=b""):
    if not isinstance(payload, dict):
        raise WebError("INVALID_COMMAND", "请求必须为对象。")
    bridge = WebBridge(Settings.from_env())
    await bridge.setup()
    action = payload.get("action")
    try:
        project_id = await bridge.authorize(payload, write=action in {
            "document.put", "analysis.start", "analysis.confirm", "analysis.correct", "analysis.retry", "authoring.start", "authoring.resume", "outline.start", "outline.save", "docx.review.start"})
    except WebError as error:
        if error.code != "PROJECT_ENDED":
            raise
        from docreader.integrations.web_replay import replay_ended
        payload = {**payload, "project_id": _uuid(payload["project_id"]),
                   "owner_user_id": _uuid(payload["owner_user_id"])}
        if action == "document.put":
            return await bridge.put_document(payload, blob), b""
        return await replay_ended(bridge, payload), b""
    payload = {**payload, "project_id": project_id, "owner_user_id": _uuid(payload["owner_user_id"])}
    if action == "document.put":
        return await bridge.put_document(payload, blob), b""
    if action == "workbench":
        return await bridge.workbench(project_id, payload.get("execution_id")), b""
    if action in {"analysis.start", "analysis.status"}:
        if action == "analysis.start":
            return await bridge.start_analysis(payload), b""
        run = await bridge.latest_run(project_id)
        return {"analysis": await bridge.analysis_view(run, project_id) if run else None}, b""
    if action in {"analysis.review", "analysis.confirm", "analysis.correct"}:
        from docreader.integrations.web_analysis import analysis_review, confirm_analysis, correct_analysis
        operation = {"analysis.review": analysis_review, "analysis.confirm": confirm_analysis,
                     "analysis.correct": correct_analysis}[action]
        return await operation(bridge, payload), b""
    if action in {"analysis.recovery", "analysis.retry"}:
        from docreader.integrations.web_recovery import analysis_recovery, retry_analysis
        operation = analysis_recovery if action == "analysis.recovery" else retry_analysis
        return await operation(bridge, payload), b""
    if action == "authoring.start":
        return await bridge.start_authoring(payload), b""
    if action in {"docx.review.start", "docx.review.status"}:
        from docreader.integrations.web_saved_review import start_review, review_status
        if action == "docx.review.start":
            return await start_review(bridge, payload, blob), b""
        return await review_status(bridge, payload), b""
    if action == "outline.start":
        return await bridge.start_outline(payload), b""
    if action == "outline.save":
        return await bridge.save_outline(payload), b""
    if action == "outline.status":
        return await bridge.outline_view(project_id), b""
    if action == "outline.history":
        from docreader.integrations.web_outline import outline_history
        return await outline_history(bridge, project_id), b""
    if action == "outline.detail":
        from docreader.integrations.web_outline import outline_detail
        return await outline_detail(bridge, project_id, payload.get("revision_id")), b""
    if action in {"authoring.resume", "authoring.cancel"}:
        return await bridge.update_job(payload, cancel=action.endswith("cancel")), b""
    if action == "authoring.status":
        if payload.get("execution_id"):
            job = await bridge.job(project_id, payload["execution_id"])
        else:
            async with bridge.store.transaction() as conn:
                job = status_row(await (await conn.execute("SELECT * FROM tender_authoring_jobs WHERE project_id=%s "
                                               "AND COALESCE(payload->>'operation','document')='document' "
                                               "ORDER BY created_at DESC,execution_id DESC LIMIT 1", (project_id,))).fetchone(), AuthoringJobStatus)
        return {"authoring": await bridge.authoring_view(job) if job else None}, b""
    if action == "artifact.get":
        kind = payload.get("kind")
        if kind not in {"docx", "package"}:
            raise WebError("INVALID_ARTIFACT_KIND", "只能读取此任务的 Word 或交付包。")
        document = await bridge.result(project_id, payload.get("execution_id"))
        reference = document[f"{kind}_ref"]
        data = await asyncio.to_thread(bridge.artifacts.read_bytes, reference)
        return {"media_type": reference["media_type"], "sha256": reference["sha256"], "size": len(data)}, data
    if action == "material.original":
        document = await bridge.result(project_id, payload.get("execution_id"))
        basis = await asyncio.to_thread(bridge.artifacts.read_json, document["basis_ref"]) if document.get("basis_ref") else {}
        material = next((value for value in basis.get("materials", [])
                         if value["material_id"] == payload.get("material_id")), None)
        if material is None:
            raise WebError("MATERIAL_NOT_FOUND", "此编制成果中的资料不存在。", 404)
        reference = material.get("original_ref")
        if reference is None:
            raise WebError("MATERIAL_ORIGINAL_UNAVAILABLE", "此资料没有可读取的原件。", 409)
        data = await asyncio.to_thread(bridge.artifacts.read_bytes, reference)
        name = material.get("locator", {}).get("file_name") or material.get("title") or "资料原件"
        return {"media_type": reference["media_type"], "sha256": reference["sha256"], "size": len(data), "file_name": name}, data
    if action in {"docx.outline", "docx_outline"}:
        from docreader.tender.word_navigation import read_docx_outline

        known = []
        if payload.get("execution_id"):
            document = await bridge.result(project_id, payload["execution_id"])
            known = (await asyncio.to_thread(bridge.artifacts.read_json, document["outline_ref"])).get("chapters", [])
        return await asyncio.to_thread(read_docx_outline, blob, known_chapters=known), b""
    raise WebError("UNKNOWN_COMMAND", "不支持的项目操作。")


async def dispatch_authoring(store, send, *, redeliver_seconds=60):
    """Publish durable jobs; an active execution lease prevents duplicate work."""
    async with store.transaction() as conn:
        if (await (await conn.execute("SELECT to_regclass('tender_authoring_jobs') AS name")).fetchone())["name"] is None:
            return 0
        jobs = await (await conn.execute(
            "SELECT j.execution_id FROM tender_authoring_jobs j LEFT JOIN tender_authoring_executions e "
            "ON e.execution_id=j.execution_id WHERE (j.status='pending' OR (j.status='published' "
            "AND j.claim_until < clock_timestamp())) AND (e.execution_id IS NULL OR "
            "(e.status NOT IN ('succeeded','cancelled') AND (e.lease_until IS NULL OR e.lease_until < clock_timestamp()))) "
            "ORDER BY j.created_at FOR UPDATE OF j SKIP LOCKED LIMIT 20",
        )).fetchall()
        for job in jobs:
            await conn.execute("UPDATE tender_authoring_jobs SET status='published', "
                               "claim_until=clock_timestamp()+%s*interval '1 second' WHERE execution_id=%s",
                               (redeliver_seconds, job["execution_id"]))
    published = 0
    for job in jobs:
        try:
            await asyncio.to_thread(send, job["execution_id"])
            published += 1
        except Exception as error:
            async with store.transaction() as conn:
                await conn.execute("UPDATE tender_authoring_jobs SET status='pending',claim_until=NULL "
                                   "WHERE execution_id=%s AND status='published'", (job["execution_id"],))
            logger.warning("Authoring queue unavailable (%s)", type(error).__name__)
    return published


async def execute_authoring_job(execution_id, settings):
    from docreader.llm.budget import RequestBudget
    from docreader.llm.config import ModelConfig
    from docreader.tender.document_execution import execute_document

    bridge = WebBridge(settings)
    await bridge.setup()
    async with bridge.store.transaction() as conn:
        job = status_row(await (await conn.execute("SELECT * FROM tender_authoring_jobs WHERE execution_id=%s",
                                       (execution_id,))).fetchone(), AuthoringJobStatus)
    if job is None or job["status"] in {AuthoringJobStatus.SUCCEEDED, AuthoringJobStatus.CANCELLED}:
        return
    try:
        await bridge.authorize({"project_id": str(job["project_id"]), "owner_user_id": job["payload"]["owner_user_id"]}, write=True)
        config = ModelConfig.from_env()
        budget = RequestBudget(context_tokens=config.chat.context_tokens, model=config.chat.model,
                               tokenizer="o200k_base", tokenized_outline_assembly=True, tokenized_authoring=True)
        async def wait_for_cancellation():
            # Cancellation can arrive before execute_document creates its row.
            while True:
                current = await bridge.job(str(job["project_id"]), execution_id)
                if current["status"] == AuthoringJobStatus.CANCELLED:
                    execution = await bridge.authoring.get_execution(execution_id, missing_ok=True)
                    if execution is not None:
                        await bridge.authoring.cancel(execution_id)
                    return
                await asyncio.sleep(0.5)

        request = {key: job["payload"]["request"].get(key) for key in ("previous_execution_id", "review_request")}
        if _operation(job) == "outline":
            request["outline_only"] = True
        if job["payload"].get("outline_ref") is not None:
            request["outline_ref"] = job["payload"]["outline_ref"]
        if job["payload"].get("authoring_input_ref") is not None:
            request["authoring_input_ref"] = job["payload"]["authoring_input_ref"]
        if job["payload"].get("knowledge_selections"):
            from docreader.integrations.web_knowledge import build_web_knowledge
            request["knowledge"] = build_web_knowledge(execution_id=execution_id,
                                                       selections=job["payload"]["knowledge_selections"])
        if _operation(job) == "saved_review":
            from docreader.tender.saved_word_review import execute_saved_review
            options = job["payload"]
            work = asyncio.create_task(execute_saved_review(bridge.store, bridge.artifacts, config, budget,
                execution_id=execution_id, snapshot=options["snapshot"], document_ref=options["document_ref"],
                saved_ref=options["saved_ref"], **options["request"]))
        else:
            work = asyncio.create_task(execute_document(bridge.store, bridge.artifacts, config, budget, execution_id=execution_id,
                **job["payload"]["identity"], **request, materials=job["payload"]["materials"]))
        cancellation = asyncio.create_task(wait_for_cancellation())
        try:
            done, _ = await asyncio.wait({work, cancellation}, return_when=asyncio.FIRST_COMPLETED)
            if cancellation in done:
                await cancellation
                return
            await work
        finally:
            for task in (work, cancellation):
                task.cancel()
            await asyncio.gather(work, cancellation, return_exceptions=True)
    except Exception as error:
        # A concurrent duplicate owns no lease and must not mark its sibling failed.
        execution = await bridge.authoring.get_execution(execution_id, missing_ok=True)
        if execution and execution["status"] == AuthoringExecutionStatus.RUNNING and execution["lease_live"]:
            return
        async with bridge.store.transaction() as conn:
            await conn.execute("UPDATE tender_authoring_jobs SET status='failed',error=%s,claim_until=NULL,"
                               "updated_at=clock_timestamp() WHERE execution_id=%s AND status<>'cancelled'",
                               (Jsonb({"type": type(error).__name__}), execution_id))
        logger.warning("Authoring execution failed (%s)", type(error).__name__)
        return
    async with bridge.store.transaction() as conn:
        await conn.execute("UPDATE tender_authoring_jobs SET status='succeeded',error=NULL,claim_until=NULL,"
                           "updated_at=clock_timestamp() WHERE execution_id=%s AND status<>'cancelled'", (execution_id,))
