import { expect, test, type Page } from "@playwright/test";
import type { AnalysisReviewView } from "../src/bid/api/analysisReview";
import { fixture, project } from "./tender.fixture";

async function structuredFixture(page: Page) {
  await fixture(page);
  const review: AnalysisReviewView = {
    run_id: "analysis", source_version: 1, domain_revision: 1, status: "waiting-confirmation",
    checklist_digest: "c".repeat(64), sources_current: true, hard_blocking: false, human_issue_ids: [], issues: [],
    correction: { base_items_digest: "d".repeat(64), items: [{ pack_id: "pack-1", item: {
      item_id: "item-1", kind: "obligation", text: "服务响应不超过2小时",
      evidence: [{ range_id: "frozen-range-3", quote: "合同服务期内，服务响应不超过2小时；持证加3分。" }],
      numbers: [{ value: 2, unit: "小时", quote: "2小时" }], conditions: ["合同服务期内"],
    } }] },
  };
  const state = { lose: false, conflict: false, calls: [] as Array<{ key?: string; body: Record<string, unknown> }> };
  await page.route("**/analysis/**", async route => {
    const kind = new URL(route.request().url()).pathname.split("/").at(-1);
    const json = (value: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(value) });
    if (kind === "review") return json({ review });
    if (kind !== "correct") return route.fallback();
    state.calls.push({ key: route.request().headers()["idempotency-key"], body: route.request().postDataJSON() });
    if (state.conflict) return json({ error: { code: "ANALYSIS_VERSION_CONFLICT", message: "分析已有新版本。" } }, 409);
    if (state.lose) { state.lose = false; return route.abort("failed"); }
    review.domain_revision++; review.status = "pending";
    return json({ analysis: review });
  });
  const open = async () => {
    await page.goto(`/#/bids/${project}/analysis`);
    await page.getByText("事项 1：服务响应不超过2小时", { exact: true }).click();
  };
  return { review, state, open };
}

