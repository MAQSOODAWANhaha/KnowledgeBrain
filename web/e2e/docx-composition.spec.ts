import { expect, test, type Page } from "@playwright/test";
const project = "11111111-1111-4111-8111-111111111111";
const workspace = "22222222-2222-4222-8222-222222222222";
const requestId = "33333333-3333-4333-8333-333333333333";
async function fixture(page: Page) {
  const analysis = { current: { status: "succeeded", error_code: null as string | null, progress: { phase: "main", draft_stage: "published", turn: 4 } } as
    { status: string; error_code: string | null; progress: { phase?: string; draft_stage?: string; turn?: number; records?: number; review_rounds?: number; checkpoint_sequence?: number; boundary?: string } } | null };
  const documents = [{ id: "19191919-1919-1919-1919-191919191919", parse_status: "completed" }];
  const freezeCalls: unknown[] = [];
  const continueCalls: unknown[] = [];
  const compositionPosts: unknown[] = [];
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
    if (path.endsWith("/tender-outline")) return json({ quality: "draft", extracted_from: "none", extracted: [], documents: [] });
    if (path.endsWith("/document-set-revisions") && req.method() === "GET") return json([]);
    if (path.endsWith("/document-set-revisions") && req.method() === "POST") {
      freezeCalls.push(req.postDataJSON());
      analysis.current = { status: "pending", error_code: null, progress: { phase: "main", draft_stage: "outline", turn: 0 } };
      return json({ request_artifact_id: requestId, request_revision: 1, request_sha256: "c".repeat(64) }, 202);
    }
    if (path.endsWith("/docx/current")) return json(null);
    if (path.endsWith("/docx-compositions") && req.method() === "POST") {
      compositionPosts.push(req.postDataJSON());
      return json({ error: { code: "OFFICIAL_COMPOSITION_REJECTS_DRAFT", message: "draft" } }, 409);
    }
    return json({ error: { code: "UNEXPECTED_TEST_ROUTE", message: path } }, 404);
  });
  return { analysis, freezeCalls, continueCalls, compositionPosts };
}

test("a succeeded draft analysis never starts the composition agent", async ({ page }) => {
  await fixture(page);
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByTestId("analysis-progress")).toContainText("已完成草稿模板");
  await expect(page.getByTestId("draft-ready")).toContainText("不会启动编制 Agent");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "生成模板", exact: true })).toHaveCount(0);
});

test("an in-flight draft job does not open composition", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current!.status = "pending";
  f.analysis.current!.progress = { phase: "main", draft_stage: "outline", turn: 4 };
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await expect(panel).toContainText("正在生成大纲");
  await expect(panel).toContainText("第 4 步");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "继续分析" })).toBeEnabled();
  f.analysis.current!.progress.draft_stage = "fill";
  await expect(panel).toContainText("正在按章填写模板");
  expect(f.compositionPosts).toHaveLength(0);
});

test("analysis failures stay visible", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current!.status = "failed"; f.analysis.current!.error_code = "AGENT_TURN_TIMEOUT";
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByTestId("analysis-progress")).toContainText("AGENT_TURN_TIMEOUT");
  await expect(page.getByRole("alert")).toContainText("AGENT_TURN_TIMEOUT");
  await expect(page.getByRole("button", { name: "开始分析" })).toBeEnabled();
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
});

test("authoring starts analysis from the current files", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current = null;
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await panel.getByRole("button", { name: "开始分析" }).click();
  await expect(panel).toContainText("正在生成大纲");
  expect(f.freezeCalls).toHaveLength(1);
  expect(f.compositionPosts).toHaveLength(0);
});

test("a prepared model call still shows the current analysis step", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current = { status: "pending", error_code: null, progress: { boundary: "prepared", checkpoint_sequence: 52, draft_stage: "outline" } };
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await expect(panel).toContainText("正在等待模型");
  await expect(panel).toContainText("第 52 步");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await panel.getByRole("button", { name: "继续分析" }).click();
  expect(f.continueCalls).toHaveLength(1);
  await expect(page.getByRole("button", { name: "开始分析" })).toHaveCount(0);
});
