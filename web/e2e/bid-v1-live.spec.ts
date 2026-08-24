import { expect, test, type Download, type Page } from "@playwright/test";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const AUTHORIZATION_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  "base64",
);

async function clickForResponse(
  page: Page,
  method: string,
  pathPattern: RegExp,
  click: () => Promise<void>,
) {
  const response = page.waitForResponse(
    (candidate) =>
      candidate.request().method() === method &&
      pathPattern.test(new URL(candidate.url()).pathname),
  );
  await click();
  const result = await response;
  expect(result.ok(), `${method} ${new URL(result.url()).pathname}`).toBeTruthy();
  return result;
}

async function downloadedBytes(download: Download): Promise<Buffer> {
  const file = await download.path();
  expect(file, "browser download must have a local artifact path").toBeTruthy();
  return readFile(file!);
}

function partKeyFromHref(href: string): string {
  const hash = new URL(href).hash;
  const queryOffset = hash.indexOf("?");
  return queryOffset === -1 ? "1" : new URLSearchParams(hash.slice(queryOffset + 1)).get("part") ?? "1";
}

test("live browser: create bid through formal PDF download", async ({ page }, testInfo) => {
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("bid-browser-acceptance@local");
  await page.getByTestId("login-password").fill("ignored");
  await clickForResponse(page, "POST", /\/api\/v1\/auth\/login$/, () =>
    page.getByTestId("login-submit").click(),
  );

  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("浏览器招投标 V1 运行验收");
  await page.getByTestId("bid-ends").fill("2099-12-31");
  const created = await clickForResponse(page, "POST", /\/api\/v1\/bids$/, () =>
    page.getByTestId("bid-create").click(),
  );
  const projectId = String((await created.json()).id);
  expect(projectId).toMatch(/^[0-9a-f-]{36}$/);
  await expect(page.getByTestId("wizard-files")).toBeVisible();

  const runtimeArtifacts = path.join(testInfo.file, "..", "artifacts", "runtime");
  const pdfTenderBytes = await readFile(path.join(runtimeArtifacts, "tender.pdf"));
  const docxTenderBytes = await readFile(path.join(runtimeArtifacts, "tender.docx"));
  const pdfTenderSha256 = createHash("sha256").update(pdfTenderBytes).digest("hex");
  const docxTenderSha256 = createHash("sha256").update(docxTenderBytes).digest("hex");
  expect(pdfTenderBytes.subarray(0, 5).toString()).toBe("%PDF-");
  expect(docxTenderBytes.subarray(0, 2).toString()).toBe("PK");
  const upload = page.locator('input[type="file"][hidden]');
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/documents$`), () =>
    upload.setInputFiles({
      name: "browser-tender.pdf",
      mimeType: "application/pdf",
      buffer: pdfTenderBytes,
    }),
  );
  await expect(page.getByText("文本已就绪，可确认事实与条款。", { exact: true })).toHaveCount(1, {
    timeout: 180_000,
  });
  await expect(page.getByTestId("upload-drop").getByRole("button", { name: "选择文件" })).toBeEnabled();
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/documents$`), () =>
    upload.setInputFiles({
      name: "browser-tender.docx",
      mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      buffer: docxTenderBytes,
    }),
  );
  await expect(page.getByText("文本已就绪，可确认事实与条款。", { exact: true })).toHaveCount(2, {
    timeout: 180_000,
  });

  await page.getByTestId("wizard-facts").click();
  const budget = page.locator(".inner").filter({ has: page.getByTestId("fact-budget_amount") });
  await expect(budget.getByRole("button", { name: "接受建议" }).first()).toBeVisible({
    timeout: 180_000,
  });
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    budget.getByRole("button", { name: "接受建议" }).first().click(),
  );
  await expect(budget).toContainText("1000.00");
  await page.getByTestId("fact-budget_amount").fill("1200.00");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    budget.getByRole("button", { name: "写入" }).click(),
  );
  await expect(budget).toContainText("1200.00");

  const validityDays = page.locator(".inner").filter({ has: page.getByTestId("fact-bid_valid_days") });
  await page.getByTestId("fact-bid_valid_days").fill("91");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    validityDays.getByRole("button", { name: "写入" }).click(),
  );
  await expect(validityDays).toContainText("91");
  const validityUntil = page.locator(".inner").filter({ has: page.getByTestId("fact-bid_valid_until") });
  await page.getByTestId("fact-bid_valid_until").fill("2099-11-30T00:00:00Z");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    validityUntil.getByRole("button", { name: "写入" }).click(),
  );
  await expect(page.getByTestId("validity-conflict")).toBeVisible();
  await page.getByTestId("nav-pending").click();
  const draftRows = page.locator("table.grid tbody tr");
  await expect.poll(() => draftRows.count(), { timeout: 60_000 }).toBeGreaterThanOrEqual(4);
  const draftRowTestIds = await draftRows.evaluateAll((rows) =>
    rows.map((row) => row.getAttribute("data-testid")),
  );
  expect(draftRowTestIds).not.toContain(null);
  for (const testId of draftRowTestIds) {
    expect(testId).toMatch(/^clause-row-[0-9a-f-]{36}$/);
    const clauseId = testId!.slice("clause-row-".length);
    const row = page.getByTestId(testId!);
    await clickForResponse(
      page,
      "PATCH",
      new RegExp(`/api/v1/bids/${projectId}/clauses/${clauseId}$`),
      () => row.getByRole("button", { name: "确认", exact: true }).click(),
    );
    await expect(row).toHaveCount(0);
  }
  await expect(draftRows).toHaveCount(0);
  await page.getByTestId("clause-text").fill("系统应提供完整的实施交付计划。");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/clauses$`), () =>
    page.getByTestId("clause-add").click(),
  );
  await clickForResponse(
    page,
    "PATCH",
    new RegExp(`/api/v1/bids/${projectId}/clauses/`),
    () => page.getByRole("button", { name: "确认", exact: true }).first().click(),
  );

  await page.getByTestId("wizard-matching").click();
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/matching/schedule$`), () =>
    page.getByTestId("schedule-match").click(),
  );
  const matchingLinks = page.locator("nav.sidenav a");
  await expect.poll(() => matchingLinks.count(), { timeout: 60_000 }).toBeGreaterThan(1);
  await matchingLinks.first().click();
  await expect(page.locator("table.grid")).toContainText("review / insufficient", { timeout: 180_000 });

  const authToken = await page.evaluate(() => localStorage.getItem("kb.token"));
  if (!authToken) throw new Error("browser login did not persist an API token");
  const authenticatedRequest = { headers: { Authorization: `Bearer ${authToken}` } };
  const unitsResponse = await page.request.get(`/api/v1/bids/${projectId}/units`, authenticatedRequest);
  expect(unitsResponse.ok(), "GET bid matching units").toBeTruthy();
  const routeUnits = ((await unitsResponse.json()) as {
    units: Array<{ id: string | null; route_id?: string | null; kind?: string }>;
  }).units.filter((unit) => unit.kind !== "commercial" && unit.route_id);
  let supportedRoute: { routeId: string; view: string; candidateCount: number } | null = null;
  await expect
    .poll(
      async () => {
        const snapshots = await Promise.all(
          routeUnits.map(async (unit) => {
            const response = await page.request.get(
              `/api/v1/bids/${projectId}/matching/routes/${unit.route_id}/pick-set`,
              authenticatedRequest,
            );
            if (!response.ok()) return null;
            const body = (await response.json()) as { supported_candidates?: unknown[] };
            return {
              routeId: unit.route_id!,
              view: unit.kind === "unsectioned" || !unit.id ? "unsectioned" : unit.id,
              candidateCount: body.supported_candidates?.length ?? 0,
            };
          }),
        );
        supportedRoute = snapshots.find((snapshot) => (snapshot?.candidateCount ?? 0) >= 2) ?? null;
        return supportedRoute?.candidateCount ?? 0;
      },
      { timeout: 180_000 },
    )
    .toBeGreaterThanOrEqual(2);
  if (!supportedRoute) throw new Error("matching completed without a technical route containing at least two supported candidates");
  await page.locator(`nav.sidenav a[href*="view=${supportedRoute.view}"]`).click();
  const supportedPicks = page.locator('[data-testid^="pick-"]');
  await expect(supportedPicks).toHaveCount(supportedRoute.candidateCount, { timeout: 60_000 });
  await expect(supportedPicks.nth(0)).not.toBeChecked();
  await expect(supportedPicks.nth(1)).not.toBeChecked();
  const firstCandidateId = (await supportedPicks.nth(0).getAttribute("data-testid"))?.slice("pick-".length);
  const secondCandidateId = (await supportedPicks.nth(1).getAttribute("data-testid"))?.slice("pick-".length);
  if (!firstCandidateId || !secondCandidateId) throw new Error("supported candidate ids are missing from the live UI");
  const pickSetPath = new RegExp(
    `/api/v1/bids/${projectId}/matching/routes/${supportedRoute.routeId}/pick-set$`,
  );
  const waitForPersistedPicks = (expectedRevision: number, candidateIds: string[]) =>
    page.waitForResponse(async (response) => {
      if (response.request().method() !== "GET" || !pickSetPath.test(new URL(response.url()).pathname)) return false;
      if (!response.ok()) return false;
      const body = (await response.json()) as {
        revision?: number;
        items?: Array<{ candidate_artifact_id?: string }>;
      };
      const persisted = new Set((body.items ?? []).map((item) => item.candidate_artifact_id));
      return (
        body.revision === expectedRevision &&
        body.items?.length === candidateIds.length &&
        candidateIds.every((candidateId) => persisted.has(candidateId))
      );
    });

  const firstReload = waitForPersistedPicks(1, [firstCandidateId]);
  await clickForResponse(page, "PUT", pickSetPath, () =>
    supportedPicks.nth(0).click(),
  );
  const firstReloadBody = (await (await firstReload).json()) as { revision: number };
  expect(firstReloadBody.revision).toBe(1);
  await expect(supportedPicks.nth(0)).toBeChecked();
  const secondReload = waitForPersistedPicks(2, [firstCandidateId, secondCandidateId]);
  await clickForResponse(page, "PUT", pickSetPath, () =>
    supportedPicks.nth(1).click(),
  );
  const secondReloadBody = (await (await secondReload).json()) as { revision: number };
  expect(secondReloadBody.revision).toBe(2);
  await expect(supportedPicks.nth(0)).toBeChecked();
  await expect(supportedPicks.nth(1)).toBeChecked();
  const technicalPickCount = await supportedPicks.evaluateAll((nodes) =>
    nodes.filter((node) => (node as HTMLInputElement).checked).length,
  );
  expect(technicalPickCount).toBe(2);

  const docxDownload = page.waitForEvent("download", { timeout: 120_000 });
  await page.getByTestId("export-docx").click();
  const docx = await docxDownload;
  const docxBytes = await downloadedBytes(docx);
  expect(docxBytes.subarray(0, 2).toString()).toBe("PK");

  await page.getByTestId("export-pdf").click();
  await expect(page.getByText(/SUBMISSION_GATE_REJECTED/)).toBeVisible();

  await page.getByTestId("wizard-facts").click();
  await expect(page.getByTestId("validity-conflict")).toBeVisible();
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    page.getByTestId("fact-clear-bid_valid_until").click(),
  );
  await expect(page.getByTestId("validity-conflict")).toHaveCount(0);

  await page.getByTestId("wizard-quote").click();
  await page.getByRole("link", { name: "公司资料", exact: true }).click();
  const company: Record<string, string> = {
    legal_name: "示例网络安全有限公司",
    unified_social_credit_code: "91310000MA00000001",
    registered_address: "上海市浦东新区示例路1号",
    legal_representative: "张三",
    contact_name: "李四",
    contact_phone: "13800000000",
    contact_email: "bid@example.test",
  };
  for (const [field, value] of Object.entries(company)) {
    await page.getByTestId(`company-${field}`).fill(value);
  }
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/company-profile$`), () =>
    page.getByRole("button", { name: "保存公司资料" }).click(),
  );

  await page.getByRole("link", { name: "投标资料", exact: true }).click();
  const submission: Record<string, string> = {
    buyer_name: "示例采购人",
    project_code: "KB-BROWSER-ACCEPTANCE",
    authorized_representative: "李四",
    submission_date: "2026-08-23",
    submission_place: "上海",
  };
  for (const [field, value] of Object.entries(submission)) {
    await page.getByTestId(`submission-${field}`).fill(value);
  }
  await page.getByRole("checkbox", { name: "已确认盖章" }).check();
  await page.getByRole("checkbox", { name: "已确认签字" }).check();
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/submission-profile$`), () =>
    page.getByRole("button", { name: "保存投标资料" }).click(),
  );

  await page.getByTestId("nav-procedural").click();
  const attachmentUpload = page.locator('input[type="file"]');
  const attachmentResponse = await clickForResponse(
    page,
    "POST",
    new RegExp(`/api/v1/bids/${projectId}/attachments$`),
    () => attachmentUpload.setInputFiles({
      name: "browser-authorization.png",
      mimeType: "image/png",
      buffer: AUTHORIZATION_PNG,
    }),
  );
  const attachmentId = String((await attachmentResponse.json()).id);
  expect(attachmentId).toMatch(/^[0-9a-f-]{36}$/);
  let attachmentRow = page.getByTestId(`attachment-${attachmentId}`);
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/attachments/${attachmentId}/validate$`), () =>
    attachmentRow.getByRole("button", { name: "校验" }).click(),
  );
  attachmentRow = page.getByTestId(`attachment-${attachmentId}`);
  await expect(attachmentRow).toContainText("draft / valid");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/attachments/${attachmentId}/confirm$`), () =>
    attachmentRow.getByRole("button", { name: "确认" }).click(),
  );
  await expect(attachmentRow).toContainText("confirmed / valid");
  const authorizationCard = page.locator(".card").filter({ hasText: "程序要求" }).locator(".inner").filter({
    hasText: "authorization_support",
  });
  await expect(authorizationCard.getByTestId(new RegExp(`resolve-attachment-.+-${attachmentId}`))).toBeVisible();
  await clickForResponse(
    page,
    "POST",
    new RegExp(`/api/v1/bids/${projectId}/procedural-requirements/.+/resolve$`),
    () => authorizationCard.getByTestId(new RegExp(`resolve-attachment-.+-${attachmentId}`)).click(),
  );

  const proceduralCards = page.locator(".card").filter({ hasText: "程序要求" }).locator(".inner");
  const proceduralCount = await proceduralCards.count();
  expect(proceduralCount).toBeGreaterThan(0);
  for (let index = 0; index < proceduralCount; index += 1) {
    const card = proceduralCards.nth(index);
    const text = await card.innerText();
    if (text.includes("authorization_support")) continue;
    const confirmation = text.includes("confirmation");
    await clickForResponse(
      page,
      "POST",
      new RegExp(`/api/v1/bids/${projectId}/procedural-requirements/.+/resolve$`),
      () => card.getByRole("button", { name: confirmation ? "人工确认" : "不适用" }).click(),
    );
  }

  await page.getByTestId("nav-quote").click();
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/draft$`), () =>
    page.getByTestId("quote-create").click(),
  );
  const addedLineResponse = await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/`), () =>
    page.getByRole("button", { name: "增行" }).click(),
  );
  const addedLineId = String((await addedLineResponse.json()).line_id);
  expect(addedLineId).toMatch(/^[0-9a-f-]{36}$/);
  const lineConfirmation = page.getByTestId(`quote-line-confirmed-${addedLineId}`);
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/`), () =>
    lineConfirmation.click(),
  );
  await expect(page.getByTestId(`quote-line-confirmed-${addedLineId}`)).toBeChecked();
  await page.getByPlaceholder("复核原因").fill("招标样例未设置最高限价，浏览器验收已人工复核");
  await page.getByTestId("no-ceiling-review").check();
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/finalize$`), () =>
    page.getByTestId("quote-finalize").click(),
  );
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/reopen$`), () =>
    page.getByTestId("quote-reopen").click(),
  );
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/finalize$`), () =>
    page.getByTestId("quote-finalize").click(),
  );

  await page.getByTestId("wizard-parts").click();
  const partHrefs = await page.locator("nav.sidenav a").evaluateAll((links) =>
    links.map((link) => (link as HTMLAnchorElement).href),
  );
  expect(partHrefs.length).toBeGreaterThan(0);
  for (const href of partHrefs) {
    const partKey = partKeyFromHref(href);
    await page.goto(href);
    const pane = page.getByTestId(`part-pane-${partKey}`);
    await expect(pane).toHaveAttribute("data-ready", "true");
    await clickForResponse(page, "POST", /\/api\/v1\/bids\/.+\/parts\/.+\/regenerate$/, () =>
      page.getByTestId(`part-regenerate-${partKey}`).click(),
    );
  }
  await expect(page.getByTestId("gate-issues")).toContainText("Gate pass", { timeout: 60_000 });

  const pdfDownload = page.waitForEvent("download", { timeout: 120_000 });
  await page.getByTestId("export-pdf").click();
  const pdf = await pdfDownload;
  const pdfBytes = await downloadedBytes(pdf);
  expect(pdfBytes.subarray(0, 5).toString()).toBe("%PDF-");

  const artifactDir = path.join(testInfo.file, "..", "artifacts", "live");
  await mkdir(artifactDir, { recursive: true });
  const docxSha256 = createHash("sha256").update(docxBytes).digest("hex");
  const pdfSha256 = createHash("sha256").update(pdfBytes).digest("hex");
  await page.screenshot({ path: path.join(artifactDir, "formal-pdf-downloaded.png"), fullPage: true });
  await writeFile(
    path.join(artifactDir, "evidence.json"),
    `${JSON.stringify({
      schema_version: 2,
      mode: "playwright-live-ui",
      project_id: projectId,
      tenders: {
        pdf: { media_type: "application/pdf", sha256: pdfTenderSha256 },
        docx: {
          media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
          sha256: docxTenderSha256,
        },
      },
      fact_suggestion_accepted_and_revised: true,
      validity_conflict_rejected_and_cleared: true,
      technical_pick_count: technicalPickCount,
      attachment_id: attachmentId,
      attachment_satisfied_requirement: true,
      quote_reopen_refinalize: true,
      docx_sha256: docxSha256,
      pdf_sha256: pdfSha256,
    }, null, 2)}\n`,
  );
});
