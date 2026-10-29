import { expect, test, type Page } from "@playwright/test";
import type { SavedWordReview, SavedWordReviewBody } from "../src/bid/api/savedWordReview";
import { fixture, type WordTestWindow } from "./tender.fixture";

const sha = "a".repeat(64);
function reviewed(version = "word-v1"): SavedWordReview {
  return { execution_id: `saved-review-${version}`, status: "succeeded", operation: "saved_review", version_id: version, docx_sha256: sha,
    analysis_identity: { run_id: "original-word-analysis", source_version: 1, domain_revision: 2 }, sources_current: true,
    report: { version_id: version, docx_sha256: sha, reason: "核对实际保存正文。", findings: [], gaps: [],
      records: [{ task_id: "saved-record-1", chapter_id: "saved-chapter-1", title: "人工修改后的实施安排" }], record_count: 1, reviewed_record_count: 1 } };
}
async function reviewFixture(page: Page) {
  const f = await fixture(page);
  const state = { values: new Map<string, SavedWordReview | null>(), reads: [] as string[],
    posts: [] as Array<{ version: string; body: SavedWordReviewBody; key?: string }>, failRead: false, loseStart: false,
    actions: [] as Array<{ execution: string; action: string; key?: string }> };
  const receipts = new Map<string, SavedWordReview>();
  await page.route("**/docx/versions/*/review", async route => {
    const request = route.request();
    const version = new URL(request.url()).pathname.split("/").at(-2)!;
    const json = (body: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    expect(request.headers().authorization).toBe("Bearer tender-browser-fixture");
    if (request.method() === "GET") {
      state.reads.push(version);
      return state.failRead ? json({ error: { code: "OFFLINE", message: "只读复核状态暂不可用" } }, 503) : json({ review: state.values.get(version) ?? null });
    }
    const body = request.postDataJSON() as SavedWordReviewBody;
    const key = request.headers()["idempotency-key"];
    state.posts.push({ version, body, key });
    const result = receipts.get(key ?? "") ?? { ...reviewed(version), status: "pending", report: null };
    state.values.set(version, result); if (key) receipts.set(key, result);
    if (state.loseStart) { state.loseStart = false; return route.abort("failed"); }
    return json({ review: result }, 202);
  });
  await page.route("**/executions/saved-review-*/*", async route => {
    const request = route.request(); const parts = new URL(request.url()).pathname.split("/");
    const execution = parts.at(-2)!, action = parts.at(-1)!;
    state.actions.push({ execution, action, key: request.headers()["idempotency-key"] });
    for (const [version, value] of state.values) if (value?.execution_id === execution) state.values.set(version, { ...value, status: action === "cancel" ? "cancelled" : "pending" });
    return route.fulfill({ status: 202, contentType: "application/json", body: "{}" });
  });
  const open = async () => {
    await page.getByText("核对已保存的 Word 内容", { exact: true }).click();
    return page.getByRole("region", { name: "已保存 Word 只读复核", exact: true });
  };
  return { ...f, reviewState: state, open };
}

test("a manual saved Word starts only explicit read-only review for its exact saved identity", async ({ page }) => {
  const f = await reviewFixture(page); f.publish(); await f.ready(); const review = await f.open();
  await expect(review).toContainText("复核版本：Word v2 word-saved-v2");
  await expect(review).toContainText("该版本尚无复核任务，不能据此判断是否存在问题。");
  await expect(review.getByRole("button", { name: "复核此保存版本", exact: true })).toBeDisabled();
  expect(f.reviewState.posts).toEqual([]);
  await review.getByLabel("保存稿复核理由", { exact: true }).fill("  核对我刚保存的人工正文，尤其是实施时间。  ");
  await review.getByRole("button", { name: "复核此保存版本", exact: true }).click();
  await expect.poll(() => f.reviewState.posts.length).toBe(1);
  expect(f.reviewState.posts[0]).toMatchObject({ version: "word-saved-v2", body: { docx_sha256: sha, reason: "核对我刚保存的人工正文，尤其是实施时间。" } });
  expect(f.reviewState.posts[0].key).toBeTruthy();
  await expect(review.getByLabel("保存稿复核理由", { exact: true })).toHaveValue("");
  expect(f.state.starts).toEqual([]); expect(f.state.saves).toEqual([]);
  expect(f.state.calls.filter(call => call === "select")).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view"]);
});

test("a lost review response survives refresh and a newer Word without changing the original request", async ({ page }) => {
  const f = await reviewFixture(page); await f.ready(); let review = await f.open();
  await review.getByLabel("保存稿复核理由", { exact: true }).fill("固定旧保存稿和原始理由。");
  f.reviewState.loseStart = true;
  await review.getByRole("button", { name: "复核此保存版本", exact: true }).click();
  await expect(review.getByRole("button", { name: "确认同一次保存稿复核请求", exact: true })).toBeVisible();
  f.publish(); f.state.current!.docx_sha256 = "b".repeat(64); f.view.current_docx!.sha256 = "b".repeat(64);
  await page.reload(); review = await f.open();
  await expect(review).toContainText("复核版本：Word v1 word-v1");
  await expect(review).toContainText("当前 Word 已更新为 v2，以下仍对应 v1，未自动复核新版本。");
  await expect(review.getByLabel("保存稿复核理由", { exact: true })).toHaveValue("固定旧保存稿和原始理由。");
  await expect(review.getByRole("button", { name: "切换到当前保存版本", exact: true })).toBeDisabled();
  await review.getByRole("button", { name: "确认同一次保存稿复核请求", exact: true }).click();
  await expect.poll(() => f.reviewState.posts.length).toBe(2);
  expect(f.reviewState.posts[1]).toEqual(f.reviewState.posts[0]);
  expect(f.reviewState.posts.every(post => post.version === "word-v1")).toBe(true);
  expect(f.state.starts).toEqual([]);
});

test("partial reads and semantic gaps stay visible and a polling error retains the known report", async ({ page }) => {
  await page.clock.install(); const f = await reviewFixture(page); const value = reviewed();
  value.report!.record_count = 3; value.report!.reviewed_record_count = 1;
  value.report!.gaps = [{ code: "unread_body", detail: "两个文本单元尚未读取。" }];
  value.report!.findings = [{ code: "inconsistent_dates", detail: "人工改写后的时间前后不一致。", task_ids: ["saved-record-1"], content_ids: [], suggestion: "请核对实施时间。", can_revise: false }];
  f.reviewState.values.set("word-v1", value); await f.ready(); const review = await f.open();
  await expect(review).toContainText("已复核 1 / 3 段可读取正文。");
  await expect(review).toContainText("仍有 2 段正文尚未完成复核。");
  await expect(review).toContainText("两个文本单元尚未读取。");
  await expect(review).toContainText("人工修改后的实施安排");
  await expect(review).toContainText("人工改写后的时间前后不一致。");
  expect(await review.getByRole("button", { name: /修订|改写/ }).count()).toBe(0);
  f.reviewState.failRead = true; await page.clock.runFor(5100);
  await expect(review.getByRole("alert").filter({ hasText: "复核状态读取失败" })).toBeVisible();
  await expect(review).toContainText("人工改写后的时间前后不一致。");
  f.reviewState.failRead = false;
  await review.getByRole("button", { name: "重试读取复核状态", exact: true }).click();
  await expect(review.getByRole("alert").filter({ hasText: "复核状态读取失败" })).toHaveCount(0);
});

test("saving a newer Word leaves the displayed report on its old version until explicit switching", async ({ page }) => {
  await page.clock.install(); const f = await reviewFixture(page);
  f.reviewState.values.set("word-v1", reviewed()); await f.ready(); const review = await f.open();
  await expect(review).toContainText("已复核范围内未报告问题。");
  await f.edit(); await f.dirty(); await page.getByTestId("docx-save").click();
  f.publish(); await page.clock.runFor(1100);
  await expect(page.getByTestId("docx-version")).toHaveText("保存版本 2");
  await expect(review).toContainText("当前 Word 已更新为 v2，以下仍对应 v1，未自动复核新版本。");
  await expect(review.getByRole("heading", { name: "Word v1 的复核报告", exact: true })).toBeVisible();
  expect(f.reviewState.posts).toEqual([]);
  expect(f.reviewState.reads.every(version => version === "word-v1")).toBe(true);
  await review.getByRole("button", { name: "切换到当前保存版本", exact: true }).click();
  await expect(review).toContainText("复核版本：Word v2 word-saved-v2");
  await expect(review).toContainText("该版本尚无复核任务，不能据此判断是否存在问题。");
  expect(f.reviewState.posts).toEqual([]);
});

test("missing, empty and mismatched reports never masquerade as zero problems", async ({ page }) => {
  await page.clock.install(); const f = await reviewFixture(page);
  f.reviewState.failRead = true; await f.ready(); const review = await f.open();
  await expect(review.getByRole("alert")).toContainText("未读取到可确认的复核结果。");
  f.reviewState.failRead = false;
  f.reviewState.values.set("word-v1", { ...reviewed(), report: null });
  await review.getByRole("button", { name: "重试读取复核状态", exact: true }).click();
  await expect(review).toContainText("任务已结束，但复核报告尚未读取。");
  const empty = reviewed(); empty.report!.records = []; empty.report!.record_count = 0; empty.report!.reviewed_record_count = 0;
  f.reviewState.values.set("word-v1", empty); await page.clock.runFor(5100);
  await expect(review).toContainText("未读到可复核正文，不能据此认定无问题。");
  const wrong = reviewed(); wrong.report!.version_id = "different-version";
  wrong.report!.findings = [{ code: "wrong_version", detail: "不应显示的其他版本结论", task_ids: [], content_ids: [], suggestion: "", can_revise: false }];
  f.reviewState.values.set("word-v1", wrong); await page.clock.runFor(5100);
  await expect(review).toContainText("报告版本与所选 Word 不一致，未展示报告内容。");
  await expect(review.getByText("不应显示的其他版本结论", { exact: true })).toHaveCount(0);
  await expect(review.getByText("已复核范围内未报告问题。请继续人工核对适用范围与业务事实。", { exact: true })).toHaveCount(0);
});

test("failed saved-Word review resumes and cancels its exact review execution", async ({ page }) => {
  const f = await reviewFixture(page); f.reviewState.values.set("word-v1", { ...reviewed(), status: "failed", report: null });
  await f.ready(); const review = await f.open();
  await review.getByRole("button", { name: "继续保存稿复核", exact: true }).click();
  await expect(review.getByRole("button", { name: "取消保存稿复核", exact: true })).toBeVisible();
  await review.getByRole("button", { name: "取消保存稿复核", exact: true }).click();
  await expect(review).toContainText("保存稿复核：已取消");
  expect(f.reviewState.actions.map(action => [action.execution, action.action])).toEqual([["saved-review-word-v1", "resume"], ["saved-review-word-v1", "cancel"]]);
  expect(f.reviewState.actions.every(action => !!action.key)).toBe(true);
  expect(f.reviewState.posts).toEqual([]); expect(f.state.starts).toEqual([]);
});

test("updated tender sources leave the review on its original analysis and display the coverage limit", async ({ page }) => {
  const f = await reviewFixture(page); const value = reviewed(); value.sources_current = false;
  f.reviewState.values.set("word-v1", value); await f.ready(); const review = await f.open();
  await expect(review).toContainText("固定复核依据：原分析 original-word-analysis，来源版本 1，分析修订 2。");
  await expect(review.getByRole("alert")).toContainText("此任务仍按生成该 Word 时的原始分析依据复核，未覆盖后续上传的补遗或资料");
  await expect(review).toContainText("Word v1 的复核报告");
  expect(f.reviewState.posts).toEqual([]); expect(f.state.starts).toEqual([]);
});

test("an unsent reason guards navigation and explicit discard allows leaving", async ({ page }) => {
  const f = await reviewFixture(page); await f.ready(); const review = await f.open();
  await review.getByLabel("保存稿复核理由", { exact: true }).fill("尚未提交的人工核对意见。");
  await page.getByRole("navigation", { name: "产品", exact: true }).getByRole("link", { name: "知识资产", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "请先提交或放弃复核修改" })).toBeVisible();
  await expect(review.getByLabel("保存稿复核理由", { exact: true })).toHaveValue("尚未提交的人工核对意见。");
  page.on("dialog", dialog => void dialog.accept());
  await review.getByRole("button", { name: "放弃保存稿复核理由", exact: true }).click();
  await page.getByRole("navigation", { name: "项目分区" }).getByRole("link", { name: "项目资料" }).click();
  await expect(page.getByRole("heading", { name: "项目资料", exact: true })).toBeVisible();
  expect(f.reviewState.posts).toEqual([]);
});
