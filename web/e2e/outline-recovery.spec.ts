import { expect, test } from "@playwright/test";
import { fixture, type WordTestWindow } from "./tender.fixture";

function seedHistory(f: Awaited<ReturnType<typeof fixture>>) {
  const historical = structuredClone(f.outline.draft!);
  historical.outline.chapters[0].title = "历史实施方案";
  const empty = { ...structuredClone(historical), revision_id: "outline-r2", revision: 2, parent_id: historical.revision_id, outline: { chapters: [] } };
  const latest = { ...structuredClone(historical), revision_id: "outline-r3", revision: 3, parent_id: empty.revision_id };
  latest.outline.chapters[0].title = "当前实施方案";
  f.outlineHistory.set(historical.revision_id, historical);
  f.outlineHistory.set(empty.revision_id, empty);
  f.outlineHistory.set(latest.revision_id, latest);
  f.outline.draft = latest;
}

test("restoring a historical outline keeps its own issues instead of the newest candidate issues", async ({ page }) => {
  const f = await fixture(page); seedHistory(f);
  f.outlineHistory.get('outline-r1')!.outline.issues = [{ code: 'historical_warning', detail: '历史大纲需要核对的原表' }];
  f.outline.candidate = { execution_id: 'different-candidate', sha256: 'b'.repeat(64),
    outline: { chapters: [], issues: [{ code: 'candidate_warning', detail: '另一候选的大纲问题' }] } };
  await f.ready();
  await page.getByRole('tab', { name: '大纲编排', exact: true }).click();
  await page.getByText('历史大纲', { exact: true }).click();
  await page.getByRole('button', { name: '读取历史修订', exact: true }).click();
  await page.getByRole('button', { name: '预览修订 1', exact: true }).click();
  await page.getByRole('button', { name: '载入此历史大纲另存', exact: true }).click();
  await page.getByText(/响应安排（/).click();
  await expect(page.getByText('历史大纲需要核对的原表', { exact: true })).toBeVisible();
  await expect(page.getByText('另一候选的大纲问题', { exact: true })).toHaveCount(0);
});

