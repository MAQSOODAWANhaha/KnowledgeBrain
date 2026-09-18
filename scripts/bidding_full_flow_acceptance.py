#!/usr/bin/env python3
"""Drive existing product APIs from an uploaded PDF/DOCX to an Office probe ticket.

`prepare` performs local checks only; `run` can start real model work. Services
must already be isolated and started by the caller. No Agent, SQL, parser or
model client is implemented here. Human expected answers are never accepted.

Private connection ticket (mode 0600):
  {"origin":"http://127.0.0.1:PORT","token":"...", "startup":{
    "env_file_sha256":"...", "runtime":{"analysis":{...production Config...}}}}
Startup snapshots are not proof of the later requests' frozen runtimes. The
caller must separately inspect those PostgreSQL records using the saved request
identities. Provider configuration is re-read exclusively from deploy/.env.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

from bidding_sample_run import provider_environment


def digest(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def private_write(path, data):
    temporary = path.with_name(path.name + ".tmp")
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "wb") as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)


def read_connection(path, env_file):
    if path.stat().st_mode & 0o077:
        raise ValueError("connection ticket must be private (mode 0600)")
    ticket = json.loads(path.read_text())
    origin = urllib.parse.urlsplit(ticket["origin"])
    if origin.scheme not in ("http", "https") or origin.hostname not in ("127.0.0.1", "localhost", "::1") \
            or origin.username or origin.password or origin.path not in ("", "/") or origin.query or origin.fragment:
        raise ValueError("acceptance API must be an explicit localhost origin")
    if not ticket.get("token"):
        raise ValueError("connection ticket has no authentication token")
    if ticket["startup"]["env_file_sha256"] != digest(env_file.read_bytes()):
        raise ValueError("deploy/.env differs from the service startup snapshot")
    env = provider_environment(env_file, {})

    def alias(primary, fallback):
        first, second = (env.get(key, "").strip() for key in (primary, fallback))
        if first and second and first != second:
            raise ValueError("conflicting provider aliases in deploy/.env")
        if not (first or second):
            raise ValueError("incomplete provider configuration in deploy/.env")
        return first or second

    model = alias("KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL")
    base = alias("KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL").rstrip("/")
    alias("KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY")
    for stage in ("analysis",):
        config = ticket["startup"]["runtime"][stage]
        provider = config["provider"]
        if not config.get("limits") or provider["model_id"] != model or provider["base_url"] != base \
                or provider["protocol"] != "openai_chat_completions_sse" \
                or provider.get("reasoning_effort") != (env.get("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT", "").strip() or None):
            raise ValueError("startup runtime snapshot differs from deploy/.env")
        for field, key in (("max_tokens", "KB_AUTHORING_MAX_OUTPUT_TOKENS"), ("timeout_ms", "KB_AUTHORING_TIMEOUT_MS")):
            if int(env.get(key, "0")) <= 0 or provider[field] != int(env[key]):
                raise ValueError("startup provider budget differs from deploy/.env")
    return ticket


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


class Api:
    def __init__(self, ticket):
        self.origin, self.token = ticket["origin"].rstrip("/"), ticket["token"]
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def request(self, method, path, body=None, headers=None, expected=200):
        request = urllib.request.Request(self.origin + path, data=body, method=method,
                                        headers={"Authorization": "Bearer " + self.token, **(headers or {})})
        try:
            with self.opener.open(request, timeout=30) as response:
                raw, status = response.read(), response.status
        except urllib.error.HTTPError as error:
            # Request identity/key remain persisted, including uncertain queue delivery.
            raise RuntimeError(f"{method} {path}: HTTP {error.code}; original intent retained") from None
        if status != expected:
            raise RuntimeError(f"{method} {path}: unexpected HTTP {status}; original intent retained")
        return raw

    def get(self, path):
        return json.loads(self.request("GET", path))


class Driver:
    def __init__(self, root, identity, api):
        self.root, self.api = root, api
        root.mkdir(parents=True, mode=0o700, exist_ok=True)
        self.path = root / "state.json"
        self.state = json.loads(self.path.read_text()) if self.path.exists() else {
            "identity": identity, "mutations": {}, "observations": {}, "stage": "prepared"}
        if self.state["identity"] != identity:
            raise ValueError("run directory belongs to another source, origin or startup configuration")
        self.save()

    def save(self):
        private_write(self.path, json_bytes(self.state))

    def mutation(self, name, path, prepare, expected):
        intent = self.state["mutations"].get(name)
        if intent is None:
            body, content_type = prepare()
            filename = name + ".request"
            private_write(self.root / filename, body)
            intent = {"path": path, "payload": filename, "sha256": digest(body), "content_type": content_type,
                      "key": str(uuid.uuid4()), "expected": expected, "receipt": None}
            self.state["mutations"][name] = intent
            self.save()
        if intent["path"] != path or intent["expected"] != expected:
            raise ValueError("attempted to change a persisted mutation")
        if intent["receipt"] is not None:
            return intent["receipt"]
        body = (self.root / intent["payload"]).read_bytes()
        if digest(body) != intent["sha256"]:
            raise ValueError("persisted mutation body changed")
        raw = self.api.request("POST", path, body, {"Content-Type": intent["content_type"],
                                                   "Idempotency-Key": intent["key"]}, expected)
        receipt = json.loads(raw)
        if not isinstance(receipt, dict):
            raise ValueError("invalid mutation receipt; original intent retained")
        keys = {"project": ("id", "workspace_id"), "upload": ("id", "original_sha256"),
                "analysis": ("request_artifact_id", "frozen_input_sha256")}[name]
        if any(not isinstance(receipt.get(key), str) or not receipt[key] for key in keys) \
                or (name == "analysis" and (not isinstance(receipt.get("request_revision"), int) or receipt["request_revision"] < 1)):
            raise ValueError("invalid mutation identity; original intent retained")
        intent["receipt"] = receipt
        self.save()
        return receipt

    def post(self, name, path, prepare, expected):
        return self.mutation(name, path, lambda: (json_bytes(prepare()), "application/json"), expected)

    def observe(self, name, read, complete, seconds, interval):
        deadline = time.monotonic() + seconds
        while True:
            value = read()
            self.state["stage"] = name
            self.state["observations"][name] = value
            self.save()
            if complete(value):
                return value
            if time.monotonic() >= deadline:
                raise TimeoutError(f"{name}: observation deadline; original request retained")
            time.sleep(interval)

    def job(self, name, path, receipt, seconds, interval):
        def complete(value):
            for key in ("request_artifact_id", "request_revision", "frozen_input_sha256"):
                if value[key] != receipt[key]:
                    raise ValueError("request identity changed")
            if value["status"] == "failed":
                raise RuntimeError(f"{name} failed: {value.get('error_code')}; original request retained")
            if value["status"] not in ("pending", "succeeded"):
                raise ValueError("unknown request state")
            return value["status"] == "succeeded"
        return self.observe(name, lambda: self.api.get(path), complete, seconds, interval)


def items(value, key):
    return value if isinstance(value, list) else value[key]


def handoff(ticket, project, current, basis, generated, docx):
    for key in ("version_id", "round_id", "docx_sha256"):
        if current[key] != generated[key]:
            raise ValueError("current DOCX changed after outline publication")
    if current["project_id"] != project["id"] or current["workspace_id"] != project["workspace_id"]:
        raise ValueError("outline belongs to another workspace")
    if current["editor"]["pending_save_id"] or current["editor"]["save_error"]:
        raise ValueError("current DOCX has an unresolved save")
    if any(current["round_basis"][key] != value for key, value in basis.items()):
        raise ValueError("current DOCX has another analysis basis")
    if digest(docx) != current["docx_sha256"]:
        raise ValueError("generated DOCX digest mismatch")
    expected = {key: current[key] for key in ("version_id", "round_id", "docx_sha256")}
    expected.update(basis)
    return {"origin": ticket["origin"], "token": ticket["token"], "project_id": project["id"],
            "workspace": project["workspace_id"], "expected": expected}


def source_media_type(source):
    media_types = {".pdf": "application/pdf",
                   ".docx": "application/vnd.openxmlformats-officedocument.wordprocessingml.document"}
    try:
        return media_types[source.suffix.lower()]
    except KeyError:
        raise ValueError("an original PDF or DOCX source is required") from None


def run(driver, source, ticket, seconds, interval):
    media_type = source_media_type(source)
    project = driver.post("project", "/api/v2/bid-projects", lambda: {"title": source.stem}, 201)
    project_path = "/api/v2/bid-projects/" + project["id"]
    base = "/api/v2/submission-workspaces/" + project["workspace_id"]

    def upload():
        boundary = uuid.uuid4().hex
        original = source.read_bytes()
        if digest(original) != driver.state["identity"]["source_sha256"]:
            raise ValueError("original source changed before upload")
        # Filename is presentation metadata; source bytes are preserved exactly.
        filename = source.name.replace('"', "_").replace("\r", "_").replace("\n", "_")
        body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{filename}"\r\n'
                f'Content-Type: {media_type}\r\n\r\n').encode()
        return body + original + f"\r\n--{boundary}--\r\n".encode(), "multipart/form-data; boundary=" + boundary

    document = driver.mutation("upload", project_path + "/tender-documents", upload, 201)
    if document["original_sha256"] != driver.state["identity"]["source_sha256"]:
        raise ValueError("uploaded source digest mismatch")

    def parsed(value):
        if value["original_sha256"] != document["original_sha256"]:
            raise ValueError("parsed source identity changed")
        if value["parse_status"] == "failed":
            raise RuntimeError("unified document parsing failed; original upload retained")
        return value["parse_status"] in ("ready", "completed")

    driver.observe("parsing", lambda: next(item for item in items(driver.api.get(project_path + "/tender-documents"), "documents")
                                          if item["id"] == document["id"]), parsed, seconds, interval)

    def freeze():
        current = items(driver.api.get(project_path + "/document-set-revisions"), "document_sets")
        head = current[0] if current else {}
        return {"document_ids": [document["id"]], "expected_artifact_id": head.get("artifact_id"), "expected_sha256": head.get("sha256")}

    accepted = driver.post("analysis", project_path + "/document-set-revisions", freeze, 201)
    analysis = driver.job("analysis", project_path + "/requirement-set-compilations/" + accepted["request_artifact_id"], accepted, seconds, interval)
    basis = driver.api.get(base + "/docx-fills/basis")
    published = analysis["result_identity"]
    if not basis or basis["requirement_set_id"] != published["requirement_set_id"] \
            or basis["requirement_set_sha256"] != published["requirement_set_sha256"] \
            or basis["document_set_id"] != analysis["document_set_revision_id"] \
            or basis["document_set_sha256"] != analysis["document_set_sha256"]:
        raise ValueError("outline basis does not reference this completed analysis")
    generated = published["draft_docx"]
    if not generated:
        raise ValueError("completed outline has no published DOCX")
    version = base + "/docx/versions/" + generated["version_id"]
    docx = driver.api.request("GET", version + "/download")
    office_ticket = handoff(ticket, project, driver.api.get(base + "/docx/current"), basis, generated, docx)
    private_write(driver.root / "generated.docx", docx)
    private_write(driver.root / "existing-ticket.json", json_bytes(office_ticket))
    driver.state.update(stage="outline_published", actual_frozen_runtime_audit="requires_external_postgres_read_only_check",
                        semantic_and_layout_acceptance="not_assessed", office_ticket="existing-ticket.json")
    driver.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("prepare", "run"))
    parser.add_argument("--connection-ticket", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--run-directory", type=Path, required=True)
    parser.add_argument("--wait-seconds", type=int, default=3600)
    parser.add_argument("--poll-seconds", type=float, default=3)
    args = parser.parse_args()
    if args.wait_seconds <= 0 or args.poll_seconds <= 0:
        parser.error("positive observation budgets are required")
    try:
        source_media_type(args.source)
    except ValueError as error:
        parser.error(str(error))
    env_file = Path(__file__).resolve().parent.parent / "deploy/.env"
    ticket = read_connection(args.connection_ticket, env_file)
    source = args.source.resolve(strict=True)
    identity = {"origin": ticket["origin"], "source_path": str(source), "source_sha256": digest(source.read_bytes()),
                "startup_sha256": digest(json_bytes(ticket["startup"]))}
    root = args.run_directory.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=True)
    with (root / ".driver.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit("another driver owns this run directory") from None
        driver = Driver(root, identity, Api(ticket))
        if args.mode == "run":
            run(driver, source, ticket, args.wait_seconds, args.poll_seconds)
        print(json.dumps({"stage": driver.state["stage"], "state": str(driver.path),
                          "runtime_audit": "startup snapshot only; frozen requests require separate PostgreSQL inspection"}))


if __name__ == "__main__":
    main()
