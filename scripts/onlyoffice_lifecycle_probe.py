#!/usr/bin/env python3
"""Opt-in ONLYOFFICE protocol probe; never connects to KnowledgeBrain's database.

Requires a cached digest-pinned Document Server image and local Playwright browser.
All edited documents, callback receipts and resources belong to a new evidence/run
subdirectory. This observes protocol/bytes, not production callback authorization,
version publication, layout acceptance or automatic retry guarantees.
"""
import argparse
import base64
import hashlib
import hmac
import io
import json
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener
import uuid
import zipfile
import xml.etree.ElementTree as ET


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("probe refuses redirects")


def request(url, value=None, headers=None, limit=64 * 1024 * 1024):
    data = None if value is None else json.dumps(value).encode()
    headers = dict(headers or {})
    if data is not None:
        headers["Content-Type"] = "application/json"
    with build_opener(NoRedirect).open(Request(url, data=data, headers=headers), timeout=20) as response:
        data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError("probe response exceeds byte budget")
        return data


def b64(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def sign(value, secret):
    data = b64(b'{"alg":"HS256","typ":"JWT"}') + "." + b64(json.dumps(value, separators=(",", ":")).encode())
    return data + "." + b64(hmac.digest(secret.encode(), data.encode(), "sha256"))


def verify(token, secret):
    header, payload, signature = token.split(".")
    decode = lambda value: base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))
    if json.loads(decode(header)).get("alg") != "HS256":
        raise ValueError("unexpected JWT algorithm")
    expected = hmac.digest(secret.encode(), (header + "." + payload).encode(), "sha256")
    if not hmac.compare_digest(decode(signature), expected):
        raise ValueError("invalid callback signature")
    return json.loads(decode(payload))


def docx_text(data):
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        xml = archive.read("word/document.xml")
    return "".join(ET.fromstring(xml).itertext())


