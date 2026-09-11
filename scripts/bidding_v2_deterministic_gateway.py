#!/usr/bin/env python3
"""Deterministic OpenAI-compatible SSE gateway for the hermetic Content E2E."""
import hashlib
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
import re


def find_node(value):
    if isinstance(value, dict):
        node = value.get("node_lineage_id")
        if isinstance(node, str):
            return node
        return next((found for nested in value.values() if (found := find_node(nested))), None)
    if isinstance(value, list):
        return next((found for nested in value if (found := find_node(nested))), None)
    if isinstance(value, str):
        match = re.search(r'"node_lineage_id"\s*:\s*"([0-9a-f-]{36})"', value)
        if match:
            return match.group(1)
        start = min((position for token in ("{", "[") if (position := value.find(token)) >= 0), default=-1)
        if start >= 0:
            try:
                return find_node(json.loads(value[start:]))
            except json.JSONDecodeError:
                return None
    return None


def output_for(request):
    node = find_node(request)
    if not node:
        serialized = json.dumps(request, ensure_ascii=False, separators=(",", ":"))
        match = re.search(r'node_lineage_id\\*"?\s*:\\*"([0-9a-f-]{36})', serialized)
        node = match.group(1) if match else None
    if not node:
        raise ValueError("frozen target node missing")
    content = {"type":"rich_text","nodes":[{"kind":"paragraph","content":[
        {"kind":"text","text":"【待人工补充】候选响应","marks":[]}]}]}
    canonical = json.dumps(content, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    block = {"schema_version":1,"block_revision_id":"00000000-0000-5000-8000-00000000e201",
        "lineage_id":"00000000-0000-5000-8000-00000000e202","revision":1,"kind":"rich_text",
        "content_sha256":hashlib.sha256(canonical).hexdigest(),"content":content,"origin":"agent_candidate"}
    return json.dumps({"schema_version":1,"operations":[{"kind":"insert_block",
        "client_operation_ref":"deterministic-op-1","target_node_lineage_id":node,
        "ordinal":0,"block":block}],"factual_claims":[],"notices":[]},ensure_ascii=False,separators=(",", ":"))

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def do_GET(self):
        if self.path == "/healthz":
            body=b"ok"
            self.send_response(200); self.send_header("content-length",str(len(body))); self.end_headers(); self.wfile.write(body)
        else:
            self.send_error(404)
    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404); return
        length=int(self.headers.get("content-length","0")); request=json.loads(self.rfile.read(length))
        if request.get("model") != os.environ.get("GATEWAY_MODEL","scripted-content") or request.get("stream") is not True:
            self.send_error(400); return
        try:
            output = output_for(request)
        except ValueError:
            self.send_error(400); return
        chunks=[json.dumps({"choices":[{"delta":{"content":output},"index":0}]},separators=(",", ":")),"[DONE]"]
        body="".join(f"data: {chunk}\n\n" for chunk in chunks).encode()
        self.send_response(200); self.send_header("content-type","text/event-stream"); self.send_header("content-length",str(len(body))); self.end_headers(); self.wfile.write(body)
    def log_message(self, format, *args):
        return

ThreadingHTTPServer(("127.0.0.1",int(os.environ.get("GATEWAY_PORT","18080"))),Handler).serve_forever()
