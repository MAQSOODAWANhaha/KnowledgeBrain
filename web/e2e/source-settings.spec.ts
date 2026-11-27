import { expect, test, type Page } from "@playwright/test";
import { fixture, path, project } from "./tender.fixture";

async function sourcesFixture(page: Page, ended = false) {
  const f = await fixture(page);
  if (ended) f.view.project.status = "ended";
  f.view.documents.push({ id: "amendment", file_name: "合成补遗.pdf", media_type: "application/pdf", byte_length: 1024 });
  const state = {
    settings: {
      version: "a".repeat(64),
      expected_document_ids: [] as string[],
      documents: f.view.documents.map(document => ({
        document_id: document.id, file_name: document.file_name, role: "main", replaces: null as string | null,
        modification_scope: null as string | null,
      })),
    },
    writes: [] as Array<{ body: unknown; key?: string }>,
    loseSave: false,
  };
  await page.route(`**${path}/sources`, async route => {
    if (route.request().method() === "GET") return route.fulfill({ json: { source_settings: state.settings } });
    const body = route.request().postDataJSON();
    const key = route.request().headers()["idempotency-key"];
    const replayed = key && state.writes.some(write => write.key === key && JSON.stringify(write.body) === JSON.stringify(body));
    state.writes.push({ body, key });
    if (replayed) return route.fulfill({ json: { source_settings: state.settings } });
    if (body.expected_version !== state.settings.version) return route.fulfill({ status: 409, json: { error: { message: "来源设置已变化" } } });
    state.settings = { version: "b".repeat(64), expected_document_ids: body.expected_document_ids,
      documents: state.settings.documents.map((document, index) => ({ ...document, ...body.documents[index] })) };
    if (state.loseSave) { state.loseSave = false; return route.abort("failed"); }
    return route.fulfill({ json: { source_settings: state.settings } });
  });
  await page.goto(`/#/bids/${project}/files`);
  const editor = page.getByRole("region", { name: "来源设置", exact: true });
  return { f, state, editor };
}

test("source settings preserve operator edits through project polling and submit the complete versioned manifest", async ({ page }) => {
  await page.clock.install();
  const { editor, state } = await sourcesFixture(page);
  await page.getByRole("link", { name: "合成补遗.pdf", exact: true }).click();
  const scope = editor.getByLabel("适用范围", { exact: true });
  await scope.fill("替换评分表第 1 行");
  await editor.getByRole("combobox", { name: "文件角色", exact: true }).click();
  await page.getByRole("option", { name: "补遗", exact: true }).click();
  await editor.getByRole("combobox", { name: "替代文件", exact: true }).click();
  await page.getByRole("option", { name: "合成招标.pdf", exact: true }).click();
  await editor.getByLabel("必要缺件（每行一项）", { exact: true }).fill("  报价附件\n\n答疑附件 ");
  await page.clock.runFor(5200);
  await expect(scope).toHaveValue("替换评分表第 1 行");
  await page.getByTestId("wizard-analysis").click();
  await page.getByTestId("wizard-files").click();
  await expect(scope).toHaveValue("替换评分表第 1 行");
  await editor.getByRole("button", { name: "保存来源设置", exact: true }).click();
  await expect(editor.getByText("来源设置已保存。", { exact: true })).toBeVisible();
  expect(state.writes).toHaveLength(1);
  expect(state.writes[0].key).toBeTruthy();
  expect(state.writes[0].body).toEqual({
    expected_version: "a".repeat(64),
    documents: [
      { document_id: "tender", role: "main", replaces: null, modification_scope: null },
      { document_id: "amendment", role: "amendment", replaces: "tender", modification_scope: "替换评分表第 1 行" },
    ],
    expected_document_ids: ["报价附件", "答疑附件"],
  });
});

test("a stale source editor keeps local changes until the operator explicitly reloads", async ({ page }) => {
  const { editor, state } = await sourcesFixture(page);
  const scope = editor.getByLabel("适用范围", { exact: true });
  await scope.fill("当前修改");
  state.settings = { ...state.settings, version: "c".repeat(64),
    documents: state.settings.documents.map(document => ({ ...document, modification_scope: "其他页面保存的范围" })) };
  await editor.getByRole("button", { name: "保存来源设置", exact: true }).click();
  await expect(editor.getByRole("alert")).toContainText("来源设置已变化");
  await expect(scope).toHaveValue("当前修改");
  await expect(editor.getByRole("button", { name: "保存来源设置", exact: true })).toBeDisabled();
  page.once("dialog", dialog => dialog.accept());
  await editor.getByRole("button", { name: "放弃修改并载入最新设置", exact: true }).click();
  await expect(scope).toHaveValue("其他页面保存的范围");
});

test("ended projects can read source settings on a narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { editor, state } = await sourcesFixture(page, true);
  await expect(editor.getByLabel("适用范围", { exact: true })).toBeDisabled();
  await expect(editor.getByLabel("必要缺件（每行一项）", { exact: true })).toBeDisabled();
  await expect(editor.getByRole("button", { name: "保存来源设置", exact: true })).toHaveCount(0);
  expect(state.writes).toHaveLength(0);
});

test("an unknown source save retries the same body and idempotency key", async ({ page }) => {
  const { editor, state } = await sourcesFixture(page);
  const scope = editor.getByLabel("适用范围", { exact: true });
  await scope.fill("报价修正");
  state.loseSave = true;
  await editor.getByRole("button", { name: "保存来源设置", exact: true }).click();
  const retry = editor.getByRole("button", { name: "确认同一次来源设置保存", exact: true });
  await expect(retry).toBeEnabled();
  await expect(scope).toBeDisabled();
  await retry.click();
  await expect(editor.getByText("来源设置已保存。", { exact: true })).toBeVisible();
  expect(state.writes).toHaveLength(2);
  expect(state.writes[0]).toEqual(state.writes[1]);
});
