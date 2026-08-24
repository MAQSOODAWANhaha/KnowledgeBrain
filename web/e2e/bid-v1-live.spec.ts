import { expect, test, type Download, type Locator, type Page } from "@playwright/test";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { inflateRawSync } from "node:zlib";

const AUTHORIZATION_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  "base64",
);

async function activateWithKeyboard(locator: Locator) {
  await expect(locator).toBeEnabled();
  await locator.focus();
  await locator.press("Enter");
}

async function toggleWithKeyboard(locator: Locator) {
  await expect(locator).toBeEnabled();
  await locator.focus();
  await locator.press("Space");
}

async function replaceWithKeyboard(locator: Locator, value: string) {
  await expect(locator).toBeEnabled();
  await locator.focus();
  await locator.press("ControlOrMeta+A");
  await locator.pressSequentially(value);
  await locator.press("Tab");
}

async function clickForResponse(
  page: Page,
  method: string,
  pathPattern: RegExp,
  action: () => Promise<void>,
) {
  const response = page.waitForResponse(
    (candidate) =>
      candidate.request().method() === method &&
      pathPattern.test(new URL(candidate.url()).pathname),
  );
  await action();
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

function zipEntry(bytes: Buffer, entryName: string): Buffer {
  const minimumEocdSize = 22;
  const maximumCommentSize = 65_535;
  let eocd = -1;
  for (
    let offset = bytes.length - minimumEocdSize;
    offset >= Math.max(0, bytes.length - minimumEocdSize - maximumCommentSize);
    offset -= 1
  ) {
    if (bytes.readUInt32LE(offset) === 0x06054b50) {
      eocd = offset;
      break;
    }
  }
  if (eocd < 0) throw new Error("DOCX ZIP end-of-central-directory record is missing");

  let cursor = bytes.readUInt32LE(eocd + 16);
  while (cursor + 46 <= bytes.length && bytes.readUInt32LE(cursor) === 0x02014b50) {
    const compressionMethod = bytes.readUInt16LE(cursor + 10);
    const compressedSize = bytes.readUInt32LE(cursor + 20);
    const fileNameLength = bytes.readUInt16LE(cursor + 28);
    const extraLength = bytes.readUInt16LE(cursor + 30);
    const commentLength = bytes.readUInt16LE(cursor + 32);
    const localHeaderOffset = bytes.readUInt32LE(cursor + 42);
    const name = bytes.subarray(cursor + 46, cursor + 46 + fileNameLength).toString("utf8");
    if (name === entryName) {
      if (bytes.readUInt32LE(localHeaderOffset) !== 0x04034b50) {
        throw new Error(`DOCX ZIP local header is invalid for ${entryName}`);
      }
      const localNameLength = bytes.readUInt16LE(localHeaderOffset + 26);
      const localExtraLength = bytes.readUInt16LE(localHeaderOffset + 28);
      const dataOffset = localHeaderOffset + 30 + localNameLength + localExtraLength;
      const compressed = bytes.subarray(dataOffset, dataOffset + compressedSize);
      if (compressionMethod === 0) return compressed;
      if (compressionMethod === 8) return inflateRawSync(compressed);
      throw new Error(`DOCX ZIP compression method ${compressionMethod} is unsupported`);
    }
    cursor += 46 + fileNameLength + extraLength + commentLength;
  }
  throw new Error(`DOCX ZIP entry ${entryName} is missing`);
}

test("live browser: create bid through formal PDF download", async ({ page }, testInfo) => {
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("bid-browser-acceptance@local");
  await page.getByTestId("login-password").fill("ignored");
  await clickForResponse(page, "POST", /\/api\/v1\/auth\/login$/, () =>
    activateWithKeyboard(page.getByTestId("login-submit")),
  );

  await activateWithKeyboard(page.getByTestId("new-bid"));
  await page.getByTestId("bid-title").fill("浏览器招投标 V1 运行验收");
  await page.getByTestId("bid-ends").fill("2099-12-31");
  const created = await clickForResponse(page, "POST", /\/api\/v1\/bids$/, () =>
    activateWithKeyboard(page.getByTestId("bid-create")),
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

  await activateWithKeyboard(page.getByTestId("wizard-facts"));
  const budget = page.locator(".inner").filter({ has: page.getByTestId("fact-budget_amount") });
  await expect(budget.getByRole("button", { name: "接受建议" }).first()).toBeVisible({
    timeout: 180_000,
  });
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    activateWithKeyboard(budget.getByRole("button", { name: "接受建议" }).first()),
  );
  await expect(budget).toContainText("1000.00");
  await page.getByTestId("fact-budget_amount").fill("1200.00");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    activateWithKeyboard(budget.getByRole("button", { name: "写入" })),
  );
  await expect(budget).toContainText("1200.00");

  const validityDays = page.locator(".inner").filter({ has: page.getByTestId("fact-bid_valid_days") });
  await page.getByTestId("fact-bid_valid_days").fill("91");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    activateWithKeyboard(validityDays.getByRole("button", { name: "写入" })),
  );
  await expect(validityDays).toContainText("91");
  const validityUntil = page.locator(".inner").filter({ has: page.getByTestId("fact-bid_valid_until") });
  await page.getByTestId("fact-bid_valid_until").fill("2099-11-30T00:00:00Z");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    activateWithKeyboard(validityUntil.getByRole("button", { name: "写入" })),
  );
  await expect(page.getByTestId("validity-conflict")).toBeVisible();
  await activateWithKeyboard(page.getByTestId("nav-pending"));
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
      () => activateWithKeyboard(row.getByRole("button", { name: "确认", exact: true })),
    );
    await expect(row).toHaveCount(0);
  }
  await expect(draftRows).toHaveCount(0);
  await page.getByTestId("clause-text").fill("系统应提供完整的实施交付计划。");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/clauses$`), () =>
    activateWithKeyboard(page.getByTestId("clause-add")),
  );
  await clickForResponse(
    page,
    "PATCH",
    new RegExp(`/api/v1/bids/${projectId}/clauses/`),
    () => activateWithKeyboard(page.getByRole("button", { name: "确认", exact: true }).first()),
  );

  await activateWithKeyboard(page.getByTestId("wizard-matching"));
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/matching/schedule$`), () =>
    activateWithKeyboard(page.getByTestId("schedule-match")),
  );
  const matchingLinks = page.locator("nav.sidenav a");
  await expect.poll(() => matchingLinks.count(), { timeout: 60_000 }).toBeGreaterThan(1);
  await activateWithKeyboard(matchingLinks.first());
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
  await activateWithKeyboard(page.locator(`nav.sidenav a[href*="view=${supportedRoute.view}"]`));
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
    toggleWithKeyboard(supportedPicks.nth(0)),
  );
  const firstReloadBody = (await (await firstReload).json()) as { revision: number };
  expect(firstReloadBody.revision).toBe(1);
  await expect(supportedPicks.nth(0)).toBeChecked();
  const secondReload = waitForPersistedPicks(2, [firstCandidateId, secondCandidateId]);
  await clickForResponse(page, "PUT", pickSetPath, () =>
    toggleWithKeyboard(supportedPicks.nth(1)),
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
  await activateWithKeyboard(page.getByTestId("export-docx"));
  const docx = await docxDownload;
  const docxBytes = await downloadedBytes(docx);
  expect(docxBytes.subarray(0, 2).toString()).toBe("PK");
  expect(zipEntry(docxBytes, "word/document.xml").toString("utf8")).toContain("报价尚未最终确认");

  await activateWithKeyboard(page.getByTestId("export-pdf"));
  await expect(page.getByText(/SUBMISSION_GATE_REJECTED/)).toBeVisible();

  await activateWithKeyboard(page.getByTestId("wizard-parts"));
  await expect(page.getByTestId("gate-issues")).toContainText("BID_VALIDITY_CONFLICT");

  await activateWithKeyboard(page.getByTestId("wizard-facts"));
  await expect(page.getByTestId("validity-conflict")).toBeVisible();
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/facts$`), () =>
    activateWithKeyboard(page.getByTestId("fact-clear-bid_valid_until")),
  );
  await expect(page.getByTestId("validity-conflict")).toHaveCount(0);

  await activateWithKeyboard(page.getByTestId("wizard-quote"));
  await activateWithKeyboard(page.getByRole("link", { name: "公司资料", exact: true }));
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
    activateWithKeyboard(page.getByRole("button", { name: "保存公司资料" })),
  );

  await activateWithKeyboard(page.getByRole("link", { name: "投标资料", exact: true }));
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
  await toggleWithKeyboard(page.getByRole("checkbox", { name: "已确认盖章" }));
  await toggleWithKeyboard(page.getByRole("checkbox", { name: "已确认签字" }));
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/submission-profile$`), () =>
    activateWithKeyboard(page.getByRole("button", { name: "保存投标资料" })),
  );

  await activateWithKeyboard(page.getByTestId("nav-procedural"));
  const attachmentKind = page.getByTestId("attachment-kind");
  await attachmentKind.focus();
  await attachmentKind.press("ArrowUp");
  await attachmentKind.press("ArrowUp");
  await attachmentKind.press("ArrowDown");
  await attachmentKind.press("Enter");
  await expect(attachmentKind).toHaveValue("授权证明");
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
    activateWithKeyboard(attachmentRow.getByRole("button", { name: "校验" })),
  );
  attachmentRow = page.getByTestId(`attachment-${attachmentId}`);
  await expect(attachmentRow).toContainText("draft / valid");
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/attachments/${attachmentId}/confirm$`), () =>
    activateWithKeyboard(attachmentRow.getByRole("button", { name: "确认" })),
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
    () => activateWithKeyboard(authorizationCard.getByTestId(new RegExp(`resolve-attachment-.+-${attachmentId}`))),
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
      () => activateWithKeyboard(card.getByRole("button", { name: confirmation ? "人工确认" : "不适用" })),
    );
  }

  await activateWithKeyboard(page.getByTestId("nav-quote"));
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/draft$`), () =>
    activateWithKeyboard(page.getByTestId("quote-create")),
  );
  const addedLineResponse = await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/`), () =>
    activateWithKeyboard(page.getByTestId("quote-add-line")),
  );
  const addedLineId = String((await addedLineResponse.json()).line_id);
  expect(addedLineId).toMatch(/^[0-9a-f-]{36}$/);
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${addedLineId}$`), () =>
    replaceWithKeyboard(page.getByTestId(`quote-line-entered-amount-${addedLineId}`), "1250.00"),
  );
  const lineConfirmation = page.getByTestId(`quote-line-confirmed-${addedLineId}`);
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${addedLineId}$`), () =>
    toggleWithKeyboard(lineConfirmation),
  );
  await expect(page.getByTestId(`quote-line-confirmed-${addedLineId}`)).toBeChecked();
  await replaceWithKeyboard(page.getByPlaceholder("复核原因"), "招标样例未设置最高限价，浏览器验收已人工复核");
  await toggleWithKeyboard(page.getByTestId("no-ceiling-review"));
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/finalize$`), () =>
    activateWithKeyboard(page.getByTestId("quote-finalize")),
  );
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/reopen$`), () =>
    activateWithKeyboard(page.getByTestId("quote-reopen")),
  );
  const reopenedLine = page.locator('tr[data-testid^="quote-line-"]').first();
  const reopenedLineTestId = await reopenedLine.getAttribute("data-testid");
  expect(reopenedLineTestId).toMatch(/^quote-line-[0-9a-f-]{36}$/);
  const reopenedLineId = reopenedLineTestId!.slice("quote-line-".length);
  const pricingMode = page.getByTestId(`quote-line-pricing-mode-${reopenedLineId}`);
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${reopenedLineId}$`), async () => {
    await pricingMode.focus();
    await pricingMode.press("ArrowDown");
    await pricingMode.press("ArrowDown");
    await pricingMode.press("Enter");
  });
  await expect(pricingMode).toHaveValue("单价计价");
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${reopenedLineId}$`), () =>
    replaceWithKeyboard(page.getByTestId(`quote-line-quantity-${reopenedLineId}`), "2.000000"),
  );
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${reopenedLineId}$`), () =>
    replaceWithKeyboard(page.getByTestId(`quote-line-unit-${reopenedLineId}`), "套"),
  );
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${reopenedLineId}$`), () =>
    replaceWithKeyboard(page.getByTestId(`quote-line-unit-price-${reopenedLineId}`), "600.000000"),
  );
  await clickForResponse(page, "PUT", new RegExp(`/api/v1/bids/${projectId}/quote/lines/${reopenedLineId}$`), () =>
    toggleWithKeyboard(page.getByTestId(`quote-line-confirmed-${reopenedLineId}`)),
  );
  await clickForResponse(page, "POST", new RegExp(`/api/v1/bids/${projectId}/quote/finalize$`), () =>
    activateWithKeyboard(page.getByTestId("quote-finalize")),
  );

  await activateWithKeyboard(page.getByTestId("wizard-parts"));
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
      activateWithKeyboard(page.getByTestId(`part-regenerate-${partKey}`)),
    );
  }
  await expect(page.getByTestId("gate-issues")).toContainText("Gate pass", { timeout: 60_000 });

  const pdfDownload = page.waitForEvent("download", { timeout: 120_000 });
  await activateWithKeyboard(page.getByTestId("export-pdf"));
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
      docx_quote_placeholder_warning: true,
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
