import { Buffer } from "node:buffer";
import { expect, test } from "@playwright/test";
import { fixture, project } from "./tender.fixture";

test("a project exposes files, source analysis, one Word editor and versioned delivery", async ({ page }) => {
  const f = await fixture(page);
  await page.goto("/#/");
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("编制工作台专项");
  await page.getByTestId("bid-create").click();
  await expect(page).toHaveURL(new RegExp(`/bids/${project}/files$`));
  const nav = page.getByRole("navigation", { name: "项目分区" });
  for (const name of ["项目资料", "招标解读", "标书编制", "交付文件"]) await expect(nav.getByRole("link", { name })).toBeVisible();
  await expect(page.getByRole("cell", { name: "合成招标.pdf" })).toBeVisible();
  await nav.getByRole("link", { name: "招标解读" }).click();
  await expect(page.getByRole("heading", { name: "招标解读" })).toBeVisible();
  await expect(page.getByText("应提交实施安排。", { exact: true })).toBeVisible();
  await nav.getByRole("link", { name: "标书编制" }).click();
  await expect(page.getByTestId("docx-editor")).toBeVisible();
  await expect(page.getByRole("complementary", { name: "文件与章节目录" })).toBeVisible();
  await nav.getByRole("link", { name: "交付文件" }).click();
  await expect(page.getByRole("button", { name: "下载 DOCX v1" })).toBeEnabled();
  expect(f.state.starts).toHaveLength(0);
});

test("enterprise proof uploads retain their purpose and file role", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/files`);
  await page.getByLabel("资料用途", { exact: true }).selectOption("enterprise");
  await page.getByLabel("文件角色", { exact: true }).selectOption("attachment");
  await page.locator('input[type="file"]').setInputFiles({ name: "合成企业证明.txt", mimeType: "text/plain", buffer: Buffer.from("synthetic enterprise evidence") });
  await expect(page.getByRole("cell", { name: "合成企业证明.txt" })).toBeVisible();
  expect(f.state.uploads).toHaveLength(1);
  expect(f.state.uploads[0]).toContain('name="kind"\r\n\r\nenterprise');
  expect(f.state.uploads[0]).toContain('name="role"\r\n\r\nattachment');
  expect(f.state.starts).toHaveLength(0);
});
