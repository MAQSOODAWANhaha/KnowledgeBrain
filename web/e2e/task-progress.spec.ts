import { test, expect } from "@playwright/test";
import { fixture, project, openTool } from "./tender.fixture";
import type { TaskProgressView } from "../src/bid/api/workbench";

test("published authoring remains cancellable and cannot start another document", async ({ page }) => {
  const f = await fixture(page);
  f.view.authoring = { execution_id: "queued-document", status: "published" };
  await f.ready();
  await expect(page.getByText("正文任务：排队中", { exact: true })).toBeVisible();
  await openTool(page, "执行详情");
  await page.getByRole("button", { name: "取消本次编制" }).click();
  expect(f.state.executions.map(x => x.action)).toEqual(["cancel"]);
  await page.keyboard.press("Escape");
  await openTool(page, "直接生成整稿");
  await expect(page.getByRole("button", { name: "生成新稿", exact: true })).toBeDisabled();
  await page.keyboard.press("Escape");
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
  await expect(page.getByRole("region", { name: "大纲任务进度", exact: true })).toContainText("大纲任务：排队中");
  await page.getByRole("button", { name: "取消大纲生成" }).click();
  expect(f.state.executions.map(x => x.action)).toEqual(["cancel"]);
  expect(f.state.starts).toEqual([]);
});

const progress = (): TaskProgressView => ({
  identity: { run_id: "analysis", source_version: 1, domain_revision: 0 }, state: "running",
  current_phase: "analysis-associate", active_tasks: [{ phase: "analysis-associate", prepared: 0, sent: 2 }],
  latest_request_phase: "analysis-associate", last_completed_phase: "analysis-review",
  accepted_units: 11, accepted_by_phase: [{ phase: "extract", count: 3, planned: 5 }, { phase: "analysis-review", count: 8, planned: null }],
  request_states: { prepared: 0, sent: 2, uncertain: 1, failed: 3, received: 12 }, rejected_responses: 4,
});

test("analysis progress distinguishes accepted work, in flight requests and unknown receipts without invented totals", async ({ page }) => {
  const f = await fixture(page);
  Object.assign(f.view.analysis!, { status: "running", progress: progress() });
  await page.goto(`/#/bids/${project}/analysis`);
  const panel = page.getByRole("region", { name: "分析任务进度", exact: true });
  await expect(panel).toContainText("当前阶段：关联要求（仅历史运行）");
  await expect(panel).toContainText("最近进展：核验事项");
  await expect(panel).toContainText("已接受 11 个工作单元，总量未知");
  await expect(panel).toContainText("1 次模型请求结果未确认（可能已计费）。");
  await expect(panel).toContainText("任务仍在执行，会按原请求继续。");
  await panel.getByText("阶段与请求回执").click();
  await expect(panel).toContainText("提取招标事项：已接受 3 个工作单元，计划 5 个");
  await expect(panel).toContainText("核验事项：已接受 8 个工作单元，计划数未知");
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
  await openTool(page, "执行详情");
  await page.getByRole("button", { name: "继续本次编制", exact: true }).click();
  expect(f.state.executions.map(x => x.action)).toEqual(["resume"]);
  expect(f.state.starts).toEqual([]);
});

test("published analysis blocks new requests but permits confirmation of its frozen unknown request", async ({ page }) => {
  const f = await fixture(page);
  f.view.analysis!.status = "published";
  await f.ready();
  await page.getByRole("navigation", { name: "项目导航" }).getByRole("link", { name: "招标解读" }).click();
  await expect(page.getByRole("button", { name: "分析招标文件", exact: true })).toBeDisabled();
  const attempt = { idempotencyKey: "queue-confirm-analysis" };
  await page.evaluate(({ project, attempt }) => sessionStorage.setItem(`kb.tender-action:${project}`, JSON.stringify({ kind: "analysis", body: {}, attempt })), { project, attempt });
  await page.reload();
  await page.getByRole("button", { name: "确认同一次分析请求" }).click();
  await expect.poll(() => f.state.starts.length).toBe(1);
  expect(f.state.starts[0].key).toBe(attempt.idempotencyKey);
});

test("execution details read the stage report once opened and rank failures without sending work", async ({ page }) => {
  const f = await fixture(page);
  f.state.report = {
    execution_id: "execution-word-v1", status: "succeeded", kind: "document", requests: 5,
    stages: {
      authoring_write: { stage: "W 写作 / 改稿", logical_requests: 2, requests: 4, reported_tokens: { total_tokens: 920 } },
      authoring_review: { stage: "正文复核 / 再核", logical_requests: 1, requests: 1, reported_tokens: {} },
    },
    failures: [
      { stage: "W 写作 / 改稿", task: "authoring_write", kind: "contract", code: "unknown_target", count: 2, reported_tokens: 410, unknown_usage: 0 },
      { stage: "W 写作 / 改稿", task: "authoring_write", kind: "send", code: "timeout", count: 1, reported_tokens: 0, unknown_usage: 1 },
    ],
  };
  await f.ready();
  expect(f.state.reportReads).toEqual([]);
  await openTool(page, "执行详情");
  const report = page.getByRole("region", { name: "执行报表", exact: true });
  await expect(report).toContainText("W 写作 / 改稿");
  await expect(report.getByRole("row").filter({ hasText: "正文复核 / 再核" })).toContainText("未知");
  const failures = report.getByRole("row").filter({ hasText: "：" });
  await expect(failures.nth(0)).toContainText("未通过业务验收：unknown_target");
  await expect(failures.nth(0)).toContainText("410");
  await expect(failures.nth(1)).toContainText("发送失败：timeout");
  await expect(failures.nth(1)).toContainText("另 1 次未知");
  expect(f.state.reportReads).toEqual(["execution-word-v1"]);
  expect(f.state.starts).toEqual([]);
  expect(f.state.executions).toEqual([]);
});

test("execution details say when the execution has not started instead of showing empty counts", async ({ page }) => {
  const f = await fixture(page);
  f.view.authoring = { execution_id: "queued-document", status: "published" };
  await f.ready();
  await openTool(page, "执行详情");
  await expect(page.getByText("此任务尚未开始执行，暂无执行报表。")).toBeVisible();
});
