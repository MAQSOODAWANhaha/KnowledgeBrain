import { test, expect } from "@playwright/test";
import { fixture, project } from "./tender.fixture";
import type { TaskProgressView } from "../src/bid/api/workbench";

test("published authoring remains cancellable and cannot start another document", async ({ page }) => {
  const f = await fixture(page);
  f.view.authoring = { execution_id: "queued-document", status: "published" };
  await f.ready();
  await expect(page.getByText("编制：排队中", { exact: true })).toBeVisible();
  await page.getByText("执行详情", { exact: true }).click();
  await page.getByRole("button", { name: "取消本次编制" }).click();
  expect(f.state.executions.map(x => x.action)).toEqual(["cancel"]);
  await page.getByText("直接生成整稿", { exact: true }).click();
  await expect(page.getByRole("button", { name: "生成新稿", exact: true })).toBeDisabled();
  await page.getByRole("tab", { name: "大纲编排" }).click();
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
  expect(f.state.starts).toEqual([]);
});

test("published outline stays queued with cancel available and duplicate generation blocked", async ({ page }) => {
  const f = await fixture(page);
  f.outline.generation = { execution_id: "queued-outline", status: "published", operation: "outline" };
  await f.ready();
  await page.getByRole("tab", { name: "大纲编排" }).click();
  await expect(page.getByRole("button", { name: "生成大纲", exact: true })).toBeDisabled();
  await expect(page.locator(".outline-execution")).toContainText("大纲任务：排队中");
  await page.getByRole("button", { name: "取消大纲生成" }).click();
  expect(f.state.executions.map(x => x.action)).toEqual(["cancel"]);
  expect(f.state.starts).toEqual([]);
});

const progress = (): TaskProgressView => ({
  identity: { run_id: "analysis", source_version: 1, domain_revision: 0 }, state: "running",
  current_phase: "analysis-associate", active_tasks: [{ phase: "analysis-associate", prepared: 0, sent: 2 }],
  latest_request_phase: "analysis-associate", last_completed_phase: "analysis-review",
  accepted_units: 8, accepted_by_phase: [{ phase: "analysis-review", count: 8 }],
  request_states: { prepared: 0, sent: 2, uncertain: 1, failed: 3, received: 12 }, rejected_responses: 4, total_units: null,
});

test("analysis progress distinguishes accepted work, in flight requests and unknown receipts without invented totals", async ({ page }) => {
  const f = await fixture(page);
  Object.assign(f.view.analysis!, { status: "running", progress: progress() });
  await page.goto(`/#/bids/${project}/analysis`);
  const panel = page.getByRole("region", { name: "分析任务进度", exact: true });
  await expect(panel).toContainText("当前阶段：关联要求");
  await expect(panel).toContainText("最近进展：核验事项");
  await expect(panel).toContainText("已接受 8 个工作单元，剩余总量尚未确定");
  await expect(panel).toContainText("1 次请求回执待确认");
  await panel.getByText("阶段与请求回执").click();
  await expect(panel).toContainText("等待返回 2 次");
  await expect(panel).toContainText("已收到返回 12 次；失败 3 次；未通过业务验收 4 次");
  await expect(panel).not.toContainText("%");
  expect(f.state.starts).toEqual([]);
});

test("inactive tasks show their last real phase and preserve resume without inventing a current phase", async ({ page }) => {
  const f = await fixture(page);
  const value = progress();
  value.current_phase = null; value.state = "failed"; value.active_tasks = [];
  value.last_completed_phase = "authoring_basis";
  f.view.authoring = { execution_id: "failed-document", status: "failed", progress: value };
  await f.ready();
  const panel = page.getByRole("region", { name: "正文任务进度", exact: true });
  await expect(panel).toContainText("执行失败");
  await expect(panel).toContainText("最近进展：整理写作依据");
  await expect(panel).not.toContainText("当前阶段");
  await page.getByText("执行详情", { exact: true }).click();
  await page.getByRole("button", { name: "继续本次编制", exact: true }).click();
  expect(f.state.executions.map(x => x.action)).toEqual(["resume"]);
  expect(f.state.starts).toEqual([]);
});

test("published analysis blocks new requests but permits confirmation of its frozen unknown request", async ({ page }) => {
  const f = await fixture(page);
  f.view.analysis!.status = "published";
  await f.ready();
  await expect(page.getByRole("button", { name: "分析招标文件", exact: true })).toBeDisabled();
  const attempt = { idempotencyKey: "queue-confirm-analysis" };
  await page.evaluate(({ project, attempt }) => sessionStorage.setItem(`kb.tender-action:${project}`, JSON.stringify({ kind: "analysis", body: {}, attempt })), { project, attempt });
  await page.reload();
  await page.getByRole("button", { name: "确认同一次分析请求" }).click();
  await expect.poll(() => f.state.starts.length).toBe(1);
  expect(f.state.starts[0].key).toBe(attempt.idempotencyKey);
});
