import { expect, test, type Page } from "@playwright/test";
const project = "11111111-1111-4111-8111-111111111111";
const workspace = "22222222-2222-4222-8222-222222222222";
const requestId = "33333333-3333-4333-8333-333333333333";
async function fixture(page: Page, completed = false) {
  const basis = { document_set_id: "documents", document_set_sha256: "a".repeat(64), requirement_set_id: "analysis", requirement_set_sha256: "b".repeat(64) };
  const identity = { request_artifact_id: requestId, request_revision: 1, frozen_input_sha256: "c".repeat(64) };
  const job = { ...identity, workspace_id: workspace, basis, expected: null, status: completed ? "succeeded" : "pending", error_code: null,
    progress: { phase: "verifying", sequence: 4, attempt: 1, detail: { sections: 3 } },
    result_identity: { version_id: "generated-version", docx_sha256: "d".repeat(64), round_id: "round", round_revision: 1, composition_status: "reviewed_template_with_open_items" } };
  const ready = { value: true };
  const analysis = { current: { status: "succeeded", error_code: null as string | null, progress: { phase: "main", turn: 4, records: 12, review_rounds: 0 } } as
    { status: string; error_code: string | null; progress: { phase?: string; turn?: number; records?: number; review_rounds?: number; checkpoint_sequence?: number; boundary?: string } } | null };
  const documents = [{ id: "19191919-1919-1919-1919-191919191919", parse_status: "completed" }];
  const freezeCalls: unknown[] = [];
  const continueCalls: unknown[] = [];
  const calls: { body: unknown; key: string | undefined }[] = [];
  const report = { status: 200, body: JSON.stringify({ source_open_items: [{ reason: "原件附件待确认" }] }) };
  const downloads: { path: string; authorization: string | undefined }[] = [];
  let submitted = completed; let lost = true;
  await page.addInitScript(() => localStorage.setItem("kb.token", "isolated-browser-fixture"));
  await page.route("**/api/**", async route => {
    const req = route.request(); const path = new URL(req.url()).pathname;
    const json = (value: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(value) });
    if (path === "/api/v1/me") return json({ id: "owner", email: "browser@local" });
    if (path === `/api/v2/bid-projects/${project}`) return json({ id: project, title: "按招标要求编制", status: "open", workspace_id: workspace });
    if (path.endsWith("/requirement-set-compilations/latest/continue") && req.method() === "POST") {
      continueCalls.push(req.method());
      return json(analysis.current);
    }
    if (path.endsWith("/requirement-set-compilations/latest")) return json(analysis.current);
    if (path.endsWith("/tender-documents")) return json(documents);
    if (path.endsWith("/document-set-revisions") && req.method() === "GET") return json([]);
    if (path.endsWith("/document-set-revisions") && req.method() === "POST") {
      freezeCalls.push(req.postDataJSON());
      analysis.current = { status: "pending", error_code: null, progress: { phase: "main", turn: 0, records: 0, review_rounds: 0 } };
      return json({ request_artifact_id: requestId, request_revision: 1, request_sha256: "c".repeat(64) }, 202);
    }
    if (path.endsWith("/docx/current")) return json(null);
    if (path.endsWith("/docx-rounds/basis") || path.endsWith("/docx-compositions/basis")) return json(ready.value ? basis : null);
    if (path.endsWith("/docx-compositions/latest")) return json(submitted ? job : null);
    if (path.endsWith(`/docx-compositions/${requestId}`)) return json(job);
    if (path.endsWith("/docx-compositions") && req.method() === "POST") {
      calls.push({ body: req.postDataJSON(), key: req.headers()["idempotency-key"] }); submitted = true;
      if (lost) { lost = false; return route.abort("failed"); }
      return json({ ...identity, status: "pending" }, 202);
    }
    if (path.includes("/docx/versions/")) downloads.push({ path, authorization: req.headers()["authorization"] });
    if (path.endsWith("/docx/versions/generated-version/composition-report")) return report.status === 200
      ? route.fulfill({ status: 200, contentType: "application/json", body: report.body })
      : json({ error: { code: report.status === 404 ? "DOCX_COMPOSITION_REPORT_NOT_FOUND" : "DOCX_COMPOSITION_REPORT_UNAVAILABLE", message: "fixture report failure" } }, report.status);
    if (path.endsWith("/docx/versions/generated-version/download")) return route.fulfill({ status: 200, contentType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", body: "browser-only download fixture" });
    return json({ error: { code: "UNEXPECTED_TEST_ROUTE", message: path } }, 404);
  });
  return { calls, job, basis, ready, analysis, freezeCalls, continueCalls, report, downloads };
}
for (const openItems of [true, false]) {
  test(`the completed version report downloads with authentication, open items: ${openItems}`, async ({ page }) => {
    const f = await fixture(page, true);
    f.job.result_identity.composition_status = openItems ? "reviewed_template_with_open_items" : "reviewed_template";
    f.report.body = JSON.stringify({ source_open_items: openItems ? [{ reason: "原件附件待确认" }] : [] });
    await page.goto(`/#/bids/${project}/authoring`);
    const panel = page.getByTestId("docx-composition");
    await expect(panel).toContainText("第 1 轮投标模板已生成");
    // A later server result must not retarget the version already displayed.
    f.job.result_identity.version_id = "later-generated-version";
    const downloaded = page.waitForEvent("download");
    await panel.getByRole("button", { name: "下载本次编制报告", exact: true }).click();
    const download = await downloaded;
    expect(download.suggestedFilename()).toBe("编制报告-1.json");
    const stream = await download.createReadStream();
    const chunks: Buffer[] = [];
    for await (const chunk of stream!) chunks.push(Buffer.from(chunk));
    expect(Buffer.concat(chunks).toString("utf8")).toBe(f.report.body);
    expect(f.downloads).toEqual([{ path: `/api/v2/submission-workspaces/${workspace}/docx/versions/generated-version/composition-report`, authorization: "Bearer isolated-browser-fixture" }]);
    await expect(panel.getByRole("button", { name: "下载本次生成稿" })).toBeEnabled();
    expect(f.calls).toHaveLength(0);
  });
}
for (const status of [404, 503]) {
  test(`report download failure ${status} preserves the DOCX and regeneration controls`, async ({ page }) => {
    const f = await fixture(page, true); f.report.status = status;
    await page.goto(`/#/bids/${project}/authoring`);
    const panel = page.getByTestId("docx-composition");
    await panel.getByRole("button", { name: "下载本次编制报告", exact: true }).click();
    await expect(panel.getByRole("alert")).toContainText(status === 404 ? "本次生成版本没有可下载的编制报告" : "无法下载本次编制报告，请稍后重试");
    await expect(panel.getByRole("button", { name: "重新生成" })).toBeEnabled();
    const docxDownload = page.waitForEvent("download");
    await panel.getByRole("button", { name: "下载本次生成稿" }).click();
    expect((await docxDownload).suggestedFilename()).toBe("投标模板-1.docx");
    f.report.status = 200;
    const reportDownload = page.waitForEvent("download");
    await panel.getByRole("button", { name: "下载本次编制报告", exact: true }).click();
    await reportDownload;
    await expect(panel.getByRole("alert")).toHaveCount(0);
    expect(f.downloads.filter(value => value.path.endsWith("/composition-report"))).toHaveLength(2);
    expect(f.calls).toHaveLength(0);
  });
}
test("generation survives a lost submission response and page reload without changing intent", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("docx-composition");
  await expect(panel.getByRole("button", { name: "生成模板", exact: true })).toBeEnabled();
  await panel.getByRole("button", { name: "生成模板", exact: true }).click();
  await expect(panel.getByRole("button", { name: "确认本次生成请求" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "上传已有 DOCX" })).toHaveCount(0);
  await expect(panel.getByRole("button", { name: "返回", exact: true })).toHaveCount(0);
  f.basis.document_set_id = "later-source";
  page.on("dialog", dialog => void dialog.accept());
  await page.reload();
  await panel.getByRole("button", { name: "确认本次生成请求" }).click();
  await expect(panel).toContainText("正在复核章节、附表及对应要求");
  expect(f.calls).toHaveLength(2); expect(f.calls[1]).toEqual(f.calls[0]);
  expect(f.calls[0].body).toMatchObject({ basis: { document_set_id: "documents" }, expected: null });
  f.job.status = "succeeded";
  // Completion is discovered automatically, without a manual refresh button.
  await expect(panel).toContainText("第 1 轮投标模板已生成");
  await expect(panel).toContainText("招标来源仍有待确认项");
  await expect(panel.getByRole("button", { name: "下载本次生成稿" })).toBeVisible();
});
test("an acknowledged background request reopens without a new submission", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByRole("button", { name: "上传已有 DOCX" })).toHaveCount(0);
  const panel = page.getByTestId("docx-composition");
  await panel.getByRole("button", { name: "生成模板", exact: true }).click();
  await panel.getByRole("button", { name: "确认本次生成请求" }).click();
  await expect(panel).toContainText("任务在后台继续执行");
  await page.reload();
  await expect(panel).toContainText("正在复核章节、附表及对应要求");
  expect(f.calls).toHaveLength(2);
});

