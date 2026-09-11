#!/usr/bin/env python3
"""Actual API/worker/HTTP-provider acceptance in exclusively owned infrastructure.

Uses synthetic source and exported tool steps, never deploy/.env or real tenders.
Requires prebuilt api, worker, migrator and the Rust test toolchain on PATH.
This checks transport/publication; it is not model or tender semantic acceptance.
"""

import argparse
import base64
import copy
import hashlib
import hmac
import http.server
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid
import zipfile


def encoded(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=")


def token(actor, secret):
    parts = [encoded(json.dumps(value).encode()) for value in (
        {"alg": "HS256", "typ": "JWT"},
        {"sub": actor.removeprefix("user:"), "exp": int(time.time()) + 600},
    )]
    signing = b".".join(parts)
    return (signing + b"." + encoded(hmac.digest(secret.encode(), signing, "sha256"))).decode()


def available_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            assert self.path == "/v1/chat/completions"
            assert self.headers["Authorization"] == "Bearer " + self.server.api_key
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert request["model"] == self.server.model
            assert request["stream"] is True
            assert request["tool_choice"] == "required"
            results = [json.loads(m["content"]) for m in request["messages"] if m["role"] == "tool"]
            assert all(v["ok"] is True for v in results), results
            with self.server.lock:
                index = len(self.server.calls)
                assert index < len(self.server.steps), "unexpected extra physical model call"
                name, args = copy.deepcopy(self.server.steps[index])
                assert name in [t["function"]["name"] for t in request["tools"]]
                context = json.loads(request["messages"][1]["content"])
                if name in ("set_presentation", "put_section"):
                    args["expected_draft_sha256"] = context["draft_sha256"]
                if name == "inspect_rendered_cells":
                    args["bookmark"] = next(
                        item["value"]["bookmark"]
                        for result in reversed(results)
                        for item in result.get("result", {}).get("items", [])
                        if isinstance(item.get("value", {}).get("table"), dict)
                    )
                self.server.calls.append({"index": index, "tool": name})
            # Split tool arguments across SSE deltas, exercising real accumulation.
            arguments = json.dumps(args, ensure_ascii=False)
            middle = len(arguments) // 2
            events = [
                {"choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{
                    "index": 0, "id": str(uuid.uuid4()), "type": "function",
                    "function": {"name": name, "arguments": arguments[:middle]},
                }]}, "finish_reason": None}]},
                {"choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0, "function": {"arguments": arguments[middle:]},
                }]}, "finish_reason": None}]},
                {"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]},
            ]
            body = ("".join("data: " + json.dumps(e, ensure_ascii=False) + "\n\n" for e in events)
                    + "data: [DONE]\n\n").encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as error:
            self.server.errors.append(repr(error))
            self.send_error(500, "script contract failed")


