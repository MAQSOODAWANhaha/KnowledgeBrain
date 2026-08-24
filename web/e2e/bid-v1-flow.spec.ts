import { expect, test, type Page } from "@playwright/test";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const PROJECT = "11111111-1111-1111-1111-111111111111";
const CLAUSE = "22222222-2222-2222-2222-222222222222";
const ROUTE = "33333333-3333-3333-3333-333333333333";
const CAND = "44444444-4444-4444-4444-444444444444";
const CAND_2 = "45454545-4545-4545-4545-454545454545";
const REQ = "55555555-5555-5555-5555-555555555555";
const MANIFEST = "66666666-6666-6666-6666-666666666666";
const OUTPUT = "77777777-7777-7777-7777-777777777777";
const OTHER_MANIFEST = "88888888-8888-8888-8888-888888888888";
const PROCEDURAL = "99999999-9999-9999-9999-999999999999";
const ATTACHMENT = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

function project(over: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    title: "示范招标",
    owner_user_id: "00000000-0000-0000-0000-000000000001",
    ends_at: "2026-12-31T15:59:59Z",
    expires_at: null,
    status: "open",
    ended_at: null,
    fact_revision: 1,
    fact_sha256: "a".repeat(64),
    budget_amount: null,
    ceiling_price: null,
    ceiling_basis: "unspecified",
    ceiling_revision: 0,
    ceiling_identity_sha256: "b".repeat(64),
    bid_open_at: null,
    bid_valid_until: "2026-12-01T00:00:00Z",
    bid_valid_days: 90,
    ...over,
  };
}

