import { expect, test } from "@playwright/test";
import type { AnalysisRecoveryView } from "../src/bid/api/analysisRecovery";
import type { AnalysisReviewView } from "../src/bid/api/analysisReview";
import type { TaskProgressView } from "../src/bid/api/workbench";
import { fixture, project } from "./tender.fixture";

const progress = (state: string): TaskProgressView => ({
  identity: { run_id: "analysis", source_version: 1, domain_revision: 1 }, state, current_phase: state === "running" ? "extract" : null,
  active_tasks: [], latest_request_phase: "extract", last_completed_phase: "extract", accepted_units: 17,
  accepted_by_phase: [{ phase: "extract", count: 15, planned: 16 }],
  request_states: { prepared: 0, sent: 0, uncertain: 1, failed: 0, received: 17 }, rejected_responses: 0,
});
const missing = ["extract", "verify", "confirm"].map(stage => ({ code: "authoring_input_missing", detail: `${stage} 尚无唯一的当前根产物；继续使用已有资料。` }));

test("a running analysis is not presented as awaiting confirmation", async ({ page }) => {
  const f = await fixture(page);
  Object.assign(f.view.analysis!, { status: "running", progress: progress("running"), issues: missing });
  const review: AnalysisReviewView = { run_id: "analysis", source_version: 1, domain_revision: 1, status: "running",
    checklist_digest: null, sources_current: true, hard_blocking: false, human_issue_ids: [], issues: [], correction: null };
  await page.route("**/analysis/review", route => route.fulfill({ contentType: "application/json", body: JSON.stringify({ review }) }));
  await page.goto(`/#/bids/${project}/analysis`);
  await expect(page.getByText("分析中", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("待人工确认", { exact: true })).toHaveCount(0);
  await expect(page.getByText(/尚无唯一的当前根产物|同一情况/)).toHaveCount(0);
  const reviewRegion = page.getByRole("region", { name: "招标分析人工复核", exact: true });
  await expect(reviewRegion).toContainText("分析尚未完成，完成后才能人工确认。");
  await expect(reviewRegion).not.toContainText("当前没有待核对的问题。");
  await expect(reviewRegion.getByRole("button", { name: "提交人工确认", exact: true })).toBeDisabled();
});

test("source location names the physical page or heading instead of an internal range id", async ({ page }) => {
  const f = await fixture(page);
  f.view.analysis!.items = [
    { item_id: "rule-1", kind: "requirement", text: "应提交实施安排。", evidence: [{ range_id: "range-5add97e470146717d23be3c2", quote: "应提交实施安排。", page: 24, heading_path: "第二章" }] },
    { item_id: "rule-2", kind: "requirement", text: "应提供售后承诺。", evidence: [{ range_id: "range-ee9359c81d4d9d1b9c28c816", quote: "应提供售后承诺。", page: null, heading_path: "第三章 > 3.1" }] },
  ];
  await page.goto(`/#/bids/${project}/analysis`);
  const table = page.getByRole("region", { name: "招标事项", exact: true });
  await expect(table.getByText("第 24 页", { exact: true })).toBeVisible();
  await expect(table.getByText("第三章 > 3.1", { exact: true })).toBeVisible();
  await expect(page.getByText(/range-[0-9a-f]{24}/)).toHaveCount(0);
});

test("a failed analysis names its failure cause, requires a reason, and reports the accepted retry as success", async ({ page }) => {
  const f = await fixture(page);
  Object.assign(f.view.analysis!, { status: "failed", progress: progress("failed") });
  const recovery: AnalysisRecoveryView = { run_id: "analysis", source_version: 1, domain_revision: 1, status: "failed",
    failed_command_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", can_retry: true, blocker: null, saved_responses: 17,
    notice: "本次执行已用尽重试。请先核对故障原因，再继续原任务；成功回执保留，未完成调用仍可能产生费用。",
    failure_reason: "model_work_unit_retries_exhausted",
    failure_causes: [{ error_type: "provider_stream_error", category: "uncertain", count: 9 }] };
  await page.route("**/analysis/recovery", route => route.fulfill({ contentType: "application/json", body: JSON.stringify({ recovery }) }));
  await page.route("**/analysis/retry", route => {
    Object.assign(recovery, { status: "running", can_retry: false, blocker: "not_failed", notice: "当前分析未失败，无需恢复。",
      failure_reason: null, failure_causes: [] });
    return route.fulfill({ contentType: "application/json", body: JSON.stringify({ command_id: "b", analysis: recovery }) });
  });
  await page.goto(`/#/bids/${project}/analysis`);
  await expect(page.getByText("分析失败", { exact: true }).first()).toBeVisible();
  await expect(page.getByRole("region", { name: "分析任务进度", exact: true })).toContainText("请在右侧「继续失败的分析」核对故障原因后继续原任务。");
  const region = page.getByRole("region", { name: "分析任务恢复", exact: true });
  const cause = region.getByRole("group", { name: "故障原因", exact: true });
  await expect(cause).toContainText("同一工作单元已用尽重试。");
  await expect(cause).toContainText("模型响应中断，结果与费用未确认（provider_stream_error）：9 次");
  await expect(region).toContainText("填写核对过的故障原因和处理情况后才能继续。");
  const reason = region.getByLabel("分析恢复理由");
  await expect(reason).toHaveAccessibleName("分析恢复理由 必填");
  await reason.fill("模型服务流式中断，已核对，继续原任务。");
  await region.getByRole("button", { name: "继续原分析任务", exact: true }).click();
  await expect(region.getByRole("status").filter({ hasText: "恢复请求已提交" })).toBeVisible();
  await expect(region).not.toContainText("已取消");
  await expect(region).not.toContainText("无需恢复");
});
