import { expect, test } from "@playwright/test";
import { fixture, project, type WordTestWindow } from "./tender.fixture";

test("a first project opens outline planning and generates only an outline candidate", async ({ page }) => {
  const f = await fixture(page); f.state.current = null; f.view.current_docx = null; f.view.versions = [];
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByRole("tab", { name: "大纲编排", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "生成大纲", exact: true }).click();
  await expect.poll(() => f.state.starts.filter(item => item.kind === "outline").length).toBe(1);
  expect(f.state.starts.filter(item => item.kind === "authoring")).toHaveLength(0);
});

test("a failed outline save retains the user's title and never claims persistence", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("人工调整的实施方案");
  f.state.failOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "大纲保存未完成" })).toBeVisible();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("人工调整的实施方案");
  await expect(page.getByTestId("outline-save-status")).toContainText("未保存");
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
});

test("an unknown outline save survives reload and confirms the exact same request", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("待确认的结构调整");
  f.state.loseOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次大纲保存", exact: true })).toBeVisible();
  page.on("dialog", dialog => void dialog.accept());
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "确认同一次大纲保存", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toContainText("已保存修订 2");
  expect(f.state.outlineSaves).toHaveLength(2);
  expect(f.state.outlineSaves[0]).toEqual(f.state.outlineSaves[1]);
});

test("outline editing and saving preserve the open dirty Word instance", async ({ page }) => {
  const f = await fixture(page); await f.ready(); await f.edit(); await f.dirty();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("不替换 Word 的大纲");
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toContainText("已保存修订 2");
  await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
  await expect(page.getByTestId("docx-status")).toHaveText("有未保存修改");
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.destroyed)).toBe(1);
});

test("the default read-only reload releases completed save status while keeping the original Word version", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page);
  f.state.current!.editor.pending_save_id = "pending-before-reload";
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByTestId("docx-status")).toHaveText("保存中");
  const initialOpens = f.state.opens.length;
  f.publish(); await page.clock.runFor(1100);
  await expect(page.getByTestId("docx-status")).toHaveText("当前保存版本已载入");
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 1");
  await expect(page.getByTestId("docx-mode")).toBeEnabled();
  expect(f.state.opens).toHaveLength(initialOpens);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view"]);
  await page.getByRole("navigation", { name: "项目分区" }).getByRole("link", { name: "项目资料" }).click();
  await expect(page).toHaveURL(new RegExp(`/bids/${project}/files$`));
});

test("chapter arrangement saves stable identities, manual goals and an editable-only DTO", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "新增子章节", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("项目实施进度");
  await expect(page.getByText("填写目标后为该章节生成正文；留空只保留章节标题。", { exact: true })).toBeVisible();
  await page.getByLabel("编写目标", { exact: true }).fill("说明阶段安排和交付节点");
  await page.getByRole("button", { name: "提升一级", exact: true }).click();
  await page.getByRole("button", { name: "上移", exact: true }).click();
  await expect(page.getByRole("button", { name: "降为子级", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "下移", exact: true }).click();
  await page.getByRole("button", { name: "降为子级", exact: true }).click();
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  expect(f.outline.draft!.outline.chapters[0].is_volume).toBe(false);
  expect(f.outline.draft!.outline.chapters[0].presentation_role).toBe("body");
  const body = f.state.outlineSaves[0].body;
  expect(body.source_revision_id).toBe("outline-r1");
  expect(body.chapters.map(node => node.chapter_id)).toEqual(["chapter-1", "chapter-2"]);
  expect(body.chapters[0].children[0].chapter_id).toMatch(/^manual-[0-9a-f-]{36}$/);
  expect(body.chapters[0].children[0].writing_goal).toBe("说明阶段安排和交付节点");
  expect(JSON.stringify(body)).not.toMatch(/targets|rule_refs|bookmarks|target-1/);
  await page.locator(".outline-chapter-editor").evaluate(element => { element.scrollTop = 0; });
  await page.screenshot({ path: testInfo.outputPath("outline-planner.png"), fullPage: true });
  await page.getByLabel("编写目标", { exact: true }).fill("");
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 3");
  expect(f.state.outlineSaves[1].body.chapters[0].children[0].writing_goal).toBe("");
  expect(f.outline.draft!.outline.chapters[0].targets[0].target_id).toBe("target-1");
  expect(f.outline.draft!.outline.chapters[0].children[0].targets).toEqual([]);
});

