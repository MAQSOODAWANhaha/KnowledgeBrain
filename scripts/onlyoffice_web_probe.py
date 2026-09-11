"""Actual React Workbench driver used by onlyoffice_product_probe --web."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
import time


@contextmanager
def product_web(root, settings, ticket):
    if not settings.get("web"):
        yield None
        return
    repo = Path(__file__).resolve().parents[1]
    address = root / "web-origin.json"
    address.unlink(missing_ok=True)
    env = dict(os.environ, KB_LIVE_API_URL=ticket["origin"], KB_PROBE_ADDRESS=str(address))
    code = """
import { createServer } from 'vite';
import { writeFileSync } from 'node:fs';
const server = await createServer({server: {host:'127.0.0.1',hmr:false,open:false}});
await new Promise(resolve => server.httpServer.listen(0, '127.0.0.1', resolve));
writeFileSync(process.env.KB_PROBE_ADDRESS, JSON.stringify('http://127.0.0.1:' + server.httpServer.address().port));
async function stop() { await server.close(); process.exit(0); }
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
"""
    with (root / "vite.log").open("w") as log:
        process = subprocess.Popen(["node", "--input-type=module", "-e", code],
            cwd=repo / "web", env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + settings["timeout"]
            while not address.exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("probe Vite startup failed; see vite.log")
                time.sleep(0.1)
            yield ProductWeb(json.loads(address.read_text()).rstrip("/"), ticket, settings["timeout"])
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


class ProductWeb:
    def __init__(self, origin, ticket, timeout):
        self.origin, self.ticket, self.timeout = origin, ticket, timeout * 1000

    def status(self, page, text):
        from playwright.sync_api import expect
        expect(page.get_by_test_id("docx-status")).to_have_text(text, timeout=self.timeout)

    def open(self, page, opened):
        page.add_init_script("if(location.origin === " + json.dumps(self.origin)
            + ") localStorage.setItem('kb.token', " + json.dumps(self.ticket["token"]) + ");")
        with page.expect_response(lambda response: response.url.endswith('/api/v1/me') and response.ok) as profile, \
                page.expect_response(lambda response: response.url.endswith('/docx/editor') and response.ok) as configured:
            page.goto(self.origin + "/#/bids/" + opened["session"]["project_id"] + "/authoring")
        account = profile.value.json()
        config = configured.value.json()["config"]["editorConfig"]
        assert config["user"] == {"id": "user:" + account["id"], "name": account["email"]}
        assert config["lang"] == page.locator("html").get_attribute("lang")
        self.status(page, "当前保存版本已载入")
        from playwright.sync_api import expect
        expect(page.locator(".acct em")).to_have_text(account["email"])
        # The profile supplied by the product removes the SDK's name prompt.
        assert page.frame_locator('iframe[name="frameEditor"]').get_by_text(
            "Enter a name to be used for collaboration", exact=True).count() == 0
        assert page.get_by_test_id("document-canvas").count() == 0

    def edited(self, page):
        self.status(page, "有修改尚未保存")
        assert page.get_by_test_id("docx-close").is_disabled()
        # Exercise both wizard navigation and links while work is unconfirmed.
        original = page.url
        page.get_by_test_id("wizard-export").click()
        page.locator('a[href="#/library"]').click()
        assert page.url == original
        assert page.get_by_test_id("docx-editor").count() == 1
        page.evaluate("location.hash = '#/library'")
        page.wait_for_function("url => location.href === url", arg=original)
        assert page.get_by_test_id("docx-editor").count() == 1

    def saved(self, page, version):
        from playwright.sync_api import expect
        self.status(page, "已保存")
        expect(page.get_by_test_id("docx-version")).to_have_text("保存版本 " + str(version["revision"]))

    def close_reopen(self, page):
        page.get_by_test_id("docx-close").click()
        self.status(page, "编辑器已关闭")
        assert page.locator('iframe[name="frameEditor"]').count() == 0
        page.get_by_test_id("docx-reopen").click()
        self.status(page, "当前保存版本已载入")

    def faults(self, context, opened):
        # Fault injection is limited to status reads; this is separate from the
        # real Document Server persistence checks.
        from playwright.sync_api import expect
        for status, code in ((403, "FORBIDDEN"), (404, "NOT_FOUND"), (503, "SERVICE_UNAVAILABLE")):
            page = context.new_page()
            page.add_init_script("if(location.origin === " + json.dumps(self.origin)
                + ") localStorage.setItem('kb.token', " + json.dumps(self.ticket["token"]) + ");")
            seen = []
            page.on("request", lambda request: seen.append(request.url.split("?")[0]))
            page.route("**/docx/current", lambda route: route.fulfill(status=status,
                content_type="application/json", body=json.dumps({"error":{"code":code,"message":code}})))
            page.goto(self.origin + "/#/bids/" + opened["session"]["project_id"] + "/authoring")
            expect(page.get_by_role("alert")).to_contain_text("无法读取当前稿件", timeout=self.timeout)
            assert page.get_by_test_id("docx-editor").count() == 0
            assert page.get_by_test_id("document-canvas").count() == 0
            assert not any(url.endswith("/editor") for url in seen)
            page.close()

    def rounds(self, context, api, root):
        """Publish through actual App controls in a separate owned project.

        Drop only the first committed HTTP response to exercise same-request
        recovery. Bytes, basis, publication and both editor opens stay real.
        """
        import hashlib
        import uuid
        from playwright.sync_api import expect

        project = api("/api/v2/bid-projects", {"title": "DOCX round UI " + uuid.uuid4().hex})
        base = "/api/v2/submission-workspaces/" + project["workspace_id"] + "/docx"
        page = context.new_page()
        page.add_init_script("if(location.origin === " + json.dumps(self.origin)
            + ") localStorage.setItem('kb.token', " + json.dumps(self.ticket["token"]) + ");")
        # Chromium interception can omit uploaded file bytes when reconstructing
        # multipart requests. Drop the response inside fetch after the browser's
        # original request completed, preserving the native upload transport.
        page.add_init_script("""
