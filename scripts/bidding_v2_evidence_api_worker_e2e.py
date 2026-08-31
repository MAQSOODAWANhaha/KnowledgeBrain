#!/usr/bin/env python3
"""Real API -> Redis -> Worker EvidenceBundle/PickSet/Candidate/acceptance acceptance."""
import json
import os
import time
from typing import Any
import urllib.error
import urllib.request

API_URL = os.environ.get("API_URL", "http://127.0.0.1:58080").rstrip("/")
TOKEN = os.environ["BID_V2_JWT"]
WORKSPACE_ID = os.environ.get("WORKSPACE_ID", "00000000-0000-4000-8000-0000000000a0")
KEY_PREFIX = os.environ.get("BID_V2_E2E_KEY_PREFIX", "phase7-evidence")


def call(method: str, path: str, body: Any = None,
         headers: dict[str, str] | None = None) -> tuple[int, dict[str, str], Any]:
    values = {"authorization": f"Bearer {TOKEN}", **(headers or {})}
    data = None
    if body is not None:
        data = json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode()
        values["content-type"] = "application/json"
    request = urllib.request.Request(API_URL + path, data=data, headers=values, method=method)
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            raw = response.read()
            value = json.loads(raw) if raw else None
            return response.status, {k.lower(): v for k, v in response.headers.items()}, value
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RuntimeError(f"{method} {path}: HTTP {error.code}: {detail}") from error


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


def wait_request(request_id: str) -> dict[str, Any]:
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        _, _, value = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/requests/{request_id}")
        if value["status"] in ("succeeded", "failed"):
            if value["status"] != "succeeded":
                raise RuntimeError(f"authoring request terminated {value}")
            return value
        time.sleep(0.25)
    raise RuntimeError(f"timed out waiting for request {request_id}")


_, _, workspace = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
_, _, projection = call(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/requirement-projection"
)
if workspace["requirement_projection_revision_id"] != projection["artifact_id"]:
    _, _, workspace = call(
        "PATCH", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/requirement-projection",
        {"expected_artifact_id": projection["artifact_id"],
         "expected_sha256": projection["sha256"],
         "expected_workspace_revision_id": workspace["revision_id"],
         "expected_workspace_sha256": workspace["sha256"]},
        {"if-match": f'"{workspace["sha256"]}"',
         "idempotency-key": f"{KEY_PREFIX}-projection-apply-v1"},
    )
fixture_node_id = os.environ.get(
    "BID_V2_NODE_ID", "00000000-0000-4000-8000-0000000000c1"
)
node = next((value for value in workspace["nodes"]
             if value["lineage_id"] == fixture_node_id and not value.get("stale")), None)
if not node:
    raise RuntimeError("workspace has no current requirement-bound fixture node")
node_id = node["lineage_id"]
_, _, requirements = call("GET", f"/api/v2/bid-projects/{workspace['project_id']}/requirements")
requirement = next((value for value in requirements if value.get("lifecycle") == "current"), None)
if not requirement:
    raise RuntimeError("project has no current requirement for evidence matching")
need_id = requirement["fulfillment_expr"]["need_occurrence_id"]
if not any(value.get("need_occurrence_id") == need_id and
           value.get("target", {}).get("node_lineage_id") == node_id and
           value.get("state") == "bound" for value in workspace["bindings"]):
    _, _, workspace = call(
        "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/fulfillment-bindings",
        {"need_occurrence_id": need_id,
         "requirement_projection_revision_id": workspace["requirement_projection_revision_id"],
         "channel": requirement["fulfillment_expr"]["channel"],
         "target": {"kind": "outline_node", "node_lineage_id": node_id},
         "reason": "evidence API/worker E2E fixture binding"},
        {"if-match": f'"{workspace["sha256"]}"',
         "idempotency-key": f"{KEY_PREFIX}-binding-v1"},
    )
call(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/outline-checkpoints",
    {"expected_workspace_revision_id": workspace["revision_id"],
     "expected_workspace_sha256": workspace["sha256"]},
    {"if-match": f'"{workspace["sha256"]}"',
     "idempotency-key": f"{KEY_PREFIX}-checkpoint-v1"},
)
match_body = {"expected_workspace_revision_id": workspace["revision_id"]}
match_headers = {"if-match": f'"{workspace["sha256"]}"',
                 "idempotency-key": f"{KEY_PREFIX}-match-v1"}
status, _, match_request = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/nodes/{node_id}/evidence-matches",
    match_body, match_headers,
)
if status != 202:
    raise RuntimeError("evidence match did not return 202")
match_terminal = wait_request(match_request["request_artifact_id"])
if match_terminal.get("operation") != "match_only":
    raise RuntimeError("match-only request view omitted its operation")
_, _, listed_requests = call(
    "GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/requests"
)
listed_match = next(
    (value for value in listed_requests
     if value.get("request_artifact_id") == match_request["request_artifact_id"]),
    None,
)
if not listed_match or listed_match.get("operation") != "match_only":
    raise RuntimeError("workspace request list omitted the match-only operation")
_, _, evidence = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/nodes/{node_id}/evidence")
bundles = evidence.get("bundles", [])
if not bundles:
    raise RuntimeError("match_only worker did not publish an EvidenceBundle")
