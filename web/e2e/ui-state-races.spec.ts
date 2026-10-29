import { expect, test, type Page, type Request } from "@playwright/test";
import type { AnalysisReviewView } from "../src/bid/api/analysisReview";
import type { AnalysisRecoveryView } from "../src/bid/api/analysisRecovery";
import type { SavedWordReview } from "../src/bid/api/savedWordReview";
import { fixture, metadata, project } from "./tender.fixture";

async function holdNextRead(page: Page, pattern: string, body: unknown, status = 200) {
  let release!: () => void, observed!: () => void, held: Request | undefined;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const received = new Promise<void>(resolve => { observed = resolve; });
  await page.route(pattern, async route => {
    if (held || route.request().method() !== "GET") return route.fallback();
    held = route.request(); observed(); await gate;
    await route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  });
  return { received, async release() {
    release(); await (await held!.response())?.finished();
    await page.evaluate(async () => {
      for (let turn = 0; turn < 2; turn++) await new Promise<void>(resolve => {
        const channel = new MessageChannel();
        channel.port1.onmessage = () => { channel.port1.close(); channel.port2.close(); resolve(); };
        channel.port2.postMessage(null);
      });
    });
  } };
}

async function analysisFixture(page: Page) {
  const f = await fixture(page);
  const review: AnalysisReviewView = { run_id: "analysis", source_version: 1, domain_revision: 1,
    status: "waiting-confirmation", checklist_digest: "c".repeat(64), sources_current: true,
    hard_blocking: false, human_issue_ids: [], issues: [],
    correction: { base_items_digest: "d".repeat(64), items: [{ pack_id: "pack", item: { item_id: "item", kind: "obligation", text: "核对服务范围" } }] } };
  const state = { review, lose: false, calls: [] as Array<{ body: unknown; key?: string }> };
  await page.route("**/analysis/review", route => route.fulfill({ json: { review: state.review } }));
  await page.route("**/analysis/confirm", async route => {
    state.calls.push({ body: route.request().postDataJSON(), key: route.request().headers()["idempotency-key"] });
    if (state.lose) { state.lose = false; return route.abort("failed"); }
    return route.fulfill({ json: {} });
  });
  await page.goto(`/#/bids/${project}/analysis`);
  const region = page.getByRole("region", { name: "招标分析人工复核", exact: true });
  await expect(region).toContainText("当前核对：来源版本 1，分析修订 1");
  return { ...f, stateReview: state, region };
}

async function recoveryFixture(page: Page) {
  const f = await fixture(page);
  const recovery: AnalysisRecoveryView = { run_id: "analysis", source_version: 1, domain_revision: 1,
    status: "failed", failed_command_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", can_retry: true,
    blocker: null, notice: "继续原分析", saved_responses: 1 };
  const state = { recovery, lose: false, calls: [] as Array<{ body: unknown; key?: string }> };
  await page.route("**/analysis/recovery", route => route.fulfill({ json: { recovery: state.recovery } }));
  await page.route("**/analysis/retry", async route => {
    state.calls.push({ body: route.request().postDataJSON(), key: route.request().headers()["idempotency-key"] });
    if (state.lose) { state.lose = false; return route.abort("failed"); }
    return route.fulfill({ json: {} });
  });
  await page.goto(`/#/bids/${project}/analysis`);
  const region = page.getByRole("region", { name: "分析任务恢复", exact: true });
  await expect(region).toBeVisible();
  return { ...f, stateRecovery: state, region };
}

function savedReport(): SavedWordReview {
  return { execution_id: "saved-review", status: "succeeded", operation: "saved_review", version_id: "word-v1",
    docx_sha256: metadata().docx_sha256, sources_current: true,
    analysis_identity: { run_id: "analysis", source_version: 1, domain_revision: 1 },
    report: { version_id: "word-v1", docx_sha256: metadata().docx_sha256, reason: "核对保存稿。",
      findings: [], gaps: [], records: [{ task_id: "record", chapter_id: "chapter-1", title: "实施方案" }], record_count: 1, reviewed_record_count: 1 } };
}

