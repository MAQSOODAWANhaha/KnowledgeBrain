#!/usr/bin/env python3
"""Live Phase 2 manual-authoring API acceptance.

Required environment: API_URL, BID_V2_JWT. Optional: WORKSPACE_ID.
Runs only against an already bootstrapped fresh V2 database.
"""
import binascii
import hashlib
import json
import os
import struct
from typing import Any
import urllib.error
import urllib.request
import uuid
import zlib

API_URL = os.environ.get("API_URL", "http://127.0.0.1:58080").rstrip("/")
TOKEN = os.environ["BID_V2_JWT"]
WORKSPACE_ID = os.environ.get("WORKSPACE_ID", "00000000-0000-4000-8000-0000000000a0")
KEY_PREFIX = os.environ.get("BID_V2_E2E_KEY_PREFIX", "phase2-api")


def call(method: str, path: str, body: Any = None, headers: dict[str, str] | None = None) -> tuple[int, dict[str, str], Any]:
    values = {"authorization": f"Bearer {TOKEN}", **(headers or {})}
    data = None
    if body is not None:
        if isinstance(body, bytes):
            data = body
        else:
            data = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode()
            values["content-type"] = "application/json"
    request = urllib.request.Request(API_URL + path, data=data, headers=values, method=method)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
            if not raw:
                raise RuntimeError(f"{method} {path}: empty response body")
            return response.status, dict(response.headers), json.loads(raw)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RuntimeError(f"{method} {path}: HTTP {error.code}: {detail}") from error


def expect_http_error(method: str, path: str, body: Any,
                      headers: dict[str, str], expected_status: int) -> str:
    values = {"authorization": f"Bearer {TOKEN}", "content-type": "application/json", **headers}
    data = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode()
    request = urllib.request.Request(API_URL + path, data=data, headers=values, method=method)
    try:
        urllib.request.urlopen(request, timeout=30)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        if error.code != expected_status:
            raise RuntimeError(f"{method} {path}: expected HTTP {expected_status}, got {error.code}: {detail}")
        return detail
    raise RuntimeError(f"{method} {path}: expected HTTP {expected_status}, request succeeded")


def content_sha(content: Any) -> str:
    canonical = json.dumps(content, ensure_ascii=False, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def block(lineage_id: uuid.UUID, content: dict[str, Any], revision: int = 1) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "block_revision_id": str(uuid.uuid4()),
        "lineage_id": str(lineage_id),
        "revision": revision,
        "kind": content["type"],
        "content": content,
        "origin": "human",
        "dependency_sha256": None,
        "stale": False,
        "content_sha256": content_sha(content),
    }


# A deterministic, structurally valid RGBA 1x1 PNG.
def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(
        ">I", binascii.crc32(kind + payload) & 0xFFFFFFFF
    )


png = b"\x89PNG\r\n\x1a\n" + png_chunk(
    b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)
) + png_chunk(b"IDAT", zlib.compress(b"\x00\x00\x80\xff\xff")) + png_chunk(b"IEND", b"")
boundary = "----knowledgebrain-phase2-" + uuid.uuid4().hex
multipart = (
    f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"phase2.png\"\r\n"
    "Content-Type: image/png\r\n\r\n"
).encode() + png + f"\r\n--{boundary}--\r\n".encode()
_, _, asset = call(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/assets",
    multipart,
    {"content-type": f"multipart/form-data; boundary={boundary}", "idempotency-key": f"{KEY_PREFIX}-asset-v1"},
)
asset_id = asset["asset_revision_id"]
_, _, preparation = call(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/assets/{asset_id}/attachment-preparations",
    {"page_asset_revision_ids": []},
    {"idempotency-key": f"{KEY_PREFIX}-attachment-preparation-v1"},
)
preparation_id = preparation["attachment_preparation_revision_id"]

