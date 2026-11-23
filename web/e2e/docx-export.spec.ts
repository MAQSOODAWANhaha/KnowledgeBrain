import { expect, test } from "@playwright/test";
import { draft, fixture, path, project } from "./tender.fixture";

test("generated, manual and recovered drafts download their own immutable Word version", async ({ page }) => {
  const f = await fixture(page);
  f.view.versions = [draft(), { ...draft("word-manual", 2), source: "manual" }, { ...draft("word-recovered", 3), source: "recovered" }];
  await page.goto(`/#/bids/${project}/export`);
  await expect(page.getByRole("cell", { name: "编制生成稿", exact: true })).toBeVisible();
  await expect(page.getByRole("cell", { name: "人工保存稿", exact: true })).toBeVisible();
  await expect(page.getByRole("cell", { name: "会话恢复稿（待核对）", exact: true })).toBeVisible();
  for (const revision of [1, 2, 3]) {
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: `下载 DOCX v${revision}`, exact: true }).click();
    expect((await download).suggestedFilename()).toBe(`投标文件-v${revision}.docx`);
  }
  expect(f.state.downloads).toEqual([`${path}/docx/versions/word-v1/download`, `${path}/docx/versions/word-manual/download`, `${path}/docx/versions/word-recovered/download`]);
  expect(f.state.starts).toHaveLength(0);
});

test("a generated delivery package explains its boundary with later manual saves", async ({ page }) => {
  const f = await fixture(page);
  f.view.current_docx = { ...draft("word-manual", 2), source: "manual" };
  f.view.versions = [f.view.current_docx, draft()];
  f.view.artifacts = { package_url: `${path}/executions/execution-word-v1/package` };
  await page.goto(`/#/bids/${project}/export`);
  await expect(page.getByText("交付包对应原编制产物，不包含后续人工修改；人工保存稿请按上方版本下载。", { exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "下载编制交付包（包含待补清单）", exact: true }).click();
  expect((await download).suggestedFilename()).toBe("投标文件及附件.zip");
  expect(f.state.downloads).toEqual([`${path}/executions/execution-word-v1/package`]);
});

test("material gaps do not hide or block a persisted Word download", async ({ page }) => {
  const f = await fixture(page);
  await page.goto(`/#/bids/${project}/export`);
  await expect(page.getByRole("main").getByText("项目人员资料待补。", { exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "下载 DOCX v1", exact: true }).click();
  await download;
  expect(f.state.downloads).toEqual([`${path}/docx/versions/word-v1/download`]);
});

test("a PDF download stays bound to its saved version when a newer draft is published", async ({ page }) => {
  await page.clock.install();
  const f = await fixture(page);
  const first = f.view.versions[0];
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/docx/versions/*/pdf", async route => {
    const request = route.request();
    const version = f.view.versions.find(draft => request.url().endsWith(`/versions/${draft.id}/pdf`));
    expect(version).toBeDefined();
    expect(request.method()).toBe("POST");
    expect(request.postDataJSON()).toEqual({ docx_sha256: version!.sha256 });
    expect(request.headers()["authorization"]).toBe("Bearer tender-browser-fixture");
    if (version!.id === first.id) await gate;
    await route.fulfill({ contentType: "application/pdf", body: `%PDF-1.7\n${version!.id}`, headers: {
      "x-kb-render-source-version": version!.id,
      "x-kb-render-source-sha256": version!.sha256,
      "x-kb-render-pdf-sha256": "c".repeat(64),
    } });
  });
  await page.goto(`/#/bids/${project}/export`);
  const old = page.getByRole("button", { name: "下载 PDF v1", exact: true });
  await expect(old).toBeVisible();
  const oldRequest = page.waitForRequest(`**/docx/versions/${first.id}/pdf`);
  try {
    await old.click();
    await oldRequest;
    await expect(old).toBeDisabled();

    f.publish();
    f.view.current_docx!.sha256 = "b".repeat(64);
    await page.clock.runFor(5000);
    const newer = page.getByRole("button", { name: "下载 PDF v2", exact: true });
    await expect(newer).toBeEnabled();
    await expect(old).toBeDisabled();
    const newDownload = page.waitForEvent("download");
    await newer.click();
    expect((await newDownload).suggestedFilename()).toBe("投标文件-v2.pdf");

    const oldDownload = page.waitForEvent("download");
    release();
    expect((await oldDownload).suggestedFilename()).toBe("投标文件-v1.pdf");
    await expect(old).toBeEnabled();
  } finally {
    release();
  }
  expect(f.state.starts).toHaveLength(0);
});

test("PDF errors stay on the requested row and allow a retry of that version", async ({ page }) => {
  const f = await fixture(page);
  const version = f.view.versions[0];
  const headers = {
    "x-kb-render-source-version": version.id,
    "x-kb-render-source-sha256": version.sha256,
    "x-kb-render-pdf-sha256": "c".repeat(64),
  };
  let attempts = 0, downloads = 0;
  page.on("download", () => { downloads++; });
  await page.route(`**/docx/versions/${version.id}/pdf`, route => {
    attempts++;
    if (attempts === 1) return route.fulfill({ status: 503, json: { error: { message: "PDF 转换暂不可用" } } });
    return route.fulfill({ contentType: attempts === 4 ? "application/json" : "application/pdf", body: "%PDF-1.7", headers: {
      ...headers,
      "x-kb-render-source-version": attempts === 2 ? "wrong-version" : version.id,
      "x-kb-render-source-sha256": attempts === 3 ? "d".repeat(64) : version.sha256,
    } });
  });
  await page.goto(`/#/bids/${project}/export`);
  const row = page.getByRole("row").filter({ has: page.getByRole("cell", { name: "v1", exact: true }) });
  const button = row.getByRole("button", { name: "下载 PDF v1", exact: true });
  const alert = row.getByRole("alert").filter({ hasText: /\S/ });
  await expect(button).toBeVisible();
  for (const message of ["PDF 转换暂不可用", "PDF 响应无法与此已保存版本核对", "PDF 响应无法与此已保存版本核对", "PDF 响应无法与此已保存版本核对"]) {
    const response = page.waitForResponse(`**/docx/versions/${version.id}/pdf`);
    await button.click();
    await response;
    await expect(alert).toContainText(message);
    await expect(button).toBeEnabled();
    expect(downloads).toBe(0);
  }
  const download = page.waitForEvent("download");
  await button.click();
  expect((await download).suggestedFilename()).toBe("投标文件-v1.pdf");
  await expect(alert).toHaveCount(0);
  expect(attempts).toBe(5);
});