async function mockApi(
  page: Page,
  options: {
    failFirstDocumentUpload?: boolean;
    failFirstRender?: boolean;
    rejectFirstManifest?: boolean;
    failFirstRenderJob?: boolean;
    holdPartLoad?: string;
    withoutPricingSet?: boolean;
    withProceduralAttachment?: boolean;
    validityConflict?: boolean;
  } = {},
) {
  let quoteExists = false;
  let quotePointer = "draft";
  const documentUploadKeys: string[] = [];
  const manifestKeys: string[] = [];
  const renderKeys: string[] = [];
  const renderJobPolls: string[] = [];
  const downloadedOutputIds: string[] = [];
  const partRegenerateBodies: Array<Record<string, unknown>> = [];
  const quoteFinalizeBodies: Array<Record<string, unknown>> = [];
  const submissionProfileBodies: Array<Record<string, unknown>> = [];
  const proceduralResolutionBodies: Array<Record<string, unknown>> = [];
  const factMutationBodies: Array<Record<string, unknown>> = [];
  const routePickBodies: Array<Record<string, unknown>> = [];
  let pickRevision = 0;
  let pickItems: Array<{
    requirement_artifact_id: string;
    candidate_artifact_id: string;
  }> = [];
  let releasePartLoad: () => void = () => {};
  const heldPartLoad = new Promise<void>((resolve) => {
    releasePartLoad = resolve;
  });
  const manifestsByKey = new Map<string, string>();
  const manifestsByJob = new Map<string, string>();
  const pollsByJob = new Map<string, number>();
  const required = ["1", "3", "4", "5", "6:letter", "6:authorization", "6:quote", "6:implementation_plan", "6:procedural"];
  await page.route("**/api/v1/**", async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    const json = (body: unknown, status = 200) =>
      route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

    if (p === "/api/v1/auth/login" && method === "POST") {
      return json({ token: "t", user_id: "00000000-0000-0000-0000-000000000001" });
    }
    if (p === "/api/v1/me") return json({ id: "00000000-0000-0000-0000-000000000001", email: "e2e@local" });
    if (p === "/api/v1/bids" && method === "GET") return json([]);
    if (p === "/api/v1/bids" && method === "POST") {
      const body = req.postDataJSON() as { owner_name?: string; ends_at?: string };
      if (body.owner_name) return json({ error: { code: "VALIDATION", message: "unknown field" } }, 400);
      if (!body.ends_at) return json({ error: { code: "VALIDATION", message: "ends_at required" } }, 400);
      return json({ id: PROJECT }, 201);
    }
    if (p === `/api/v1/bids/${PROJECT}` && method === "GET") {
      return json({
        project: project(options.validityConflict
          ? { bid_valid_days: 90, bid_valid_until: "2026-12-01T00:00:00Z" }
          : {}),
        documents: [],
        quote: quoteExists
          ? { exists: true, pointer: quotePointer, edit_version: 1, title: "示范招标 报价", tax_mode: "tax_exclusive", lines: [] }
          : { exists: false },
        facts: { revision: 1, suggestions: [], budget_amount: null, ceiling_price: null, ceiling_basis: "unspecified", ceiling_revision: 0, ceiling_identity_sha256: "b".repeat(64), expires_at: null, bid_open_at: null, bid_valid_until: null, bid_valid_days: null },
        clause_sets: options.withoutPricingSet
          ? []
          : [{ set_kind: "pricing", revision: 0, content_sha256: "c".repeat(64) }],
        matching: {
          routes: [{ route_id: ROUTE, route_kind: "technical", unit_id: null }],
          reports: [],
          commercial_decisions: [],
          technical_candidates: [
            { requirement_artifact_id: REQ, candidate_artifact_id: CAND, product_id: "prod-1", product_version_id: "ver-1", recommended: true },
            { requirement_artifact_id: REQ, candidate_artifact_id: CAND_2, product_id: "prod-2", product_version_id: "ver-2", recommended: false },
          ],
          project_pick_set: null,
        },
        parts: required.map((part_key) => ({ part_key, stale: false })),
        outputs: [],
        derived: { has_files: false, files_ready: false, extract_running: false, unconfirmed_drafts: 1, match_running: false, has_picks: false, files_not_in_clauses: 0 },
      });
    }
    if (p.endsWith("/documents") && method === "GET") return json({ documents: [] });
    if (p.endsWith("/documents") && method === "POST") {
      documentUploadKeys.push(req.headers()["idempotency-key"] ?? "");
      if (options.failFirstDocumentUpload && documentUploadKeys.length === 1) return route.abort("connectionreset");
      return json({ id: "doc-1" }, 201);
    }
    if (p.endsWith("/clauses") && method === "GET") {
      return json({
        clauses: [
          {
            id: CLAUSE,
            project_id: PROJECT,
            publication_id: null,
            provenance: "manual",
            status: "draft",
            kind: "technical",
            family: "technical",
            text: "系统必须支持双千兆网络接口。",
            must: true,
            revision: 1,
            current_source_span_v2: null,
            extracted_origin_source_span_v2: null,
            confirmation_required_reason: null,
            confirmation_required_router_generation: null,
          },
        ],
      });
    }
    if (p.endsWith("/clauses") && method === "POST") {
      const body = req.postDataJSON() as { family?: string };
      if (body.family) return json({ error: { code: "VALIDATION", message: "client must not submit family" } }, 400);
      return json({ id: CLAUSE, revision: 1, status: "draft", kind: "technical", family: "technical" }, 201);
    }
    if (p.includes("/clauses/") && method === "PATCH") return json({ id: CLAUSE, revision: 2, status: "confirmed" });
    if (p.endsWith("/facts") && method === "GET") {
      return json({ project_facts: { revision: 1, bid_valid_days: 90, bid_valid_until: "2026-12-01T00:00:00Z" }, suggestions: [], history: [] });
    }
    if (p.endsWith("/facts") && method === "POST") {
      factMutationBodies.push(req.postDataJSON() as Record<string, unknown>);
      return json({ fact_revision: 2 });
    }
    if (p.endsWith("/units")) return json({ units: [{ id: null, route_id: ROUTE, kind: "unsectioned", heading_path: "未归段" }] });
    if (p.endsWith("/matching") && method === "GET") {
      return json({
        routes: [{ route_id: ROUTE, route_kind: "technical", unit_id: "00000000-0000-0000-0000-000000000000" }],
        reports: [{ id: "rep-1", route_id: ROUTE, content_sha256: "d".repeat(64), generation: 1 }],
        commercial_decisions: [],
        technical_candidates: [
          { requirement_artifact_id: REQ, candidate_artifact_id: CAND, product_id: "prod-1", product_version_id: "ver-1", recommended: true },
          { requirement_artifact_id: REQ, candidate_artifact_id: CAND_2, product_id: "prod-2", product_version_id: "ver-2", recommended: false },
        ],
        route_pick_sets: [],
        project_pick_set: null,
      });
    }
    if (p.endsWith("/matching/schedule")) return json({ job_id: null });
    if (p.endsWith("/pick-set") && method === "GET") {
      return json({
        route_id: ROUTE,
        route_kind: "technical",
        unit_id: "00000000-0000-0000-0000-000000000000",
        source_report_artifact_id: "rep-1",
        report_sha256: "d".repeat(64),
        report_generation: 1,
        revision: pickRevision,
        items: pickItems,
        supported_candidates: [
          { requirement_artifact_id: REQ, candidate_artifact_id: CAND, product_id: "prod-1", product_version_id: "ver-1", recommended: true },
          { requirement_artifact_id: REQ, candidate_artifact_id: CAND_2, product_id: "prod-2", product_version_id: "ver-2", recommended: false },
        ],
      });
    }
    if (p.endsWith("/pick-set") && method === "PUT") {
      const body = req.postDataJSON() as {
        expected_revision: number;
        items: typeof pickItems;
      };
      routePickBodies.push({
        expected_revision: body.expected_revision,
        items: body.items,
      });
      pickItems = body.items.map((item) => ({ ...item }));
      pickRevision += 1;
      return json({ route_revision: pickRevision });
    }
    if (p.endsWith("/quote") && method === "GET") {
      return json(
        quoteExists
          ? { exists: true, pointer: quotePointer, edit_version: 1, title: "示范招标 报价", tax_mode: "tax_exclusive", lines: [], snapshot_id: quotePointer === "finalized" ? "snap-1" : null }
          : { exists: false },
      );
    }
    if (p.endsWith("/quote/draft")) {
      quoteExists = true;
      quotePointer = "draft";
      return json({ exists: true, pointer: "draft", edit_version: 0 }, 201);
    }
    if (p.endsWith("/quote/finalize")) {
      quoteFinalizeBodies.push(req.postDataJSON() as Record<string, unknown>);
      quotePointer = "finalized";
      return json({ snapshot_id: "snap-1", eligibility: "eligible" });
    }
    if (p.endsWith("/quote/reopen")) {
      quotePointer = "draft";
      return json({ pointer: "draft" });
    }
    if (p.endsWith("/quote/preview")) return json({ net_total: "0.00", tax_total: "0.00", gross_total: "0.00" });
    if (p.endsWith("/company-profile")) return json({ revision: 0, legal_name: "示例公司" });
    if (p.endsWith("/submission-profile")) {
      if (method === "PUT") submissionProfileBodies.push(req.postDataJSON() as Record<string, unknown>);
      return json({ revision: 0, buyer_name: "招标人", seal_confirmed: false, signature_confirmed: false });
    }
    if (p.endsWith("/procedural-requirements")) {
      return json({
        classifications: options.withProceduralAttachment
          ? [{
              id: PROCEDURAL,
              effective_requirement_kind: "authorization_support",
              router_result_status: "classified",
              lifecycle_status: "current",
            }]
          : [],
      });
    }
    if (p.includes("/procedural-requirements/") && p.endsWith("/resolve") && method === "POST") {
      proceduralResolutionBodies.push(req.postDataJSON() as Record<string, unknown>);
      return json({ revision: 1 });
    }
    if (p.endsWith("/attachments") && method === "GET") {
      return json({
        attachments: options.withProceduralAttachment
          ? [{
              id: ATTACHMENT,
              kind: "authorization_support",
              status: "confirmed",
              validation_status: "valid",
              revision: 3,
            }]
          : [],
      });
    }
    if (p.endsWith("/parts") && method === "GET") return json({ required_part_keys: required, parts: required.map((part_key) => ({ part_key, stale: false })) });
    if (p.includes("/parts/") && method === "GET") {
      const partKey = decodeURIComponent(p.split("/parts/")[1] ?? "");
      if (partKey === options.holdPartLoad) await heldPartLoad;
      if (partKey === "1") {
        return json({
          part_key: "1",
          markdown: "# 项目概况\n",
          content_revision: 1,
          dependency_sha256: "d".repeat(64),
          stale: false,
        });
      }
      return json({ error: { code: "NOT_FOUND", message: partKey } }, 404);
    }
    if (p.includes("/parts/") && method === "POST") {
      partRegenerateBodies.push(req.postDataJSON() as Record<string, unknown>);
      return json({ revision: 1 });
    }
    if (p.includes("/parts/") && method === "PUT") return json({ revision: 2 });
    if (p.endsWith("/gate-issues")) {
      return json({
        format: url.searchParams.get("format"),
        status: "reject",
        issues: [
          { code: "QUOTE_NOT_FINALIZED", part_key: "6:quote" },
          { code: "BID_VALIDITY_CONFLICT", part_key: "6:letter" },
        ],
        required_part_keys: required,
      });
    }
    if (p.endsWith("/submission/manifests") && method === "POST") {
      const key = req.headers()["idempotency-key"] ?? "";
      manifestKeys.push(key);
      if (options.rejectFirstManifest && manifestKeys.length === 1) {
        return json({ error: { code: "SUBMISSION_GATE_REJECTED", message: "submission gate rejected" } }, 400);
      }
      if (!manifestsByKey.has(key)) {
        manifestsByKey.set(key, manifestsByKey.size === 0 ? MANIFEST : OTHER_MANIFEST);
      }
      return json({ manifest_id: manifestsByKey.get(key), content_sha256: "e".repeat(64), format: "docx" }, 201);
    }
    if (p.endsWith("/render") && method === "POST") {
      renderKeys.push(req.headers()["idempotency-key"] ?? "");
      if (options.failFirstRender && renderKeys.length === 1) return route.abort("connectionreset");
      const manifestId = p.split("/").at(-2) ?? "";
      const renderJobId = `render-job-${renderKeys.length}`;
      manifestsByJob.set(renderJobId, manifestId);
      return json({ render_job_id: renderJobId, manifest_id: manifestId, status: "queued" }, 202);
    }
    if (p.includes("/submission/render-jobs/") && method === "GET") {
      const renderJobId = p.split("/").at(-1) ?? "";
      const manifestId = manifestsByJob.get(renderJobId) ?? "";
      renderJobPolls.push(renderJobId);
      const polls = (pollsByJob.get(renderJobId) ?? 0) + 1;
      pollsByJob.set(renderJobId, polls);
      if (options.failFirstRenderJob && renderJobId === "render-job-1") {
        return json({
          render_job_id: renderJobId,
          manifest_id: manifestId,
          status: "failed",
          attempt_count: 1,
          max_attempts: 4,
          error_code: "SUBMISSION_END_STATE_CHANGED",
        });
      }
      return polls === 1
        ? json({ render_job_id: renderJobId, manifest_id: manifestId, status: "running", attempt_count: 1, max_attempts: 4 })
        : json({ render_job_id: renderJobId, manifest_id: manifestId, status: "completed", attempt_count: 1, max_attempts: 4, output_id: OUTPUT });
    }
    if (p.includes("/submission/artifacts/")) {
      downloadedOutputIds.push(p.split("/").at(-1) ?? "");
      return route.fulfill({
        status: 200,
        contentType: "application/pdf",
        headers: { "content-disposition": 'attachment; filename="submission.pdf"' },
        body: "%PDF-1.4\n",
      });
    }
    return json({ error: { code: "NOT_FOUND", message: p } }, 404);
  });
  return {
    documentUploadKeys,
    manifestKeys,
    renderKeys,
    renderJobPolls,
    downloadedOutputIds,
    partRegenerateBodies,
    quoteFinalizeBodies,
    submissionProfileBodies,
    proceduralResolutionBodies,
    factMutationBodies,
    routePickBodies,
    releasePartLoad,
  };
}

