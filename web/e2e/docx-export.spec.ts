import { expect, test } from "@playwright/test";
import { draft, fixture, path, project } from "./tender.fixture";

test("generated, manual and recovered drafts download their own immutable Word version", async ({ page }) => {
  const f = await fixture(page);
  f.view.versions = [draft(), { ...draft("word-manual", 2), source: "manual" }, { ...draft("word-recovered", 3), source: "recovered" }];
  await page.goto(`/#/bids/${project}/export`);
  await expect(page.getByRole("cell", { name: "编制生成稿", exact: true })).toBeVisible();
  await expect(page.getByRole("cell", { name: "人工保存稿", exact: true })).toBeVisible();
  await expect(page.getByRole("cell", { name: "会话恢复稿（待核对）", exact: true })).toBeVisible();
  for (const revision of [1, 2, 3]) {
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: `下载 DOCX v${revision}`, exact: true }).click();
    expect((await download).suggestedFilename()).toBe(`投标文件-v${revision}.docx`);
  }
  expect(f.state.downloads).toEqual([`${path}/docx/versions/word-v1/download`, `${path}/docx/versions/word-manual/download`, `${path}/docx/versions/word-recovered/download`]);
  expect(f.state.starts).toHaveLength(0);
});

test("a generated delivery package explains its boundary with later manual saves", async ({ page }) => {
  const f = await fixture(page);
  f.view.current_docx = { ...draft("word-manual", 2), source: "manual" };
  f.view.versions = [f.view.current_docx, draft()];
  f.view.artifacts = { package_url: `${path}/executions/execution-word-v1/package` };
  await page.goto(`/#/bids/${project}/export`);
  await expect(page.getByText("交付包对应原编制产物，不包含后续人工修改；人工保存稿请按上方版本下载。", { exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "下载编制交付包（包含待补清单）", exact: true }).click();
  expect((await download).suggestedFilename()).toBe("投标文件及附件.zip");
  expect(f.state.downloads).toEqual([`${path}/executions/execution-word-v1/package`]);
});

test("material gaps do not hide or block a persisted Word download", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/export`);
  await expect(page.getByText("项目人员资料待补。", { exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "下载 DOCX v1", exact: true }).click();
  await download;
  expect(f.state.downloads).toEqual([`${path}/docx/versions/word-v1/download`]);
});
