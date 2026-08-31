import { expect, test, type Locator, type Page } from "@playwright/test";

const PROJECT = "11111111-1111-1111-1111-111111111111";
const WORKSPACE = "12121212-1212-1212-1212-121212121212";
const ROOT_NODE = "13131313-1313-1313-1313-131313131313";
const SHA = "a".repeat(64);

async function activateWithKeyboard(locator: Locator) {
  await expect(locator).toBeEnabled();
  await locator.focus();
  await locator.press("Enter");
}

async function login(page: Page) {
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill("e2e@local");
  await page.getByTestId("login-password").fill("pw");
  await activateWithKeyboard(page.getByTestId("login-submit"));
  await expect(page.getByTestId("new-bid")).toBeVisible();
}

function project() {
  return {
    id: PROJECT,
    title: "示范招标",
    status: "open",
    ended_at: null,
    ends_at: "2026-12-31T15:59:59Z",
    workspace_id: WORKSPACE,
  };
}

function workspace(over: Record<string, unknown> = {}) {
  return {
    workspace_id: WORKSPACE,
    project_id: PROJECT,
    revision_id: "14141414-1414-1414-1414-141414141414",
    sha256: SHA,
    scope: "project_wide",
    outline_checkpoint_id: null,
    outline_checkpoint_sha256: null,
    requirement_projection_revision_id: "15151515-1515-1515-1515-151515151515",
    requirement_projection_sha256: SHA,
    document_settings_revision_id: "16161616-1616-1616-1616-161616161616",
    document_settings_sha256: SHA,
    document_settings: {
      page_size: "A4",
      margins_mm: { top: 25, right: 25, bottom: 25, left: 25 },
      body_font_pt: 12,
      line_spacing: 1.5,
      heading_numbering: "decimal",
      header: "",
      footer: "",
      page_number: "footer_center",
    },
    document_set_revision_id: "17171717-1717-1717-1717-171717171717",
    document_set_sha256: SHA,
    nodes: [
      {
        lineage_id: ROOT_NODE,
        revision_id: "18181818-1818-1818-1818-181818181818",
        parent_lineage_id: null,
        ordinal: 0,
        title: "投标文件",
        semantic_role: "other",
        render_role: "section",
        stale: false,
        block_lineage_ids: [],
      },
    ],
    blocks: [],
    bindings: [],
    quote_snapshot: null,
    ...over,
  };
}

function readyDocument() {
  return {
    id: "19191919-1919-1919-1919-191919191919",
    project_id: PROJECT,
    file_name: "tender.pdf",
    media_type: "application/pdf",
    byte_length: 12,
    document_role: "primary_tender",
    role_revision_id: "19191919-1919-1919-1919-191919191919",
    role_revision_sha256: SHA,
    role_provenance: "system_suggested",
    parse_status: "ready",
    conversion_generation: 1,
    error_code: null,
    original_sha256: SHA,
  };
}