test("mocked UI contract: keyboard walk and export buttons visible", async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await expect(page.getByTestId("new-bid")).toBeVisible();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();
  await expect(page.getByTestId("wizard-files")).toBeVisible();
  await page.getByTestId("wizard-facts").click();
  await expect(page.getByTestId("validity-conflict")).toBeVisible();
  await page.getByTestId("nav-pending").click();
  await page.getByTestId("clause-text").fill("系统必须支持双千兆网络接口。");
  await page.getByTestId("clause-add").click();
  await page.getByTestId("wizard-matching").click();
  await page.getByTestId("schedule-match").click();
  await page.getByTestId("wizard-quote").click();
  await page.getByTestId("quote-create").click();
  await page.getByTestId("no-ceiling-review").click();
  await page.getByTestId("quote-finalize").click();
  await page.getByTestId("wizard-parts").click();
  await expect(page.getByTestId("gate-issues")).toContainText("BID_VALIDITY_CONFLICT");
  await expect(page.getByTestId("export-docx")).toBeVisible();
  await expect(page.getByTestId("export-pdf")).toBeVisible();
  await page.screenshot({ path: path.join(testInfo.outputDir, "bid-v1-parts.png"), fullPage: true });
  const digest = createHash("sha256").update("mocked-ui-contract-not-a-live-pdf").digest("hex");
  const artifactDir = path.join(testInfo.file, "..", "artifacts");
  await mkdir(artifactDir, { recursive: true });
  await writeFile(
    path.join(artifactDir, "mocked-ui-contract.sha256"),
    `${digest}  mocked UI contract only; live PDF pack was not executed\n`,
  );
});

