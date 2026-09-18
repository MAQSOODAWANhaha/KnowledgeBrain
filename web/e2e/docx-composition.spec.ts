import { expect, test, type Page } from "@playwright/test";
const project = "11111111-1111-4111-8111-111111111111";
const workspace = "22222222-2222-4222-8222-222222222222";
const requestId = "33333333-3333-4333-8333-333333333333";
type DocxCurrent = {
  project_id: string; workspace_id: string; round_id: string; revision: number;
  version_id: string; docx_sha256: string;
  editor: { key: string | null; base_version_id: string | null; pending_save_id: string | null; save_error: null };
};
function savedDocx(editor: Partial<DocxCurrent["editor"]> = {}): DocxCurrent {
  return {
    project_id: project, workspace_id: workspace, round_id: "44444444-4444-4444-8444-444444444444",
    revision: 2, version_id: "55555555-5555-4555-8555-555555555555", docx_sha256: "d".repeat(64),
    editor: { key: null, base_version_id: null, pending_save_id: null, save_error: null, ...editor },
  };
}
async function fixture(page: Page) {
  const analysis = { current: { status: "succeeded", error_code: null as string | null, progress: { phase: "main", draft_stage: "published", turn: 4 } } as
    { status: string; error_code: string | null; progress: { phase?: string; draft_stage?: string; turn?: number; records?: number; review_rounds?: number; checkpoint_sequence?: number; boundary?: string } } | null };
  const docx = { current: null as DocxCurrent | null };
  const fill = { latest: null as unknown, posts: [] as unknown[], stops: [] as string[] };
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
    if (path.endsWith("/docx/current")) return json(docx.current);
    if (path.endsWith("/docx-compositions") && req.method() === "POST") {
      compositionPosts.push(req.postDataJSON());
      return json({ error: { code: "OFFICIAL_COMPOSITION_REMOVED", message: "use /docx-fills" } }, 410);
    }
    if (path.endsWith("/docx-fills/basis")) {
      return json({ document_set_id: "documents", document_set_sha256: "a".repeat(64), requirement_set_id: "analysis", requirement_set_sha256: "b".repeat(64) });
    }
    if (path.endsWith("/docx-fills/latest")) return json(fill.latest);
    if (path.endsWith("/docx-fills") && req.method() === "POST") {
      fill.posts.push(req.postDataJSON());
      return json({ request_artifact_id: requestId, request_revision: 1, frozen_input_sha256: "c".repeat(64) }, 202);
    }
    if (path.endsWith(`/docx-fills/${requestId}/stop`) && req.method() === "POST") {
      fill.stops.push(path);
      return json({ request_artifact_id: requestId, status: "pending", stop_requested_at: "2026-09-18T00:00:00Z" });
    }
    if (path.endsWith(`/docx-fills/${requestId}`)) {
      return json({
        request_artifact_id: requestId, request_revision: 1, frozen_input_sha256: "c".repeat(64),
        workspace_id: workspace, status: "pending", error_code: null, result_identity: null,
        progress: { phase: "main", sequence: 3, attempt: 1, detail: { draft_chapters: 9, draft_filled: 2, draft_active_title: "施工组织设计" } },
      });
    }
    // 编辑器脚本不在测试环境内；填章面板必须独立于它渲染。
    if (path.endsWith("/docx/editor")) return json({ error: { code: "DOCX_EDITOR_UNAVAILABLE", message: "no editor" } }, 503);
    return json({ error: { code: "UNEXPECTED_TEST_ROUTE", message: path } }, 404);
  });
  return { analysis, docx, fill, freezeCalls, continueCalls, compositionPosts };
}

test("a succeeded draft analysis never starts the composition agent", async ({ page }) => {
  await fixture(page);
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByTestId("analysis-progress")).toContainText("已完成章节大纲与骨架 Word");
  await expect(page.getByTestId("draft-ready")).toContainText("正文可自行编写，也可一键 AI 填充");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "生成章节大纲", exact: true })).toHaveCount(0);
});

test("an in-flight draft job does not open composition", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current!.status = "pending";
  f.analysis.current!.progress = { phase: "main", draft_stage: "outline", turn: 4 };
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await expect(panel).toContainText("正在生成章节大纲");
  await expect(panel).toContainText("第 4 步");
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "继续生成大纲" })).toBeEnabled();
  expect(f.compositionPosts).toHaveLength(0);
});

test("analysis failures stay visible", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current!.status = "failed"; f.analysis.current!.error_code = "AGENT_TURN_TIMEOUT";
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByTestId("analysis-progress")).toContainText("AGENT_TURN_TIMEOUT");
  await expect(page.getByRole("alert")).toContainText("AGENT_TURN_TIMEOUT");
  await expect(page.getByRole("button", { name: "生成章节大纲" })).toBeEnabled();
  await expect(page.getByTestId("docx-composition")).toHaveCount(0);
});

test("authoring starts analysis from the current files", async ({ page }) => {
  const f = await fixture(page);
  f.analysis.current = null;
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("analysis-progress");
  await panel.getByRole("button", { name: "生成章节大纲" }).click();
  await expect(panel).toContainText("正在生成章节大纲");
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
  await panel.getByRole("button", { name: "继续生成大纲" }).click();
  expect(f.continueCalls).toHaveLength(1);
  await expect(page.getByRole("button", { name: "生成章节大纲" })).toHaveCount(0);
});

test("filling a saved draft posts the version it fills and shows N/M progress", async ({ page }) => {
  const f = await fixture(page);
  f.docx.current = savedDocx();
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("docx-fill");
  await expect(panel).toContainText("新增章请使用标题样式");
  await panel.getByRole("button", { name: "一键填充整份" }).click();
  expect(f.fill.posts).toEqual([{
    basis: { document_set_id: "documents", document_set_sha256: "a".repeat(64), requirement_set_id: "analysis", requirement_set_sha256: "b".repeat(64) },
    expected: { version_id: f.docx.current.version_id, docx_sha256: f.docx.current.docx_sha256 },
  }]);
  await expect(page.getByTestId("docx-fill-progress")).toContainText("已填 2/9 章");
  await expect(page.getByTestId("docx-fill-progress")).toContainText("当前章：施工组织设计");
  expect(f.compositionPosts).toHaveLength(0);
});

test("stopping a fill asks once and says it finishes the current chapter first", async ({ page }) => {
  const f = await fixture(page);
  f.docx.current = savedDocx();
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("docx-fill");
  await panel.getByRole("button", { name: "一键填充整份" }).click();
  await panel.getByTestId("docx-fill-stop").click();
  await expect(page.getByTestId("docx-fill-progress")).toContainText("正在停止：当前章写完后收尾");
  await expect(panel.getByTestId("docx-fill-stop")).toBeDisabled();
  expect(f.fill.stops).toHaveLength(1);
});

test("an open editor session blocks filling instead of racing the save", async ({ page }) => {
  const f = await fixture(page);
  f.docx.current = savedDocx({ key: "editor-key" });
  await page.goto(`/#/bids/${project}/authoring`);
  const panel = page.getByTestId("docx-fill");
  await expect(panel).toContainText("请先保存并关闭编辑器");
  await expect(panel.getByRole("button", { name: "一键填充整份" })).toBeDisabled();
  expect(f.fill.posts).toHaveLength(0);
});
