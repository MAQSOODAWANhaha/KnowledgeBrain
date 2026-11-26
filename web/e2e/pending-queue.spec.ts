import { expect, test } from "@playwright/test";
import { fixture, openTool } from "./tender.fixture";

test("authoring pending queue groups by response location, updates status with notes, and surfaces metrics in execution report", async ({ page }) => {
  const f = await fixture(page);
  f.view.pending_queue = [
    {
      gap_id: "gap-ch1-1",
      code: "export_enterprise_fact_pending",
      detail: "保证金金额没有企业资料，单元格留空待补。",
      chapter_id: "chapter-1",
      chapter_title: "第一章 投标函",
      target_id: "target-1",
      target_purpose: "投标函及保证金",
      bookmark: "kb_target_1",
      status: "pending",
      note: "",
      created_at: "2026-11-25T10:00:00Z",
      handled_at: null,
      handling_duration_ms: null,
    },
    {
      gap_id: "gap-ch2-1",
      code: "missing_material",
      detail: "项目人员资料待补。",
      chapter_id: "chapter-2",
      chapter_title: "第二章 实施团队",
      target_id: "target-2",
      target_purpose: "项目团队成员证书",
      bookmark: "kb_target_2",
      status: "pending",
      note: "",
      created_at: "2026-11-25T10:00:00Z",
      handled_at: null,
      handling_duration_ms: null,
    },
  ];
  f.state.report = {
    execution_id: "execution-word-v1",
    status: "succeeded",
    kind: "document",
    requests: 2,
    stages: {
      authoring_write: { stage: "W 写作 / 改稿", logical_requests: 2, requests: 2, reported_tokens: { total_tokens: 400 } },
    },
    failures: [],
    pending_queue: {
      total: 2,
      pending: 2,
      handled: 0,
      by_status: { pending: 2, resolved: 0, need_material: 0, accepted_gap: 0 },
      handling_duration_ms: { observations: 0, avg: null, max: null, p95: null },
    },
  };

  await f.ready();
  await openTool(page, "编制待核队列");
  const queue = page.getByRole("region", { name: "编制待核队列", exact: true });
  await expect(queue).toContainText("第一章 投标函");
  await expect(queue).toContainText("响应位置：投标函及保证金");
  await expect(queue).toContainText("第二章 实施团队");
  await expect(queue).toContainText("响应位置：项目团队成员证书");

  // Unsaved draft note survives a background workbench reload.
  const firstNote = queue.getByLabel("处理备注").nth(0);
  await firstNote.fill("已确认无保证金要求，接受空缺");
  await page.waitForTimeout(100);
  await expect(firstNote).toHaveValue("已确认无保证金要求，接受空缺");

  await queue.getByRole("button", { name: "接受空缺", exact: true }).nth(0).click();
  await expect.poll(() => f.state.pendingUpdates.length).toBe(1);
  expect(f.state.pendingUpdates[0]).toEqual({
    execution: "execution-word-v1",
    gapId: "gap-ch1-1",
    status: "accepted_gap",
    note: "已确认无保证金要求，接受空缺",
  });
  await expect(queue).toContainText("待处理");
  await expect(queue).toContainText("1 项");
  await expect(queue).toContainText("平均处理时长");
  await expect(queue).toContainText("25 秒");

  await queue.getByRole("button", { name: "标记补资料", exact: true }).nth(1).click();
  await expect.poll(() => f.state.pendingUpdates.length).toBe(2);
  await expect(queue.getByRole("link", { name: "到项目资料补充文件" })).toBeVisible();

  await page.keyboard.press("Escape");
  await openTool(page, "执行详情");
  const report = page.getByRole("region", { name: "执行报表", exact: true });
  await expect(report).toContainText("待核人工处理");
  await expect(report).toContainText("均值 25 秒");
  await expect(report).toContainText("2 项样本");
});