test("history preview preserves local and Word edits, then restores through a new CAS revision", async ({ page }) => {
  const f = await fixture(page); seedHistory(f); await f.ready(); await f.edit(); await f.dirty();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("尚未保存的本地编排");
  await page.getByText("历史大纲", { exact: true }).click();
  await page.getByRole("button", { name: "读取历史修订", exact: true }).click();
  await page.getByRole("button", { name: "预览修订 1", exact: true }).click();
  await expect(page.getByRole("table")).toContainText("历史实施方案");
  await expect(page.getByRole("table")).toContainText("尚未保存的本地编排");
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("尚未保存的本地编排");
  expect(f.state.outlineSaves).toHaveLength(0);
  page.once("dialog", dialog => void dialog.accept());
  await page.getByRole("button", { name: "载入此历史大纲另存", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("历史实施方案");
  await expect(page.getByRole("button", { name: /按修订.*生成正文/ })).toBeDisabled();
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 4");
  expect(f.state.outlineSaves[0].body).toMatchObject({ expected_revision_id: "outline-r3", source_revision_id: "outline-r1" });
  expect(f.outlineHistory.get("outline-r1")!.outline.chapters[0].title).toBe("历史实施方案");
  expect(f.outlineHistory.get("outline-r3")!.outline.chapters[0].title).toBe("当前实施方案");
  await page.getByRole("tab", { name: "正文编辑", exact: true }).click();
  await expect(page.getByTestId("docx-status")).toHaveText("有修改尚未保存");
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 1");
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
});

test("an empty historical outline can be restored and saved as an empty new revision", async ({ page }) => {
  const f = await fixture(page); seedHistory(f); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByText("历史大纲", { exact: true }).click();
  await page.getByRole("button", { name: "读取历史修订", exact: true }).click();
  await page.getByRole("button", { name: "预览修订 2", exact: true }).click();
  await page.getByRole("button", { name: "载入此历史大纲另存", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("未保存");
  await expect(page.getByRole("button", { name: "保存大纲草稿", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 4");
  expect(f.state.outlineSaves[0].body).toMatchObject({ expected_revision_id: "outline-r3", source_revision_id: "outline-r2", chapters: [] });
  expect(f.outline.draft!.outline.chapters).toEqual([]);
});

test("a removed response survives save and reload and can be arranged into another chapter", async ({ page }) => {
  const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  page.once("dialog", dialog => void dialog.accept());
  await page.getByRole("button", { name: "删除章节", exact: true }).click();
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  await page.reload();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByText("响应安排（1 项未安排）", { exact: true }).click();
  const recovery = page.getByRole("region", { name: "大纲历史与响应安排", exact: true });
  await expect(recovery).toContainText("说明实施方法");
  await expect(recovery).toContainText("尚未安排到章节");
  await recovery.getByRole("button", { name: "安排到当前章节", exact: true }).click();
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 3");
  expect(f.state.outlineSaves[1].body.source_revision_id).toBe("outline-r2");
  expect(f.state.outlineSaves[1].body.chapters[0]).toMatchObject({ chapter_id: "chapter-2", target_ids: ["target-1"] });
  expect(JSON.stringify(f.state.outlineSaves[1].body)).not.toContain("rule_refs");
  expect(f.outline.draft!.outline.chapters[0].targets[0].rule_refs).toEqual([{ item_ref: { item_id: "rule-1" } }]);
  await expect(page.getByText("响应安排（0 项未安排）", { exact: true })).toBeVisible();
});

test("an unknown save prevents restoring or discarding, and later discarding requires confirmation", async ({ page }) => {
  const f = await fixture(page); seedHistory(f); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("保存结果待确认的调整");
  f.state.loseOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByRole("button", { name: "确认同一次大纲保存", exact: true })).toBeEnabled();
  await page.getByText("历史大纲", { exact: true }).click();
  await page.getByRole("button", { name: "读取历史修订", exact: true }).click();
  await page.getByRole("button", { name: "预览修订 1", exact: true }).click();
  await expect(page.getByRole("button", { name: "载入此历史大纲另存", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "放弃当前调整", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "确认同一次大纲保存", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 4");
  await page.getByLabel("章节名称", { exact: true }).fill("准备放弃的调整");
  page.once("dialog", dialog => void dialog.dismiss());
  await page.getByRole("button", { name: "放弃当前调整", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("准备放弃的调整");
  page.once("dialog", dialog => void dialog.accept());
  await page.getByRole("button", { name: "放弃当前调整", exact: true }).click();
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("保存结果待确认的调整");
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 4");
});

test("successful history reads never hide a failed response catalog or claim zero unassigned responses", async ({ page }) => {
  const f = await fixture(page);
  f.outline.draft = { ...structuredClone(f.outline.draft!), revision_id: "outline-r2", revision: 2, parent_id: "outline-r1",
    outline: { chapters: [structuredClone(f.outline.draft!.outline.chapters[1])] } };
  f.outlineHistory.set("outline-r2", structuredClone(f.outline.draft));
  let failCatalog = true;
  await page.route("**/outline/revisions/outline-r2", route => failCatalog ? route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { message: "响应目录暂不可读取" } }) }) : route.fallback());
  await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  const recovery = page.getByRole("region", { name: "大纲历史与响应安排", exact: true });
  await expect(recovery.getByRole("alert")).toContainText("响应目录暂不可读取");
  await expect(recovery.getByText(/响应安排（响应目录待读取）/)).toBeVisible();
  await page.getByText("历史大纲", { exact: true }).click();
  await page.getByRole("button", { name: "读取历史修订", exact: true }).click();
  await page.getByRole("button", { name: "预览修订 1", exact: true }).click();
  await expect(recovery.getByRole("alert")).toContainText("响应目录暂不可读取");
  failCatalog = false;
  await recovery.getByRole("button", { name: "重新读取响应位置", exact: true }).click();
  await expect(recovery.getByText("响应安排（1 项未安排）", { exact: true })).toBeVisible();
  await expect(recovery.getByRole("alert")).toHaveCount(0);
});

test("structural warnings are deduplicated and clear only when their exact references are restored", async ({ page }) => {
  const f = await fixture(page);
  const original = structuredClone(f.outline.draft!);
  original.outline.chapters[0].targets[0].related_target_ids = ["target-2"];
  original.outline.chapters[1].targets = [
    { target_id: "target-2", kind: "attachment", purpose: "相关证明" },
    { target_id: "target-3", kind: "narrative", purpose: "关系待确认的响应" },
  ];
  f.outlineHistory.set("outline-r1", original);
  f.outline.draft = { ...structuredClone(original), revision_id: "outline-r2", revision: 2, parent_id: "outline-r1" };
  f.outline.draft.outline.chapters[1].targets = f.outline.draft.outline.chapters[1].targets.filter(target => target.target_id !== "target-2");
  f.outline.draft.outline.issues = [
    { code: "outline_edit_target_removed", target_id: "target-2", detail: "相关证明响应已移出" },
    { code: "outline_edit_target_removed", target_id: "target-2", detail: "相关证明响应已移出" },
    { code: "outline_edit_reference_unbound", target_id: "target-1", detail: "实施方法关联位置尚未安排" },
    { code: "outline_edit_reference_unbound", target_id: "target-1", detail: "实施方法关联位置尚未安排" },
    { code: "outline_edit_reference_unbound", target_id: "target-3", detail: "缺少关系元数据，仍需核对" },
  ];
  f.outlineHistory.set("outline-r2", structuredClone(f.outline.draft));
  await f.ready(); await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByText("响应安排（1 项未安排）", { exact: true }).click();
  const recovery = page.getByRole("region", { name: "大纲历史与响应安排", exact: true });
  await expect(recovery.getByText("相关证明响应已移出", { exact: true })).toHaveCount(1);
  await expect(recovery.getByText("实施方法关联位置尚未安排", { exact: true })).toHaveCount(1);
  const response = recovery.locator(".tender-info-card").filter({ has: page.getByText("相关证明", { exact: true }) });
  await response.getByRole("button", { name: "安排到当前章节", exact: true }).click();
  await expect(recovery.getByText("相关证明响应已移出", { exact: true })).toHaveCount(0);
  await expect(recovery.getByText("实施方法关联位置尚未安排", { exact: true })).toHaveCount(0);
  await expect(recovery.getByText("缺少关系元数据，仍需核对", { exact: true })).toBeVisible();
});

test("a newer candidate never replaces the response catalog of the working candidate", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); f.outline.draft = null;
  f.outline.candidate = { execution_id: "candidate-A", sha256: "a".repeat(64), outline: { chapters: [
    { chapter_id: "chapter-A", title: "候选甲章节", targets: [{ target_id: "target-A", kind: "narrative", purpose: "候选甲原响应" }], children: [] },
    { chapter_id: "chapter-destination", title: "保留章节", targets: [], children: [] },
  ] } };
  await f.ready(); await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "载入候选大纲", exact: true }).click();
  page.once("dialog", dialog => void dialog.accept());
  await page.getByRole("button", { name: "删除章节", exact: true }).click();
  await page.getByText("响应安排（1 项未安排）", { exact: true }).click();
  f.outline.candidate = { execution_id: "candidate-B", sha256: "b".repeat(64), outline: { chapters: [
    { chapter_id: "chapter-B", title: "候选乙章节", targets: [{ target_id: "target-B", kind: "narrative", purpose: "候选乙的其他响应" }], children: [] },
  ] } };
  await page.clock.runFor(5100);
  const recovery = page.getByRole("region", { name: "大纲历史与响应安排", exact: true });
  await expect(recovery).toContainText("候选甲原响应");
  await expect(recovery.getByText("候选乙的其他响应", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "载入候选大纲", exact: true })).toBeDisabled();
});