test("an unanalysed initial requirement set cannot start composition", async ({ page }) => {
  const f = await fixture(page); f.ready.value = false; f.analysis.current!.status = "pending";
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await expect(panel).toContainText("第 4 步");
  await expect(panel).toContainText("已提取 12 条");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "继续分析" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "开始分析" })).toHaveCount(0);
  await expect(page.getByText("刷新编制依据")).toHaveCount(0);
  await expect(page.getByText("尚未开始招标分析")).toHaveCount(0);
  await page.reload();
  await expect(panel).toContainText("已提取 12 条");
  f.analysis.current!.progress.phase = "reviewer";
  await expect(panel).toContainText("正在复核");
  f.analysis.current!.status = "succeeded"; f.ready.value = true;
  await expect(page.getByRole("button", { name: "生成模板", exact: true })).toBeEnabled();
  expect(f.calls).toHaveLength(0);
});

test("analysis failures stay visible", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current!.status = "failed"; f.analysis.current!.error_code = "AGENT_TURN_TIMEOUT";
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByTestId("analysis-progress")).toContainText("AGENT_TURN_TIMEOUT");
  await expect(page.getByRole("alert")).toContainText("AGENT_TURN_TIMEOUT");
  await expect(page.getByText("尚未开始招标分析")).toHaveCount(0);
  await expect(page.getByRole("link", { name: "前往文件页处理" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "开始分析" })).toBeEnabled();
  expect(f.calls).toHaveLength(0);
});

test("authoring starts analysis from the current files", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current = null;
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await expect(page.getByText("尚未开始招标分析")).toHaveCount(0);
  await expect(page.getByRole("link", { name: "前往文件页处理" })).toHaveCount(0);
  await panel.getByRole("button", { name: "开始分析" }).click();
  await expect(panel).toContainText("正在分析");
  expect(f.freezeCalls).toHaveLength(1);
});

test("a prepared model call still shows the current analysis step", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current = { status: "pending", error_code: null, progress: { boundary: "prepared", checkpoint_sequence: 52 } };
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await expect(panel).toContainText("正在等待模型");
  await expect(panel).toContainText("第 52 步");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await panel.getByRole("button", { name: "继续分析" }).click();
  expect(f.continueCalls).toHaveLength(1);
  await expect(page.getByRole("button", { name: "开始分析" })).toHaveCount(0);
});
