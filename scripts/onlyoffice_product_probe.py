#!/usr/bin/env python3
"""Opt-in real ONLYOFFICE integration through the formal product export queue.

Owned probes use disposable services and synthetic source identities. Explicit
--existing-ticket finalization retains an already published real composition.
Requires Playwright/browser; owned mode also needs cached images and Rust tools.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import ProxyHandler, Request, build_opener
import uuid

from onlyoffice_lifecycle_probe import NoRedirect, docx_text
from onlyoffice_web_probe import product_web


def request(url, value=None, headers=None, raw=None, timeout=30):
    headers = dict(headers or {})
    if value is not None:
        raw = json.dumps(value).encode()
        headers["Content-Type"] = "application/json"
    try:
        with build_opener(ProxyHandler({}), NoRedirect).open(
            Request(url, data=raw, headers=headers), timeout=timeout
        ) as response:
            return response.read()
    except HTTPError as error:
        # Never include a URL containing capabilities or Authorization headers.
        raise RuntimeError(f"HTTP {error.code}: {error.read().decode(errors='replace')}") from None


def wait_until(label, fn, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = fn()
        if result:
            return result
        time.sleep(0.5)
    raise TimeoutError(label)


def toc_entries(data):
    """Inspect native TOC field results in generated-template acceptance only."""
    import io
    import zipfile
    import xml.etree.ElementTree as ET
    ns = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        document = ET.fromstring(archive.read("word/document.xml"))
        styles = ET.fromstring(archive.read("word/styles.xml"))
    styles = {style.get(ns + "styleId"): style for style in styles if style.tag == ns + "style"}
    fields, entries, headings, targets = [], [], [], {}
    for paragraph in document.iter(ns + "p"):
        parts, in_toc = [], False
        for node in paragraph.iter():
            if node.tag == ns + "fldChar":
                kind = node.get(ns + "fldCharType")
                if kind == "begin":
                    fields.append({"instruction": "", "result": False})
                elif kind == "separate" and fields:
                    fields[-1]["result"] = True
                elif kind == "end" and fields:
                    fields.pop()
            elif node.tag == ns + "instrText" and fields:
                fields[-1]["instruction"] += node.text or ""
            if any(f["instruction"].lstrip().startswith("TOC ") and f["result"] for f in fields):
                in_toc = True
                if node.tag == ns + "t":
                    parts.append(node.text or "")
                elif node.tag == ns + "tab":
                    parts.append("\t")
        text = "".join(parts).strip()
        if text:
            title, separator, page = text.rpartition("\t")
            assert separator and title.strip() and page.isdecimal(), "TOC lacks computed page result"
            anchors = [n.get(ns + "anchor") for n in paragraph.iter(ns + "hyperlink")]
            assert len(anchors) == 1 and anchors[0], "TOC entry lacks its heading bookmark"
            entries.append({"title": title.strip(), "page": int(page), "bookmark": anchors[0]})
        if not in_toc:
            style = paragraph.find(ns + "pPr/" + ns + "pStyle")
            level = paragraph.find(ns + "pPr/" + ns + "outlineLvl")
            definition = styles.get(style.get(ns + "val")) if style is not None else None
            if level is None and definition is not None:
                level = definition.find(ns + "pPr/" + ns + "outlineLvl")
            if level is not None and level.get(ns + "val") in tuple(map(str, range(9))):
                heading = "".join(n.text or "" for n in paragraph.iter(ns + "t")).strip()
                headings.append(heading)
                for bookmark in paragraph.iter(ns + "bookmarkStart"):
                    name = bookmark.get(ns + "name")
                    assert name not in targets, "duplicate heading bookmark"
                    targets[name] = heading
    assert entries and [e["title"] for e in entries] == headings, "computed TOC does not match body headings"
    assert all(targets.get(e["bookmark"]) == e["title"] for e in entries), "TOC bookmark targets another heading"
    return entries


def body_text_without_toc(data):
    """Compare body characters across Office saves, excluding computed TOC text.

    This is a text-preservation check, not a table geometry or layout verdict.
    """
    import io
    import zipfile
    import xml.etree.ElementTree as ET
    ns = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        document = ET.fromstring(archive.read("word/document.xml"))
    fields, parts = [], []
    for node in document.iter():
        if node.tag == ns + "fldChar":
            kind = node.get(ns + "fldCharType")
            if kind == "begin":
                fields.append({"instruction": "", "result": False})
            elif kind == "separate" and fields:
                fields[-1]["result"] = True
            elif kind == "end" and fields:
                fields.pop()
        elif node.tag == ns + "instrText" and fields:
            fields[-1]["instruction"] += node.text or ""
        elif node.tag == ns + "t" and not any(
                field["instruction"].lstrip().startswith("TOC ") and field["result"] for field in fields):
            parts.append(node.text or "")
    assert not fields, "unclosed DOCX field"
    return "".join("".join(parts).split())


def verify_attached_identity(ticket, current, docx, download):
    expected = ticket["expected"]
    for key in ("round_id", "version_id", "docx_sha256"):
        assert current[key] == expected[key], "attached saved DOCX identity changed"
    assert current["workspace_id"] == ticket["workspace"]
    assert current["project_id"] == ticket["project_id"]
    assert current["editor"]["pending_save_id"] is None and current["editor"]["save_error"] is None
    for key in ("document_set_id", "document_set_sha256", "requirement_set_id", "requirement_set_sha256"):
        assert current["round_basis"][key] == expected[key], "attached analysis basis changed"
    assert hashlib.sha256(docx).hexdigest() == expected["docx_sha256"]
    base = f"/api/v2/submission-workspaces/{ticket['workspace']}/docx/versions/{expected['version_id']}"
    assert download(base + "/download") == docx, "local sample differs from the actual saved version"
    manifest_bytes = download(base + "/composition-report")
    assert hashlib.sha256(manifest_bytes).hexdigest() == expected["composition_manifest_sha256"]
    manifest = json.loads(manifest_bytes)
    assert manifest["docx_sha256"] == expected["docx_sha256"]
    assert manifest["analysis_sha256"] == expected["analysis_sha256"]
    assert manifest["status"] in ("reviewed_template", "reviewed_template_with_open_items")
    return current


def verify_export_package(package, report, outputs, version, docx):
    source = package["source"]
    assert source["version_id"] == version["version_id"] and source["round_id"] == version["round_id"]
    assert source["docx_sha256"] == version["docx_sha256"]
    assert outputs["docx"] == docx, "formal DOCX differs from its frozen saved source"
    for filetype in ("docx", "pdf"):
        identity = package["outputs"][filetype]
        assert len(outputs[filetype]) == identity["byte_length"]
        assert hashlib.sha256(outputs[filetype]).hexdigest() == identity["sha256"]
    assert outputs["pdf"].startswith(b"%PDF-")
    assert report["schema_version"] == 2 and report["source"] == source
    assert report["outputs"] == package["outputs"]
    assert report["content_sha256"] == package["assessment_report_sha256"]
    assert report["checks"], "empty final checks are not a report"


def run_attached(args):
    """Finalize an already published real composition; never manufacture its basis."""
    ticket = json.loads(args.existing_ticket.read_text())
    endpoint = urlsplit(ticket["origin"])
    if endpoint.scheme not in ("http", "https") or not endpoint.netloc or endpoint.username \
            or endpoint.password or endpoint.path not in ("", "/") or endpoint.query or endpoint.fragment:
        raise ValueError("ticket origin must be an explicit HTTP(S) origin")
    evidence = Path(args.evidence_dir).resolve()
    if not evidence.is_relative_to(Path(tempfile.gettempdir()).resolve()):
        raise ValueError("--evidence-dir must be inside the temporary directory")
    root = evidence / ("attached-" + uuid.uuid4().hex)
    root.mkdir(parents=True, mode=0o700)
    (root / "initial.docx").write_bytes(Path(args.sample).read_bytes())
    (root / "driver.json").write_text(json.dumps({"attached": True, "browser": str(Path(args.browser).resolve()),
        "timeout": args.timeout, "verify_config_expiry": False, "web": True, "pdf": True,
        "update_toc": True, "finalize_only": True}))
    private = root / "browser-ticket.json"
    private.touch(mode=0o600)
    private.write_text(json.dumps(ticket))
    print(root, flush=True)
    try:
        drive(root)
        (root / "run.exit").write_text("0\n")
        return 0
    except Exception:
        (root / "run.exit").write_text("1\n")
        raise
    finally:
        private.unlink(missing_ok=True)


def drive(root):
    from playwright.sync_api import sync_playwright

    settings = json.loads((root / "driver.json").read_text())
    ticket = json.loads((root / "browser-ticket.json").read_text())
    origin = ticket["origin"]
    attached = settings.get("attached", False)
    object_directory = None if attached else Path(ticket["object_directory"])
    if object_directory is not None:
        assert object_directory.resolve().is_relative_to((root / "objects").resolve())
    base = f"/api/v2/submission-workspaces/{ticket['workspace']}/docx"
    timeout = settings["timeout"]
    headers = {"Authorization": "Bearer " + ticket["token"]}

    def api(path, value=None, raw=None, extra=None):
        auth = dict(headers)
        if value is not None or raw is not None:
            auth["Idempotency-Key"] = str(uuid.uuid4())
        auth.update(extra or {})
        return json.loads(request(origin + path, value, auth, raw))

    def identity(version):
        return {key: version[key] for key in ("version_id", "docx_sha256")}

    old = api(base + "/current")
    original = (root / "initial.docx").read_bytes()
    if attached:
        initial = verify_attached_identity(ticket, old, original,
            lambda path: request(origin + path, headers=headers))
        (root / "attached-identity.json").write_text(json.dumps({
            "project_id": old["project_id"], "workspace_id": ticket["workspace"],
            "expected": ticket["expected"], "initial": initial}, ensure_ascii=False, indent=2))
    else:
        metadata = {"basis": old["round_basis"], "expected": identity(old)}
        metadata["basis"] = {key: old["round_basis"][key] for key in (
            "document_set_id", "document_set_sha256", "requirement_set_id", "requirement_set_sha256")}
        boundary = uuid.uuid4().hex
        body = (f'--{boundary}\r\nContent-Disposition: form-data; name="metadata"\r\n\r\n'
            + json.dumps(metadata) + f'\r\n--{boundary}\r\nContent-Disposition: form-data; name="file"\r\n'
            'Content-Type: application/octet-stream\r\n\r\n').encode()
        body += original + f"\r\n--{boundary}--\r\n".encode()
        initial = api(base + "-rounds", raw=body,
            extra={"Content-Type": "multipart/form-data; boundary=" + boundary})
        assert initial["docx_sha256"] == hashlib.sha256(original).hexdigest()
    session = api(base + "/editor", identity(initial))
    key = session["session"]["editor_key"]
    source = session["config"]["document"]["url"]
    try:
        request(source)
    except RuntimeError as error:
        assert str(error).startswith("HTTP 401:"), "unsigned source request failed unexpectedly"
    else:
        raise AssertionError("source scope URL allowed unsigned access")
    receipts, markers = [], []
    rich = None
    if settings.get("rich_edit"):
        from onlyoffice_rich_probe import RichEdits
        rich = RichEdits(root, original)

    def verified_source(url):
        # Replay an actual, still-fresh Document Server source signature. The
        # browser driver never knows the service secret or forges source auth.
        parsed = urlsplit(url)
        uri = parsed.path + "?" + parsed.query
        captures = [json.loads(line) for line in (root / "source-tickets.jsonl").read_text().splitlines()]
        capture = next(item for item in reversed(captures) if item["uri"] == uri)
        return request(url, headers={"Authorization":capture["authorization"]})

    def published(previous, final=False):
        current = api(base + "/current")
        if current["editor"]["save_error"] is not None:
            raise RuntimeError("product save error: " + json.dumps(current["editor"]["save_error"]))
        if current["version_id"] == previous["version_id"]:
            return None
        assert current["round_id"] == initial["round_id"]
        assert current["editor"]["pending_save_id"] is None
        assert current["editor"]["key"] == (None if final else key)
        assert current["editor"]["base_version_id"] == (None if final else initial["version_id"])
        return current

    def verify_file(version, name):
        data = request(origin + base + f"/versions/{version['version_id']}/download", headers=headers)
        (root / name).write_bytes(data)
        assert hashlib.sha256(data).hexdigest() == version["docx_sha256"]
        if object_directory is not None:
            assert (object_directory / version["docx_sha256"]).read_bytes() == data
        assert all(docx_text(data).count(marker) == 1 for marker in markers)
        if rich:
            rich.verify(data)
        return data

    def convert_pdf(opened, version, data, prefix=""):
        if not settings.get("pdf"):
            return None
        before_conversion = api(base + "/current")
        export_base = base.removesuffix("/docx")
        accepted = api(export_base + "/exports", {"version_id": version["version_id"]},
            extra={"If-Match": version["docx_sha256"]})
        (root / (prefix + "export-request.json")).write_text(json.dumps(accepted, indent=2))

        def completed():
            receipt = api(export_base + "/requests/" + accepted["request_artifact_id"])
            if receipt["status"] == "failed":
                raise RuntimeError("formal export failed: " + str(receipt.get("error_code")))
            return receipt["result_identity"] if receipt["status"] == "succeeded" else None

        package = wait_until("formal export worker publication", completed, timeout)
        report = api(export_base + "/exports/" + package["manifest_id"] + "/assessment-report")
        outputs = {}
        for filetype in ("docx", "pdf"):
            output = package["outputs"][filetype]
            outputs[filetype] = request(origin + export_base + "/exports/" + output["artifact_id"]
                + "/download", headers=headers, timeout=timeout)
        verify_export_package(package, report, outputs, version, data)
        if attached:
            for field in ("round_id", "document_set_id", "document_set_sha256", "requirement_set_id", "requirement_set_sha256"):
                assert package["source"][field] == ticket["expected"][field], "export changed real analysis/round identity"
        pdf = outputs["pdf"]
        (root / (prefix + "export-package.json")).write_text(json.dumps(package, ensure_ascii=False, indent=2))
        (root / (prefix + "final-report.json")).write_text(json.dumps(report, ensure_ascii=False, indent=2))
        (root / (prefix + "pdf-source.docx")).write_bytes(data)
        (root / (prefix + "same-version.pdf")).write_bytes(pdf)
        # Reuse the application's Python parser for semantic readback.
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "services"))
        from docreader.main import DocReaderServicer
        from docreader.proto.docreader_pb2 import ReadRequest
        parsed, _ = DocReaderServicer()._parse_request(ReadRequest(
            file_content=pdf, file_name=(prefix + "same-version.pdf"), file_type="pdf"))
        (root / (prefix + "pdf-readback.json")).write_text(parsed.model_dump_json())
        assert parsed.is_valid() and not parsed.metadata.get("table_extraction_error")
        # PDF text extraction can introduce layout whitespace between
        # glyphs. Compare marker characters exactly after whitespace
        # removal; retain the raw parsed text as independent evidence.
        marker_text = "".join(parsed.content.split())
        assert all(marker_text.count(marker) == 1 for marker in markers)
        computed_toc = None
        if settings.get("update_toc"):
            computed_toc = toc_entries(data)
            # Generated templates currently use one section with default
            # page numbering. This does not assert arbitrary DOCX schemes.
            for entry in computed_toc:
                target_text = "".join(unit.text for unit in parsed.structured_source_units
                    if getattr(unit.locator, "page_ordinal", None) == entry["page"] - 1)
                assert "".join(entry["title"].split()) in "".join(target_text.split()), \
                    "TOC target heading is absent from its actual PDF page"
        if not attached:
            assert verified_source(opened["config"]["document"]["url"]) == data
        assert api(base + "/current") == before_conversion
        assert verify_file(version, (prefix + "pdf-source-after.docx")) == data
        pdf_report = {"version_id": version["version_id"], "docx_sha256": version["docx_sha256"],
            "pdf_sha256": hashlib.sha256(pdf).hexdigest(), "pdf_bytes": len(pdf),
            "page_count": parsed.metadata["page_count"], "all_saved_markers_present_once": True if markers else None,
            "marker_comparison": "exact_characters_ignoring_layout_whitespace",
            "computed_toc": computed_toc,
            "toc_page_check": "heading text present on referenced PDF page; independent layout review still required",
            "source_and_version_unchanged": True, "conversion_api_tested": True,
            "product_export_flow_tested": True, "whole_document_layout_accepted": False,
            "manifest_id": package["manifest_id"], "final_report_status": report.get("status"),
            "analysis_identity_preserved": attached}
        (root / (prefix + "pdf-result.json")).write_text(json.dumps(pdf_report, indent=2))
        print("same saved DOCX converted to PDF; version and source bytes unchanged", flush=True)
        return pdf_report

    with product_web(root, settings, ticket) as web, sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=settings["browser"], headless=True,
            args=["--no-sandbox", "--no-proxy-server"])
        try:
            context = browser.new_context(viewport={"width": 1440, "height": 1000})

            def open_page(opened, retry_navigation=True):
                page = context.new_page()
                network_changed, editor_requests = [], []
                def event(kind, value):
                    redacted = re.sub(r"[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+", "[REDACTED-JWT]", value)
                    with (root / "browser-events.jsonl").open("a") as log:
                        log.write(json.dumps({"kind": kind, "value": redacted}) + "\n")
                page.on("pageerror", lambda error: event("pageerror", str(error)))
                page.on("console", lambda message: event("console-error", message.text) if message.type == "error" else None)
                def request_failed(req):
                    event("requestfailed", urlsplit(req.url).path + " " + str(req.failure))
                    if req.failure == "net::ERR_NETWORK_CHANGED":
                        network_changed.append(True)
                page.on("requestfailed", request_failed)
                page.on("request", lambda req: editor_requests.append(True)
                    if urlsplit(req.url).path.endswith("/docx/editor") else None)
                page.on("response", lambda response: event("http-error", str(response.status) + " " + urlsplit(response.url).path)
                    if response.status >= 400 else None)
                if web:
                    try:
                        web.open(page, opened)
                    except Exception:
                        page.screenshot(path=str(root / "open-failed.png"))
                        if retry_navigation and network_changed and not editor_requests:
                            event("navigation-retry", "network changed before any editor request; one fresh page")
                            page.close()
                            return open_page(opened, retry_navigation=False)
                        raise
                    return page
                # Only the local browser's harness page is fulfilled. All editor,
                # source, command and callback traffic uses real network routes.
                html = ('<!doctype html><html><body style="margin:0;height:100vh"><div id="editor"></div>'
                    '<script src="' + opened["api_script_url"] + '"></script><script>'
                    'window.probeReady=false;window.probeChanges=[];window.probeErrors=[];let cfg='
                    + json.dumps(opened["config"]) + ';cfg.events={'
                    'onDocumentReady:()=>window.probeReady=true,'
                    'onDocumentStateChange:e=>window.probeChanges.push(e.data),'
                    'onError:e=>window.probeErrors.push(e.data)};'
                    'window.docEditor=new DocsAPI.DocEditor("editor",cfg);</script></body></html>')
                # The harness must also be loopback: an insecure bridge-origin
                # page cannot load a more-private loopback script in Chromium.
                # This does not disable browser security or rewrite signed URLs.
                script_origin = urlsplit(opened["api_script_url"])
                url = f"{script_origin.scheme}://{script_origin.netloc}/native-harness-" + uuid.uuid4().hex
                page.route(url, lambda route: route.fulfill(content_type="text/html", body=html))
                page.goto(url)
                try:
                    page.wait_for_function("window.probeReady || window.probeErrors.length", timeout=timeout * 1000)
                except Exception:
                    page.screenshot(path=str(root / "open-failed.png"))
                    raise
                errors = page.evaluate("window.probeErrors")
                assert not errors, errors
                return page

            def insert(page):
                marker = "KB-NATIVE-" + uuid.uuid4().hex
                markers.append(marker)
                if not web:
                    page.evaluate("window.probeChanges=[]")
                page.frame_locator('iframe[name="frameEditor"]').locator("#id_viewer_overlay").click(
                    position={"x": 350, "y": 200})
                page.keyboard.press("Escape")
                page.keyboard.press("Control+Home")
                page.keyboard.type(marker, delay=30)
                page.keyboard.press("Enter")
                if web:
                    web.edited(page)
                else:
                    page.wait_for_function("window.probeChanges.includes(true)", timeout=timeout * 1000)
                    page.wait_for_function("window.probeChanges.at(-1) === false", timeout=timeout * 1000)

            def update_toc(page):
                if not settings.get("update_toc"):
                    return
                frame = page.frame_locator('iframe[name="frameEditor"]')
                try:
                    frame.locator('a[data-tab="links"]').click()
                    frame.locator('#slot-btn-contents-update button:not(.dropdown-toggle)').click()
                    web.edited(page)
                except Exception:
                    page.screenshot(path=str(root / "toc-update-failed.png"))
                    raise

            page = open_page(session)
            if not attached:
                assert verified_source(source) == original
            if settings["verify_config_expiry"]:
                wait_until("editor configuration expiry", lambda: time.time() > session["config"]["exp"] + 1, timeout)
                assert verified_source(source) == original
                if not web:
                    assert not page.evaluate("window.probeErrors")
                print("opening configuration expired; active source remains authenticated", flush=True)
            current = initial
            if settings.get("finalize_only"):
                # Final deliverables must contain no probe edits. Use only the
                # normal TOC action, correlated product save and persisted file.
                update_toc(page)
                with page.expect_response(lambda response: response.url.endswith(base + "/editor/save")
                        and response.request.method == "POST", timeout=timeout * 1000) as response:
                    page.get_by_test_id("docx-save").click()
                assert response.value.ok
                command = response.value.json()
                assert command["dispatch"] is True
                current = wait_until("clean forcesave publication", lambda: published(initial), timeout)
                web.saved(page, current)
                saved_data = verify_file(current, "finalized-forcesave.docx")
                assert body_text_without_toc(saved_data) == body_text_without_toc(original), \
                    "Office finalization changed body characters outside the TOC"
                toc_entries(saved_data)
                page.close()

                def clean_closed():
                    closed = api(base + "/current")
                    assert closed["round_id"] == initial["round_id"]
                    assert closed["editor"]["save_error"] is None
                    if closed["editor"]["key"] is not None or closed["editor"]["pending_save_id"] is not None:
                        return None
                    return closed

                final = wait_until("clean editor close", clean_closed, timeout)
                final_data = verify_file(final, "finalized.docx")
                assert body_text_without_toc(final_data) == body_text_without_toc(original)
                reopened = api(base + "/editor", identity(final))
                assert reopened["session"]["editor_key"] != key
                page = open_page(reopened)
                if not attached:
                    assert verified_source(reopened["config"]["document"]["url"]) == final_data
                pdf_report = convert_pdf(reopened, final, final_data, "finalized-")
                page.screenshot(path=str(root / "finalized-reopened.png"))
                page.close()
                # A no-edit close can send status 4; the product intentionally
                # retains its reusable editor key. Do not wait for a fictitious
                # new version or key rotation to establish saved-file identity.
                closed = api(base + "/current")
                assert closed["editor"]["save_error"] is None
                assert closed["editor"]["pending_save_id"] is None
                assert identity(closed) == identity(final)
                assert verify_file(closed, "finalized-after-reopen.docx") == final_data
                report = {"mode": "finalize_only", "browser": browser.version,
                    "initial": initial, "save_id": command["save_id"], "final": final,
                    "body_characters_preserved_excluding_toc_and_whitespace": True,
                    "probe_edits_inserted": False, "native_toc_saved_and_reopened": True,
                    "same_version_pdf": pdf_report, "product_router_tcp": True,
                    "product_frontend_tested": True, "native_document_server": True,
                    "edit_and_callback_fault_suite_tested": False,
                    "real_tender_semantic_acceptance": False,
                    "analysis_identity_preserved": attached}
                (root / "result.json").write_text(json.dumps(report, indent=2))
                print("clean DOCX and same-version PDF finalized without probe edits", flush=True)
                return
            for number in range(2):
                insert(page)
                if rich and number == 0:
                    rich.edit(page, web)
                update_toc(page)
                if web:
                    with page.expect_response(lambda response: response.url.endswith(base + "/editor/save")
                            and response.request.method == "POST", timeout=timeout * 1000) as response:
                        page.get_by_test_id("docx-save").click()
                    assert response.value.ok
                    command = response.value.json()
                else:
                    command = api(base + "/editor/save", {"editor_key": key, "expected": identity(current)})
                assert command["dispatch"] is True
                current = wait_until("product forcesave publication", lambda: published(current), timeout)
                if web:
                    web.saved(page, current)
                    with page.expect_download() as download:
                        page.get_by_role("button", name="下载已保存稿", exact=True).click()
                    assert hashlib.sha256(Path(download.value.path()).read_bytes()).hexdigest() == current["docx_sha256"]
                saved_data = verify_file(current, f"forcesave-{number + 1}.docx")
                if settings.get("update_toc"):
                    (root / f"toc-forcesave-{number + 1}.json").write_text(
                        json.dumps(toc_entries(saved_data), ensure_ascii=False, indent=2))
                assert verified_source(source) == original, "forcesave changed the opening baseline"
                receipts.append({"save_id": command["save_id"], "version": current})
                print(f"product forcesave {number + 1} persisted", flush=True)
            insert(page)
            update_toc(page)
            page.close()
            final = wait_until("product final publication", lambda: published(current, True), timeout)
            final_data = verify_file(final, "final.docx")
            assert markers[-1] not in docx_text((root / "forcesave-2.docx").read_bytes())
            reopened = api(base + "/editor", identity(final))
            assert reopened["session"]["editor_key"] != key
            page = open_page(reopened)
            if web:
                web.close_reopen(page)
            assert verified_source(reopened["config"]["document"]["url"]) == final_data
            pdf_report = convert_pdf(reopened, final, final_data)
            page.screenshot(path=str(root / "reopened.png"))
            # A further edit/save through the reopened editor proves the service
            # actually loaded the persisted final bytes (all previous marks stay).
            key = reopened["session"]["editor_key"]
            insert(page)
            update_toc(page)
            page.close()
            reopened_final = wait_until("reopened final publication", lambda: published(final, True), timeout)
            verify_file(reopened_final, "reopened-final.docx")
            if web:
                web.rounds(context, api, root)
                # These are application status-read faults, independent of the
                # stopped-server callback replay below. Open their pages before
                # Docker changes host interfaces by stopping Document Server.
                web.faults(context, session)
            # Historical artifacts remain exact after later publication.
            for number, receipt in enumerate(receipts, 1):
                history = request(origin + base + f"/versions/{receipt['version']['version_id']}/download", headers=headers)
                assert history == (root / f"forcesave-{number}.docx").read_bytes()
            captures = {}
            for line in (root / "callback-tickets.jsonl").read_text().splitlines():
                capture = json.loads(line)
                body = capture["body"]
                captures[(body["key"], body["status"], body.get("userdata"))] = capture
            assert len(captures) == 4, "expected two forcesaves and two final saves"
            owned = json.loads((root / "owned.json").read_text())
            ds = owned["documentserver"]
            owner = subprocess.check_output(["docker", "inspect", ds, "--format",
                '{{index .Config.Labels "kb.docx.native.owner"}}'], text=True).strip()
            assert owner == owned["owner"], "Document Server stop ownership mismatch"
            current_before_replay = api(base + "/current")
            files_before_replay = {p.name: (hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_mtime_ns)
                for p in object_directory.iterdir()}
            # End editing first, then stop only this run's owned service. Its
            # cache is now physically unreachable; no download proxy is used.
            subprocess.run(["docker", "stop", ds], check=True, stdout=subprocess.DEVNULL)
            script_origin = urlsplit(session["api_script_url"])
            try:
                request(f"{script_origin.scheme}://{script_origin.netloc}/healthcheck", timeout=5)
            except (URLError, OSError, RuntimeError):
                pass
            else:
                raise AssertionError("stopped Document Server is still reachable")
            for capture in captures.values():
                result = json.loads(request(origin + capture["uri"], capture["body"],
                    {"Authorization": capture["authorization"]}))
                assert result == {"error": 0}
            assert api(base + "/current") == current_before_replay
            assert {p.name: (hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_mtime_ns)
                for p in object_directory.iterdir()} == files_before_replay
            report = {"browser": browser.version, "initial": initial, "forcesaves": receipts,
                "final": final, "reopened_final": reopened_final, "markers": markers,
                "product_router_tcp": True, "native_document_server": True,
                "authentic_callback_redeliveries_with_server_stopped": len(captures),
                "saved_after_opening_config_expired": settings["verify_config_expiry"],
                "unsigned_source_rejected": True,
                "product_frontend_tested": bool(web), "app_auth_bootstrap_tested": bool(web),
                "credential_login_tested": False,
                "rich_edit_tested": bool(rich),
                "same_version_pdf": pdf_report,
                "api_executable_startup_tested": False}
            (root / "result.json").write_text(json.dumps(report, indent=2))
            print("product final save, reopen, history and four offline callback replays verified", flush=True)
        finally:
            browser.close()


def run_probe(args):
    repo = Path(__file__).resolve().parents[1]
    evidence = Path(args.evidence_dir).resolve()
    if not evidence.is_relative_to(Path(tempfile.gettempdir()).resolve()):
        raise ValueError("--evidence-dir must be inside the system temporary directory (native test isolation contract)")
    evidence.mkdir(parents=True, exist_ok=True)
    root = evidence / ("run-" + uuid.uuid4().hex)
    root.mkdir(mode=0o700)
    source = Path(args.sample).resolve()
    sample = source.read_bytes()
    docx_text(sample)
    original_hash = hashlib.sha256(sample).hexdigest()
    (root / "initial.docx").write_bytes(sample)
    (root / "source.json").write_text(json.dumps({"path": str(source), "sha256": original_hash}))
    (root / "objects").mkdir()
    for name in ("shared_platform_baseline.sql", "knowledge_base_baseline.sql", "bidding_v2_baseline.sql"):
        shutil.copyfile(repo / "migrations" / name, root / name)
    fixture = (repo / "crates/bidding/tests/sql/document_collection_acceptance.sql").read_text()
    assert fixture.count("\nROLLBACK;") == 1
    (root / "document_collection_acceptance.sql").write_text(fixture)
    for relative in ("scripts/onlyoffice_product_probe.py", "scripts/onlyoffice_lifecycle_probe.py",
        "scripts/onlyoffice_web_probe.py", "scripts/onlyoffice_rich_probe.py", "web/index.html", "web/src/main.tsx", "web/src/App.tsx",
        "web/src/bid/Workbench.tsx", "web/src/bid/authoring/DocxEditor.tsx",
        "web/src/bid/authoring/docxSession.ts", "web/src/bid/api/docx.ts",
        "web/src/bid/authoring/DocxRound.tsx", "web/src/bid/authoring/docxRoundSession.ts",
        "web/src/hash.ts", "web/src/Shell.tsx",
        "crates/api/tests/docx_native.rs", "crates/api/src/bid_v2_routes/docx/editor.rs"):
        destination = root / "source" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(repo / relative, destination)
    owner = uuid.uuid4().hex
    label_name = "kb.docx.native.owner"
    label = label_name + "=" + owner
    owned = {"owner": owner, "network": None, "containers": []}
    status = 1
    env_file = root / "runtime.env"
    env_file.touch(mode=0o600)
    secret = secrets.token_hex(32)
    env_file.write_text("JWT_ENABLED=true\nJWT_SECRET=" + secret + "\nALLOW_PRIVATE_IP_ADDRESS=true\n")

    def run(command, data=None):
        result = subprocess.run(command, input=data, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        with (root / "run.log").open("a") as log:
            log.write(result.stdout)
        if result.returncode:
            raise RuntimeError(f"{command[:2]} failed ({result.returncode}); see run.log")
        return result.stdout.strip()

    def docker(*command):
        return run(["docker", *command])

    def record():
        (root / "owned.json").write_text(json.dumps(owned, indent=2))

    def container(image, *options):
        identity = json.loads(docker("image", "inspect", image))[0]
        cid = docker("create", "--pull=never", "--label", label, "--network", owned["network"],
            *options, identity["Id"])
        owned["containers"].append(cid)
        record()
        docker("start", cid)
        return cid

    def sql(text):
        return run(["docker", "exec", "-i", pg, "psql", "-X", "-U", "postgres", "-d", database,
            "-v", "ON_ERROR_STOP=1"], text)

    print(root, flush=True)
    try:
        assert "@sha256:" in args.image, "digest-pinned Document Server image required"
        image = json.loads(docker("image", "inspect", args.image))[0]
        assert args.image in image["RepoDigests"]
        (root / "image.json").write_text(json.dumps({"image": args.image, "id": image["Id"]}))
        owned["network"] = docker("network", "create", "--label", label, "kb-docx-native-" + owner)
        record()
        database = "knowledgebrain_test_" + owner
        pg = container(args.postgres_image, "-p", "127.0.0.1::5432", "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
            "-e", "POSTGRES_DB=" + database)
        redis = container(args.redis_image, "-p", "127.0.0.1::6379")
        ds = container(args.image, "-p", "127.0.0.1::80", "--cpus", "2", "--memory", "4g",
            "--shm-size", "256m", "--restart", "no", "--env-file", str(env_file))
        owned["documentserver"] = ds
        record()
        networks = json.loads(docker("inspect", ds, "--format", "{{json .NetworkSettings.Networks}}"))
        assert len(networks) == 1
        gateway = next(iter(networks.values()))["Gateway"]
        wait_until("Postgres readiness", lambda: subprocess.run(["docker", "exec", pg, "pg_isready",
            "-h", "127.0.0.1", "-U", "postgres", "-d", database],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0, args.timeout)
        pg_port = docker("port", pg, "5432/tcp").rsplit(":", 1)[1]
        redis_port = docker("port", redis, "6379/tcp").rsplit(":", 1)[1]
        ds_origin = "http://127.0.0.1:" + docker("port", ds, "80/tcp").rsplit(":", 1)[1]
        sql('''CREATE ROLE kb_app_owner NOLOGIN;
CREATE ROLE kb_migrator LOGIN NOINHERIT;
CREATE ROLE kb_runtime_api LOGIN INHERIT;
CREATE ROLE kb_runtime_worker LOGIN INHERIT;
CREATE ROLE kb_runtime_retention LOGIN INHERIT;
GRANT kb_app_owner TO kb_migrator;
CREATE EXTENSION pgcrypto;
CREATE EXTENSION vector;
ALTER SCHEMA public OWNER TO kb_app_owner;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
''' + f'REVOKE TEMPORARY,CREATE ON DATABASE "{database}" FROM PUBLIC;')

        def health():
            try:
                return request(ds_origin + "/healthcheck", timeout=5) == b"true"
            except (URLError, OSError, RuntimeError):
                return False

        wait_until("Document Server readiness", health, args.timeout)
        print("isolated services ready", flush=True)
        with socket.socket() as listener:
            listener.bind((gateway, 0))
            bind = f"{gateway}:{listener.getsockname()[1]}"
        env = os.environ.copy()
        for name in list(env):
            if name.startswith(("KNOWLEDGEBRAIN_S3_", "MINIO_")):
                del env[name]
        cargo = Path(args.cargo).resolve()
        env.update(RUSTC=str(cargo.with_name("rustc")), RUSTDOC=str(cargo.with_name("rustdoc")),
            DATABASE_URL=f"postgresql://kb_runtime_api@127.0.0.1:{pg_port}/{database}",
            KNOWLEDGEBRAIN_TEST_DATABASE_URL=f"postgresql://postgres@127.0.0.1:{pg_port}/{database}",
            REDIS_URL=f"redis://127.0.0.1:{redis_port}", KB_DEPLOYMENT_NAMESPACE_ID=str(uuid.uuid4()),
            OBJECT_DIR=str(root / "objects"), KB_DOCX_NATIVE_BIND=bind, KB_DOCX_NATIVE_RUN_DIR=str(root),
            KB_DOCX_NATIVE_PYTHON=sys.executable, KB_DOCX_NATIVE_DRIVER=str(Path(__file__).resolve()),
            KB_ONLYOFFICE_SERVER_ORIGIN=ds_origin, KB_ONLYOFFICE_API_ORIGIN="http://" + bind,
            KB_ONLYOFFICE_JWT_SECRET=secret, KB_ONLYOFFICE_CAPABILITY_SECRET=secrets.token_hex(32),
            KB_ONLYOFFICE_TOKEN_TTL_SECONDS=str(args.config_ttl or args.timeout * 10), KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS=str(args.timeout))
        (root / "driver.json").write_text(json.dumps({"browser": str(Path(args.browser).resolve()), "timeout": args.timeout,
            "verify_config_expiry": args.config_ttl is not None, "web": args.web, "rich_edit": args.rich_edit,
            "pdf": args.pdf, "update_toc": args.update_toc, "finalize_only": args.finalize_only}))
        if args.pdf:
            with (root / "worker-build.log").open("w") as log:
                subprocess.run([str(cargo), "build", "--locked", "--offline", "-p", "worker", "--bin", "worker"],
                    cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            target = Path(env.get("CARGO_TARGET_DIR", repo / "target"))
            if not target.is_absolute():
                target = repo / target
            env["KB_DOCX_NATIVE_WORKER_BIN"] = str((target / "debug/worker").resolve())
            env["KB_DOCX_NATIVE_WORKER_URL"] = f"postgresql://kb_runtime_worker@127.0.0.1:{pg_port}/{database}"
        with (root / "native-test.log").open("w") as log:
            # New process group lets timeout cleanup also stop the browser driver.
            process = subprocess.Popen([str(cargo), "test", "--locked", "--offline", "-p", "api",
                "--features", "docx-native-tests", "--test", "docx_native", "--", "--nocapture"],
                cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = process.wait(timeout=args.timeout * 12)
            finally:
                import signal
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        (root / "native-test.exit").write_text(str(code) + "\n")
        if code:
            raise RuntimeError("native product test failed; see native-test.log")
        status = 0
    finally:
        (root / "run.exit").write_text(str(status) + "\n")
        cleanup_errors = []
        for cid in reversed(owned["containers"]):
            try:
                assert docker("inspect", cid, "--format", '{{index .Config.Labels "' + label_name + '"}}') == owner
                docker("rm", "-f", "-v", cid)
            except Exception as error:
                cleanup_errors.append(str(error))
        if owned["network"]:
            try:
                assert docker("network", "inspect", owned["network"], "--format", '{{index .Labels "' + label_name + '"}}') == owner
                docker("network", "rm", owned["network"])
            except Exception as error:
                cleanup_errors.append(str(error))
        env_file.unlink(missing_ok=True)
        (root / "browser-ticket.json").unlink(missing_ok=True)
        (root / "callback-tickets.jsonl").unlink(missing_ok=True)
        (root / "source-tickets.jsonl").unlink(missing_ok=True)
        shutil.rmtree(root / "objects")
        for command in (("ps", "-aq"), ("network", "ls", "-q")):
            if docker(*command, "--filter", "label=" + label):
                cleanup_errors.append("owned resource remains")
        assert hashlib.sha256(source.read_bytes()).hexdigest() == original_hash
        (root / "cleanup.json").write_text(json.dumps({"errors": cleanup_errors}))
        (root / "cleanup.exit").write_text("1\n" if cleanup_errors else "0\n")
        if cleanup_errors:
            raise RuntimeError("; ".join(cleanup_errors))
    return status


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "drive":
        drive(Path(sys.argv[2]))
    else:
        parser = argparse.ArgumentParser(description=__doc__)
        for option in ("sample", "browser", "evidence-dir"):
            parser.add_argument("--" + option, required=True)
        for option in ("image", "postgres-image", "redis-image", "cargo"):
            parser.add_argument("--" + option)
        parser.add_argument("--existing-ticket", type=Path, help="private authenticated real-product identity ticket; requires --finalize-only and never imports a fixture")
        parser.add_argument("--timeout", type=int, default=180, help="per-operation test timeout seconds")
        parser.add_argument("--config-ttl", type=int, help="test opening-config TTL; when set, wait for expiry before editing")
        parser.add_argument("--web", action="store_true", help="exercise the actual React Workbench through an owned Vite server")
        parser.add_argument("--rich-edit", action="store_true", help="edit a real table and image through the product UI (requires --web)")
        parser.add_argument("--pdf", action="store_true", help="run formal queued DOCX/PDF/report export and read PDF back through DocReader")
        parser.add_argument("--update-toc", action="store_true", help="update a generated template's native TOC in the editor and check its PDF target pages (requires --web --pdf)")
        parser.add_argument("--finalize-only", action="store_true", help="save a clean TOC-updated DOCX and same-version PDF without probe edits; does not run the edit/fault suite")
        arguments = parser.parse_args()
        if arguments.timeout <= 0:
            parser.error("--timeout must be positive")
        if arguments.rich_edit and not arguments.web:
            parser.error("--rich-edit requires --web")
        if arguments.update_toc and not (arguments.web and arguments.pdf):
            parser.error("--update-toc requires --web and --pdf")
        if arguments.finalize_only and not (arguments.web and arguments.pdf and arguments.update_toc):
            parser.error("--finalize-only requires --web --pdf --update-toc")
        if arguments.finalize_only and (arguments.rich_edit or arguments.config_ttl is not None):
            parser.error("--finalize-only cannot be combined with rich edits or configuration-expiry testing")
        if arguments.config_ttl is not None and not 0 < arguments.config_ttl < arguments.timeout:
            parser.error("--config-ttl must be positive and below --timeout")
        if arguments.existing_ticket:
            if not arguments.finalize_only:
                parser.error("--existing-ticket requires --finalize-only")
            sys.exit(run_attached(arguments))
        if not all((arguments.image, arguments.postgres_image, arguments.redis_image, arguments.cargo)):
            parser.error("owned Office probes require --image --postgres-image --redis-image --cargo")
        sys.exit(run_probe(arguments))
