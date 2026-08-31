#!/usr/bin/env python3
"""Real API -> Redis -> Worker -> DOCX/PDF two-phase export acceptance."""
import hashlib
import json
import os
import time
from typing import Any
import uuid
import urllib.error
import urllib.request

API_URL = os.environ.get("API_URL", "http://127.0.0.1:58080").rstrip("/")
TOKEN = os.environ["BID_V2_JWT"]
WORKSPACE_ID = os.environ.get("WORKSPACE_ID", "00000000-0000-4000-8000-0000000000a0")
KEY_PREFIX = os.environ.get("BID_V2_E2E_KEY_PREFIX", "phase7")


def request(method: str, path: str, body: Any = None, headers: dict[str, str] | None = None,
            expect_json: bool = True) -> tuple[int, dict[str, str], Any]:
    values = {"authorization": f"Bearer {TOKEN}", **(headers or {})}
    data = None
    if body is not None:
        data = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode()
        values["content-type"] = "application/json"
    operation = urllib.request.Request(API_URL + path, data=data, headers=values, method=method)
    try:
        with urllib.request.urlopen(operation, timeout=60) as response:
            raw = response.read()
            value = json.loads(raw) if expect_json else raw
            normalized_headers = {key.lower(): value for key, value in response.headers.items()}
            return response.status, normalized_headers, value
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RuntimeError(f"{method} {path}: HTTP {error.code}: {detail}") from error


def upload_workspace_asset(file_name: str, media_type: str, payload: bytes,
                           idempotency_key: str) -> dict[str, Any]:
    boundary = "----knowledgebrain-export-" + uuid.uuid4().hex
    multipart = (
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n"
        f"Content-Type: {media_type}\r\n\r\n"
    ).encode() + payload + f"\r\n--{boundary}--\r\n".encode()
    operation = urllib.request.Request(
        API_URL + f"/api/v2/submission-workspaces/{WORKSPACE_ID}/assets",
        data=multipart,
        headers={"authorization": f"Bearer {TOKEN}",
                 "content-type": f"multipart/form-data; boundary={boundary}",
                 "idempotency-key": idempotency_key},
        method="POST",
    )
    try:
        with urllib.request.urlopen(operation, timeout=60) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RuntimeError(f"asset upload: HTTP {error.code}: {detail}") from error


def expect_http_error(method: str, path: str, body: Any,
                      headers: dict[str, str], expected_status: int) -> str:
    values = {"authorization": f"Bearer {TOKEN}", "content-type": "application/json", **headers}
    data = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode()
    operation = urllib.request.Request(API_URL + path, data=data, headers=values, method=method)
    try:
        urllib.request.urlopen(operation, timeout=60)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        if error.code != expected_status:
            raise RuntimeError(f"{method} {path}: expected HTTP {expected_status}, got {error.code}: {detail}")
        return detail
    raise RuntimeError(f"{method} {path}: expected HTTP {expected_status}, request succeeded")


def content_sha(content: Any) -> str:
    canonical = json.dumps(content, ensure_ascii=False, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def export_ids() -> set[str]:
    _, _, values = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports")
    return {value["export_id"] for value in values}


def wait_for_new_export(before: set[str], format_name: str) -> dict[str, Any]:
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        _, _, values = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports")
        for value in values:
            if value["export_id"] not in before and value["format"] == format_name and value["status"] == "ready":
                return value
        time.sleep(0.25)
    raise RuntimeError(f"timed out waiting for {format_name} export")


_, _, workspace = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
old_workspace_revision_id = workspace["revision_id"]
quote_body = {
    "title": "端到端冻结报价",
    "notes": "WorkspaceRevision quote identity acceptance",
    "tax_mode": "tax_exclusive",
    "lines": [{
        "description": "实施服务",
        "pricing_mode": "unit_price",
        "quantity": "2.000000",
        "unit": "项",
        "unit_price": "100.000000",
        "entered_amount": None,
        "tax_rate": "0.060000",
        "user_confirmed": True,
    }],
    "no_ceiling_review_reason": "测试项目未设置最高限价，已人工复核",
}
_, _, quote = request(
    "POST", f"/api/v2/bid-projects/{workspace['project_id']}/quote-snapshots", quote_body,
    {"idempotency-key": f"{KEY_PREFIX}-quote-v1"},
)
_, _, workspace_after_publish = request(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}"
)
if workspace_after_publish["revision_id"] != old_workspace_revision_id:
    raise RuntimeError("quote publication moved WorkspaceHead without explicit apply")
