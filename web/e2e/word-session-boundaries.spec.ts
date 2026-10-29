import { expect, test } from "@playwright/test";
import { fixture, type WordTestWindow } from "./tender.fixture";

test("close locks iframe keyboard input and late document events retain a recoverable Word", async ({ page }) => {
  const f = await fixture(page); await f.ready(); await f.edit();
  await expect.poll(() => page.frames().some(frame => frame.url().endsWith("/__office/frame"))).toBe(true);
  const office = page.frames().find(frame => frame.url().endsWith("/__office/frame"))!;
  await office.evaluate(() => {
    const input = document.createElement("textarea"); input.id = "synthetic-input";
    input.value = "original"; document.body.append(input); input.focus();
  });
  let release!: () => void;
  const response = new Promise<void>(resolve => { release = resolve; });
  let closing = false;
  await page.route("**/docx/editor/*/close", async route => { closing = true; await response; await route.fallback(); });
  await page.evaluate(() => (document.querySelector('[data-testid="docx-close"]') as HTMLButtonElement).click());
  await expect.poll(() => closing).toBe(true);
  await expect(page.locator(".docx-editor-frame")).toHaveAttribute("inert", "");
  await page.keyboard.type("must-not-enter-the-document");
  expect(await office.locator("#synthetic-input").inputValue()).toBe("original");
  await f.dirty(); release();
  await expect(page.getByTestId("docx-recovery")).toContainText("文件 → 下载为 → DOCX");
  await expect(page.locator(".docx-editor-frame iframe")).toHaveCount(1);
  await expect(page.getByTestId("docx-save")).toBeDisabled();
  await expect(page.getByTestId("docx-status")).toHaveText("修改待导出恢复");
  expect(f.state.opens).toHaveLength(2);
  expect(f.state.saves).toHaveLength(0);
  await page.getByRole("button", { name: "已导出恢复稿，关闭编辑器", exact: true }).click();
  await expect(page.locator(".docx-editor-frame iframe")).toHaveCount(0);
  await expect(page.getByTestId("docx-status")).toHaveText("编辑器已关闭");
  expect(f.state.starts).toEqual([]);
});

test("an unknown edit opening survives reload and confirms the same full request without replacing its session", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  const opens: Array<{ body: unknown; key?: string }> = [];
  await page.route("**/docx/editor", async route => {
    const request = route.request();
    const body = request.postDataJSON() as { mode: string; version_id: string };
    if (body.mode !== "edit") return route.fallback();
    opens.push({ body, key: request.headers()["idempotency-key"] });
    if (opens.length === 1) {
      f.state.current!.editor.key = "owned-unknown-editor";
      return route.abort("failed");
    }
    if (opens.at(-1)!.key !== opens[0].key) return route.fulfill({ status: 409, contentType: "application/json", body: '{"error":{"message":"another editor is active"}}' });
    return route.fulfill({ contentType: "application/json", body: JSON.stringify({
      api_script_url: new URL("/__office/api.js", request.url()).href,
      config: { document: { permissions: { edit: true } }, editorConfig: { mode: "edit" } },
      session: { editor_key: "owned-unknown-editor", round_id: f.state.current!.round_id, version_id: body.version_id },
    }) });
  });
  await page.getByTestId("docx-mode").click();
  await expect(page.getByTestId("docx-confirm-open")).toBeVisible();
  const closes = f.state.calls.filter(call => call === "close").length;
  page.once("dialog", dialog => void dialog.accept());
  await page.reload();
  await expect(page.getByTestId("docx-confirm-open")).toBeVisible();
  expect(opens).toHaveLength(1);
  await page.evaluate(() => { document.documentElement.lang = "en-US"; });
  await page.getByTestId("docx-confirm-open").click();
  await expect(page.getByTestId("docx-status")).toHaveText("当前保存版本已载入");
  expect(opens).toHaveLength(2);
  expect(opens[1]).toEqual(opens[0]);
  expect(f.state.calls.filter(call => call === "close")).toHaveLength(closes);
  expect(await page.evaluate(() => Object.keys(sessionStorage).filter(key => key.startsWith("kb.docx-open:")))).toEqual([]);
  expect(f.state.starts).toEqual([]);
});

test("a lost close response locks the Word until the same editor key is confirmed", async ({ page }) => {
  const f = await fixture(page); await f.ready(); await f.edit();
  const addresses: string[] = [];
  await page.route("**/docx/editor/*/close", async route => {
    addresses.push(new URL(route.request().url()).pathname);
    if (addresses.length === 1) return route.abort("failed");
    return route.fallback();
  });
  await page.getByTestId("docx-close").click();
  await expect(page.getByTestId("docx-status")).toHaveText("关闭结果待确认");
  await expect(page.locator(".docx-editor-frame")).toHaveAttribute("inert", "");
  await expect(page.getByTestId("docx-save")).toBeDisabled();
  await expect(page.getByTestId("docx-mode")).toBeDisabled();
  await page.getByTestId("docx-confirm-close").click();
  await expect(page.getByTestId("docx-status")).toHaveText("编辑器已关闭");
  expect(addresses).toHaveLength(2);
  expect(addresses[1]).toBe(addresses[0]);
  expect(f.state.opens).toHaveLength(2);
  expect(f.state.saves).toEqual([]);
});

test("a status change to read-only retains saving and confirmation for the already open edit session", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready(); await f.edit(); await f.dirty();
  f.view.project.status = "ended"; f.view.project.ended_at = "2026-10-28T00:00:00Z";
  await page.clock.runFor(5100);
  await expect(page.getByTestId("docx-save")).toBeEnabled();
  await page.getByTestId("docx-save").click();
  f.publish(); await page.clock.runFor(1100);
  await expect(page.getByTestId("docx-status")).toHaveText("已保存");
  await page.getByTestId("docx-mode").click();
  await expect(page.getByTestId("docx-mode")).toHaveText("编辑 Word");
  await expect(page.getByTestId("docx-mode")).toBeDisabled();
  expect(f.state.saves).toHaveLength(1);
  expect(f.state.opens.map(open => open.mode)).toEqual(["view", "edit", "view"]);
  expect(f.state.downloads).toEqual([]);
  expect(f.state.starts).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.destroyed)).toBe(2);
});