test("technical matching preserves both manual picks after controlled reloads", async ({ page }) => {
  const requests = await mockApi(page);
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.goto(`/#/bids/${PROJECT}?step=matching&view=unsectioned`);

  const firstPick = page.getByTestId(`pick-${CAND}`);
  const secondPick = page.getByTestId(`pick-${CAND_2}`);
  await expect(firstPick).not.toBeChecked();
  await expect(secondPick).not.toBeChecked();

  const firstResponse = page.waitForResponse((response) =>
    response.request().method() === "PUT" &&
    new URL(response.url()).pathname === `/api/v1/bids/${PROJECT}/matching/routes/${ROUTE}/pick-set`,
  );
  await firstPick.click();
  expect((await firstResponse).ok()).toBeTruthy();
  await expect(firstPick).toBeChecked();

  const secondResponse = page.waitForResponse((response) =>
    response.request().method() === "PUT" &&
    new URL(response.url()).pathname === `/api/v1/bids/${PROJECT}/matching/routes/${ROUTE}/pick-set`,
  );
  await secondPick.click();
  expect((await secondResponse).ok()).toBeTruthy();
  await expect(firstPick).toBeChecked();
  await expect(secondPick).toBeChecked();

  expect(requests.routePickBodies).toEqual([
    {
      expected_revision: 0,
      items: [{ requirement_artifact_id: REQ, candidate_artifact_id: CAND }],
    },
    {
      expected_revision: 1,
      items: [
        { requirement_artifact_id: REQ, candidate_artifact_id: CAND },
        { requirement_artifact_id: REQ, candidate_artifact_id: CAND_2 },
      ],
    },
  ]);
});