_, _, workspace = request(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/quote-snapshots/{quote['quote_snapshot_id']}/apply",
    {
        "quote_snapshot_sha256": quote["sha256"],
        "expected_workspace_revision_id": workspace_after_publish["revision_id"],
        "expected_workspace_sha256": workspace_after_publish["sha256"],
    },
    {
        "if-match": f'"{workspace_after_publish["sha256"]}"',
        "idempotency-key": f"{KEY_PREFIX}-quote-apply-v1",
    },
)
if workspace["revision_id"] == old_workspace_revision_id:
    raise RuntimeError("explicit quote apply did not advance WorkspaceRevision")
if workspace["quote_snapshot"]["artifact_id"] != quote["quote_snapshot_id"]:
    raise RuntimeError("explicitly applied WorkspaceRevision did not freeze the quote")

quote_v2_body = {**quote_body, "notes": "new current quote must fence old apply"}
_, _, quote_v2 = request(
    "POST", f"/api/v2/bid-projects/{workspace['project_id']}/quote-snapshots", quote_v2_body,
    {"idempotency-key": f"{KEY_PREFIX}-quote-v2"},
)
expect_http_error(
    "POST",
    "/api/v2/bid-projects/00000000-0000-4000-8000-000000000019/quote-snapshots",
    quote_v2_body,
    {"idempotency-key": f"{KEY_PREFIX}-quote-v2"},
    409,
)
head_before_stale = workspace
expect_http_error(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/quote-snapshots/{quote['quote_snapshot_id']}/apply",
    {
        "quote_snapshot_sha256": quote["sha256"],
        "expected_workspace_revision_id": head_before_stale["revision_id"],
        "expected_workspace_sha256": head_before_stale["sha256"],
    },
    {
        "if-match": f'"{head_before_stale["sha256"]}"',
        "idempotency-key": f"{KEY_PREFIX}-stale-quote-apply",
    },
    409,
)
_, _, unchanged = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
if unchanged["revision_id"] != head_before_stale["revision_id"] or unchanged["sha256"] != head_before_stale["sha256"]:
    raise RuntimeError("stale Q1 apply changed WorkspaceHead")
apply_v2_body = {
    "quote_snapshot_sha256": quote_v2["sha256"],
    "expected_workspace_revision_id": unchanged["revision_id"],
    "expected_workspace_sha256": unchanged["sha256"],
}
apply_v2_headers = {
    "if-match": f'"{unchanged["sha256"]}"',
    "idempotency-key": f"{KEY_PREFIX}-quote-apply-v2",
}
_, _, workspace = request(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/quote-snapshots/{quote_v2['quote_snapshot_id']}/apply",
    apply_v2_body,
    apply_v2_headers,
)
_, _, apply_v2_replay = request(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/quote-snapshots/{quote_v2['quote_snapshot_id']}/apply",
    apply_v2_body,
    apply_v2_headers,
)
if apply_v2_replay != workspace:
    raise RuntimeError("quote apply replay changed the committed response")
_, _, initial_preview = request(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/preview", expect_json=False,
)
if b"<html" not in initial_preview:
    raise RuntimeError("manual image attachment preview was not renderable")
results: list[dict[str, Any]] = []
pdf_payload: bytes | None = None
preparation_count = 0
for format_name in ("docx", "pdf"):
    before = export_ids()
    body = {
        "mode": "review_draft",
        "format": format_name,
        "expected_workspace_revision_id": workspace["revision_id"],
        "watermark": {"text": "两阶段真实导出验收"},
    }
    headers = {"if-match": f'"{workspace["sha256"]}"',
               "idempotency-key": f"{KEY_PREFIX}-real-export-{format_name}-v1"}
    status, _, first = request("POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports", body, headers)
    if status != 202:
        raise RuntimeError(f"{format_name} export did not return 202")
    exported = wait_for_new_export(before, format_name)
    _, _, replay = request("POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports", body, headers)
    if replay != first:
        raise RuntimeError(f"{format_name} terminal replay changed first request receipt")
    after_replay = export_ids()
    time.sleep(1.0)
    if export_ids() != after_replay:
        raise RuntimeError(f"{format_name} terminal replay enqueued another output")
    export_id = exported["export_id"]
    _, _, detail = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports/{export_id}")
    _, _, report = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports/{export_id}/assessment-report")
    _, response_headers, payload = request(
        "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports/{export_id}/download",
        expect_json=False,
    )
    expected_media = "application/pdf" if format_name == "pdf" else "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    content_type = response_headers.get("content-type", "").split(";", 1)[0]
    if detail["media_type"] != expected_media or content_type != expected_media:
        raise RuntimeError(f"{format_name} download media identity mismatch")
    if (format_name == "pdf" and not payload.startswith(b"%PDF")) or (format_name == "docx" and not payload.startswith(b"PK")):
        raise RuntimeError(f"{format_name} download signature invalid")
    if report["manifest_id"] != detail["manifest_id"] or report["submission_output_id"] != export_id:
        raise RuntimeError(f"{format_name} assessment report identity mismatch")
    preparation_count = max(preparation_count, len(detail["attachment_preparations"]))
    if format_name == "pdf":
        pdf_payload = payload
    results.append({"format": format_name, "request_artifact_id": first["request_artifact_id"],
                    "export_id": export_id, "manifest_id": detail["manifest_id"],
                    "sha256": detail["sha256"], "byte_length": detail["byte_length"]})
