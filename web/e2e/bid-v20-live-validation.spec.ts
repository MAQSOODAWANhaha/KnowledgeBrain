import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "@playwright/test";

const projectId = "65cc6307-cea5-4cc2-a170-f3723cedc929";
const validation = JSON.parse(
  readFileSync(resolve(process.cwd(), "../artifacts/outline-v20-validation.json"), "utf8"),
) as {
  candidate_id: string;
  top_level_titles: string[];
  requirement_binding_count: number;
  obligation_binding_count: number;
};
const token = process.env.KB_LIVE_TOKEN;
const acceptedOnly = process.env.KB_LIVE_ACCEPTED_ONLY === "true";

if (!token) throw new Error("KB_LIVE_TOKEN is required");

function chineseOrdinal(value: number): string {
  const digits = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
  if (value <= 9) return digits[value] ?? String(value);
  if (value === 10) return "十";
  if (value < 20) return `十${digits[value % 10]}`;
  if (value < 100 && value % 10 === 0) return `${digits[Math.floor(value / 10)]}十`;
  if (value < 100) return `${digits[Math.floor(value / 10)]}十${digits[value % 10]}`;
  return String(value);
}

test("V20 real candidate survives refresh, accepts, numbers, and exports", async ({ page }) => {
  await page.addInitScript((value) => localStorage.setItem("kb.token", value), token);
  await page.goto(`/#/bids/${projectId}/authoring`);
  await expect(page.getByTestId("outline-tree")).toBeVisible({ timeout: 30_000 });
  if (acceptedOnly) {
    await expect(page.getByTestId("candidate-review")).toHaveCount(0);
  } else {
    await expect(page.getByTestId("candidate-review")).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId("outline-quality-summary")).toContainText(
      `一级章节 ${validation.top_level_titles.length}`,
    );
    await expect(page.getByTestId("outline-quality-summary")).toContainText(
      `要求绑定 ${validation.requirement_binding_count}`,
    );
    await expect(page.getByTestId("outline-quality-summary")).toContainText(
      `子节义务绑定 ${validation.obligation_binding_count}`,
    );
    await expect(page.getByTestId("outline-quality-blocked")).toHaveCount(0);
    await expect(page.getByTestId("outline-quality-empty-branches")).toHaveCount(0);
    await expect(page.getByTestId("outline-quality-high-notices")).toHaveCount(0);
    await expect(page.getByTestId("candidate-accept")).toBeEnabled();
    for (const [index, title] of validation.top_level_titles.entries()) {
      await expect(page.getByTestId("candidate-review")).toContainText(
        `${chineseOrdinal(index + 1)}、${title}`,
      );
    }
    await page.screenshot({
      path: resolve(process.cwd(), "../artifacts/outline-v20-candidate-review.png"),
      fullPage: true,
    });

    await page.reload();
    await expect(page.getByTestId("candidate-review")).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId("candidate-accept")).toBeEnabled();
    await page.getByTestId("candidate-accept").click();
    await expect(page.getByTestId("candidate-review")).toHaveCount(0, {
      timeout: 60_000,
    });
    await expect(page.getByTestId("outline-tree")).toBeVisible();
  }
  await page.reload();
  await expect(page.getByTestId("outline-tree")).toBeVisible({ timeout: 30_000 });
  for (const [index, title] of validation.top_level_titles.entries()) {
    await expect(page.getByTestId("outline-tree")).toContainText(
      `${chineseOrdinal(index + 1)}、${title}`,
    );
  }
  await page.screenshot({
    path: resolve(process.cwd(), "../artifacts/outline-v20-accepted-refresh.png"),
    fullPage: true,
  });

  await page.getByTestId("wizard-export").click();
  await expect(page.getByTestId("export-docx")).toBeVisible();
  const succeeded = page.getByTestId("export-status").filter({ hasText: "succeeded" });
  const succeededBefore = await succeeded.count();
  await page.getByTestId("export-docx").click();
  await expect(succeeded).toHaveCount(succeededBefore + 1, {
    timeout: 8 * 60_000,
  });
  await page.getByTestId("export-pdf").click();
  await expect(succeeded).toHaveCount(succeededBefore + 2, {
    timeout: 8 * 60_000,
  });
  await page.reload();
  await expect(page.getByText("submission · docx · ready").last()).toBeVisible({
    timeout: 30_000,
  });
  await expect(page.getByText("submission · pdf · ready").last()).toBeVisible({
    timeout: 30_000,
  });
  await page.screenshot({
    path: resolve(process.cwd(), "../artifacts/outline-v20-export.png"),
    fullPage: true,
  });
});
