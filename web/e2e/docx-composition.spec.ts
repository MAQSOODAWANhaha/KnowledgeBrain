import { expect, test, type Page } from "@playwright/test";
const project = "11111111-1111-4111-8111-111111111111";
const workspace = "22222222-2222-4222-8222-222222222222";
const requestId = "33333333-3333-4333-8333-333333333333";
async function fixture(page: Page) {
  const basis = { document_set_id: "documents", document_set_sha256: "a".repeat(64), requirement_set_id: "analysis", requirement_set_sha256: "b".repeat(64) };
  const identity = { request_artifact_id: requestId, request_revision: 1, frozen_input_sha256: "c".repeat(64) };
  const job = { ...identity, workspace_id: workspace, basis, expected: null, status: "pending", error_code: null,
    progress: { phase: "verifying", sequence: 4, attempt: 1, detail: { sections: 3 } },
    result_identity: { version_id: "generated-version", docx_sha256: "d".repeat(64), round_id: "round", round_revision: 1, composition_status: "reviewed_template_with_open_items" } };
  const ready = { value: true };
  const calls: { body: unknown; key: string | undefined }[] = [];
  let submitted = false; let lost = true;
  await page.addInitScript(() => localStorage.setItem("kb.token", "isolated-browser-fixture"));
  await page.route("**/api/**", async route => {
    const req = route.request(); const path = new URL(req.url()).pathname;
    const json = (value: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(value) });
    if (path === "/api/v1/me") return json({ id: "owner", email: "browser@local" });
    if (path === `/api/v2/bid-projects/${project}`) return json({ id: project, title: "按招标要求编制", status: "open", workspace_id: workspace });
    if (path.endsWith("/docx/current")) return json(null);
    if (path.endsWith("/docx-rounds/basis") || path.endsWith("/docx-compositions/basis")) return json(ready.value ? basis : null);
    if (path.endsWith("/docx-compositions/latest")) return json(submitted ? job : null);
    if (path.endsWith(`/docx-compositions/${requestId}`)) return json(job);
    if (path.endsWith("/docx-compositions") && req.method() === "POST") {
      calls.push({ body: req.postDataJSON(), key: req.headers()["idempotency-key"] }); submitted = true;
      if (lost) { lost = false; return route.abort("failed"); }
      return json({ ...identity, status: "pending" }, 202);
    }
    if (path.endsWith("/docx/versions/generated-version/download")) return route.fulfill({ status: 200, contentType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", body: "browser-only download fixture" });
    return json({ error: { code: "UNEXPECTED_TEST_ROUTE", message: path } }, 404);
  });
  return { calls, job, basis, ready };
}
test("generation survives a lost submission response and page reload without changing intent", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("docx-composition");
  await expect(panel.getByRole("button", { name: "生成完整投标模板", exact: true })).toBeEnabled();
  await panel.getByRole("button", { name: "生成完整投标模板", exact: true }).click();
  await expect(panel.getByRole("button", { name: "确认本次生成请求" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "上传已有 DOCX" })).toBeDisabled();
  await expect(panel.getByRole("button", { name: "返回", exact: true })).toBeDisabled();
  f.basis.document_set_id = "later-source";
  page.on("dialog", dialog => void dialog.accept());
  await page.reload();
  await panel.getByRole("button", { name: "确认本次生成请求" }).click();
  await expect(panel).toContainText("正在复核章节、附表及对应要求");
  expect(f.calls).toHaveLength(2); expect(f.calls[1]).toEqual(f.calls[0]);
  expect(f.calls[0].body).toMatchObject({ basis: { document_set_id: "documents" }, expected: null });
  f.job.status = "succeeded";
  await panel.getByRole("button", { name: "刷新生成进度" }).click();
  await expect(panel).toContainText("第 1 轮投标模板已生成");
  await expect(panel).toContainText("招标来源仍有待确认项");
  await expect(panel.getByRole("button", { name: "下载本次生成稿" })).toBeVisible();
});
test("an acknowledged background request reopens without a new submission and import remains available", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/authoring`);
  await page.getByRole("button", { name: "上传已有 DOCX" }).click();
  await expect(page.getByTestId("docx-round").locator('input[type="file"]')).toBeVisible();
  await page.getByRole("button", { name: "按招标要求生成" }).click();
  const panel = page.getByTestId("docx-composition");
  await panel.getByRole("button", { name: "生成完整投标模板", exact: true }).click();
  await panel.getByRole("button", { name: "确认本次生成请求" }).click();
  await expect(panel).toContainText("任务在后台继续执行");
  await page.reload();
  await expect(panel).toContainText("正在复核章节、附表及对应要求");
  expect(f.calls).toHaveLength(2);
});

test("an unanalysed initial requirement set cannot start composition", async ({ page }) => {
  const f = await fixture(page); f.ready.value = false;
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("docx-composition");
  await expect(panel).toContainText("请先完成招标文件分析");
  await expect(panel.getByRole("button", { name: "生成完整投标模板", exact: true })).toHaveCount(0);
  expect(f.calls).toHaveLength(0);
});