async function mockApi(page: Page) {
  const documents: Array<Record<string, unknown>> = [readyDocument()];
  let freezeIssued = false;
  let workspaceReadsAfterFreeze = 0;
  await page.route("**/api/**", async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    const json = (body: unknown, status = 200) =>
      route.fulfill({
        status,
        contentType: "application/json",
        headers: { ETag: `"${SHA}"` },
        body: JSON.stringify(body),
      });
    if (p.endsWith("/auth/login") && method === "POST") {
      return json({ token: "t", user_id: "u" });
    }
    if (p.endsWith("/me")) return json({ id: "u", email: "e2e@local" });
    if (p === "/api/v2/bid-projects" && method === "GET")
      return json([project()]);
    if (p === "/api/v2/bid-projects" && method === "POST")
      return json(project());
    if (p === `/api/v2/bid-projects/${PROJECT}` && method === "GET")
      return json(project());
    if (p.endsWith("/tender-documents") && method === "GET")
      return json({ documents });
    if (p.endsWith("/tender-documents") && method === "POST") {
      const uploaded = readyDocument();
      documents.push(uploaded);
      return json(uploaded);
    }
    if (p.endsWith("/tender-document-relations"))
      return json({ relations: [] });
    if (p.endsWith("/source-units")) return json({ source_units: [] });
    if (p.endsWith("/requirements")) return json({ requirements: [] });
    if (
      p.endsWith("/workspace") ||
      p === `/api/v2/submission-workspaces/${WORKSPACE}`
    ) {
      if (!freezeIssued) return json(workspace());
      workspaceReadsAfterFreeze += 1;
      return json(
        workspace({
          revision_id:
            workspaceReadsAfterFreeze === 1
              ? "24242424-2424-2424-2424-242424242424"
              : "25252525-2525-2525-2525-252525252525",
        }),
      );
    }
    if (p.endsWith("/mutations") && method === "POST") return json(workspace());
    if (p.endsWith("/document-set-revisions") && method === "POST") {
      freezeIssued = true;
      workspaceReadsAfterFreeze = 0;
      return json({
        artifact_id: "17171717-1717-1717-1717-171717171717",
        sha256: SHA,
      });
    }
    if (p.endsWith("/outline-candidates") && method === "POST") {
      return json({
        request_artifact_id: "outline-req",
        kind: "OutlineGenerate",
        status: "succeeded",
        result_identity: { artifact_id: "cand-outline", sha256: SHA },
      });
    }
    if (p.endsWith("/content-candidates") && method === "POST") {
      return json({
        request_artifact_id: "content-req",
        kind: "ContentGenerate",
        status: "succeeded",
        result_identity: { artifact_id: "cand-content", sha256: SHA },
      });
    }
    if (p.includes("/requests/")) {
      return json({
        request_artifact_id: "outline-req",
        kind: "OutlineGenerate",
        status: "succeeded",
        result_identity: { artifact_id: "cand-outline", sha256: SHA },
      });
    }
    if (p.includes("/candidates/") && method === "GET") {
      return json({
        schema_version: 2,
        candidate_id: "cand-outline",
        kind: "outline",
        status: "proposed",
        base_workspace_revision_id: "25252525-2525-2525-2525-252525252525",
        base_workspace_sha256: SHA,
        nodes: [
          {
            client_node_ref: "root",
            parent_client_node_ref: null,
            ordinal: 0,
            title: "投标文件",
            semantic_role: "cover",
            render_role: "front_matter",
            origin_source_unit_revision_ids: [],
          },
          {
            client_node_ref: "toc",
            parent_client_node_ref: "root",
            ordinal: 0,
            title: "目录",
            semantic_role: "toc",
            render_role: "toc",
            origin_source_unit_revision_ids: [],
          },
          {
            client_node_ref: "commercial",
            parent_client_node_ref: "root",
            ordinal: 1,
            title: "商务文件",
            semantic_role: "commercial",
            render_role: "section",
            origin_source_unit_revision_ids: ["source-commercial"],
          },
          {
            client_node_ref: "commercial-child",
            parent_client_node_ref: "commercial",
            ordinal: 0,
            title: "资格响应",
            semantic_role: "qualification",
            render_role: "section",
            origin_source_unit_revision_ids: ["source-commercial"],
          },
          {
            client_node_ref: "technical",
            parent_client_node_ref: "root",
            ordinal: 2,
            title: "技术文件",
            semantic_role: "technical",
            render_role: "section",
            origin_source_unit_revision_ids: ["source-technical"],
          },
          {
            client_node_ref: "technical-child",
            parent_client_node_ref: "technical",
            ordinal: 0,
            title: "技术要求响应",
            semantic_role: "technical",
            render_role: "section",
            origin_source_unit_revision_ids: ["source-technical"],
          },
        ],
        bindings: [
          {
            need_occurrence_id: "need-1",
            channel: "narrative_content",
            target_client_node_ref: "technical-child",
          },
        ],
        section_obligation_bindings: [
          {
            obligation_id: "b".repeat(64),
            target_client_node_ref: "technical-child",
          },
        ],
        notices: [],
      });
    }
    if (p.includes("/candidates/") && method === "POST") {
      return json(
        workspace({
          document_set_revision_id: "17171717-1717-1717-1717-171717171717",
          document_set_sha256: SHA,
        }),
      );
    }
    if (p.endsWith("/assessments/current")) {
      return json({
        outline: {
          assessment_snapshot_id: "a",
          status: "has_warnings",
          issues: [],
        },
        submission: {
          assessment_snapshot_id: "b",
          status: "has_warnings",
          issues: [],
        },
      });
    }
    if (p.endsWith("/evidence-overview")) {
      return json({
        node_lineage_id: null,
        covered_requirement_ids: [],
        missing_requirement_ids: [],
        bundles: [],
      });
    }
    if (p.endsWith("/assets") && method === "GET") return json({ assets: [] });
    if (p.includes("/preview")) return json({ html: "<p>preview</p>" });
    if (p.endsWith("/exports") && method === "GET")
      return json({ exports: [] });
    if (p.endsWith("/exports") && method === "POST") {
      return json({
        request_artifact_id: "e",
        kind: "SubmissionExport",
        status: "pending",
      });
    }
    if (p.includes("/quote")) return json({ exists: false });
    return json({ message: `unmocked ${method} ${p}` }, 404);
  });
}

test("V2 golden path: files, authoring canvas, outline candidate, export without Gate", async ({
  page,
}) => {
  await mockApi(page);
  await login(page);
  await activateWithKeyboard(page.getByTestId("new-bid"));
  await page.getByTestId("bid-title").fill("示范招标");
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await activateWithKeyboard(page.getByTestId("bid-create"));
  await expect(page.getByTestId("wizard-files")).toBeVisible();
  await expect(page.getByTestId("upload-drop")).toBeVisible();
  await expect(page.getByTestId("wizard-facts")).toHaveCount(0);
  await expect(page.getByTestId("wizard-parts")).toHaveCount(0);
  await expect(page.getByTestId("wizard-requirements")).toHaveCount(0);
  await expect(page.getByTestId("wizard-preview")).toHaveCount(0);
  await expect(page.getByTestId("gate-issues")).toHaveCount(0);

  await activateWithKeyboard(page.getByTestId("wizard-authoring"));
  await expect(page.getByTestId("outline-tree")).toBeVisible();
  await expect(page.getByTestId("document-canvas")).toBeVisible();
  await expect(page.getByTestId("section-editor")).toBeVisible();
  await expect(page.getByTestId(`outline-node-${ROOT_NODE}`)).toContainText(
    "投标文件",
  );
  await expect(page.locator(`#canvas-section-${ROOT_NODE}`)).toBeVisible();
  await expect(page.getByTestId("generate-outline")).toBeEnabled();
  await activateWithKeyboard(page.getByTestId("generate-outline"));
  await expect(page.getByTestId("candidate-review")).toBeVisible();
  await expect(page.getByTestId("outline-quality-summary")).toContainText(
    "一级章节 2",
  );
  await expect(page.getByTestId("candidate-accept")).toBeEnabled();
  await expect(page.getByTestId("generate-outline")).toBeEnabled();

  await activateWithKeyboard(page.getByTestId("wizard-export"));
  await expect(page.getByTestId("export-docx")).toBeVisible();
  await expect(page.getByTestId("export-pdf")).toBeVisible();
  await expect(page.getByTestId("assessment-report")).toContainText(
    "不是 Gate",
  );
});
