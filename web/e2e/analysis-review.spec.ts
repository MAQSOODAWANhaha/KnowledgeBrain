import { expect, test, type Page } from "@playwright/test";
import type { AnalysisReviewView } from "../src/bid/api/analysisReview";
import { fixture, project } from "./tender.fixture";

async function reviewFixture(page: Page) {
  await fixture(page);
  const review: AnalysisReviewView = {
    run_id: "analysis", source_version: 1, domain_revision: 1, status: "waiting-confirmation",
    checklist_digest: "c".repeat(64), sources_current: true, hard_blocking: false,
    human_issue_ids: ["semantic-1"], issues: [{ issue_id: "semantic-1", code: "semantic_ambiguity", detail: "请核对服务响应范围。" }],
    correction: { base_items_digest: "d".repeat(64), items: [{ pack_id: "pack-1", item: {
      item_id: "item-1", text: "服务响应不超过2小时", kind: "obligation",
      evidence: [{ range_id: "来源第3段", quote: "服务响应不超过2小时" }],
      numbers: [{ value: 2, unit: "小时", quote: "2小时" }], conditions: ["合同服务期内"],
    } }] },
  };
  const state = { lose: false, conflict: false, calls: [] as Array<{ kind: string; key?: string; body: unknown }> };
  await page.route("**/analysis/**", async route => {
    const request = route.request(), kind = new URL(request.url()).pathname.split("/").at(-1)!;
    const json = (value: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(value) });
    if (kind === "review") return json({ review });
    if (kind !== "confirm" && kind !== "correct") return route.fallback();
    state.calls.push({ kind, key: request.headers()["idempotency-key"], body: request.postDataJSON() });
    if (state.conflict) return json({ error: { code: "ANALYSIS_VERSION_CONFLICT", message: "分析已有新版本，请刷新核对。" } }, 409);
    if (state.lose) { state.lose = false; return route.abort("failed"); }
    if (kind === "correct") { review.domain_revision++; review.status = "pending"; }
    else review.status = "pending";
    return json({ analysis: review });
  });
  const open = async () => {
    await page.goto(`/#/bids/${project}/analysis`);
    await expect(page.getByRole("heading", { name: "人工复核与事项纠正", exact: true })).toBeVisible();
    await expect(page.getByText("请核对服务响应范围。", { exact: true })).toBeVisible();
  };
  const decide = async () => {
    await page.getByLabel("问题 1 人工判定").selectOption("acknowledged");
    await page.getByLabel("问题 1 判定理由").fill("已人工核对招标原件，保留待业务落实事项。");
  };
  const edit = async () => {
    await page.getByText("事项 1：服务响应不超过2小时", { exact: true }).click();
    await page.getByLabel("事项 1 纠正文案").fill("合同服务期内，服务响应不超过2小时。");
    await page.getByLabel("事项纠正理由").fill("人工核对后补充适用范围文案。");
  };
  return { review, state, open, decide, edit };
}

test("human confirmation requires an explicit verdict and reason for each issue", async ({ page }) => {
  const f = await reviewFixture(page); await f.open();
  const submit = page.getByRole("button", { name: "提交人工确认", exact: true });
  await expect(submit).toBeDisabled();
  await page.getByLabel("问题 1 人工判定").selectOption("dismissed");
  await expect(submit).toBeDisabled();
  await page.getByLabel("问题 1 判定理由").fill("人工核对后排除本例歧义。");
  await submit.click();
  await expect.poll(() => f.state.calls.length).toBe(1);
  expect(f.state.calls[0]).toMatchObject({ kind: "confirm", body: {
    run_id: "analysis", source_version: 1, domain_revision: 1,
    checklist_digest: "c".repeat(64), issue_resolutions: { schema_version: 1, resolutions: [
      { issue_id: "semantic-1", verdict: "dismissed", reason: "人工核对后排除本例歧义。" }] },
  } });
});