window.__roundRequests = [];
const originalFetch = window.fetch;
window.fetch = async function(input, init) {
    const path = new URL(String(input), location.href).pathname;
    if (init?.method !== 'POST' || !path.endsWith('/docx-rounds'))
        return originalFetch.call(this, input, init);
    const file = init.body.get('file');
    const digest = await crypto.subtle.digest('SHA-256', await file.arrayBuffer());
    window.__roundRequests.push({key: new Headers(init.headers).get('Idempotency-Key'),
        metadata: init.body.get('metadata'),
        sha256: Array.from(new Uint8Array(digest), x => x.toString(16).padStart(2, '0')).join('')});
    const response = await originalFetch.call(this, input, init);
    if (window.__roundRequests.length === 1 && response.ok) {
        await response.arrayBuffer();
        throw new TypeError('probe: committed response lost');
    }
    return response;
};
""")

        def closed():
            current = api(base + "/current")
            return current if not current["editor"]["pending_save_id"] else None

        def wait_closed():
            deadline = time.monotonic() + self.timeout / 1000
            while time.monotonic() < deadline:
                if current := closed():
                    return current
                page.wait_for_timeout(250)
            raise TimeoutError("new-round predecessor save is still pending")

        try:
            page.goto(self.origin + "/#/bids/" + project["id"] + "/authoring")
            # A project without a saved DOCX opens the first-round panel itself.
            # The explicit new-round button belongs to an existing saved draft.
            page.get_by_role("button", name="上传已有 DOCX", exact=True).click()
            panel = page.get_by_test_id("docx-round")
            expect(panel).to_contain_text("本次将创建首份 DOCX 投标稿", timeout=self.timeout)
            panel.locator('input[type="file"]').set_input_files(str(root / "initial.docx"))
            panel.get_by_role("button", name="确认创建新轮", exact=True).click()
            retry = panel.get_by_role("button", name="确认发布结果", exact=True)
            expect(retry).to_be_enabled(timeout=self.timeout)
            expect(panel.get_by_role("button", name="取消", exact=True)).to_be_disabled()
            expect(panel.locator('input[type="file"]')).to_be_disabled()
            original_url = page.url
            page.evaluate("location.hash = '#/library'")
            page.wait_for_function("url => location.href === url", arg=original_url)
            retry.click()
            expect(panel).to_contain_text("第 1 轮已发布", timeout=self.timeout)
            payloads = page.evaluate("window.__roundRequests")
            assert len(payloads) == 2 and payloads[0] == payloads[1]
            assert json.loads(payloads[0]["metadata"])["expected"] is None
            assert payloads[0]["sha256"] == hashlib.sha256((root / "initial.docx").read_bytes()).hexdigest()
            first = api(base + "/current")
            assert first["docx_sha256"] == hashlib.sha256((root / "initial.docx").read_bytes()).hexdigest()
            panel.get_by_role("button", name="进入当前稿件", exact=True).click()
            self.status(page, "当前保存版本已载入")
            first_key = api(base + "/current")["editor"]["key"]
            page.get_by_test_id("docx-close").click()
            self.status(page, "编辑器已关闭")
            wait_closed()
            page.get_by_role("button", name="生成或导入新轮", exact=True).click()
            page.get_by_role("button", name="上传已有 DOCX", exact=True).click()
            expect(panel).to_contain_text("本次将替换当前编制轮次", timeout=self.timeout)
            panel.locator('input[type="file"]').set_input_files(str(root / "initial.docx"))
            panel.get_by_role("button", name="确认创建新轮", exact=True).click()
            expect(panel).to_contain_text("第 2 轮已发布", timeout=self.timeout)
            second = api(base + "/current")
            assert second["round_id"] != first["round_id"]
            assert second["editor"]["key"] is None
            panel.get_by_role("button", name="进入当前稿件", exact=True).click()
            self.status(page, "当前保存版本已载入")
            assert api(base + "/current")["editor"]["key"] != first_key
            page.get_by_test_id("docx-close").click()
            wait_closed()
            page.screenshot(path=str(root / "round-ui.png"))
            (root / "round-ui.json").write_text(json.dumps({
                "first": first, "second": second, "exact_uncertain_retry": True,
                "first_current_was_null": True, "real_editor_open_close": True,
                "new_editor_key": True, "navigation_guard": True}, indent=2))
            print("product first/new round, exact uncertain retry and fresh editor key verified", flush=True)
        except Exception:
            page.screenshot(path=str(root / "round-ui-failed.png"))
            raise
        finally:
            page.close()