if pdf_payload is None:
    raise RuntimeError("PDF fixture output missing")
pdf_asset = upload_workspace_asset(
    "worker-preparation-source.pdf", "application/pdf", pdf_payload,
    f"{KEY_PREFIX}-worker-pdf-source-v1",
)
pdf_asset_id = pdf_asset["asset_revision_id"]
manual_error = expect_http_error(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/assets/{pdf_asset_id}/attachment-preparations",
    {"page_asset_revision_ids": []},
    {"idempotency-key": f"{KEY_PREFIX}-client-pdf-preparation-must-fail-v1"}, 400,
)
if "SubmissionExport worker" not in manual_error:
    raise RuntimeError("PDF manual preparation was not rejected by the trusted-preparer contract")

_, _, workspace = request("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
node = workspace["nodes"][0]
attachment_lineage = uuid.uuid4()
content = {
    "type": "attachment_ref",
    "asset_revision_id": pdf_asset_id,
    "preparation_revision_id": None,
    "render_mode": "embedded_pages",
    "start_new_page": True,
}
attachment = {
    "schema_version": 1,
    "block_revision_id": str(uuid.uuid4()),
    "lineage_id": str(attachment_lineage),
    "revision": 1,
    "kind": "attachment_ref",
    "content": content,
    "origin": "human",
    "content_sha256": content_sha(content),
}
mutation = {
    "schema_version": 1,
    "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": workspace["revision_id"],
    "expected_workspace_sha256": workspace["sha256"],
    "operations": [{
        "kind": "insert_block",
        "node_lineage_id": node["lineage_id"],
        "ordinal": len(node["block_lineage_ids"]),
        "block": attachment,
    }],
}
_, _, mutation_receipt = request(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", mutation,
    {"if-match": f'"{workspace["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-worker-pdf-block-v1"},
)
_, _, unprepared_preview = request(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/preview", expect_json=False,
)
if "导出时嵌入页面".encode() not in unprepared_preview:
    raise RuntimeError("unprepared PDF attachment preview did not render the safe placeholder")
before = export_ids()
worker_prepared_body = {
    "mode": "review_draft",
    "format": "docx",
    "expected_workspace_revision_id": mutation_receipt["revision_id"],
    "watermark": {"text": "可信PDF附件准备验收"},
}
_, _, worker_request = request(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports", worker_prepared_body,
    {"if-match": f'"{mutation_receipt["sha256"]}"',
     "idempotency-key": f"{KEY_PREFIX}-worker-prepared-pdf-export-v1"},
)
worker_export = wait_for_new_export(before, "docx")
worker_export_id = worker_export["export_id"]
_, _, worker_detail = request(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports/{worker_export_id}"
)
_, _, worker_docx = request(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/exports/{worker_export_id}/download",
    expect_json=False,
)
if not worker_docx.startswith(b"PK"):
    raise RuntimeError("worker-prepared PDF attachment DOCX signature invalid")
if len(worker_detail["attachment_preparations"]) <= preparation_count:
    raise RuntimeError("worker-prepared PDF attachment was not frozen into manifest dependencies")
_, _, prepared_preview = request(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/preview", expect_json=False,
)
if "导出时嵌入页面".encode() in prepared_preview or b"data:image/png;base64," not in prepared_preview:
    raise RuntimeError("trusted prepared PDF pages did not replace the preview placeholder")
results.append({
    "format": "docx_with_worker_prepared_pdf",
    "request_artifact_id": worker_request["request_artifact_id"],
    "export_id": worker_export_id,
    "manifest_id": worker_detail["manifest_id"],
    "sha256": worker_detail["sha256"],
    "byte_length": worker_detail["byte_length"],
    "attachment_preparations": worker_detail["attachment_preparations"],
})
print(json.dumps({"status": "PASS", "exports": results}, ensure_ascii=False))