test("part route change cannot reuse the previous part CAS identity", async ({ page }) => {
  const requests = await mockApi(page, { holdPartLoad: "3" });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.goto(`/#/bids/${PROJECT}?step=parts&part=1`);
  await expect(page.getByTestId("part-pane-1")).toHaveAttribute("data-ready", "true");

  await page.goto(`/#/bids/${PROJECT}?step=parts&part=3`);
  const regenerate = page.getByTestId("part-regenerate-3");
  await expect(regenerate).toBeDisabled();
  requests.releasePartLoad();
  await expect(page.getByTestId("part-pane-3")).toHaveAttribute("data-ready", "true");
  await regenerate.click();
  await expect.poll(() => requests.partRegenerateBodies.length).toBe(1);

  expect(requests.partRegenerateBodies[0]).toEqual({
    expected_content_revision: 0,
    expected_dependency_sha256: null,
  });
});

test("quote finalization is blocked when the pricing set identity is unavailable", async ({ page }) => {
  const requests = await mockApi(page, { withoutPricingSet: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.goto(`/#/bids/${PROJECT}?step=quote&view=quote`);
  await page.getByTestId("quote-create").click();
  await page.getByTestId("quote-finalize").click();

  await expect(page.getByText("缺少价格条款集，无法定稿报价")).toBeVisible();
  expect(requests.quoteFinalizeBodies).toHaveLength(0);
});

test("submission profile save is blocked when the date is empty", async ({ page }) => {
  const requests = await mockApi(page);
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.goto(`/#/bids/${PROJECT}?step=quote&view=submission`);
  await page.getByRole("button", { name: "保存投标资料" }).click();

  await expect(page.getByText("请填写投标日期")).toBeVisible();
  expect(requests.submissionProfileBodies).toHaveLength(0);
});

test("a confirmed valid attachment can satisfy its procedural requirement", async ({ page }) => {
  const requests = await mockApi(page, { withProceduralAttachment: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.goto(`/#/bids/${PROJECT}?step=quote&view=procedural`);
  await page.getByTestId(`resolve-attachment-${PROCEDURAL}-${ATTACHMENT}`).click();

  await expect.poll(() => requests.proceduralResolutionBodies).toEqual([
    { resolution: "satisfied_by_attachment", attachment_id: ATTACHMENT },
  ]);
});

test("a conflicting validity fact can be cleared from the UI", async ({ page }) => {
  const requests = await mockApi(page, { validityConflict: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.goto(`/#/bids/${PROJECT}?step=facts&view=facts`);
  await expect(page.getByTestId("validity-conflict")).toBeVisible();
  await page.getByTestId("fact-clear-bid_valid_until").click();

  await expect.poll(() => requests.factMutationBodies).toEqual([
    { action: "clear", expected_fact_revision: 1, field: "bid_valid_until" },
  ]);
});

test("submission export retry reuses request idempotency keys", async ({ page }) => {
  const requests = await mockApi(page, { failFirstRender: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();

  await page.getByTestId("export-docx").click();
  await expect.poll(() => requests.renderKeys.length).toBe(1);
  await page.waitForTimeout(50);
  await page.getByTestId("export-docx").click();
  await expect.poll(() => requests.renderKeys.length).toBe(2);
  await expect.poll(() => requests.downloadedOutputIds).toEqual([OUTPUT]);

  expect(requests.manifestKeys).toHaveLength(2);
  expect(requests.renderJobPolls).toHaveLength(2);
  expect(requests.manifestKeys[0]).not.toBe("");
  expect(requests.renderKeys[0]).not.toBe("");
  expect(requests.manifestKeys[1]).toBe(requests.manifestKeys[0]);
  expect(requests.renderKeys[1]).toBe(requests.renderKeys[0]);
});

test("definitive manifest rejection starts a new submission export attempt", async ({ page }) => {
  const requests = await mockApi(page, { rejectFirstManifest: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();

  await page.getByTestId("export-docx").click();
  await expect(page.getByText("submission gate rejected")).toBeVisible();
  await page.getByTestId("export-docx").click();
  await expect.poll(() => requests.downloadedOutputIds).toEqual([OUTPUT]);

  expect(requests.manifestKeys).toHaveLength(2);
  expect(requests.renderKeys).toHaveLength(1);
  expect(requests.manifestKeys[0]).not.toBe("");
  expect(requests.manifestKeys[1]).not.toBe(requests.manifestKeys[0]);
});

test("durable render failure starts a new submission export attempt", async ({ page }) => {
  const requests = await mockApi(page, { failFirstRenderJob: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();

  await page.getByTestId("export-docx").click();
  await expect(page.getByText("渲染失败，请重试")).toBeVisible();
  await page.getByTestId("export-docx").click();
  await expect.poll(() => requests.downloadedOutputIds).toEqual([OUTPUT]);

  expect(requests.manifestKeys).toHaveLength(2);
  expect(requests.renderKeys).toHaveLength(2);
  expect(requests.manifestKeys[1]).not.toBe(requests.manifestKeys[0]);
  expect(requests.renderKeys[1]).not.toBe(requests.renderKeys[0]);
});

test("document upload retry reuses its request idempotency key", async ({ page }) => {
  const requests = await mockApi(page, { failFirstDocumentUpload: true });
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();

  const upload = page.locator('input[type="file"][hidden]');
  const tender = { name: "tender.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.7\n%%EOF\n") };
  await upload.setInputFiles(tender);
  await expect.poll(() => requests.documentUploadKeys.length).toBe(1);
  await page.waitForTimeout(50);
  await upload.setInputFiles(tender);
  await expect.poll(() => requests.documentUploadKeys.length).toBe(2);

  expect(requests.documentUploadKeys[0]).not.toBe("");
  expect(requests.documentUploadKeys[1]).toBe(requests.documentUploadKeys[0]);
});

test("different upload bytes never share an idempotency key", async ({ page }) => {
  const requests = await mockApi(page);
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();

  const upload = page.locator('input[type="file"][hidden]');
  await upload.setInputFiles([
    { name: "same.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-A\n%%EOF\n") },
    { name: "same.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-B\n%%EOF\n") },
  ]);
  await expect.poll(() => requests.documentUploadKeys.length).toBe(2);

  expect(requests.documentUploadKeys[0]).not.toBe(requests.documentUploadKeys[1]);
});

test("identical initial uploads remain separate logical attempts", async ({ page }) => {
  const requests = await mockApi(page);
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await page.getByTestId("login-submit").click();
  await page.getByTestId("new-bid").click();
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await page.getByTestId("bid-create").click();

  const upload = page.locator('input[type="file"][hidden]');
  const tender = { name: "same.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.7\n%%EOF\n") };
  await upload.setInputFiles([tender, tender]);
  await expect.poll(() => requests.documentUploadKeys.length).toBe(2);

  expect(requests.documentUploadKeys[0]).not.toBe(requests.documentUploadKeys[1]);
});