test("an unknown human confirmation survives reload and retries the same frozen request", async ({ page }) => {
  const f = await reviewFixture(page); await f.open(); await f.decide(); f.state.lose = true;
  await page.getByRole("button", { name: "提交人工确认", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次人工确认请求", exact: true })).toBeEnabled();
  page.on("dialog", dialog => void dialog.accept()); await page.reload();
  await expect(page.getByLabel("问题 1 判定理由")).toHaveValue("已人工核对招标原件，保留待业务落实事项。");
  await page.getByRole("button", { name: "确认同一次人工确认请求", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(2);
  expect(f.state.calls[1]).toEqual(f.state.calls[0]);
  await expect(page.getByText("人工确认已提交。", { exact: true })).toBeVisible();
});

test("a text correction retains unedited business fields and retries its exact request", async ({ page }, testInfo) => {
  const f = await reviewFixture(page); await f.open(); await f.edit();
  await expect(page.getByText("数值：2小时", { exact: true })).toBeVisible();
  await expect(page.getByText("条件：合同服务期内", { exact: true })).toBeVisible();
  await expect(page.getByText(/来源位置和证据保持冻结/)).toBeVisible();
  f.state.lose = true;
  await page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次事项纠正请求", exact: true })).toBeEnabled();
  page.on("dialog", dialog => void dialog.accept()); await page.reload();
  await page.getByText("事项 1：服务响应不超过2小时", { exact: true }).click();
  await expect(page.getByLabel("事项 1 纠正文案")).toHaveValue("合同服务期内，服务响应不超过2小时。");
  await expect(page.getByLabel("事项 1 纠正文案")).toBeDisabled();
  await page.screenshot({ path: testInfo.outputPath("analysis-review-recovery.png"), fullPage: true });
  await page.getByRole("button", { name: "确认同一次事项纠正请求", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(2);
  expect(f.state.calls[1]).toEqual(f.state.calls[0]);
  expect(f.state.calls[0].body).toEqual({ run_id: "analysis", source_version: 1, expected_revision: 1,
    base_items_digest: "d".repeat(64), reason: "人工核对后补充适用范围文案。",
    changes: [{ pack_id: "pack-1", item_id: "item-1", text: "合同服务期内，服务响应不超过2小时。" }] });
});

test("a correction conflict and remote version refresh retain local edits", async ({ page }) => {
  const f = await reviewFixture(page); await f.open(); await f.edit(); f.state.conflict = true;
  await page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "当前输入已保留" })).toBeVisible();
  f.review.domain_revision = 2; f.review.correction!.items[0].item.text = "另一页面的事项表述";
  await page.getByRole("button", { name: "刷新复核状态", exact: true }).click();
  await expect(page.getByLabel("事项 1 纠正文案")).toHaveValue("合同服务期内，服务响应不超过2小时。");
  await expect(page.getByLabel("事项纠正理由")).toHaveValue("人工核对后补充适用范围文案。");
  await expect(page.getByText("最新来源版本 1，分析修订 2；待确认问题 1 项。", { exact: true })).toBeVisible();
  await page.getByText("查看最新分析内容，对照当前输入", { exact: true }).click();
  await expect(page.getByText("另一页面的事项表述", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true })).toBeDisabled();
  expect(f.state.calls).toHaveLength(1);
  page.once("dialog", dialog => void dialog.dismiss());
  await page.getByRole("button", { name: "载入最新分析重新核对", exact: true }).click();
  await expect(page.getByLabel("事项 1 纠正文案")).toHaveValue("合同服务期内，服务响应不超过2小时。");
});

test("hard blocking findings have an exit and cannot be cleared with a human verdict", async ({ page }) => {
  const f = await reviewFixture(page); f.review.hard_blocking = true; f.review.status = "blocked";
  f.review.sources_current = false; await f.open(); await f.decide();
  await expect(page.getByText(/人工判定不能解除/)).toBeVisible();
  await expect(page.getByText(/补充来源、使用完整纠正流程或联系管理员处理/)).toBeVisible();
  await expect(page.getByText(/项目来源已变化/)).toBeVisible();
  await expect(page.getByRole("button", { name: "提交人工确认", exact: true })).toBeDisabled();
  expect(f.state.calls).toHaveLength(0);
});

test("local human input can be explicitly discarded without sending a request", async ({ page }) => {
  const f = await reviewFixture(page); await f.open(); await f.decide(); await f.edit();
  await expect(page.getByRole("button", { name: "放弃未提交输入", exact: true })).toBeVisible();
  page.once("dialog", dialog => void dialog.accept());
  await page.getByRole("button", { name: "放弃未提交输入", exact: true }).click();
  await expect(page.getByLabel("问题 1 人工判定")).toHaveValue("");
  await expect(page.getByLabel("问题 1 判定理由")).toHaveValue("");
  await expect(page.getByLabel("事项 1 纠正文案")).toHaveValue("服务响应不超过2小时");
  await expect(page.getByLabel("事项纠正理由")).toHaveValue("");
  expect(f.state.calls).toHaveLength(0);
  await expect.poll(() => page.evaluate(id => sessionStorage.getItem(`kb.analysis-review:${id}`), project)).toBeNull();
});