def collect(root):
    config = json.loads((root / "runtime.json").read_text())
    sessions, events, captures = {}, [], []
    lock = threading.RLock()
    def record(event):
        events.append(event)
        (root / "events.json").write_text(json.dumps(events, indent=2))

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass  # Do not log source tickets or JWTs.

        def reply(self, value, status=200, content_type="application/json"):
            data = value if isinstance(value, bytes) else json.dumps(value).encode()
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            path = urlsplit(self.path).path
            with lock:
                if path.startswith("/editor/"):
                    key = path.removeprefix("/editor/")
                    if key not in sessions:
                        return self.reply({}, 404)
                    cfg = {
                        "documentType": "word",
                        "document": {"fileType": "docx", "key": key, "title": key + ".docx",
                            "url": config["collector_internal"] + "/source/" + sessions[key]["ticket"],
                            "permissions": {"edit": True}},
                        "editorConfig": {"mode": "edit", "lang": "en",
                            "user": {"id": config["user"], "name": "Lifecycle probe"},
                            "callbackUrl": config["collector_internal"] + "/callback/" + key,
                            "customization": {"forcesave": False}},
                        "width": "100%", "height": "100%"}
                    cfg["token"] = sign(cfg, config["secret"])
                    html = ('<!doctype html><html><body style="margin:0;height:100vh"><div id="editor"></div>'
                        '<script src="' + config["documentserver_public"] + '/web-apps/apps/api/documents/api.js"></script>'
                        '<script>window.probeReady=false;window.probeChanges=[];window.probeErrors=[];let cfg='
                        + json.dumps(cfg) + ';cfg.events={onDocumentReady:()=>window.probeReady=true,'
                        'onDocumentStateChange:e=>window.probeChanges.push(e.data),'
                        'onError:e=>window.probeErrors.push(e.data)};window.docEditor=new DocsAPI.DocEditor("editor",cfg);</script></body></html>')
                    return self.reply(html.encode(), content_type="text/html")
                if path.startswith("/source/"):
                    ticket = path.removeprefix("/source/")
                    source = next((v["source"] for v in sessions.values() if hmac.compare_digest(v["ticket"], ticket)), None)
                    if source is None:
                        return self.reply({}, 404)
                    return self.reply((root / source).read_bytes(), content_type="application/vnd.openxmlformats-officedocument.wordprocessingml.document")
                if path == "/admin/events" and self.headers.get("X-Probe-Token") == config["admin"]:
                    return self.reply(events)
                return self.reply({}, 404)

        def do_POST(self):
            path = urlsplit(self.path).path
            try:
                size = int(self.headers.get("Content-Length", "0"))
                if not 0 < size <= 1024 * 1024:
                    raise ValueError("invalid request byte budget")
                body = json.loads(self.rfile.read(size))
                with lock:
                    if path.startswith("/admin/"):
                        if self.headers.get("X-Probe-Token") != config["admin"]:
                            return self.reply({}, 403)
                        if path == "/admin/open":
                            source = body["source"]
                            allowed = {"initial.docx"} | {e["file"] for e in events if "file" in e}
                            if source not in allowed:
                                raise ValueError("source is not an owned document")
                            key = uuid.uuid4().hex
                            sessions[key] = {"source": source, "ticket": secrets.token_hex(32)}
                            return self.reply({"key": key, "url": config["collector_public"] + "/editor/" + key})
                        if path == "/admin/command":
                            command = {"c": "forcesave", "key": body["key"], "userdata": body["userdata"]}
                            command["token"] = sign(command, config["secret"])
                            result = json.loads(request("http://127.0.0.1/coauthoring/CommandService.ashx", command))
                            return self.reply(result)
                        if path == "/admin/replay":
                            captured = captures[body["index"]]
                            # Explicit harness redelivery; this is not a server retry claim.
                            self.reply({"queued": True})
                            threading.Thread(target=lambda: request(config["collector_internal"] + captured["path"],
                                captured["body"], captured["headers"]), daemon=True).start()
                            return
                        return self.reply({}, 404)
                    key = path.removeprefix("/callback/")
                    if not path.startswith("/callback/") or key not in sessions or body.get("key") != key:
                        return self.reply({"error": 1}, 403)
                    auth = self.headers.get("Authorization", "")
                    token = auth.removeprefix("Bearer ")
                    payload = verify(token, config["secret"])
                    signed = payload.get("payload", payload)
                    if signed != {k: v for k, v in body.items() if k != "token"}:
                        raise ValueError("callback payload differs from signed content")
                    index = len(captures)
                    captures.append({"path": path, "body": body, "headers": {"Authorization": auth}})
                    event = {"index": index, "key": key, "status": body["status"], "signature_verified": True,
                        "userdata": body.get("userdata"), "forcesavetype": body.get("forcesavetype"),
                        "actions": body.get("actions"), "users": body.get("users")}
                    if body["status"] in (2, 6):
                        url = urlsplit(body["url"])
                        allowed = {urlsplit(config["documentserver_public"]).netloc, "127.0.0.1"}
                        if url.scheme != "http" or url.netloc not in allowed or not url.path.startswith("/cache/files/"):
                            raise ValueError("callback download outside owned document server")
                        # Map the known browser-facing origin to the same container's port 80.
                        # This translation exists only in this disposable test collector.
                        data = request("http://127.0.0.1" + url.path + ("?" + url.query if url.query else ""), limit=config["max_bytes"])
                        docx_text(data)
                        filename = "saved-" + str(index) + ".docx"
                        (root / filename).write_bytes(data)
                        event.update(file=filename, sha256=hashlib.sha256(data).hexdigest(), byte_length=len(data))
                    record(event)
                    return self.reply({"error": 0})
            except Exception as error:
                with lock:
                    record({"collector_error": type(error).__name__ + ": " + str(error)})
                return self.reply({"error": 1}, 400)

    # Only the dedicated container's loopback is used by Document Server. Docker
    # publishes this listener on host loopback for the owned browser driver.
    ThreadingHTTPServer(("0.0.0.0", config["collector_port"]), Handler).serve_forever()


