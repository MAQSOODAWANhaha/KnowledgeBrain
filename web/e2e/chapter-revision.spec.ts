import { expect, test } from "@playwright/test";
import { fixture, type WordTestWindow } from "./tender.fixture";

test("volume and cover settings survive save/reload and can be explicitly cleared without replacing Word", async ({ page }) => {
  const f = await fixture(page); await f.ready(); await f.edit(); await f.dirty();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("导出为独立分册", { exact: true }).check();
  await page.getByLabel("章节展示方式", { exact: true }).selectOption("cover");
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  expect(f.state.outlineSaves[0].body.chapters[0]).toMatchObject({ chapter_id: "chapter-1", is_volume: true, presentation_role: "cover" });
  expect(f.outline.draft!.outline.chapters[0].targets[0].target_id).toBe("target-1");
  await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
  await expect(page.getByTestId("docx-status")).toHaveText("有未保存修改");
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
  page.on("dialog", dialog => void dialog.accept());
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await expect(page.getByLabel("导出为独立分册", { exact: true })).toBeChecked();
  await expect(page.getByLabel("章节展示方式", { exact: true })).toHaveValue("cover");
  await page.getByLabel("导出为独立分册", { exact: true }).uncheck();
  await page.getByLabel("章节展示方式", { exact: true }).selectOption("body");
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 3");
  expect(f.state.outlineSaves[1].body.chapters[0]).toMatchObject({ is_volume: false, presentation_role: "body" });
});

test("a chapter review uses real subtree response identities and leaves the current Word instance intact", async ({ page }) => {
  const f = await fixture(page);
  f.view.outline!.chapters = structuredClone(f.view.outline!.chapters);
  f.view.outline!.chapters[0].children = [{ chapter_id: "nested-chapter", title: "嵌套实施步骤", children: [], targets: [
    { target_id: "real-nested-response", kind: "form", purpose: "填写真实实施步骤" },
    { target_id: "signature-only", kind: "form", purpose: "人工签字", manual_fill: true },
  ] }];
  await f.ready();
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  const revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await expect(revision).toContainText("execution-word-v1");
  await expect(revision.getByRole("button", { name: "生成复核修订稿", exact: true })).toBeDisabled();
  await expect(revision.getByLabel("复核章节：项目人员", { exact: true })).toBeDisabled();
  await expect(revision.getByLabel("复核响应：人工签字", { exact: true })).toBeDisabled();
  await revision.getByLabel("复核章节：实施方案", { exact: true }).check();
  await expect(revision.getByLabel("复核响应：填写真实实施步骤", { exact: true })).toBeChecked();
  await expect(revision.getByRole("button", { name: "生成复核修订稿", exact: true })).toBeDisabled();
  await revision.getByLabel("复核与修订理由", { exact: true }).fill("  请核对实施节点与已上传资料的一致性。  ");
  await revision.getByRole("button", { name: "生成复核修订稿", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(1);
  expect(f.state.starts[0].body).toEqual({ previous_execution_id: "execution-word-v1", review_request: {
    target_ids: ["target-1", "real-nested-response"], reason: "请核对实施节点与已上传资料的一致性。",
  } });
  await expect(revision.getByLabel("复核与修订理由", { exact: true })).toHaveValue("");
  await expect(page.getByTestId("docx-status")).toHaveText("当前保存版本已载入");
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view"]);
});

test("a dirty Word draft prevents starting a review while retaining the editor and local choices", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  const revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await revision.getByLabel("复核响应：说明实施方法", { exact: true }).check();
  await revision.getByLabel("复核与修订理由", { exact: true }).fill("已选择的复核理由不能丢失。");
  await f.edit(); await f.dirty();
  await expect(revision.getByRole("button", { name: "生成复核修订稿", exact: true })).toBeDisabled();
  await expect(revision.getByLabel("复核与修订理由", { exact: true })).toHaveValue("已选择的复核理由不能丢失。");
  await expect(page.getByTestId("docx-status")).toHaveText("有未保存修改");
  expect(f.state.starts).toEqual([]);
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
});

test("an unknown review request keeps its exact generation, reason and request identity after reload", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  let revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await revision.getByLabel("复核响应：说明实施方法", { exact: true }).check();
  await revision.getByLabel("复核与修订理由", { exact: true }).fill("保留原范围并重新核对证明资料。");
  f.state.loseStart = true;
  await revision.getByRole("button", { name: "生成复核修订稿", exact: true }).click();
  await expect(revision.getByRole("button", { name: "确认同一次定向复核请求", exact: true })).toBeVisible();
  f.view.context_execution_id = "new-context-after-submit";
  f.view.outline = { chapters: [{ chapter_id: "other", title: "其他生成结果", children: [], targets: [
    { target_id: "other-response", kind: "narrative", purpose: "新响应" },
  ] }] };
  await page.reload();
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await expect(revision).toContainText("待确认请求的生成基准：execution-word-v1");
  await expect(revision.getByLabel("复核与修订理由", { exact: true })).toHaveValue("保留原范围并重新核对证明资料。");
  await expect(revision.getByLabel("复核与修订理由", { exact: true })).toBeDisabled();
  await expect(revision.getByLabel("复核响应：新响应", { exact: true })).toHaveCount(0);
  await revision.getByRole("button", { name: "确认同一次定向复核请求", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[1]).toEqual(f.state.starts[0]);
});

test("manual Word edits are explicitly outside the generated-content review input", async ({ page }) => {
  const f = await fixture(page); f.publish(); await f.ready();
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  const revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await expect(revision).toContainText("当前人工 Word v2 的编辑内容不作为本次复核输入。");
  await expect(revision).toContainText("现有 Word 稿继续保留，新稿生成后由你选择载入。");
  expect(f.state.starts).toEqual([]);
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
});

test("a changed generation cannot silently reuse the previous selection and its draft can be explicitly discarded", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  const revision = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await revision.getByLabel("复核响应：说明实施方法", { exact: true }).check();
  await revision.getByLabel("复核与修订理由", { exact: true }).fill("属于原生成执行的意见。");
  f.view.context_execution_id = "different-generation";
  await page.clock.runFor(5100);
  await expect(revision.getByRole("alert")).toContainText("生成稿已切换，原选择与理由仍保留。");
  await expect(revision).toContainText("execution-word-v1");
  await expect(revision.getByLabel("复核与修订理由", { exact: true })).toHaveValue("属于原生成执行的意见。");
  await expect(revision.getByRole("button", { name: "生成复核修订稿", exact: true })).toBeDisabled();
  await page.getByRole("navigation", { name: "产品", exact: true }).getByRole("link", { name: "知识资产", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "请先提交或放弃复核修改" })).toBeVisible();
  page.on("dialog", dialog => void dialog.accept());
  await revision.getByRole("button", { name: "放弃本次复核选择", exact: true }).click();
  await expect(revision.getByLabel("复核与修订理由", { exact: true })).toHaveValue("");
  await page.getByRole("navigation", { name: "项目分区" }).getByRole("link", { name: "项目资料" }).click();
  await expect(page.getByRole("heading", { name: "项目资料", exact: true })).toBeVisible();
  expect(f.state.starts).toEqual([]);
});
