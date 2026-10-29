import { test, expect } from "@playwright/test";
import { fixture, project } from "./tender.fixture";

for (const width of [1440, 390]) {
  test(`large analysis and chapter navigation remain usable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const f = await fixture(page);
    f.view.analysis!.items = Array.from({ length: 300 }, (_, i) => ({ item_id: `requirement-${i}`, kind: i % 2 ? "scoring_rule" : "requirement", text: `第${i + 1}项合成要求：${"企业证明材料及实施方案须按照招标原文提交。".repeat(8)}` }));
    const chapters = Array.from({ length: 80 }, (_, i) => ({ chapter_id: `large-chapter-${i}`, title: `第${i + 1}章 ${"长章节名称及交付实施要求".repeat(3)}`, bookmark: `kb_c_${(i + 1).toString(16).padStart(28, "0")}`, targets: [], children: [] }));
    f.view.current_docx!.outline = { chapters };
    Object.assign(f.state.current!, { outline: { chapters } });
    f.view.outline = { chapters };
    await page.goto(`/#/bids/${project}/analysis`);
    await expect(page.locator(".tender-analysis-item")).toHaveCount(300);
    await page.getByLabel("搜索招标事项").fill("第300项");
    await expect(page.locator(".tender-analysis-item")).toHaveCount(1);
    await expect(page.locator(".tender-analysis-item")).toContainText("第300项");
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
    await page.screenshot({ path: test.info().outputPath("analysis-filtered.png") });
    await page.getByLabel("搜索招标事项").clear();
    await expect(page.locator(".tender-analysis-item")).toHaveCount(300);
    await page.goto(`/#/bids/${project}/authoring`);
    await expect(page.getByTestId("docx-status")).toContainText("当前保存版本已载入");
    if (width < 700) await page.getByRole("button", { name: "展开目录", exact: true }).click();
    await page.getByLabel("搜索章节").fill("第80章");
    await page.getByRole("button", { name: chapters[79].title, exact: true }).click();
    if (width < 700) await page.getByRole("button", { name: "收起目录", exact: true }).click();
    await expect(page.getByText("已定位 Word 中的书签位置。", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "下载已保存稿", exact: true })).toBeInViewport();
    const editor = page.locator(".docx-editor-frame");
    expect((await editor.boundingBox())!.height).toBeGreaterThanOrEqual(420);
    await editor.scrollIntoViewIfNeeded();
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
    await page.screenshot({ path: test.info().outputPath("word-workspace.png") });
    expect(f.state.starts).toEqual([]);
  });
}

test("resizing an open inspector to mobile preserves a reachable close control and the Word toolbar", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const f = await fixture(page);
  await f.ready();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "收起核对栏", exact: true }).click({ timeout: 3000 });
  await expect(page.getByRole("button", { name: "展开核对栏", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "下载已保存稿", exact: true }).click();
  expect(f.state.downloads).toHaveLength(1);
  expect(f.state.starts).toEqual([]);
});
