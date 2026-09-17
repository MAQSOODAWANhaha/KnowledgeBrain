import { expect, test, type Page } from "@playwright/test";

const project = "11111111-1111-4111-8111-111111111111";
const workspace = "22222222-2222-4222-8222-222222222222";
async function fixture(page: Page) {
  const current = { project_id: project, workspace_id: workspace, version_id: "saved-version", docx_sha256: "a".repeat(64),
    revision: 2, round_id: "round", editor: { key: "editor", base_version_id: "saved-version", pending_save_id: null as string | null, save_error: null } };
  const result = { manifest_id: "package", source: { version_id: "saved-version", docx_sha256: current.docx_sha256 },
    outputs: { docx: { artifact_id: "output-docx", sha256: current.docx_sha256, byte_length: 10 }, pdf: { artifact_id: "output-pdf", sha256: "b".repeat(64), byte_length: 11 } },
    assessment_report_id: "report", assessment_report_sha256: "c".repeat(64) };
  const job = { request_artifact_id: "export-request", request_revision: 1, frozen_input_sha256: "d".repeat(64),
    kind: "SubmissionExport", status: "pending", result_identity: result, error_code: null };
  const calls: { body: unknown; key: string | undefined; sha: string | undefined }[] = [];
  const downloads: string[] = []; const network = { loseReceipt: false };
  await page.addInitScript(() => localStorage.setItem("kb.token", "export-browser-fixture"));
  await page.route("**/api/**", async route => {
    const req = route.request(); const path = new URL(req.url()).pathname;
    const json = (value: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(value) });
    if (path === "/api/v1/me") return json({ id: "owner", email: "browser@local" });
    if (path === "/api/v2/bid-projects") return json([]);
    if (path === `/api/v2/bid-projects/${project}`) return json({ id: project, title: "导出测试", status: "open", workspace_id: workspace });
    if (path.endsWith("/docx/current")) return json(current);
    if (path.endsWith("/exports") && req.method() === "GET") return json([]);
    if (path.endsWith("/exports") && req.method() === "POST") {
      calls.push({ body: req.postDataJSON(), key: req.headers()["idempotency-key"], sha: req.headers()["if-match"] });
      if (network.loseReceipt) { network.loseReceipt = false; return route.abort("failed"); }
      return json(job, 202);
    }
    if (path.endsWith("/requests/export-request")) return json(job);
    if (path.endsWith("/download") || path.endsWith("/assessment-report")) {
      expect(req.headers()["authorization"]).toBe("Bearer export-browser-fixture"); downloads.push(path);
      return route.fulfill({ status: 200, contentType: "application/octet-stream", body: "isolated browser download fixture" });
    }
    return json({ error: { code: "UNEXPECTED_TEST_ROUTE", message: path } }, 404);
  });
  return { current, result, job, calls, downloads, network };
}
test("lost export receipt survives reload and retains exact version, key and request", async ({ page }) => {
  const f = await fixture(page); f.network.loseReceipt = true;
  await page.goto(`/#/bids/${project}/export`);
  const panel = page.getByTestId("docx-export");
  await expect(panel).toContainText("终稿需另一次独立复核请求");
  await panel.getByRole("button", { name: "生成导出文件" }).click();
  await expect(panel.getByRole("button", { name: "确认本次导出请求" })).toBeVisible();
  f.current.version_id = "later-version"; page.on("dialog", dialog => void dialog.accept()); await page.reload();
  await panel.getByRole("button", { name: "确认本次导出请求" }).click();
  await expect(panel).toContainText("正在转换 PDF");
  expect(f.calls).toHaveLength(2); expect(f.calls[0]).toEqual(f.calls[1]);
  expect(f.calls[0].body).toEqual({ version_id: "saved-version" }); expect(f.calls[0].sha).toBe(f.current.docx_sha256);
  await page.reload(); await expect(panel).toContainText("正在转换 PDF"); expect(f.calls).toHaveLength(2);
  f.job.status = "succeeded"; await expect(panel.getByRole("button", { name: "下载 PDF", exact: true })).toBeVisible();
  await expect(panel).toContainText("文件对应保存版本：saved-version");
  await expect(panel).toContainText("尚不能证明内容、附表对应关系和版式已经通过验收");
});
test("three downloads use frozen outputs after the current draft changes", async ({ page }) => {
  const f = await fixture(page); f.job.status = "succeeded";
  await page.goto(`/#/bids/${project}/export`); const panel = page.getByTestId("docx-export");
  await panel.getByRole("button", { name: "生成导出文件" }).click();
  await expect(panel.getByRole("button", { name: "下载 DOCX", exact: true })).toBeVisible(); f.current.version_id = "newer-version";
  for (const name of ["下载 DOCX", "下载 PDF", "下载校核报告"]) {
    const download = page.waitForEvent("download"); await panel.getByRole("button", { name, exact: true }).click();
    expect((await download).suggestedFilename()).toContain("saved-version");
  }
  expect(f.downloads).toEqual([`/api/v2/submission-workspaces/${workspace}/exports/output-docx/download`,
    `/api/v2/submission-workspaces/${workspace}/exports/output-pdf/download`,
    `/api/v2/submission-workspaces/${workspace}/exports/package/assessment-report`]);
  expect(f.calls).toHaveLength(1);
});
test("pending save blocks a new export and refresh restores the control", async ({ page }) => {
  const f = await fixture(page); f.current.editor.pending_save_id = "saving";
  await page.goto(`/#/bids/${project}/export`); const panel = page.getByTestId("docx-export");
  await expect(panel).toContainText("请返回编制页面完成保存后再导出");
  await expect(panel.getByRole("button", { name: "生成导出文件" })).toHaveCount(0); expect(f.calls).toHaveLength(0);
  f.current.editor.pending_save_id = null; await panel.getByRole("button", { name: "刷新状态" }).click();
  await expect(panel.getByRole("button", { name: "生成导出文件" })).toBeEnabled(); expect(f.calls).toHaveLength(0);
});