async function savedFixture(page: Page) {
  const f = await fixture(page);
  const state = { review: null as SavedWordReview | null, failRead: false, lose: false,
    calls: [] as Array<{ body: unknown; key?: string }> };
  await page.route("**/docx/versions/word-v1/review", async route => {
    if (route.request().method() === "GET") return state.failRead
      ? route.fulfill({ status: 503, json: { error: { message: "复核读取失败" } } })
      : route.fulfill({ json: { review: state.review } });
    state.calls.push({ body: route.request().postDataJSON(), key: route.request().headers()["idempotency-key"] });
    state.review = { ...savedReport(), status: "pending", report: null };
    if (state.lose) { state.lose = false; return route.abort("failed"); }
    return route.fulfill({ json: { review: state.review } });
  });
  return { ...f, stateReview: state, async open() {
    await f.ready(); await page.getByText("核对已保存的 Word 内容", { exact: true }).click();
    return page.getByRole("region", { name: "已保存 Word 只读复核", exact: true });
  } };
}

test("an old workbench poll cannot replace the new authoring queue state", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  const held = await holdNextRead(page, "**/workbench", structuredClone(f.view));
  await page.clock.runFor(5100); await held.received;
  f.view.authoring = { execution_id: "new-authoring", status: "published" };
  await page.getByText("直接生成整稿", { exact: true }).click();
  await page.getByRole("button", { name: "生成新稿", exact: true }).click();
  await expect(page.getByText("编制：排队中", { exact: true })).toBeVisible();
  await held.release();
  await expect(page.getByText("编制：排队中", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "生成新稿", exact: true })).toBeDisabled();
  expect(f.state.starts).toHaveLength(1);
});

test("an old outline poll cannot offer an earlier revision as the latest after a save", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("新保存的编排");
  const held = await holdNextRead(page, "**/outline", structuredClone(f.outline));
  await page.clock.runFor(5100); await held.received;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  await expect(page.getByTestId("outline-save-status")).toHaveText("已保存修订 2");
  await held.release();
  await expect(page.getByText("已有更新的大纲修订 1。", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "载入最新大纲", exact: true })).toHaveCount(0);
  await expect(page.getByLabel("章节名称", { exact: true })).toHaveValue("新保存的编排");
});

test("an old workbench read error cannot replace the successful post-mutation state", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  const held = await holdNextRead(page, "**/workbench", { error: { message: "过期的工作台错误" } }, 503);
  await page.clock.runFor(5100); await held.received;
  f.view.authoring = { execution_id: "new-authoring", status: "published" };
  await page.getByText("直接生成整稿", { exact: true }).click();
  await page.getByRole("button", { name: "生成新稿", exact: true }).click();
  await expect(page.getByText("编制：排队中", { exact: true })).toBeVisible();
  await held.release();
  await expect(page.getByRole("alert").filter({ hasText: "过期的工作台错误" })).toHaveCount(0);
});

test("overlapping analysis refreshes keep the most recently requested revision", async ({ page }) => {
  await page.clock.install(); const f = await analysisFixture(page);
  const held = await holdNextRead(page, "**/analysis/review", { review: structuredClone(f.stateReview.review) });
  await f.region.getByRole("button", { name: "刷新复核状态", exact: true }).click(); await held.received;
  f.stateReview.review = { ...f.stateReview.review, domain_revision: 2 };
  await f.region.getByRole("button", { name: "刷新复核状态", exact: true }).click();
  await expect(f.region).toContainText("当前核对：来源版本 1，分析修订 2");
  await held.release();
  await expect(f.region).toContainText("当前核对：来源版本 1，分析修订 2");
});

test("an old saved-review retry cannot clear a newer report for the same Word", async ({ page }) => {
  await page.clock.install(); const f = await savedFixture(page); f.stateReview.failRead = true;
  const region = await f.open(); await expect(region.getByRole("alert")).toContainText("复核读取失败");
  const held = await holdNextRead(page, "**/docx/versions/word-v1/review", { review: null });
  f.stateReview.failRead = false;
  await region.getByRole("button", { name: "重试读取复核状态", exact: true }).click(); await held.received;
  f.stateReview.review = savedReport();
  await region.getByRole("button", { name: "重试读取复核状态", exact: true }).click();
  const heading = region.getByRole("heading", { name: "Word v1 的复核报告", exact: true });
  await expect(heading).toBeVisible(); await held.release(); await expect(heading).toBeVisible();
});

