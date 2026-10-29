import { expect, test, type Page } from "@playwright/test";
import type { AnalysisRecoveryView } from "../src/bid/api/analysisRecovery";
import { fixture, project } from "./tender.fixture";

async function recoveryFixture(page: Page) {
  const workbench = await fixture(page);
  workbench.view.analysis!.status = "failed";
  const recovery: AnalysisRecoveryView = { run_id: "original-analysis", source_version: 1, domain_revision: 1,
    failed_command_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", status: "failed", can_retry: true, blocker: null,
    notice: "继续原任务的固定来源和分析版本，复用成功回执；未完成调用仍可能产生费用。", saved_responses: 3 };
  const state = { lose: false, conflict: false, failRead: false, accepted: 0,
    calls: [] as Array<{ body: unknown; key?: string }> };
  const receipts = new Set<string>();
  await page.route("**/analysis/recovery", route => state.failRead
    ? route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { message: "恢复状态暂不可用" } }) })
    : route.fulfill({ contentType: "application/json", body: JSON.stringify({ recovery }) }));
  await page.route("**/analysis/retry", async route => {
    const key = route.request().headers()["idempotency-key"];
    state.calls.push({ body: route.request().postDataJSON(), key });
    if (state.conflict) return route.fulfill({ status: 409, contentType: "application/json",
      body: JSON.stringify({ error: { code: "ANALYSIS_RECOVERY_CONFLICT", message: "分析失败记录已有变化，请刷新核对。" } }) });
    if (!receipts.has(key)) { receipts.add(key); state.accepted++; }
    recovery.status = "pending"; recovery.can_retry = false;
    recovery.failed_command_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    if (state.lose) { state.lose = false; return route.abort("failed"); }
    return route.fulfill({ contentType: "application/json", body: JSON.stringify({ command_id: recovery.failed_command_id, analysis: recovery }) });
  });
  const region = page.getByRole("region", { name: "分析任务恢复", exact: true });
  const open = async () => {
    await page.goto(`/#/bids/${project}/analysis`);
    await expect(region.getByRole("heading", { name: "继续失败的分析", exact: true })).toBeVisible();
  };
  return { recovery, state, region, open, workbench };
}

test("recovering a failed analysis continues its original run and requires a reason", async ({ page }) => {
  const f = await recoveryFixture(page); await f.open();
  const button = f.region.getByRole("button", { name: "继续原分析任务", exact: true });
  await expect(button).toBeDisabled();
  await expect(f.region.getByText(/成功回执保留；未完成的模型调用仍可能产生费用/)).toBeVisible();
  await page.getByLabel("分析恢复理由").fill("已核实网络故障修复，请继续原任务。");
  await button.click();
  await expect.poll(() => f.state.calls.length).toBe(1);
  expect(f.state.calls[0].body).toEqual({ run_id: "original-analysis", source_version: 1, domain_revision: 1,
    failed_command_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", reason: "已核实网络故障修复，请继续原任务。" });
  expect(f.state.calls[0].key).toBeTruthy();
  expect(f.workbench.state.starts).toHaveLength(0);
});

test("an unknown recovery keeps its exact body and key across a pending state and reload", async ({ page }) => {
  const f = await recoveryFixture(page); await f.open();
  await page.getByLabel("分析恢复理由").fill("仅恢复本次失败的分析，不重新提取已成功的包。"); f.state.lose = true;
  await f.region.getByRole("button", { name: "继续原分析任务", exact: true }).click();
  await expect(f.region.getByRole("button", { name: "确认同一次恢复请求", exact: true })).toBeEnabled();
  await expect(page.getByLabel("分析恢复理由")).toBeDisabled();
  page.on("dialog", dialog => void dialog.accept()); await page.reload();
  await expect(page.getByLabel("分析恢复理由")).toHaveValue("仅恢复本次失败的分析，不重新提取已成功的包。");
  await f.region.getByRole("button", { name: "确认同一次恢复请求", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(2);
  expect(f.state.calls[1]).toEqual(f.state.calls[0]);
  expect(f.state.accepted).toBe(1);
  expect(f.workbench.state.starts).toHaveLength(0);
});

test("a conflict retains the reason and requires explicit review of the latest failed command", async ({ page }) => {
  const f = await recoveryFixture(page); await f.open();
  await page.getByLabel("分析恢复理由").fill("人工保留的恢复理由"); f.state.conflict = true;
  await f.region.getByRole("button", { name: "继续原分析任务", exact: true }).click();
  await expect(f.region.getByRole("alert").filter({ hasText: "恢复理由已保留" })).toBeVisible();
  f.recovery.domain_revision = 2; f.recovery.failed_command_id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
  await f.region.getByRole("button", { name: "刷新分析恢复状态", exact: true }).click();
  await expect(page.getByLabel("分析恢复理由")).toHaveValue("人工保留的恢复理由");
  await expect(f.region.getByRole("button", { name: "继续原分析任务", exact: true })).toBeDisabled();
  await f.region.getByRole("button", { name: "按最新失败记录核对，保留理由", exact: true }).click();
  await expect(page.getByLabel("分析恢复理由")).toHaveValue("人工保留的恢复理由");
  f.state.conflict = false;
  await f.region.getByRole("button", { name: "继续原分析任务", exact: true }).click();
  await expect.poll(() => f.state.calls.length).toBe(2);
  expect(f.state.calls[1].body).toMatchObject({ domain_revision: 2, failed_command_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc" });
  expect(f.state.calls[1].key).not.toEqual(f.state.calls[0].key);
});

test("uncertain legacy payments show an operator exit and cannot create a new run", async ({ page }) => {
  const f = await recoveryFixture(page); f.recovery.can_retry = false; f.recovery.blocker = "payment_reconciliation";
  f.recovery.notice = "存在尚待核对的旧发送记录，请管理员核对调用结果和费用后处理原任务。";
  await f.open(); await page.getByLabel("分析恢复理由").fill("保留备注，待核账后处理。");
  await expect(f.region.getByText(/请管理员核对调用结果和费用后处理原任务/)).toBeVisible();
  await expect(f.region.getByRole("button", { name: "继续原分析任务", exact: true })).toBeDisabled();
  expect(f.state.calls).toHaveLength(0); expect(f.workbench.state.starts).toHaveLength(0);
});

test("read failures preserve recovery input and an explicit discard sends nothing", async ({ page }) => {
  const f = await recoveryFixture(page); await f.open();
  await page.getByLabel("分析恢复理由").fill("尚未提交的恢复理由"); f.state.failRead = true;
  await f.region.getByRole("button", { name: "刷新分析恢复状态", exact: true }).click();
  await expect(f.region.getByRole("alert").filter({ hasText: "恢复状态暂不可用" })).toBeVisible();
  await expect(page.getByLabel("分析恢复理由")).toHaveValue("尚未提交的恢复理由");
  page.once("dialog", dialog => void dialog.accept());
  await f.region.getByRole("button", { name: "放弃恢复理由", exact: true }).click();
  await expect(page.getByLabel("分析恢复理由")).toHaveValue("");
  expect(f.state.calls).toHaveLength(0);
});
