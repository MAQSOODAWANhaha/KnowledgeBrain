import { expect, test, type Page } from "@playwright/test";
import { draft, fixture, metadata, project, type WordTestWindow } from "./tender.fixture";

async function guidanceFixture(page: Page) {
  const f = await fixture(page);
  await page.route("**/docx/versions/*/review", route => route.fulfill({
    contentType: "application/json", body: JSON.stringify({ review: null }),
  }));
  return f;
}

for (const width of [1440, 390]) {
  test(`first Word has an explicit entry at ${width}px and leaves an unsaved outline intact`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.clock.install();
    const f = await guidanceFixture(page);
    f.state.current = null; f.view.current_docx = null; f.view.versions = [];
    await page.goto(`/#/bids/${project}/authoring`);
    const title = page.getByLabel("章节名称", { exact: true });
    await title.fill("尚未保存的实施编排");
    await expect(page.getByTestId("outline-save-status")).toHaveText("未保存");
    await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
    await expect(page.getByRole("heading", { name: "投标稿尚未形成", exact: true })).toBeVisible();
    await page.getByRole("tab", { name: "大纲编排", exact: true }).click();

    f.state.current = metadata(); f.view.current_docx = draft(); f.view.versions = [draft()];
    await page.clock.runFor(5100);
    const open = page.getByRole("button", { name: "正文已生成，查看Word", exact: true });
    await expect(open).toBeVisible();
    await expect(page.getByRole("tab", { name: "大纲编排", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(title).toHaveValue("尚未保存的实施编排");
    expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual([]);
    expect(f.state.calls).toEqual([]);

    await open.click();
    await expect(page.getByTestId("docx-status")).toHaveText("当前保存版本已载入");
    expect(f.state.calls).toEqual(["open"]);
    await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
    await expect(title).toHaveValue("尚未保存的实施编排");
    await expect(page.getByTestId("outline-save-status")).toHaveText("未保存");
    expect(f.state.outlineSaves).toEqual([]); expect(f.state.starts).toEqual([]);
    if (width === 390) expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });
}

test("a background generated draft leaves the current dirty Word open until an explicit choice", async ({ page }) => {
  await page.clock.install();
  const f = await guidanceFixture(page); await f.ready(); await f.edit(); await f.dirty();
  const opens = [...f.state.opens];
  const destroyed = await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.destroyed);
  f.candidate(); await page.clock.runFor(5100);
  await expect(page.getByRole("button", { name: "查看新稿", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "正文已生成，查看Word", exact: true })).toHaveCount(0);
  await expect(page.getByTestId("docx-status")).toHaveText("有修改尚未保存");
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 1");
  await expect(page.getByRole("tab", { name: "正文编辑", exact: true })).toHaveAttribute("aria-selected", "true");
  expect(f.state.opens).toEqual(opens);
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.destroyed)).toBe(destroyed);
});

for (const width of [1440, 390]) {
  test(`review entries at ${width}px name saved and generated versions and keep existing form state`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const f = await guidanceFixture(page); f.publish();
    const current = metadata("word-saved-v2", 2); current.execution_id = "execution-word-v1"; f.state.current = current;
    await f.ready();
    const entries = page.getByRole("group", { name: "稿件复核入口", exact: true });
    await expect(entries).toContainText("当前保存稿：Word v2");
    await expect(entries).toContainText("生成稿修订基准：生成稿 v1");
    const savedEntry = entries.getByRole("button", { name: "打开保存稿复核", exact: true });
    const revisionEntry = entries.getByRole("button", { name: "基于生成稿修订", exact: true });
    await savedEntry.click();
    const saved = page.getByRole("region", { name: "已保存 Word 只读复核", exact: true });
    await expect(saved).toBeVisible();
    await saved.getByLabel("保存稿复核理由", { exact: true }).fill("保留当前保存稿的核对意见。");
    await revisionEntry.click();
    const revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
    await expect(revision).toContainText("复核基准：生成稿 v1");
    await expect(revision.locator("code").filter({ hasText: "execution-word-v1" })).not.toBeVisible();
    await revision.getByLabel("复核与修订理由", { exact: true }).fill("保留原生成稿的修订意见。");
    await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
    await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
    await savedEntry.click();
    await expect(saved.getByLabel("保存稿复核理由", { exact: true })).toHaveValue("保留当前保存稿的核对意见。");
    await revisionEntry.click();
    await expect(revision.getByLabel("复核与修订理由", { exact: true })).toHaveValue("保留原生成稿的修订意见。");
    expect(f.state.starts).toEqual([]);
    expect(f.state.calls).toEqual(["open"]);
    expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view"]);
    if (width === 390) {
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      for (const entry of [savedEntry, revisionEntry]) {
        const box = await entry.boundingBox();
        expect(box).not.toBeNull();
        expect(box!.x).toBeGreaterThanOrEqual(0);
        expect(box!.x + box!.width).toBeLessThanOrEqual(width);
      }
    }
  });
}

test("the saved-review shortcut keeps the original report and local reason after saving a newer Word", async ({ page }) => {
  await page.clock.install();
  const f = await guidanceFixture(page);
  await page.route("**/docx/versions/word-v1/review", route => route.fulfill({
    contentType: "application/json", body: JSON.stringify({ review: {
      execution_id: "saved-review-v1", status: "succeeded", operation: "saved_review", version_id: "word-v1",
      docx_sha256: metadata().docx_sha256, sources_current: true,
      analysis_identity: { run_id: "analysis", source_version: 1, domain_revision: 0 },
      report: { version_id: "word-v1", docx_sha256: metadata().docx_sha256, reason: "核对原保存稿。",
        findings: [], gaps: [], records: [{ task_id: "old-record", chapter_id: "chapter-1", title: "原保存稿正文" }], record_count: 1, reviewed_record_count: 1 },
    } }),
  }));
  await f.ready();
  const summary = page.getByText("核对已保存的 Word 内容", { exact: true });
  await summary.click();
  const saved = page.getByRole("region", { name: "已保存 Word 只读复核", exact: true });
  await expect(saved.getByRole("heading", { name: "Word v1 的复核报告", exact: true })).toBeVisible();
  await saved.getByLabel("保存稿复核理由", { exact: true }).fill("保留原版本的待提交意见。");
  await summary.click();
  await f.edit(); await f.dirty(); await page.getByTestId("docx-save").click();
  f.publish(); await page.clock.runFor(1100);
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 2");
  const entries = page.getByRole("group", { name: "稿件复核入口", exact: true });
  await expect(entries).toContainText("当前保存稿：Word v2");
  await expect(entries).toContainText("进入后确认复核版本");
  await expect(entries.getByRole("button", { name: "核对当前保存稿", exact: true })).toHaveCount(0);
  await entries.getByRole("button", { name: "打开保存稿复核", exact: true }).click();
  await expect(saved.getByRole("heading", { name: "Word v1 的复核报告", exact: true })).toBeVisible();
  await expect(saved).toContainText("当前 Word 已更新为 v2，以下仍对应 v1，未自动复核新版本。");
  await expect(saved.getByLabel("保存稿复核理由", { exact: true })).toHaveValue("保留原版本的待提交意见。");
  await expect(saved.getByRole("button", { name: "切换到当前保存版本", exact: true })).toBeDisabled();
  expect(f.state.starts).toEqual([]);
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
});