def run_probe(args):
    from playwright.sync_api import sync_playwright
    evidence = Path(args.evidence_dir).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    root = evidence / ("run-" + uuid.uuid4().hex)
    root.mkdir(mode=0o700)
    source = Path(args.sample).resolve()
    data = source.read_bytes()
    docx_text(data)
    shutil.copyfile(source, root / "initial.docx")
    shutil.copyfile(__file__, root / "probe.py")
    original_hash = hashlib.sha256(data).hexdigest()
    (root / "source.json").write_text(json.dumps({"path": str(source), "sha256": original_hash}))
    owner = uuid.uuid4().hex
    owned = {"owner": owner, "container": None, "network": None}
    def docker(*command):
        return subprocess.check_output(["docker", *command], text=True, stderr=subprocess.PIPE).strip()
    def record():
        (root / "owned.json").write_text(json.dumps(owned, indent=2))
    config = {"secret": secrets.token_hex(32), "admin": secrets.token_hex(32), "user": uuid.uuid4().hex,
        "max_bytes": args.max_bytes}
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        config["collector_port"] = sock.getsockname()[1]
    port = config["collector_port"]
    runtime = root / "runtime.json"
    runtime.touch(mode=0o600)
    env_file = root / "runtime.env"
    env_file.touch(mode=0o600)
    env_file.write_text("JWT_ENABLED=true\nJWT_SECRET=" + config["secret"] + "\nALLOW_PRIVATE_IP_ADDRESS=true\n")
    status, browser = 1, None
    print(str(root), flush=True)
    try:
        if "@sha256:" not in args.image:
            raise ValueError("a cached digest-pinned image is required")
        identity = json.loads(docker("image", "inspect", args.image))[0]
        if args.image not in identity["RepoDigests"]:
            raise ValueError("image RepoDigest mismatch")
        (root / "image.json").write_text(json.dumps({"image": args.image, "id": identity["Id"]}, indent=2))
        owned["network"] = docker("network", "create", "--label", "kb.lifecycle.owner=" + owner, "kb-lifecycle-" + owner)
        record()
        owned["container"] = docker("create", "--pull=never", "--label", "kb.lifecycle.owner=" + owner,
            "--network", owned["network"], "--publish", "127.0.0.1::80", "--publish", f"127.0.0.1::{port}",
            "--cpus", "2", "--memory", "4g", "--shm-size", "256m", "--restart", "no",
            "--env-file", str(env_file), "--mount", f"type=bind,src={root},dst=/probe", identity["Id"])
        record()
        docker("start", owned["container"])
        ds_port = docker("port", owned["container"], "80/tcp").rsplit(":", 1)[1]
        callback_port = docker("port", owned["container"], f"{port}/tcp").rsplit(":", 1)[1]
        config.update(documentserver_public="http://127.0.0.1:" + ds_port,
            collector_public="http://127.0.0.1:" + callback_port, collector_internal=f"http://127.0.0.1:{port}")
        runtime.write_text(json.dumps(config))
        docker("exec", "-d", owned["container"], "python3", "/probe/probe.py", "collect", "/probe")
        def wait_until(label, fn, timeout=None):
            deadline = time.monotonic() + (timeout or args.timeout)
            while time.monotonic() < deadline:
                result = fn()
                if result:
                    return result
                time.sleep(0.5)
            raise TimeoutError(label)
        def health():
            try:
                return request(config["documentserver_public"] + "/healthcheck") == b"true"
            except (URLError, OSError):
                return False
        wait_until("document server readiness", health)
        print("document server ready", flush=True)
        def admin(path, value=None):
            return json.loads(request(config["collector_public"] + "/admin/" + path, value,
                {"X-Probe-Token": config["admin"]}))
        def saved(key, state, userdata=None):
            events = admin("events")
            errors = [e for e in events if "collector_error" in e]
            if errors:
                raise RuntimeError(json.dumps(errors))
            return next((e for e in events if e.get("key") == key and e.get("status") == state
                and (userdata is None or e.get("userdata") == userdata)), None)
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(executable_path=str(Path(args.browser).resolve()), headless=True,
                args=["--no-sandbox"])
            context = browser.new_context(viewport={"width": 1440, "height": 1000})
            def open_page(session):
                page = context.new_page()
                page.goto(session["url"])
                page.wait_for_function("window.probeReady || window.probeErrors.length", timeout=args.timeout * 1000)
                errors = page.evaluate("window.probeErrors")
                if errors:
                    raise RuntimeError("editor errors: " + json.dumps(errors))
                return page
            def insert(page, marker):
                frame = page.frame_locator('iframe[name="frameEditor"]')
                frame.locator("#id_viewer_overlay").click(position={"x": 350, "y": 200})
                page.keyboard.press("Control+Home")
                page.keyboard.type(marker, delay=30)
                page.keyboard.press("Enter")
                page.wait_for_function("window.probeChanges.includes(true)", timeout=30000)
                # Wait for the editor's public state event to acknowledge local
                # changes sent to the editing service, not for a storage save.
                page.wait_for_function("window.probeChanges.at(-1) === false", timeout=30000)
            first = admin("open", {"source": "initial.docx"})
            page = open_page(first)
            peer = open_page(first)
            peer.close()
            marks, receipts = [], []
            for n in range(2):
                marker = "KB-LIFECYCLE-" + uuid.uuid4().hex
                marks.append(marker)
                page.evaluate("window.probeChanges=[]")
                insert(page, marker)
                correlation = uuid.uuid4().hex
                command = admin("command", {"key": first["key"], "userdata": correlation})
                assert command.get("error") == 0, command
                event = wait_until("forcesave callback", lambda: saved(first["key"], 6, correlation))
                text = docx_text((root / event["file"]).read_bytes())
                assert all(text.count(m) == 1 for m in marks)
                receipts.append(event)
                print("forcesave", n + 1, "verified", flush=True)
            marker = "KB-FINAL-" + uuid.uuid4().hex
            marks.append(marker)
            page.evaluate("window.probeChanges=[]")
            insert(page, marker)
            page.close()
            final = wait_until("final save callback", lambda: saved(first["key"], 2))
            final_bytes = (root / final["file"]).read_bytes()
            assert all(docx_text(final_bytes).count(m) == 1 for m in marks)
            assert marker not in docx_text((root / receipts[-1]["file"]).read_bytes())
            print("final save includes later edit", flush=True)
            # Redeliver the captured, genuinely signed old forcesave to the
            # collector after final save. The collector is evidence-only.
            before = len(admin("events"))
            admin("replay", {"index": receipts[0]["index"]})
            replay = wait_until("explicit old callback redelivery", lambda: next((e for e in admin("events")[before:]
                if e.get("status") == 6 and e.get("userdata") == receipts[0]["userdata"]), None))
            assert replay["sha256"] == receipts[0]["sha256"]
            reopened = admin("open", {"source": final["file"]})
            assert reopened["key"] != first["key"]
            page = open_page(reopened)
            frame = page.frame_locator('iframe[name="frameEditor"]')
            frame.locator("#id_viewer_overlay").click(position={"x": 350, "y": 200})
            page.keyboard.press("Control+f")
            page.keyboard.type(marker, delay=20)
            page.keyboard.press("Escape")
            page.screenshot(path=str(root / "reopened.png"))
            page.close()
            wait_until("unchanged close", lambda: saved(reopened["key"], 4))
            # This is a normal unchanged close/reopen observation; it does not
            # reproduce a network outage or assume a fixed reconnect timeout.
            page = open_page(reopened)
            wait_until("same-key reconnection notification", lambda: len([e for e in admin("events")
                if e.get("key") == reopened["key"] and e.get("status") == 1]) >= 2)
            page.close()
            report = {"browser": browser.version, "key": first["key"], "reopened_key": reopened["key"],
                "markers": marks, "forcesaves": receipts, "final": final, "explicit_redelivery": replay,
                "same_key_unchanged_reopen": True, "network_loss_reconnect_tested": False,
                "production_version_publication_tested": False}
            (root / "result.json").write_text(json.dumps(report, indent=2))
            browser.close()
            browser = None
        status = 0
    finally:
        if browser is not None:
            try:
                browser.close()
            except Exception:
                pass
        (root / "run.exit").write_text(str(status) + "\n")
        if owned["container"]:
            if docker("inspect", owned["container"], "--format", '{{index .Config.Labels "kb.lifecycle.owner"}}') != owner:
                raise RuntimeError("container ownership changed")
            docker("rm", "-f", "-v", owned["container"])
        if owned["network"]:
            if docker("network", "inspect", owned["network"], "--format", '{{index .Labels "kb.lifecycle.owner"}}') != owner:
                raise RuntimeError("network ownership changed")
            docker("network", "rm", owned["network"])
        for command in [("ps", "-aq"), ("network", "ls", "-q")]:
            if docker(*command, "--filter", "label=kb.lifecycle.owner=" + owner):
                raise RuntimeError("owned resource remains")
        runtime.unlink(missing_ok=True)
        env_file.unlink(missing_ok=True)
        assert hashlib.sha256(source.read_bytes()).hexdigest() == original_hash
        (root / "cleanup.exit").write_text("0\n")
    return status


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "collect":
        collect(Path(sys.argv[2]))
    else:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("--image", required=True)
        parser.add_argument("--sample", required=True)
        parser.add_argument("--browser", required=True)
        parser.add_argument("--evidence-dir", required=True)
        parser.add_argument("--timeout", type=int, default=180, help="per-operation timeout seconds")
        parser.add_argument("--max-bytes", type=int, default=64 * 1024 * 1024, help="probe download byte budget")
        sys.exit(run_probe(parser.parse_args()))