test("remote candidates and newer revisions never replace a dirty arrangement", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("仍在编排的本地章节");
  f.outline.candidate = { execution_id: "new-candidate", sha256: "b".repeat(64), outline: { chapters: [{ chapter_id: "candidate", title: "新候选章节", targets: [], children: [] }] } };
  f.outline.draft = { ...f.outline.draft!, revision_id: "outline-r2", revision: 2 };
  await page.clock.runFor(5100);
  await expect(page.getByRole("button", { name: "载入候选大纲", exact: true })).toBeDisabled();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("仍在编排的本地章节");
  await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 1");
  expect(f.state.opens).toHaveLength(1);
});

test("confirming an earlier unknown save preserves edits made afterwards", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("已发送版本");
  f.state.loseOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次大纲保存", exact: true })).toBeEnabled();
  await page.getByLabel("章节名称", { exact: true }).fill("发送后继续调整的版本");
  await page.getByRole("button", { name: "确认同一次大纲保存", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("有未保存修改");
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("发送后继续调整的版本");
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
  expect(f.state.outlineSaves[0]).toEqual(f.state.outlineSaves[1]);
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 3");
  expect(f.state.outlineSaves[2].body.expected_revision_id).toBe("outline-r2");
});

test("repeated unknown retries preserve later edits across reload without changing the original request", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("首发保存 A");
  f.state.loseOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次大纲保存", exact: true })).toBeEnabled();
  await page.getByLabel("章节名称", { exact: true }).fill("后续编辑 B");
  let failRetry = true;
  const retried: typeof f.state.outlineSaves = [];
  await page.route("**/outline", async route => {
    if (route.request().method() !== "PUT" || !failRetry) return route.fallback();
    retried.push({ body: route.request().postDataJSON(), key: route.request().headers()["idempotency-key"] });
    await route.abort("failed");
  });
  await page.getByRole("button", { name: "确认同一次大纲保存", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次大纲保存", exact: true })).toBeEnabled();
  page.on("dialog", dialog => void dialog.accept());
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("后续编辑 B");
  expect(retried[0]).toEqual(f.state.outlineSaves[0]);
  failRetry = false;
  await page.getByRole("button", { name: "确认同一次大纲保存", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("有未保存修改");
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("后续编辑 B");
  expect(f.state.outlineSaves[1]).toEqual(f.state.outlineSaves[0]);
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 3");
  expect(f.state.outlineSaves[2].body.chapters[0].title).toBe("后续编辑 B");
});

test("body generation freezes the saved revision and hash across an unknown result and reload", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("用于生成的修订");
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  f.state.loseStart = true;
  await page.getByRole("button", { name: "按修订 2 生成正文", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次编制请求", exact: true })).toBeVisible();
  expect(f.state.starts[0].body).toEqual({ outline_revision_id: "outline-r2", outline_sha256: "a".repeat(64) });
  f.outline.draft = { ...f.outline.draft!, revision_id: "outline-r3", revision: 3, sha256: "c".repeat(64) };
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "确认同一次编制请求", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[0]).toEqual(f.state.starts[1]);
});

test("deleting all chapters saves an empty outline and generates from that exact revision", async ({ page }) => {
  const f = await fixture(page); await f.ready(); page.on("dialog", dialog => void dialog.accept());
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "删除章节", exact: true }).click();
  await page.getByRole("button", { name: "删除章节", exact: true }).click();
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  expect(f.state.outlineSaves[0].body.chapters).toEqual([]);
  await page.getByRole("button", { name: "按修订 2 生成正文", exact: true }).click();
  expect(f.state.starts[0].body).toEqual({ outline_revision_id: "outline-r2", outline_sha256: "a".repeat(64) });
});

test("unsaved outline changes block leaving the project in both views", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("未保存编排");
  await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
  await page.evaluate(() => { location.hash = "/products"; });
  await expect(page).toHaveURL(new RegExp(`/bids/${project}/authoring$`));
  await expect(page.getByRole("alert").filter({ hasText: "请先保存并确认大纲调整" })).toBeVisible();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("未保存编排");
});

test("a project with no outline source must generate and explicitly adopt a candidate first", async ({ page }) => {
  const f = await fixture(page); f.outline.draft = null; f.state.current = null; f.view.current_docx = null; f.view.versions = [];
  await page.goto(`/#/bids/${project}/authoring`);
  await expect(page.getByRole("button", { name: "新增顶级章节", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "保存大纲草稿", exact: true })).toBeDisabled();
  expect(f.state.opens).toHaveLength(0);
  f.outline.candidate = { execution_id: "fresh-candidate", sha256: "b".repeat(64), outline: { chapters: [{ chapter_id: "new-chapter", title: "可编排章节", targets: [], children: [] }] } };
  await page.getByRole("button", { name: "生成大纲", exact: true }).click();
  await page.getByRole("button", { name: "载入候选大纲", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("可编排章节");
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 1");
  expect(f.state.outlineSaves[0].body.source_execution_id).toBe("fresh-candidate");
});

test("source changes discovered by polling disable generation without replacing local edits", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeEnabled();
  f.outline.draft!.sources_current = false;
  await page.clock.runFor(5100);
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
  await expect(page.getByRole("alert").filter({ hasText: "项目来源已变化" })).toBeVisible();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("实施方案");
  expect(f.state.starts).toHaveLength(0);
});

test("a conflict offers explicit full-structure replacement while retaining local chapters", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("冲突后保留的编排");
  f.state.failOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByText("当前编排与最新修订可能不同。保留当前编排另存会用当前全部章节替换最新结构；也可载入最新大纲后重新调整。", { exact: true })).toBeVisible();
  f.outline.draft = { ...f.outline.draft!, revision_id: "outline-other", revision: 2 };
  await page.getByRole("button", { name: "保留当前编排，按最新修订另存", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("冲突后保留的编排");
  f.state.failOutlineSave = false;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 3");
  expect(f.state.outlineSaves[1].body.expected_revision_id).toBe("outline-other");
  expect(f.state.outlineSaves[1].body.source_revision_id).toBe("outline-r1");
});

test("native drag supports nesting and rejects dragging a parent into its descendant", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  const rootRows = page.locator(".outline-arrangement > ol > li > button");
  await rootRows.nth(1).dragTo(rootRows.nth(0));
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  expect(f.state.outlineSaves[0].body.chapters[0].children[0].chapter_id).toBe("chapter-2");
  await rootRows.nth(0).dragTo(page.locator(".outline-arrangement > ol > li > ol > li > button"));
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  await expect(page.getByRole("button", { name: "保存大纲草稿", exact: true })).toBeDisabled();
});

test("an unknown outline generation restores the same request without auto-retrying", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  f.state.loseOutlineStart = true;
  await page.getByRole("button", { name: "生成大纲", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次大纲生成请求", exact: true })).toBeEnabled();
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  expect(f.state.starts).toHaveLength(1);
  await page.getByRole("button", { name: "确认同一次大纲生成请求", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[0]).toEqual(f.state.starts[1]);
});

test("confirming a pending body request works even when the outline head is temporarily absent", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  f.state.loseStart = true;
  await page.getByRole("button", { name: "按修订 1 生成正文", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次编制请求", exact: true })).toBeVisible();
  f.outline.draft = null;
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "确认同一次编制请求", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[0]).toEqual(f.state.starts[1]);
});
