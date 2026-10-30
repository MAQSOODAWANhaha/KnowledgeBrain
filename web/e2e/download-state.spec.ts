import { expect, test, type Page } from "@playwright/test";
import { fixture, path, type WordTestWindow } from "./tender.fixture";

async function downloadSaved(page: Page, version: string, revision: number) {
  const button = page.getByRole("button", { name: "下载已保存稿", exact: true });
  await expect(button).toBeEnabled();
  await button.scrollIntoViewIfNeeded();
  await expect(button).toBeInViewport({ ratio: 1 });
  const box = await button.boundingBox();
  expect(box).toBeTruthy();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  const result = page.waitForEvent("download");
  await button.click();
  expect((await result).suggestedFilename()).toBe(`投标文件-${revision}.docx`);
  return `${path}/docx/versions/${version}/download`;
}

for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
  test.describe(`saved download boundary at ${viewport.width}px`, () => {
    test.use({ viewport });

    test("syncing, dirty and saving keep the persisted download explicit until the correlated receipt", async ({ page }) => {
      await page.clock.install(); const f = await fixture(page); await f.ready(); await f.edit();
      const warning = page.getByTestId("docx-download-boundary");
      await expect(warning).toHaveCount(0);
      await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.events!.onDocumentStateChange({ data: true }));
      await expect(page.getByTestId("docx-status")).toHaveText("正在同步修改…");
      await expect(warning).toHaveText("下载仅包含已保存的 Word v1，不包含当前尚未确认保存的修改。");
      const expected = [await downloadSaved(page, "word-v1", 1)];

      await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.events!.onDocumentStateChange({ data: false }));
      await expect(page.getByTestId("docx-status")).toHaveText("有未保存修改");
      await expect(warning).toBeVisible();
      expected.push(await downloadSaved(page, "word-v1", 1));
      await page.getByTestId("docx-save").click();
      await expect(page.getByTestId("docx-status")).toHaveText("保存中");
      await expect(warning).toBeVisible();
      expected.push(await downloadSaved(page, "word-v1", 1));

      f.publish(); await page.clock.runFor(1100);
      await expect(page.getByTestId("docx-status")).toHaveText("已保存");
      await expect(page.getByTestId("docx-version")).toHaveText("保存版本 2");
      await expect(warning).toHaveCount(0);
      expected.push(await downloadSaved(page, "word-saved-v2", 2));
      expect(f.state.downloads).toEqual(expected);
      expect(f.state.starts).toEqual([]);
    });

    test("an unknown save still downloads the old version and keeps the exact request for recovery", async ({ page }) => {
      await page.clock.install(); const f = await fixture(page); await f.ready(); await f.edit(); await f.dirty();
      let lose = true;
      await page.route("**/docx/editor/save", async route => {
        if (!lose) return route.fallback();
        lose = false;
        f.state.saves.push({ body: route.request().postDataJSON(), key: route.request().headers()["idempotency-key"] });
        return route.abort("failed");
      });
      await page.getByTestId("docx-save").click();
      await expect(page.getByTestId("docx-status")).toHaveText("保存结果待确认");
      await expect(page.getByTestId("docx-download-boundary")).toHaveText("下载仅包含已保存的 Word v1，不包含当前尚未确认保存的修改。");
      const expected = [await downloadSaved(page, "word-v1", 1)];
      await page.getByTestId("docx-save").click();
      await expect.poll(() => f.state.saves.length).toBe(2);
      expect(f.state.saves[1]).toEqual(f.state.saves[0]);
      await expect(page.getByTestId("docx-download-boundary")).toBeVisible();
      f.publish(); await page.clock.runFor(1100);
      await expect(page.getByTestId("docx-status")).toHaveText("已保存");
      await expect(page.getByTestId("docx-download-boundary")).toHaveCount(0);
      expected.push(await downloadSaved(page, "word-saved-v2", 2));
      expect(f.state.downloads).toEqual(expected);
      expect(f.state.starts).toEqual([]);
    });

    test("a pending platform save is disclosed even in reading mode without inventing a saved receipt", async ({ page }) => {
      await page.clock.install(); const f = await fixture(page);
      f.state.current!.editor.pending_save_id = "previous-session-save";
      await page.goto(`/#/bids/${f.view.project.id}/authoring`);
      await expect(page.getByTestId("docx-status")).toHaveText("保存中");
      await expect(page.getByTestId("docx-download-boundary")).toHaveText("下载仅包含已保存的 Word v1，不包含当前尚未确认保存的修改。");
      const expected = await downloadSaved(page, "word-v1", 1);
      f.state.current!.editor.pending_save_id = null;
      await page.clock.runFor(1100);
      await expect(page.getByTestId("docx-status")).toHaveText("当前保存版本已载入");
      await expect(page.getByTestId("docx-download-boundary")).toHaveCount(0);
      expect(f.state.downloads).toEqual([expected]);
      expect(f.state.saves).toEqual([]); expect(f.state.starts).toEqual([]);
    });
  });
}