def product_generation(root, evidence, browser_path, origin, auth, fixture, request_http, wait_until):
    """Join existing real browser/editor helpers to the real generation request."""
    from docx import Document
    from playwright.sync_api import sync_playwright, expect
    from onlyoffice_lifecycle_probe import docx_text
    from onlyoffice_rich_probe import tables
    from onlyoffice_web_probe import product_web

    base = f'/api/v2/submission-workspaces/{fixture["workspace"]}'
    ticket = {"origin": origin, "workspace": fixture["workspace"], "token": auth}
    settings = {"web": True, "timeout": 120}

    def get(path):
        return json.loads(request_http("GET", origin + path, bearer=auth))

    with product_web(root, settings, ticket) as web, sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=str(browser_path.resolve()), headless=True,
                                             args=["--no-sandbox", "--no-proxy-server"])
        page = browser.new_page(viewport={"width": 1440, "height": 1000})
        try:
            web.open(page, {"session": {"project_id": fixture["project"]}})
            page.get_by_test_id("docx-close").click()
            web.status(page, "编辑器已关闭")
            page.get_by_role("button", name="生成或导入新轮", exact=True).click()
            panel = page.get_by_test_id("docx-composition")
            # The prior HTTP-generated round is the latest completed task.
            panel.get_by_role("button", name="准备重新生成", exact=True).click()
            button = panel.get_by_role("button", name="生成完整投标模板", exact=True)
            expect(button).to_be_enabled()
            with page.expect_response(lambda response: response.url.endswith(base + "/docx-compositions")
                                      and response.request.method == "POST") as accepted_response:
                button.click()
            assert accepted_response.value.status == 202
            accepted = accepted_response.value.json()
            status_path = base + "/docx-compositions/" + accepted["request_artifact_id"]
            expect(panel.get_by_role("button", name="下载本次生成稿", exact=True)).to_be_visible(timeout=120000)
            job = get(status_path)
            assert job["status"] == "succeeded", job
            generated = job["result_identity"]
            with page.expect_download() as downloaded:
                panel.get_by_role("button", name="下载本次生成稿", exact=True).click()
            original = Path(downloaded.value.path()).read_bytes()
            assert hashlib.sha256(original).hexdigest() == generated["docx_sha256"]
            (evidence / "synthetic-browser-generated.docx").write_bytes(original)
            manifest = generated["manifest"]
            manifest_bytes = (Path(fixture["object_directory"]) / manifest["sha256"]).read_bytes()
            assert hashlib.sha256(manifest_bytes).hexdigest() == manifest["sha256"]
            assert len(manifest_bytes) == manifest["byte_length"]
            (evidence / "synthetic-browser-manifest.json").write_bytes(manifest_bytes)
            original_tables = tables(Document(io.BytesIO(original)))
            assert original_tables, "generated template must exercise editable tables"
            panel.get_by_role("button", name="进入当前稿件", exact=True).click()
            web.status(page, "当前保存版本已载入")
            markers, saves, keys = [], [], []
            for index in range(2):
                before = get(base + "/docx/current")
                key = before["editor"]["key"]
                assert key and key not in keys
                keys.append(key)
                marker = "KB-COMPOSITION-" + uuid.uuid4().hex
                markers.append(marker)
                page.frame_locator('iframe[name="frameEditor"]').locator("#id_viewer_overlay").click(
                    position={"x": 350, "y": 200})
                page.keyboard.press("Escape")
                page.keyboard.press("Control+Home")
                if index == 1:
                    # Edit an actual generated cell after reopening, using its
                    # fixture-derived wording rather than a business keyword.
                    needle = next(cell["text"] for table in original_tables for row in table["rows"]
                                  for cell in row["cells"] if cell["text"].strip())
                    page.keyboard.press("Control+f")
                    page.keyboard.type(needle, delay=30)
                    page.keyboard.press("Enter")
                    page.keyboard.press("Escape")
                    page.wait_for_timeout(300)
                    page.keyboard.press("ArrowRight")
                    page.wait_for_timeout(100)
                page.keyboard.type(marker, delay=30)
                if index == 0:
                    page.keyboard.press("Enter")
                web.edited(page)
                with page.expect_response(lambda response: response.url.endswith(base + "/docx/editor/save")
                                          and response.request.method == "POST") as saved_response:
                    page.get_by_test_id("docx-save").click()
                assert saved_response.value.ok
                command = saved_response.value.json()
                assert command["dispatch"] is True
                web.status(page, "已保存")
                current = get(base + "/docx/current")
                assert current["version_id"] != before["version_id"]
                assert current["round_id"] == generated["round_id"]
                assert current["editor"]["pending_save_id"] is None
                with page.expect_download() as saved:
                    page.get_by_role("button", name="下载已保存稿", exact=True).click()
                data = Path(saved.value.path()).read_bytes()
                assert hashlib.sha256(data).hexdigest() == current["docx_sha256"]
                assert all(docx_text(data).count(value) == 1 for value in markers)
                edited_document = Document(io.BytesIO(data))
                assert tables(edited_document, remove=marker if index == 1 else "") == original_tables
                if index == 1:
                    assert sum(cell["text"].count(marker) for table in tables(edited_document)
                               for row in table["rows"] for cell in row["cells"]) == 1
                (evidence / f"synthetic-browser-save-{index + 1}.docx").write_bytes(data)
                page.screenshot(path=str(evidence / f"synthetic-browser-save-{index + 1}.png"))
                saves.append({"save_id": command["save_id"], "version": current})
                page.get_by_test_id("docx-close").click()
                web.status(page, "编辑器已关闭")
                wait_until(lambda: get(base + "/docx/current")["editor"]["key"] is None, 120)
                if index == 0:
                    page.get_by_test_id("docx-reopen").click()
                    web.status(page, "当前保存版本已载入")
            page.screenshot(path=str(evidence / "synthetic-browser-completed.png"))
            # Office edits advance current, never the generation task's result.
            assert get(status_path)["result_identity"] == generated
            history = request_http("GET", origin + base + f'/docx/versions/{generated["version_id"]}/download',
                                   bearer=auth)
            assert history == original
            return {"status": "passed", "request": accepted, "generated": generated, "saves": saves,
                    "new_editor_key_after_reopen": keys[0] != keys[1], "tables_preserved": len(original_tables),
                    "generated_cell_edit_persisted": True,
                    "generation_result_immutable_after_edits": True, "semantic_acceptance": False}
        except Exception:
            page.screenshot(path=str(evidence / "synthetic-browser-failed.png"))
            raise
        finally:
            browser.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--postgres-image", required=True)
    parser.add_argument("--redis-image", required=True)
    parser.add_argument("--browser", type=Path, help="also exercise the actual App and Document Server")
    parser.add_argument("--onlyoffice-image", help="cached digest-pinned image; required with --browser")
    args = parser.parse_args()
    if bool(args.browser) != bool(args.onlyoffice_image):
        parser.error("--browser and --onlyoffice-image must be supplied together")
    if args.onlyoffice_image and "@sha256:" not in args.onlyoffice_image:
        parser.error("--onlyoffice-image must be digest pinned")
    repo = Path(__file__).resolve().parent.parent
    target = args.target_dir.resolve()
    evidence = args.evidence_dir.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    for binary in ("api", "worker", "migrator"):
        assert (target / "debug" / binary).is_file(), f"build {binary} first"
    owner = uuid.uuid4().hex
    database = "knowledgebrain_test_" + owner
    label = "kb.composition.acceptance.owner"
    containers, processes, logs = [], [], []
    root = Path(tempfile.mkdtemp(prefix="kb-composition-transport-"))
    server = None
    summary = {"status": "failed", "semantic_acceptance": False, "source": "synthetic_fixture", "owner": owner}
    summary["binary_sha256"] = {}
    for binary in ("api", "worker", "migrator"):
        with (target / "debug" / binary).open("rb") as source:
            summary["binary_sha256"][binary] = hashlib.file_digest(source, "sha256").hexdigest()

    def run(command, data=None, env=None, cwd=None):
        result = subprocess.run(command, input=data, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, env=env, cwd=cwd, timeout=300)
        with (evidence / "run.log").open("a") as log:
            log.write(result.stdout)
        result.check_returncode()
        return result.stdout.strip()

    def container(image, port, extra=()):
        cid = run(["docker", "run", "-d", "--pull=never", "--label", f"{label}={owner}",
                   "-p", f"127.0.0.1::{port}", *extra, image])
        containers.append(cid)
        return cid

    def sql(statement):
        return run(["docker", "exec", "-i", pg, "psql", "-X", "-qAt", "-U", "postgres",
                    "-d", database, "-v", "ON_ERROR_STOP=1"], statement)

    def wait_until(check, seconds=60):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            for process in processes:
                assert process.poll() is None, "service exited; inspect service log"
            if check():
                return
            time.sleep(0.2)
        raise TimeoutError("acceptance deadline exceeded")

    def request_http(method, path, payload=None, bearer=None, key=None, expected=200):
        headers = {}
        if bearer:
            headers["Authorization"] = "Bearer " + bearer
        if key:
            headers["Idempotency-Key"] = key
        data = None if payload is None else json.dumps(payload).encode()
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(path, data=data, headers=headers, method=method)
        try:
            response = urllib.request.urlopen(request, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            body = response.read()
            assert response.status == expected, (response.status, body.decode(errors="replace"))
            return body

    def launch(binary, kind, env):
        local = dict(env, DATABASE_URL=f"postgresql://kb_runtime_{kind}@127.0.0.1:{pg_port}/{database}",
                     KB_COMPONENT_KIND=kind,
                     KB_COMPONENT_IMAGE_DIGEST=descriptor["images"][kind].split("@")[1])
        log = (evidence / f"{binary}-{len(logs)}.log").open("w")
        logs.append(log)
        process = subprocess.Popen([str(target / "debug" / binary)], cwd=root, env=local,
                                   stdout=log, stderr=subprocess.STDOUT)
        processes.append(process)

    try:
        pg = container(args.postgres_image, 5432, ("-e", "POSTGRES_HOST_AUTH_METHOD=trust",
                                                  "-e", "POSTGRES_DB=" + database))
        wait_until(lambda: subprocess.run(["docker", "exec", pg, "pg_isready", "-h", "127.0.0.1", "-U", "postgres",
                                            "-d", database], capture_output=True).returncode == 0)
        pg_port = run(["docker", "port", pg, "5432/tcp"]).rsplit(":", 1)[1]
        sql(f'''CREATE ROLE kb_app_owner NOLOGIN NOINHERIT;
CREATE ROLE kb_migrator LOGIN NOINHERIT;
CREATE ROLE kb_runtime_api LOGIN INHERIT;
CREATE ROLE kb_runtime_worker LOGIN INHERIT;
CREATE ROLE kb_runtime_retention LOGIN INHERIT;
GRANT kb_app_owner TO kb_migrator;
CREATE EXTENSION pgcrypto;
CREATE EXTENSION vector;
ALTER SCHEMA public OWNER TO kb_app_owner;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE TEMPORARY,CREATE ON DATABASE "{database}" FROM PUBLIC;''')
        redis = container(args.redis_image, 6379)
        wait_until(lambda: subprocess.run(["docker", "exec", redis, "redis-cli", "ping"],
                                           capture_output=True).returncode == 0)
        redis_port = run(["docker", "port", redis, "6379/tcp"]).rsplit(":", 1)[1]
        # Intentionally do not inherit application settings or load either .env.
        env = {name: os.environ[name] for name in ("PATH", "RUSTC", "RUSTDOC", "CARGO_HOME", "RUSTUP_HOME")
               if name in os.environ}
        descriptor_path = root / "release.json"
        shutil.copyfile(repo / "deploy/release-descriptor-v1.development.json", descriptor_path)
        descriptor = json.loads(descriptor_path.read_text())
        descriptor_sha = hashlib.sha256(b"KB:ReleaseDescriptor:v1\0" + json.dumps(
            descriptor, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        objects = root / "objects"
        objects.mkdir()
        env.update(KB_RELEASE_DESCRIPTOR_PATH=str(descriptor_path),
                   KB_RELEASE_DESCRIPTOR_SHA256=descriptor_sha,
                   KB_DEPLOYMENT_NAMESPACE_ID=str(uuid.uuid4()),
                   DATABASE_URL=f"postgresql://kb_migrator@127.0.0.1:{pg_port}/{database}",
                   KB_COMPONENT_KIND="migrator",
                   KB_COMPONENT_IMAGE_DIGEST=descriptor["images"]["migrator"].split("@")[1],
                   KB_TENDER_AGENT_TEST_DATABASE_URL=f"postgresql://postgres@127.0.0.1:{pg_port}/{database}",
                   OBJECT_DIR=str(objects), KNOWLEDGEBRAIN_S3_BUCKET="",
                   REDIS_URL=f"redis://127.0.0.1:{redis_port}",
                   KB_COMPOSITION_HTTP_FIXTURE=str(objects / "fixture.json"), RUST_LOG="info,sqlx=error")
        run([str(target / "debug" / "migrator")], env=env, cwd=root)
        run(["cargo", "test", "-p", "bidding", "--test", "tender_analysis_postgres", "--locked",
             "--offline", "--target-dir", str(target), "export_composition_http_fixture", "--",
             "--ignored", "--exact", "--nocapture"], env=env, cwd=repo)
        fixture = json.loads((objects / "fixture.json").read_text())
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        server.steps = fixture["model_steps"] * (3 if args.browser else 2)
        server.calls, server.errors, server.lock = [], [], threading.Lock()
        server.api_key, server.model = uuid.uuid4().hex, "synthetic-composition-transport"
        threading.Thread(target=server.serve_forever, daemon=True).start()
        api_port, probe_port = available_port(), available_port()
        secret = uuid.uuid4().hex
        env.update(KNOWLEDGEBRAIN_CHAT_BASE_URL=f"http://127.0.0.1:{server.server_port}/v1",
                   KNOWLEDGEBRAIN_CHAT_API_KEY=server.api_key, KNOWLEDGEBRAIN_CHAT_MODEL=server.model,
                   KB_DOCX_COMPOSITION_LIMITS=json.dumps(fixture["config"]["limits"]),
                   API_PORT=str(api_port), JWT_SECRET=secret,
                   KNOWLEDGEBRAIN_WORKER_PROBE_ADDR=f"127.0.0.1:{probe_port}")
        if args.browser:
            office_env = root / "office.env"
            office_env.touch(mode=0o600)
            office_secret = uuid.uuid4().hex
            office_env.write_text("JWT_ENABLED=true\nJWT_SECRET=" + office_secret
                                  + "\nALLOW_PRIVATE_IP_ADDRESS=true\n")
            office = container(args.onlyoffice_image, 80, ("--env-file", str(office_env)))
            office_port = run(["docker", "port", office, "80/tcp"]).rsplit(":", 1)[1]
            networks = json.loads(run(["docker", "inspect", office, "--format", "{{json .NetworkSettings.Networks}}"] ))
            assert len(networks) == 1, "test Document Server must have one network"
            gateway = next(iter(networks.values()))["Gateway"]
            assert gateway
            office_origin = f"http://127.0.0.1:{office_port}"
            env.update(KB_ONLYOFFICE_SERVER_ORIGIN=office_origin,
                       KB_ONLYOFFICE_API_ORIGIN=f"http://{gateway}:{api_port}",
                       KB_ONLYOFFICE_JWT_SECRET=office_secret,
                       KB_ONLYOFFICE_CAPABILITY_SECRET=uuid.uuid4().hex,
                       KB_ONLYOFFICE_TOKEN_TTL_SECONDS="1200", KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS="120")

            def office_ready():
                try:
                    return request_http("GET", office_origin + "/healthcheck") == b"true"
                except (OSError, AssertionError):
                    return False

            wait_until(office_ready, 240)
            summary["onlyoffice_image"] = run(["docker", "image", "inspect", args.onlyoffice_image,
                                               "--format", "{{.Id}}"])
        launch("api", "api", env)
        launch("worker", "worker", env)
        api = f"http://127.0.0.1:{api_port}"

        def ready():
            try:
                request_http("GET", api + "/ready")
                request_http("GET", f"http://127.0.0.1:{probe_port}/ready")
                return True
            except (OSError, AssertionError):
                return False

        wait_until(ready)
        summary["schema_verified_api_and_worker_ready"] = True
        auth = token(fixture["actor"], secret)
        wrong_auth = token(str(uuid.uuid4()), secret)
        base = api + f'/api/v2/submission-workspaces/{fixture["workspace"]}'
        compositions = base + "/docx-compositions"
        assert json.loads(request_http("GET", compositions + "/basis", bearer=auth)) == fixture["basis"]
        previous = fixture["current"]
        receipts = []
        for round_index in range(2):
            intent = {"basis": fixture["basis"], "expected": {
                "version_id": previous["version_id"], "docx_sha256": previous["docx_sha256"]}}
            key = str(uuid.uuid4())
            accepted = json.loads(request_http("POST", compositions, intent, auth, key, 202))
            assert json.loads(request_http("POST", compositions, intent, auth, key, 202)) == accepted
            status_url = compositions + "/" + accepted["request_artifact_id"]
            statuses = []

            def finished():
                value = json.loads(request_http("GET", status_url, bearer=auth))
                statuses.append(value)
                assert value["status"] not in ("failed", "canceled"), value
                return value["status"] == "succeeded"

            wait_until(finished, 120)
            result = statuses[-1]["result_identity"]
            assert result["version_id"] != previous["version_id"]
            assert result["round_id"] != previous["round_id"]
            assert len(server.calls) == len(fixture["model_steps"]) * (round_index + 1)
            assert not server.errors, server.errors
            download = base + f'/docx/versions/{result["version_id"]}/download'
            data = request_http("GET", download, bearer=auth)
            assert hashlib.sha256(data).hexdigest() == result["docx_sha256"]
            assert len(data) == result["byte_length"]
            with zipfile.ZipFile(io.BytesIO(data)) as document:
                xml = document.read("word/document.xml").decode()
                assert "响应及证明材料" in xml and "<w:tbl>" in xml
            (evidence / f"synthetic-round-{round_index + 1}.docx").write_bytes(data)
            request_http("GET", download, bearer=wrong_auth, expected=403)
            request_http("GET", status_url, bearer=wrong_auth, expected=403)
            manifest = result["manifest"]
            object_directory = Path(fixture["object_directory"])
            assert object_directory.resolve().is_relative_to(objects.resolve())
            manifest_bytes = (object_directory / manifest["sha256"]).read_bytes()
            assert hashlib.sha256(manifest_bytes).hexdigest() == manifest["sha256"]
            assert len(manifest_bytes) == manifest["byte_length"]
            (evidence / f"synthetic-round-{round_index + 1}-manifest.json").write_bytes(manifest_bytes)
            # Replay after completion retains the original result and makes no new model calls.
            assert json.loads(request_http("POST", compositions, intent, auth, key, 202)) == accepted
            assert json.loads(request_http("GET", status_url, bearer=auth))["result_identity"] == result
            receipts.append({"request": accepted, "result": result, "intent": intent, "key": key})
            previous = result
            if round_index == 0:
                for process in reversed(processes):
                    process.terminate()
                    assert process.wait(timeout=40) == 0
                processes.clear()
                launch("api", "api", env)
                launch("worker", "worker", env)
                wait_until(ready)
                summary["restart_after_publication_ready"] = True
        first = receipts[0]
        assert json.loads(request_http("POST", compositions, first["intent"], auth, first["key"], 202)) == first["request"]
        assert hashlib.sha256(request_http("GET", base + f'/docx/versions/{first["result"]["version_id"]}/download',
                                   bearer=auth)).hexdigest() == first["result"]["docx_sha256"]
        if args.browser:
            summary["product"] = product_generation(root, evidence, args.browser, api, auth, fixture,
                                                    request_http, wait_until)
        rounds = sql("SELECT count(*) FROM bid_docx_round_artifacts WHERE workspace_id='"
                     + str(uuid.UUID(fixture["workspace"])) + "';")
        assert int(rounds) == (4 if args.browser else 3), rounds
        assert len(server.calls) == len(server.steps) and not server.errors
        summary.update(status="passed", generated_rounds=3 if args.browser else 2, physical_model_calls=len(server.calls),
                       completed_and_historical_replay=True, authenticated_downloads=True,
                       docx_and_manifest_hashes_verified=True, results=receipts, calls=server.calls)
    finally:
        cleanup_errors = []
        service_exits = []
        for process in reversed(processes):
            process.terminate()
            try:
                process.wait(timeout=40)
                service_exits.append(process.returncode)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
                cleanup_errors.append("service shutdown deadline exceeded")
        for log in logs:
            log.close()
        if server:
            server.shutdown()
            server.server_close()
            summary["provider_errors"] = server.errors
        for cid in reversed(containers):
            try:
                actual = run(["docker", "inspect", cid, "--format", '{{index .Config.Labels "' + label + '"}}'])
                assert actual == owner, "container ownership changed"
                run(["docker", "rm", "-f", "-v", cid])
            except Exception as error:
                cleanup_errors.append(repr(error))
        if (root / "vite.log").exists():
            shutil.copyfile(root / "vite.log", evidence / "vite.log")
        shutil.rmtree(root)
        summary["service_exit_codes"] = service_exits
        summary["cleanup"] = "passed" if not cleanup_errors else cleanup_errors
        if cleanup_errors or any(code != 0 for code in service_exits):
            summary["status"] = "failed"
        (evidence / "verification.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
        if cleanup_errors or any(code != 0 for code in service_exits):
            raise RuntimeError({"cleanup_errors": cleanup_errors, "service_exit_codes": service_exits})
    print(json.dumps({"status": summary["status"], "evidence": str(evidence)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
