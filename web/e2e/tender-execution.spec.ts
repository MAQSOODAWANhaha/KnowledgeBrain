import { expect, test } from "@playwright/test";
import { fixture, project } from "./tender.fixture";

test("analysis completion and polling never start a paid authoring task automatically", async ({ page }) => {
  await page.clock.install();
  const f = await fixture(page); f.view.analysis!.status = "succeeded";
  await f.ready(); await page.clock.runFor(5100);
  expect(f.state.starts).toHaveLength(0);
  await page.getByRole("tab", { name: "大纲编排" }).click();
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeEnabled();
});

test("a running task preserves Word reading and disables duplicate generation", async ({ page }) => {
  const f = await fixture(page); f.view.authoring!.status = "running";
  await f.ready();
  await page.getByRole("tab", { name: "大纲编排" }).click();
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
  await page.getByText("执行详情", { exact: true }).click();
  await page.getByRole("button", { name: "取消本次编制" }).click();
  expect(f.state.executions).toHaveLength(1);
  expect(f.state.executions[0].action).toBe("cancel");
  expect(f.state.executions[0].key).toBeTruthy();
  expect(f.state.starts).toHaveLength(0);
});

test("analysis failures remain visible and retry uses the explicit analysis endpoint", async ({ page }) => {
  const f = await fixture(page); f.view.analysis!.status = "failed";
  f.view.analysis!.error = { code: "SOURCE_READING_FAILED", message: "来源读取失败，请重试。" };
  await page.goto(`/#/bids/${project}/analysis`);
  await expect(page.getByRole("alert")).toContainText("来源读取失败");
  await page.getByRole("button", { name: "分析招标文件", exact: true }).click();
  expect(f.state.starts).toHaveLength(1);
  expect(f.state.starts[0].kind).toBe("analysis");
  expect(f.state.starts[0].key).toBeTruthy();
});

test("a failed authoring task can resume its execution without starting a new one", async ({ page }) => {
  const f = await fixture(page); f.view.authoring!.status = "failed";
  f.view.authoring!.error = { message: "模型连接中断，可继续本次编制。" };
  await f.ready();
  await page.getByText("执行详情", { exact: true }).click();
  await expect(page.getByText("模型连接中断，可继续本次编制。", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "继续本次编制" }).click();
  expect(f.state.executions).toHaveLength(1);
  expect(f.state.executions[0].action).toBe("resume");
  expect(f.state.starts).toHaveLength(0);
});
