import { expect, test } from "@playwright/test";
import { fixture, project, path, type WordTestWindow } from "./tender.fixture";

test("the Word workspace keeps project actions in view when navigating chapters", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const f = await fixture(page); await f.ready();
  await page.getByRole("complementary", { name: "文件与章节目录" }).getByRole("button", { name: /项目人员/ }).click();
  await expect(page.getByRole("button", { name: "编排大纲", exact: true })).toBeInViewport({ ratio: 1 });
  await expect(page.getByRole("navigation", { name: "项目分区" })).toBeInViewport({ ratio: 1 });
  const editor = page.locator(".docx-editor-frame");
  await editor.scrollIntoViewIfNeeded();
  await expect(editor).toBeInViewport({ ratio: 0.99 });
  const box = await editor.boundingBox();
  expect(box).toBeTruthy();
  expect(box!.height).toBeGreaterThanOrEqual(420);
  expect(box!.y + box!.height).toBeLessThanOrEqual(901);
});

test("unconfirmed analysis does not hide an existing Word draft or add a second body editor", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await expect(page.getByTestId("docx-editor")).toBeVisible();
  await expect(page.getByTestId("docx-save")).toHaveCount(0);
  await expect(page.getByTestId("docx-mode")).toHaveText("编辑 Word");
  await expect(page.getByRole("main", { name: "Word 稿件工作区" }).locator("textarea,[contenteditable=true]")).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /Markdown|内容浏览/ })).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view"]);
});

test("a candidate and chapter selection never remount the open Word document", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready(); f.candidate();
  await page.clock.runFor(5100);
  await expect(page.getByRole("button", { name: "查看新稿", exact: true })).toBeVisible();
  const frame = page.locator('.docx-editor-frame > iframe[name="frameEditor"]');
  await expect(frame).toBeVisible();
  expect(await frame.getAttribute("id")).toBeNull();
  await page.getByRole("complementary", { name: "文件与章节目录" }).getByRole("button", { name: "项目人员" }).click();
  await expect.poll(() => page.evaluate(() => (window as unknown as WordTestWindow).wordTest.navigated.length)).toBe(1);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view"]);
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 1");
});

test("dirty edits block candidate switching and retain the editor", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await f.edit();
  await f.dirty(); f.candidate(); await page.clock.runFor(5100);
  await expect(page.getByRole("button", { name: "查看新稿", exact: true })).toBeDisabled();
  await expect(page.getByTestId("docx-status")).toHaveText("有修改尚未保存");
  expect(f.state.calls.filter(call => call === "select")).toHaveLength(0);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
});

test("a correlated save updates metadata and rebinds navigation without remounting Word", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await f.edit();
  await f.dirty(); await page.getByTestId("docx-save").click();
  await expect(page.getByTestId("docx-status")).toContainText("正在保存");
  await page.getByTestId("docx-save").click();
  await expect.poll(() => f.state.saves.length).toBe(2);
  expect(f.state.saves[0]).toEqual(f.state.saves[1]);
  f.publish(); await page.clock.runFor(1100);
  await expect(page.getByTestId("docx-status")).toHaveText("已保存");
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 2");
  await page.getByRole("complementary", { name: "文件与章节目录" }).getByRole("button", { name: "项目人员" }).click();
  await expect.poll(() => page.evaluate(() => (window as unknown as WordTestWindow).wordTest.navigated.length)).toBe(1);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.destroyed)).toBe(1);
  const nav = page.getByRole("navigation", { name: "项目分区" });
  await nav.getByRole("link", { name: "项目资料" }).click();
  await nav.getByRole("link", { name: "标书编制" }).click();
  await expect(page.getByTestId("docx-status")).toHaveText("已保存");
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 2");
  expect(f.state.opens).toHaveLength(2);
});

test("safe candidate switching closes the previous session before selecting and downloads the selected version", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready(); f.candidate();
  await page.clock.runFor(5100); f.state.calls.length = 0;
  await page.getByRole("button", { name: "查看新稿", exact: true }).click();
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 2");
  expect(f.state.calls.indexOf("close")).toBeLessThan(f.state.calls.indexOf("select"));
  const downloading = page.waitForEvent("download");
  await page.getByRole("button", { name: "下载已保存稿", exact: true }).click();
  expect((await downloading).suggestedFilename()).toContain("2");
  expect(f.state.downloads).toEqual([`${path}/docx/versions/word-v2/download`]);
});

test("a failed close cannot select the candidate or destroy the previous document", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready(); f.candidate();
  await page.clock.runFor(5100); f.state.failClose = true;
  await page.getByRole("button", { name: "查看新稿", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "当前文档会话尚未安全关闭" })).toBeVisible();
  expect(f.state.calls.filter(call => call === "select")).toHaveLength(0);
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 1");
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.destroyed)).toBe(0);
});

test("an unknown authoring submission survives reload and retries the same idempotency key", async ({ page }) => {
  const f = await fixture(page); await f.ready(); f.state.loseStart = true;
  await page.getByText("直接生成整稿", { exact: true }).click();
  await page.getByRole("button", { name: "生成新稿", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次编制请求", exact: true })).toBeVisible();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
  await page.reload();
  await page.getByText("直接生成整稿", { exact: true }).click();
  await page.getByRole("button", { name: "确认同一次编制请求", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[0]).toEqual(f.state.starts[1]);
});

test("the structure remains readable before Word generation and incomplete material does not disable authoring", async ({ page }) => {
  const f = await fixture(page); f.state.current = null; f.view.current_docx = null; f.view.versions = [];
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByRole("tab", { name: "大纲编排" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("实施方案");
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeEnabled();
  await expect(page.getByTestId("docx-editor")).toHaveCount(0);
});

test("a failed initial read shows a retry error rather than an empty project", async ({ page }) => {
  const f = await fixture(page); f.state.failRead = true;
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByRole("alert")).toContainText("工作台读取失败");
  await expect(page.getByText("还没有项目", { exact: true })).toHaveCount(0);
  f.state.failRead = false; await page.getByRole("button", { name: "重试", exact: true }).click();
  await expect(page.getByTestId("docx-editor")).toBeVisible();
});
