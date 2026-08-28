import type { BidV2Api } from "../api/client";
import type { WorkspaceEnvelope } from "../api/types";
import { emptyRichText } from "./contentBlock";
import { hasDrafts } from "./drafts";
import { describe, expect, it } from "./harness";
import { createBidV2Session } from "./session";

const SHA = "a".repeat(64);
const PROJECT = "11111111-1111-1111-1111-111111111111";
const WORKSPACE = "12121212-1212-1212-1212-121212121212";
const ROOT = "13131313-1313-1313-1313-131313131313";
const DOC = "19191919-1919-1919-1919-191919191919";
const EMPTY_SET = "10101010-1010-1010-1010-101010101010";
const FROZEN_SET = "17171717-1717-1717-1717-171717171717";

function envelope(over: Record<string, unknown> = {}): WorkspaceEnvelope {
  return {
    etag: SHA,
    workspace: {
      workspace_id: WORKSPACE,
      project_id: PROJECT,
      revision_id: "14141414-1414-1414-1414-141414141414",
      sha256: SHA,
      scope: "project_wide",
      outline_checkpoint_id: null,
      outline_checkpoint_sha256: null,
      requirement_projection_revision_id:
        "15151515-1515-1515-1515-151515151515",
      requirement_projection_sha256: SHA,
      document_settings_revision_id: "16161616-1616-1616-1616-161616161616",
      document_settings_sha256: SHA,
      document_settings: {
        page_size: "A4",
        margins_mm: { top: 25, right: 25, bottom: 25, left: 25 },
        cjk_font: "Noto Sans CJK SC",
        latin_font: "Times New Roman",
        body_font_pt: 12,
        line_spacing: 1.5,
        heading_numbering: "decimal",
        header: "",
        footer: "",
        page_number: "footer_center",
      },
      document_set_revision_id: EMPTY_SET,
      document_set_sha256: SHA,
      nodes: [
        {
          lineage_id: ROOT,
          revision_id: "18181818-1818-1818-1818-181818181818",
          parent_lineage_id: null,
          ordinal: 0,
          title: "投标文件",
          semantic_role: "other",
          render_role: "section",
          stale: false,
          block_lineage_ids: ["b1"],
        },
      ],
      blocks: [
        {
          schema_version: 1,
          block_revision_id: "b1",
          lineage_id: "b1",
          revision: 1,
          origin: "human",
          dependency_sha256: null,
          content_sha256: SHA,
          kind: "rich_text",
          content: emptyRichText(),
        },
      ],
      bindings: [],
      quote_snapshot: null,
      ...over,
    },
  };
}

function mockApi(impl: Partial<BidV2Api> = {}): BidV2Api {
  return new Proxy(impl as BidV2Api, {
    get(target, prop) {
      if (prop in target) return target[prop as keyof BidV2Api];
      return async () => {
        throw new Error(`unmocked ${String(prop)}`);
      };
    },
  });
}

const baseLists = {
  getProject: async () => ({
    id: PROJECT,
    title: "t",
    status: "open",
    ended_at: null,
    workspace_id: WORKSPACE,
  }),
  listTenderDocuments: async () => [
    {
      id: DOC,
      project_id: PROJECT,
      file_name: "tender.pdf",
      media_type: "application/pdf",
      byte_length: 12,
      document_role: "primary_tender" as const,
      role_revision_id: DOC,
      role_revision_sha256: SHA,
      role_provenance: "system_suggested" as const,
      parse_status: "ready" as const,
      conversion_generation: 1,
      error_code: null,
      original_sha256: SHA,
    },
  ],
  listRelations: async () => [],
  getAssessments: async () => ({ outline: null, submission: null }),
  getEvidenceOverview: async () => ({
    node_lineage_id: null,
    covered_requirement_ids: [],
    missing_requirement_ids: [],
    bundles: [],
  }),
  listAssets: async () => [],
};

describe("session poll and generateOutline", () => {
  it("poll does not reload workspace while drafts exist", async () => {
    let workspaceLoads = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => {
        workspaceLoads += 1;
        return envelope();
      },
      getRequest: async () => {
        throw new Error("no request");
      },
    });
    const session = createBidV2Session({
      api,
      clock: { now: () => 0, schedule: () => () => undefined },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    const before = workspaceLoads;
    session.editRichText("b1", {
      type: "doc",
      content: [{ type: "paragraph", content: [{ type: "text", text: "x" }] }],
    });
    expect(hasDrafts(session.getState().drafts)).toBe(true);
    await session.poll();
    expect(workspaceLoads).toBe(before);
    expect(hasDrafts(session.getState().drafts)).toBe(true);
    session.dispose();
  });

  it("generateOutline freezes even when a DocumentSet pointer already exists", async () => {
    let frozeExpected: string | null = null;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () =>
        envelope({
          document_set_revision_id: frozeExpected ? FROZEN_SET : EMPTY_SET,
          document_set_sha256: SHA,
        }),
      freezeDocumentSet: async (_project, _ids, expected) => {
        frozeExpected = expected?.artifact_id ?? null;
        return { artifact_id: FROZEN_SET, sha256: SHA };
      },
      createOutlineCandidate: async () => ({
        request_artifact_id: "req-1",
        kind: "OutlineGenerate",
        status: "pending",
      }),
    });
    const session = createBidV2Session({
      api,
      clock: { now: () => 0, schedule: () => () => undefined },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    await session.generateOutline();
    expect(frozeExpected).toBe(EMPTY_SET);
    expect(session.getState().asyncRequests[0]?.kind).toBe("OutlineGenerate");
    session.dispose();
  });

  it("hydrates an outline candidate from a succeeded generate request", async () => {
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope(),
      freezeDocumentSet: async () => ({ artifact_id: FROZEN_SET, sha256: SHA }),
      createOutlineCandidate: async () => ({
        request_artifact_id: "req-2",
        kind: "OutlineGenerate",
        status: "succeeded",
        result_identity: { artifact_id: "cand-outline", sha256: SHA },
      }),
      getCandidate: async () => ({
        candidate_id: "cand-outline",
        kind: "outline",
        status: "proposed",
        base_workspace_revision_id: "14141414-1414-1414-1414-141414141414",
        base_workspace_sha256: SHA,
        nodes: [
          {
            client_node_ref: "n1",
            parent_client_node_ref: null,
            ordinal: 0,
            title: "投标文件",
          },
        ],
        notices: [],
      }),
    });
    const session = createBidV2Session({
      api,
      clock: { now: () => 0, schedule: () => () => undefined },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    await session.generateOutline();
    expect(session.getState().candidate?.kind).toBe("outline");
    expect(session.getState().selectedOutlineNodeRefs).toEqual(["n1"]);
    session.dispose();
  });
});