for (const change of ["status", "hard_blocking"] as const) {
  test(`same-identity analysis ${change} updates the new-request gate while preserving input`, async ({ page }) => {
    await page.clock.install(); const f = await analysisFixture(page);
    await f.region.getByLabel("事项纠正理由", { exact: true }).fill("尚未提交的核对备注。");
    const submit = f.region.getByRole("button", { name: "提交人工确认", exact: true });
    await expect(submit).toBeEnabled();
    f.stateReview.review = { ...f.stateReview.review, ...(change === "status" ? { status: "pending" } : { hard_blocking: true }) };
    await f.region.getByRole("button", { name: "刷新复核状态", exact: true }).click();
    await expect(submit).toBeDisabled();
    await expect(f.region.getByLabel("事项纠正理由", { exact: true })).toHaveValue("尚未提交的核对备注。");
    expect(f.stateReview.calls).toEqual([]);
  });
}

test("same-command recovery blockers disable a new retry without dropping its reason", async ({ page }) => {
  await page.clock.install(); const f = await recoveryFixture(page);
  await f.region.getByLabel("分析恢复理由", { exact: true }).fill("保留的恢复理由。");
  const submit = f.region.getByRole("button", { name: "继续原分析任务", exact: true });
  await expect(submit).toBeEnabled();
  f.stateRecovery.recovery = { ...f.stateRecovery.recovery, can_retry: false, blocker: "active_lease", notice: "原执行仍持有任务，请等待其释放后刷新状态。" };
  await f.region.getByRole("button", { name: "刷新分析恢复状态", exact: true }).click();
  await expect(submit).toBeDisabled();
  await expect(f.region).toContainText("原执行仍持有任务");
  await expect(f.region.getByLabel("分析恢复理由", { exact: true })).toHaveValue("保留的恢复理由。");
  expect(f.stateRecovery.calls).toEqual([]);
});

test("ending a project still permits the exact unknown authoring receipt to be confirmed", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready(); f.state.loseStart = true;
  await page.getByText("直接生成整稿", { exact: true }).click();
  await page.getByRole("button", { name: "生成新稿", exact: true }).click();
  const confirm = page.getByRole("button", { name: "确认同一次编制请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(page.getByRole("button", { name: "分析招标文件", exact: true })).toBeDisabled();
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[1]).toEqual(f.state.starts[0]);
});

test("ending a project still permits the exact unknown outline save receipt to be confirmed", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByLabel("章节名称", { exact: true }).fill("已发送但回执未知的编排"); f.state.loseOutlineSave = true;
  await page.getByRole("button", { name: "保存大纲草稿", exact: true }).click();
  const confirm = page.getByRole("button", { name: "确认同一次大纲保存", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(page.getByLabel("章节名称", { exact: true })).toBeDisabled();
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.state.outlineSaves.length).toBe(2);
  expect(f.state.outlineSaves[1]).toEqual(f.state.outlineSaves[0]);
});