bundle = bundles[-1]
items = [item for item in bundle.get("items", []) if item.get("kind") != "no_evidence"]
if not items:
    raise RuntimeError("knowledge retrieval produced only NO_EVIDENCE")
item = items[0]
for field in ("evidence_item_id", "document_id", "source_chunk_id", "product_version_id",
              "quote_utf8", "quote_sha256", "quote_start_offset", "quote_end_offset"):
    if field not in item:
        raise RuntimeError(f"frozen evidence item is missing {field}")
if item.get("kind") == "image":
    for field in ("image_artifact_revision_id", "object_ref", "sha256", "media_type", "width", "height"):
        if field not in item:
            raise RuntimeError(f"frozen image evidence is missing {field}")
report_id = bundle["matching_report_id"]
pick_body = {"matching_report_id": report_id,
             "selected_evidence_item_ids": [item["evidence_item_id"]]}
_, _, pick = call(
    "PUT", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/nodes/{node_id}/evidence-pick-set",
    pick_body, {"idempotency-key": f"{KEY_PREFIX}-pick-v1"},
)
pick_id = pick.get("selection_id")
if not pick_id:
    raise RuntimeError("PickSet identity missing")
_, _, current = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
expect_http_error(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/content-candidates",
    {
        "target": "subtree",
        "node_lineage_id": "00000000-0000-4000-8000-0000000001d2",
        "fill_policy": "append_candidate",
        "insertion_anchor": {
            "node_revision_id": "00000000-0000-4000-8000-00000000013b",
            "block_revision_id": None,
        },
        "selection_mode": "user_pick_set",
        "pick_set_artifact_id": pick_id,
        "expected_workspace_revision_id": current["revision_id"],
    },
    {
        "if-match": f'"{current["sha256"]}"',
        "idempotency-key": f"{KEY_PREFIX}-invalid-anchor-v1",
    },
    400,
)
generate_body = {"target": "node", "node_lineage_id": node_id,
                 "fill_policy": "append_candidate", "insertion_anchor": None,
                 "selection_mode": "user_pick_set", "pick_set_artifact_id": pick_id,
                 "expected_workspace_revision_id": current["revision_id"]}
generate_headers = {"if-match": f'"{current["sha256"]}"',
                    "idempotency-key": f"{KEY_PREFIX}-generate-v1"}
status, _, generation = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/content-candidates",
    generate_body, generate_headers,
)
if status != 202:
    raise RuntimeError("user PickSet generation did not return 202")
generation_terminal = wait_request(generation["request_artifact_id"])
if generation_terminal.get("operation") != "generate":
    raise RuntimeError("generation request view omitted its operation")
result_identity = generation_terminal.get("result_identity") or {}
candidate_id = result_identity.get("artifact_id")
if not candidate_id:
    raise RuntimeError("ContentGenerate did not publish a Candidate")
_, _, candidate = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/candidates/{candidate_id}")
if candidate.get("status") != "proposed" or not candidate.get("operations"):
    raise RuntimeError("ContentCandidate publication is incomplete")
_, _, second_generation = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/content-candidates",
    generate_body,
    {"if-match": f'"{current["sha256"]}"', "idempotency-key": f"{KEY_PREFIX}-generate-v2"},
)
second_terminal = wait_request(second_generation["request_artifact_id"])
second_candidate_id = (second_terminal.get("result_identity") or {}).get("artifact_id")
if not second_candidate_id or second_candidate_id == candidate_id:
    raise RuntimeError("second Candidate identity was not independently published")
_, _, before_accept = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
accept_body = {"expected_workspace_revision_id": before_accept["revision_id"],
               "expected_workspace_sha256": before_accept["sha256"],
               "operation_indexes": [0], "client_node_refs": []}
accept_headers = {"if-match": f'"{before_accept["sha256"]}"',
                  "idempotency-key": f"{KEY_PREFIX}-accept-v1"}
_, _, accepted = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/candidates/{candidate_id}/accept",
    accept_body, accept_headers,
)
expect_http_error(
    "POST",
    f"/api/v2/submission-workspaces/{WORKSPACE_ID}/candidates/{second_candidate_id}/accept",
    accept_body,
    accept_headers,
    409,
)
_, _, accepted_replay = call(
    "POST", f"/api/v2/submission-workspaces/{WORKSPACE_ID}/candidates/{candidate_id}/accept",
    accept_body, {"if-match": f'"{before_accept["sha256"]}"',
                  "idempotency-key": f"{KEY_PREFIX}-accept-replay-v1"},
)
if accepted_replay != accepted:
    raise RuntimeError("accepted Candidate did not replay its immutable first receipt")
_, _, restored = call("GET", f"/api/v2/submission-workspaces/{WORKSPACE_ID}")
if restored["revision_id"] != accepted["revision_id"]:
    raise RuntimeError("accepted ContentCandidate was not persisted as Workspace head")
print(json.dumps({"status": "PASS", "match_request_id": match_request["request_artifact_id"],
                  "matching_report_id": report_id, "evidence_item_kind": item["kind"],
                  "pick_set_id": pick_id, "candidate_id": candidate_id,
                  "workspace_revision_id": restored["revision_id"]}, ensure_ascii=False))
