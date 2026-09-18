#!/usr/bin/env python3
"""Opt-in ONLYOFFICE roundtrip probe for draft chapter identity; never connects to KnowledgeBrain's database.

Answers whether the compiler's `kb_s*` bookmarks survive an editor save, and what
heading style names remain, after a user renames a chapter, fills a body block,
deletes a chapter and appends a new chapter. Reuses the lifecycle probe's evidence
collector. Requires a cached digest-pinned Document Server image and a local
Playwright browser. Observes bytes only; no callback authorization, version
publication or layout claim.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import time
import uuid
import zipfile
from urllib.error import URLError
import xml.etree.ElementTree as ET

sys.path.insert(0, str(Path(__file__).resolve().parent))
from onlyoffice_lifecycle_probe import request  # noqa: E402

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"


def describe(data):
    """Report bookmark names with their carrier, plus body paragraph styles."""
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        document = ET.fromstring(archive.read("word/document.xml"))
        styles = ET.fromstring(archive.read("word/styles.xml"))
    body = document.find(W + "body")
    names = {}
    for style in styles.iter(W + "style"):
        name = style.find(W + "name")
        if name is not None:
            names[style.get(W + "styleId")] = name.get(W + "val")
    bookmarks, paragraphs = [], []
    for child in body:
        tag = child.tag.removeprefix(W)
        if tag == "bookmarkStart":
            bookmarks.append({"name": child.get(W + "name"), "placement": "block", "carrier": None})
        for node in child.iter(W + "bookmarkStart"):
            bookmarks.append({"name": node.get(W + "name"), "placement": "inline", "carrier": tag})
        if tag == "p":
            style = child.find(W + "pPr/" + W + "pStyle")
            style_id = None if style is None else style.get(W + "val")
            paragraphs.append({"style_id": style_id, "style_name": names.get(style_id),
                "text": "".join(child.itertext()).strip()})
    return {"bookmarks": bookmarks, "paragraphs": paragraphs, "style_names": names,
        "sha256": hashlib.sha256(data).hexdigest(), "byte_length": len(data)}


def run(args):
    from playwright.sync_api import sync_playwright
    evidence = Path(args.evidence_dir).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    root = evidence / ("roundtrip-" + uuid.uuid4().hex)
    root.mkdir(mode=0o700)
    source = Path(args.sample).resolve()
    original = source.read_bytes()
    original_hash = hashlib.sha256(original).hexdigest()
    shutil.copyfile(source, root / "initial.docx")
    shutil.copyfile(Path(__file__).resolve().parent / "onlyoffice_lifecycle_probe.py", root / "probe.py")
    owner = uuid.uuid4().hex
    owned = {"owner": owner, "container": None, "network": None}

    def docker(*command):
        return subprocess.check_output(["docker", *command], text=True, stderr=subprocess.PIPE).strip()

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
        owned["network"] = docker("network", "create", "--label", "kb.roundtrip.owner=" + owner,
            "kb-roundtrip-" + owner)
        owned["container"] = docker("create", "--pull=never", "--label", "kb.roundtrip.owner=" + owner,
            "--network", owned["network"], "--publish", "127.0.0.1::80", "--publish", f"127.0.0.1::{port}",
            "--cpus", "2", "--memory", "4g", "--shm-size", "256m", "--restart", "no",
            "--env-file", str(env_file), "--mount", f"type=bind,src={root},dst=/probe", identity["Id"])
        (root / "owned.json").write_text(json.dumps(owned, indent=2))
        docker("start", owned["container"])
        ds_port = docker("port", owned["container"], "80/tcp").rsplit(":", 1)[1]
        callback_port = docker("port", owned["container"], f"{port}/tcp").rsplit(":", 1)[1]
        config.update(documentserver_public="http://127.0.0.1:" + ds_port,
            collector_public="http://127.0.0.1:" + callback_port, collector_internal=f"http://127.0.0.1:{port}")
        runtime.write_text(json.dumps(config))
        docker("exec", "-d", owned["container"], "python3", "/probe/probe.py", "collect", "/probe")

        def wait_until(label, fn):
            deadline = time.monotonic() + args.timeout
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

        def saved(key, state, userdata):
            events = admin("events")
            errors = [e for e in events if "collector_error" in e]
            if errors:
                raise RuntimeError(json.dumps(errors))
            return next((e for e in events if e.get("key") == key and e.get("status") == state
                and e.get("userdata") == userdata), None)

        token = uuid.uuid4().hex[:8]
        edits = {"fill": "KB-FILL-" + token, "rename": "二、法定代表人身份证明（用户改名 " + token + "）",
            "added": "三、用户新增章 " + token}
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(executable_path=str(Path(args.browser).resolve()),
                headless=True, args=["--no-sandbox"])
            context = browser.new_context(viewport={"width": 1440, "height": 1000})
            session = admin("open", {"source": "initial.docx"})
            page = context.new_page()
            page.goto(session["url"])
            page.wait_for_function("window.probeReady || window.probeErrors.length",
                timeout=args.timeout * 1000)
            errors = page.evaluate("window.probeErrors")
            if errors:
                raise RuntimeError("editor errors: " + json.dumps(errors))
            frame = page.frame_locator('iframe[name="frameEditor"]')
            frame.locator("#id_viewer_overlay").click(position={"x": 350, "y": 200})
            page.evaluate("window.probeChanges=[]")
            # Body order after the page break is heading/blank per chapter, so
            # arrow steps counted from the document end address chapters exactly.
            page.keyboard.press("Control+End")
            page.keyboard.type(edits["fill"], delay=25)
            page.keyboard.press("ArrowUp")
            page.keyboard.press("Home")
            page.keyboard.press("Shift+End")
            page.keyboard.type(edits["rename"], delay=25)
            page.keyboard.press("Control+End")
            for _ in range(3):
                page.keyboard.press("ArrowUp")
            page.keyboard.press("Home")
            page.keyboard.press("Shift+ArrowDown")
            page.keyboard.press("Shift+ArrowDown")
            page.keyboard.press("Delete")
            page.keyboard.press("Control+End")
            page.keyboard.press("Enter")
            page.keyboard.press("Control+Alt+2")
            page.keyboard.type(edits["added"], delay=25)
            page.wait_for_function("window.probeChanges.includes(true)", timeout=args.timeout * 1000)
            page.wait_for_function("window.probeChanges.at(-1) === false", timeout=args.timeout * 1000)
            page.screenshot(path=str(root / "edited.png"), full_page=True)
            correlation = uuid.uuid4().hex
            command = admin("command", {"key": session["key"], "userdata": correlation})
            if command.get("error") != 0:
                raise RuntimeError(json.dumps(command))
            event = wait_until("forcesave callback", lambda: saved(session["key"], 6, correlation))
            page.close()
            saved_bytes = (root / event["file"]).read_bytes()
            report = {"browser": browser.version, "image": args.image, "edits": edits,
                "before": describe(original), "after": describe(saved_bytes)}
            before_marks = {b["name"] for b in report["before"]["bookmarks"]}
            after_marks = {b["name"] for b in report["after"]["bookmarks"]}
            report["verdict"] = {
                "bookmarks_before": sorted(before_marks), "bookmarks_after": sorted(after_marks),
                "survived": sorted(before_marks & after_marks), "lost": sorted(before_marks - after_marks),
                "introduced": sorted(after_marks - before_marks),
                "placement_after": sorted({b["placement"] for b in report["after"]["bookmarks"]}),
                "renamed_chapter_present": any(edits["rename"] in p["text"]
                    for p in report["after"]["paragraphs"]),
                "added_chapter_style": next((p["style_id"] for p in report["after"]["paragraphs"]
                    if edits["added"] in p["text"]), None),
                "deleted_chapter_absent": all("投标函附录" not in p["text"]
                    for p in report["after"]["paragraphs"][8:]),
                "heading_style_names_after": sorted({p["style_name"] for p in report["after"]["paragraphs"]
                    if (p["style_id"] or "").startswith("Heading")}),
            }
            (root / "roundtrip.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
            print(json.dumps(report["verdict"], ensure_ascii=False, indent=2), flush=True)
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
            if docker("inspect", owned["container"], "--format",
                    '{{index .Config.Labels "kb.roundtrip.owner"}}') != owner:
                raise RuntimeError("container ownership changed")
            docker("rm", "-f", "-v", owned["container"])
        if owned["network"]:
            if docker("network", "inspect", owned["network"],
                    "--format", '{{index .Labels "kb.roundtrip.owner"}}') != owner:
                raise RuntimeError("network ownership changed")
            docker("network", "rm", owned["network"])
        for command in [("ps", "-aq"), ("network", "ls", "-q")]:
            if docker(*command, "--filter", "label=kb.roundtrip.owner=" + owner):
                raise RuntimeError("owned resource remains")
        runtime.unlink(missing_ok=True)
        env_file.unlink(missing_ok=True)
        assert hashlib.sha256(source.read_bytes()).hexdigest() == original_hash
        (root / "cleanup.exit").write_text("0\n")
    return status


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    parser.add_argument("--sample", required=True)
    parser.add_argument("--browser", required=True)
    parser.add_argument("--evidence-dir", required=True)
    parser.add_argument("--timeout", type=int, default=180, help="per-operation timeout seconds")
    parser.add_argument("--max-bytes", type=int, default=64 * 1024 * 1024, help="download byte budget")
    sys.exit(run(parser.parse_args()))