test("ending a project still permits the exact unknown outline generation receipt", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click(); f.state.loseOutlineStart = true;
  await page.getByRole("button", { name: "生成大纲", exact: true }).click();
  const confirm = page.getByRole("button", { name: "确认同一次大纲生成请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[1]).toEqual(f.state.starts[0]);
});

test("ending a project still permits the exact unknown outline-based body receipt", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click(); f.state.loseStart = true;
  await page.getByRole("button", { name: "按修订 1 生成正文", exact: true }).click();
  const confirm = page.getByRole("button", { name: "确认同一次编制请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[1]).toEqual(f.state.starts[0]);
});

test("ending a project still permits the exact unknown human confirmation receipt", async ({ page }) => {
  await page.clock.install(); const f = await analysisFixture(page); f.stateReview.lose = true;
  await f.region.getByRole("button", { name: "提交人工确认", exact: true }).click();
  const confirm = f.region.getByRole("button", { name: "确认同一次人工确认请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.stateReview.calls.length).toBe(2);
  expect(f.stateReview.calls[1]).toEqual(f.stateReview.calls[0]);
});

test("ending a project still permits the exact unknown analysis recovery receipt", async ({ page }) => {
  await page.clock.install(); const f = await recoveryFixture(page); f.stateRecovery.lose = true;
  await f.region.getByLabel("分析恢复理由", { exact: true }).fill("原恢复请求的理由。");
  await f.region.getByRole("button", { name: "继续原分析任务", exact: true }).click();
  const confirm = f.region.getByRole("button", { name: "确认同一次恢复请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.stateRecovery.calls.length).toBe(2);
  expect(f.stateRecovery.calls[1]).toEqual(f.stateRecovery.calls[0]);
});

test("ending a project still permits the exact unknown saved-Word review receipt", async ({ page }) => {
  await page.clock.install(); const f = await savedFixture(page); const region = await f.open(); f.stateReview.lose = true;
  await region.getByLabel("保存稿复核理由", { exact: true }).fill("原保存稿复核的理由。");
  await region.getByRole("button", { name: "复核此保存版本", exact: true }).click();
  const confirm = region.getByRole("button", { name: "确认同一次保存稿复核请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.stateReview.calls.length).toBe(2);
  expect(f.stateReview.calls[1]).toEqual(f.stateReview.calls[0]);
});

test("ending a project still permits the exact unknown chapter revision receipt", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await f.ready(); f.state.loseStart = true;
  await page.getByText("按章节定向复核与修订", { exact: true }).click();
  const region = page.getByRole("region", { name: "定向复核与修订", exact: true });
  await region.getByLabel("复核响应：说明实施方法", { exact: true }).check();
  await region.getByLabel("复核与修订理由", { exact: true }).fill("固定原章节范围。");
  await region.getByRole("button", { name: "生成复核修订稿", exact: true }).click();
  const confirm = region.getByRole("button", { name: "确认同一次定向复核请求", exact: true });
  await expect(confirm).toBeEnabled(); f.view.project.status = "ended"; await page.clock.runFor(5100);
  await expect(confirm).toBeEnabled(); await confirm.click(); await expect.poll(() => f.state.starts.length).toBe(2);
  expect(f.state.starts[1]).toEqual(f.state.starts[0]);
});

test("an ended project keeps downloads but blocks selecting another current draft", async ({ page }) => {
  const f = await fixture(page); f.view.project.status = "ended"; f.candidate(); await f.ready();
  await expect(page.getByRole("button", { name: "查看新稿", exact: true })).toBeDisabled();
  await page.getByRole("navigation", { name: "项目分区", exact: true }).getByRole("link", { name: "交付文件", exact: true }).click();
  await expect(page.getByRole("button", { name: "选为当前稿 v2", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "下载 DOCX v2", exact: true })).toBeEnabled();
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
});

for (const saved of [true, false]) {
  test(`an ended project can discard local outline input with ${saved ? "a saved revision" : "only a candidate"}`, async ({ page }) => {
    await page.clock.install(); const f = await fixture(page);
    if (!saved) {
      f.outline.candidate = { execution_id: "outline-generated", sha256: metadata().docx_sha256,
        outline: { chapters: structuredClone(f.view.outline!.chapters) } };
      f.outline.draft = null; f.view.current_docx = null; f.view.versions = []; f.state.current = null;
    }
    await page.goto(`/#/bids/${project}/authoring`);
    await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
    if (!saved) await page.getByRole("button", { name: "载入候选大纲", exact: true }).click();
    await page.getByLabel("章节名称", { exact: true }).fill("项目结束后需要明确放弃的本地输入");
    f.view.project.status = "ended"; await page.clock.runFor(5100);
    await expect(page.getByLabel("章节名称", { exact: true })).toBeDisabled();
    const discard = page.getByRole("button", { name: "放弃当前调整", exact: true });
    await expect(discard).toBeEnabled(); page.once("dialog", dialog => void dialog.accept()); await discard.click();
    await expect(page.getByTestId("outline-save-status")).not.toHaveText("未保存");
    await page.getByRole("navigation", { name: "产品", exact: true }).getByRole("link", { name: "知识资产", exact: true }).click();
    await expect(page).toHaveURL(/#\/library$/);
    expect(f.state.outlineSaves).toEqual([]);
  });
}