test("business correction edits quantities and scoring while freezing evidence and replaying the same request", async ({ page }) => {
  const f = await structuredFixture(page); await f.open();
  await page.getByLabel("事项 1 类型", { exact: true }).selectOption("scoring_rule");
  await page.getByLabel("事项 1 纠正文案", { exact: true }).fill("持证加3分。");
  await page.getByLabel("事项 1 涉及领域", { exact: true }).fill("技术\n服务");
  await page.getByLabel("事项 1 主体", { exact: true }).fill("投标人");
  await page.getByLabel("事项 1 适用范围", { exact: true }).fill("本项目");
  await page.getByLabel("事项 1 条件", { exact: true }).fill("");
  await page.getByLabel("事项 1 例外", { exact: true }).fill("不可抗力除外");
  await page.getByLabel("事项 1 数值 1 值", { exact: true }).fill("3");
  await page.getByLabel("事项 1 数值 1 单位", { exact: true }).fill("分");
  await page.getByLabel("事项 1 数值 1 原文片段", { exact: true }).fill("3分");
  await page.getByLabel("事项 1 评分办法", { exact: true }).fill("持证加分");
  await page.getByLabel("事项 1 分值", { exact: true }).fill("3");
  await page.getByLabel("事项 1 最高分", { exact: true }).fill("5");
  await page.getByLabel("事项纠正理由", { exact: true }).fill("核对原文后修正评分与数值字段。");
  await expect(page.locator("blockquote").filter({ hasText: "合同服务期内，服务响应不超过2小时；持证加3分。" })).toBeVisible();
  f.state.lose = true;
  await page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次事项纠正请求", exact: true })).toBeEnabled();
  page.on("dialog", dialog => void dialog.accept()); await page.reload();
  await page.getByText("事项 1：服务响应不超过2小时", { exact: true }).click();
  await expect(page.getByLabel("事项 1 最高分", { exact: true })).toHaveValue("5");
  await expect(page.getByLabel("事项 1 最高分", { exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "确认同一次事项纠正请求", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(2);
  expect(f.state.calls[1]).toEqual(f.state.calls[0]);
  expect(f.state.calls[0].body.changes).toEqual([{ pack_id: "pack-1", item_id: "item-1", kind: "scoring_rule", text: "持证加3分。",
    domains: ["技术", "服务"], subject: "投标人", scope: "本项目", conditions: null, exceptions: ["不可抗力除外"],
    numbers: [{ value: "3", unit: "分", quote: "3分" }], scoring: { method: "持证加分", points: 3, max_points: 5 } }]);
});

test("business correction exposes consequence and submission forms and can remove optional quantities", async ({ page }) => {
  const f = await structuredFixture(page); await f.open();
  await page.getByLabel("事项 1 类型", { exact: true }).selectOption("consequence_rule");
  await page.getByLabel("事项 1 触发条件", { exact: true }).fill("缺少证明");
  await page.getByLabel("事项 1 后果", { exact: true }).fill("否决投标");
  await page.getByRole("button", { name: "事项 1 删除数值 1", exact: true }).click();
  await page.getByLabel("事项纠正理由", { exact: true }).fill("补齐否决业务字段。");
  await page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(1);
  expect(f.state.calls[0].body.changes).toEqual([{ pack_id: "pack-1", item_id: "item-1", kind: "consequence_rule",
    numbers: null, consequence: { trigger: "缺少证明", effect: "否决投标" } }]);
  await page.getByText("事项 1：服务响应不超过2小时", { exact: true }).click();
  await page.getByLabel("事项 1 类型", { exact: true }).selectOption("submission_requirement");
  await page.getByLabel("事项 1 填写字段", { exact: true }).fill("联系人\n联系电话");
  await page.getByLabel("事项 1 提交材料", { exact: true }).fill("资质证明");
  await page.getByLabel("事项纠正理由", { exact: true }).fill("补齐提交要求。");
  await page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(2);
  expect(f.state.calls[1].body.changes).toEqual([{ pack_id: "pack-1", item_id: "item-1", kind: "submission_requirement",
    submission: { fields: ["联系人", "联系电话"], documents: ["资质证明"] } }]);
});

test("structured correction conflict retains fields across reload and guards leaving the project", async ({ page }) => {
  const f = await structuredFixture(page); await f.open(); f.state.conflict = true;
  await page.getByLabel("事项 1 适用范围", { exact: true }).fill("人工编辑的固定范围");
  await page.getByLabel("事项纠正理由", { exact: true }).fill("范围更正");
  await page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "当前输入已保留" })).toBeVisible();
  f.review.domain_revision = 2; f.review.correction!.items[0].item.scope = "远程最新范围";
  await page.getByRole("button", { name: "刷新复核状态", exact: true }).click();
  await expect(page.getByLabel("事项 1 适用范围", { exact: true })).toHaveValue("人工编辑的固定范围");
  await page.getByText("查看最新分析内容，对照当前输入", { exact: true }).click();
  await expect(page.getByText("适用范围：远程最新范围", { exact: true })).toBeVisible();
  page.once("dialog", dialog => void dialog.dismiss());
  await page.getByRole("button", { name: "载入最新分析重新核对", exact: true }).click();
  await expect(page.getByLabel("事项 1 适用范围", { exact: true })).toHaveValue("人工编辑的固定范围");
  await page.getByRole("navigation", { name: "面包屑" }).getByRole("link", { name: "投标项目", exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/bids/${project}/analysis$`));
  await expect(page.getByRole("alert").filter({ hasText: "请先提交或放弃分析修改" })).toBeVisible();
  page.once("dialog", dialog => void dialog.accept()); await page.reload();
  await page.getByText("事项 1：服务响应不超过2小时", { exact: true }).click();
  await expect(page.getByLabel("事项 1 适用范围", { exact: true })).toHaveValue("人工编辑的固定范围");
  await expect(page.getByRole("button", { name: "提交事项纠正并重新审核", exact: true })).toBeDisabled();
  expect(f.state.calls).toHaveLength(1);
});