_, _, current = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
node_lineage = uuid.uuid4()
secondary_lineage = uuid.uuid4()
rich_lineage = uuid.uuid4()
table_lineage = uuid.uuid4()
attachment_lineage = uuid.uuid4()
root_ordinal = sum(1 for node in current["nodes"] if node.get("parent_lineage_id") is None)
rich = block(
    rich_lineage,
    {"type": "rich_text", "nodes": [{"kind": "paragraph", "content": [{"kind": "text", "text": "人工编制初稿", "marks": []}]}]},
)
table = block(
    table_lineage,
    {"type": "table", "row_count": 1, "column_count": 1,
     "cells": [{"row": 0, "column": 0, "rowspan": 1, "colspan": 1, "content": []}],
     "widths_mm": [100.0], "repeat_header_rows": 0},
)
attachment = block(
    attachment_lineage,
    {"type": "attachment_ref", "asset_revision_id": asset_id,
     "preparation_revision_id": preparation_id, "render_mode": "embedded_pages",
     "start_new_page": True},
)
first_request = {
    "schema_version": 1,
    "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": current["revision_id"],
    "expected_workspace_sha256": current["sha256"],
    "operations": [
        {"kind": "insert_node", "client_node_ref": "phase2-manual", "lineage_id": str(node_lineage),
         "revision_id": str(uuid.uuid4()), "parent_lineage_id": None, "ordinal": root_ordinal,
         "title": "人工编制验收", "semantic_role": "technical", "render_role": "section"},
        {"kind": "insert_node", "client_node_ref": "phase2-secondary", "lineage_id": str(secondary_lineage),
         "revision_id": str(uuid.uuid4()), "parent_lineage_id": None, "ordinal": root_ordinal + 1,
         "title": "待拆分章节", "semantic_role": "technical", "render_role": "section"},
        {"kind": "insert_block", "node_lineage_id": str(node_lineage), "ordinal": 0,
         "insertion_anchor": None, "block": rich},
        {"kind": "insert_block", "node_lineage_id": str(node_lineage), "ordinal": 1,
         "insertion_anchor": None, "block": table},
        {"kind": "insert_asset_block", "node_lineage_id": str(node_lineage),
         "asset_revision_id": asset_id, "ordinal": 2},
        {"kind": "insert_block", "node_lineage_id": str(node_lineage), "ordinal": 3,
         "insertion_anchor": None, "block": attachment},
    ],
}
_, _, first = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", first_request,
    {"if-match": f'"{current["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-first-v1"},
)
oversized_table = block(
    table_lineage,
    {"type": "table", "row_count": 1, "column_count": 1,
     "cells": [{"row": 0, "column": 0, "rowspan": 1, "colspan": 1, "content": []}],
     "widths_mm": [190.0], "repeat_header_rows": 0},
    revision=2,
)
oversized_request = {
    "schema_version": 1,
    "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": first["revision_id"],
    "expected_workspace_sha256": first["sha256"],
    "operations": [{"kind": "update_block", "block_lineage_id": str(table_lineage),
                    "block": oversized_table}],
}
oversized_error = expect_http_error(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", oversized_request,
    {"if-match": f'"{first["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-oversized-table-v1"}, 400,
)
if "TABLE_EXCEEDS_PRINTABLE_WIDTH" not in oversized_error:
    raise RuntimeError("oversized table did not fail with the printable-width contract")
_, _, after_first = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
image_current = next(
    value for value in after_first["blocks"]
    if value["kind"] == "image" and value["content"]["asset_revision_id"] == asset_id
)
image_lineage = uuid.UUID(image_current["lineage_id"])
oversized_image = block(
    image_lineage,
    {"type": "image", "asset_revision_id": asset_id, "width_mm": 190.0,
     "alignment": "center", "crop": {"left": 0.0, "top": 0.0, "right": 0.0, "bottom": 0.0},
     "caption": None, "alt": "oversized image"},
    revision=2,
)
oversized_image_request = {
    "schema_version": 1,
    "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": first["revision_id"],
    "expected_workspace_sha256": first["sha256"],
    "operations": [{"kind": "update_block", "block_lineage_id": str(image_lineage),
                    "block": oversized_image}],
}
oversized_image_error = expect_http_error(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", oversized_image_request,
    {"if-match": f'"{first["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-oversized-image-v1"}, 400,
)
if "IMAGE_EXCEEDS_PRINTABLE_WIDTH" not in oversized_image_error:
    raise RuntimeError("oversized image did not fail with the printable-width contract")
updated = block(
    rich_lineage,
    {"type": "rich_text", "nodes": [{"kind": "paragraph", "content": [{"kind": "text", "text": "保存恢复后的人工响应", "marks": []}]}]},
    revision=2,
)
second_request = {
    "schema_version": 1,
    "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": first["revision_id"],
    "expected_workspace_sha256": first["sha256"],
    "operations": [
        {"kind": "update_block", "block_lineage_id": str(rich_lineage), "block": updated},
        {"kind": "delete_block", "block_lineage_id": str(table_lineage)},
        {"kind": "move_node", "node_lineage_id": str(secondary_lineage),
         "parent_lineage_id": str(node_lineage), "ordinal": 0},
        {"kind": "rename_node", "node_lineage_id": str(secondary_lineage), "title": "待拆分并合并章节"},
    ],
}
headers = {"if-match": f'"{first["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-second-v1"}
_, _, second = call("POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", second_request, headers)
_, _, replay = call("POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", second_request, headers)
if replay != second:
    raise RuntimeError("manual mutation idempotency replay changed the first receipt")
split_request = {
    "schema_version": 1, "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": second["revision_id"],
    "expected_workspace_sha256": second["sha256"],
    "operations": [{"kind": "split_node", "node_lineage_id": str(secondary_lineage),
                    "titles": ["拆分章节一", "拆分章节二"]}],
}
_, _, split = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", split_request,
    {"if-match": f'"{second["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-split-v1"},
)
_, _, split_workspace = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
split_ids = [node["lineage_id"] for node in split_workspace["nodes"]
             if node["title"] in ("拆分章节一", "拆分章节二")]
if len(split_ids) != 2:
    raise RuntimeError("manual split did not publish two lineage replacements")
merge_request = {
    "schema_version": 1, "workspace_id": WORKSPACE_ID,
    "expected_workspace_revision_id": split["revision_id"],
    "expected_workspace_sha256": split["sha256"],
    "operations": [{"kind": "merge_nodes", "node_lineage_ids": split_ids, "title": "合并章节"}],
}
_, _, merged = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/mutations", merge_request,
    {"if-match": f'"{split["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-merge-v1"},
)
_, _, restored = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
blocks = {value["lineage_id"]: value for value in restored["blocks"]}
if restored["revision_id"] != merged["revision_id"] or str(rich_lineage) not in blocks:
    raise RuntimeError("manual workspace did not persist across reload")
if blocks[str(rich_lineage)]["content"]["nodes"][0]["content"][0]["text"] != "保存恢复后的人工响应":
    raise RuntimeError("manual rich text update was not restored")
if str(table_lineage) in blocks:
    raise RuntimeError("deleted manual table was restored")
if not any(value.get("content", {}).get("asset_revision_id") == asset_id for value in blocks.values()):
    raise RuntimeError("manual image block was not restored")
if blocks.get(str(attachment_lineage), {}).get("content", {}).get("preparation_revision_id") != preparation_id:
    raise RuntimeError("manual attachment preparation was not restored")
merged_nodes = [node for node in restored["nodes"] if node["title"] == "合并章节"]
if len(merged_nodes) != 1 or merged_nodes[0].get("parent_lineage_id") != str(node_lineage):
    raise RuntimeError("manual move/split/merge lineage was not restored")
print(json.dumps({"status": "PASS", "workspace_revision_id": merged["revision_id"],
                  "rich_block_lineage_id": str(rich_lineage), "asset_revision_id": asset_id}, ensure_ascii=False))
