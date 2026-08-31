import { ApiError, NetworkTransportError } from "../../api";
import type { BidV2Api } from "../api/client";
import type { RequirementSetCompileRequestView, WorkspaceEnvelope } from "../api/types";
import { emptyRichText } from "./contentBlock";
import { hasDrafts } from "./drafts";
import { describe, expect, it } from "./harness";
import {
  asyncRequestMode,
  createBidV2Session,
  workspaceRequestMode,
} from "./session";

const SHA = "a".repeat(64);
const PROJECT = "11111111-1111-1111-1111-111111111111";
const WORKSPACE = "12121212-1212-1212-1212-121212121212";
const ROOT = "13131313-1313-1313-1313-131313131313";
const DOC = "19191919-1919-1919-1919-191919191919";
const EMPTY_SET = "10101010-1010-1010-1010-101010101010";
const FROZEN_SET = "17171717-1717-1717-1717-171717171717";
const FROZEN_REQUEST = "20202020-2020-4020-8020-202020202020";
const FROZEN_PROJECTION = "21212121-2121-4121-8121-212121212121";

function frozenSet() {
  return {
    artifact_id: FROZEN_SET,
    sha256: SHA,
    revision: 2,
    disposition_set_artifact_id: "22222222-2222-4222-8222-222222222222",
    disposition_set_sha256: SHA,
    request_artifact_id: FROZEN_REQUEST,
    request_revision: 1,
    request_sha256: SHA,
    frozen_input_sha256: SHA,
  };
}

function compiledRequirement(
  over: Partial<RequirementSetCompileRequestView> = {},
): RequirementSetCompileRequestView {
  return {
    request_artifact_id: FROZEN_REQUEST,
    kind: "RequirementSetCompile" as const,
    status: "succeeded" as const,
    request_revision: 1,
    request_sha256: SHA,
    frozen_input_sha256: SHA,
    document_set_revision_id: FROZEN_SET,
    document_set_sha256: SHA,
    disposition_set_revision_id: "22222222-2222-4222-8222-222222222222",
    disposition_set_sha256: SHA,
    result_identity: {
      status: "succeeded" as const,
      published_current: true,
      workspace_apply_required: true,
      requirement_set_id: "23232323-2323-4232-8232-232323232323",
      requirement_set_sha256: SHA,
      document_set_revision_id: FROZEN_SET,
      document_set_sha256: SHA,
      requirement_count: 1,
      requirement_projection_id: FROZEN_PROJECTION,
      requirement_projection_sha256: SHA,
      compiler_version: 3 as const,
      replayed: false,
    },
    error_code: null,
    ...over,
  };
}

class MemoryStorage implements Storage {
  private values = new Map<string, string>();
  get length(): number { return this.values.size; }
  clear(): void { this.values.clear(); }
  getItem(key: string): string | null { return this.values.get(key) ?? null; }
  key(index: number): string | null { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string): void { this.values.delete(key); }
  setItem(key: string, value: string): void { this.values.set(key, value); }
}

class ThrowingStorage extends MemoryStorage {
  constructor(
    private readonly fail: "get" | "set" | "remove",
  ) {
    super();
  }
  override getItem(key: string): string | null {
    if (this.fail === "get") throw new Error("storage get denied");
    return super.getItem(key);
  }
  override setItem(key: string, value: string): void {
    if (this.fail === "set") throw new Error("storage set denied");
    super.setItem(key, value);
  }
  override removeItem(key: string): void {
    if (this.fail === "remove") throw new Error("storage remove denied");
    super.removeItem(key);
  }
}

function installSessionStorage(value: MemoryStorage = new MemoryStorage()): MemoryStorage {
  const storage = value;
  Object.defineProperty(globalThis, "sessionStorage", {
    configurable: true,
    value: storage,
  });
  return storage;
}

function storedRecord(storage: Storage, slotIncludes: string): Record<string, unknown> {
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (key?.includes(slotIncludes)) {
      return JSON.parse(storage.getItem(key) ?? "null") as Record<string, unknown>;
    }
  }
  throw new Error(`missing stored record ${slotIncludes}`);
}

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
  listWorkspaceRequests: async () => [],
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
  listDocumentSets: async () => [
    { artifact_id: EMPTY_SET, sha256: SHA, revision: 1, items: [] },
  ],
  listSourceUnits: async () => [],
  listRequirements: async () => [],
  getRequirementSetCompilation: async () => compiledRequirement(),
  applyRequirementProjection: async () =>
    envelope({
      revision_id: "24242424-2424-4242-8242-242424242424",
      requirement_projection_revision_id: FROZEN_PROJECTION,
      requirement_projection_sha256: SHA,
      document_set_revision_id: FROZEN_SET,
      document_set_sha256: SHA,
    }),
};

describe("session poll and generateOutline", () => {
  it("never hydrates project-scoped requests through the workspace request route", () => {
    expect(workspaceRequestMode(
      `/api/v2/projects/${PROJECT}/requirement-set/compile`,
      WORKSPACE,
    )).toBe(null);
    expect(workspaceRequestMode(
      `/api/v2/submission-workspaces/${WORKSPACE}/outline-candidates`,
      WORKSPACE,
    )).toBe("candidate");
  });

  it("classifies match-only content requests as evidence after reload", () => {
    expect(
      asyncRequestMode({
        request_artifact_id: "20202020-2020-4020-8020-202020202020",
        kind: "ContentGenerate",
        operation: "match_only",
        status: "succeeded",
        result_identity: {
          artifact_id: "20202020-2020-4020-8020-202020202020",
          sha256: SHA,
        },
      }),
    ).toBe("evidence");
  });

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
      clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } },
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
          requirement_projection_revision_id: frozeExpected
            ? "19191919-1919-1919-1919-191919191919"
            : "15151515-1515-1515-1515-151515151515",
        }),
      freezeDocumentSet: async (_project, _ids, expected) => {
        frozeExpected = expected?.artifact_id ?? null;
        return frozenSet();
      },
      createOutlineCandidate: async () => ({
        request_artifact_id: "req-1",
        kind: "OutlineGenerate",
        status: "pending",
      }),
    });
    const session = createBidV2Session({
      api,
      clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } },
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

  it("freezes from the project DocumentSet current pointer instead of a stale Workspace projection", async () => {
    const currentSet = "25252525-2525-4252-8252-252525252525";
    let expectedSet: string | null = null;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () =>
        envelope({ document_set_revision_id: EMPTY_SET }),
      listDocumentSets: async () => [
        {
          artifact_id: currentSet,
          sha256: "b".repeat(64),
          revision: 2,
          items: [],
        },
      ],
      freezeDocumentSet: async (_project, _documents, expected) => {
        expectedSet = expected?.artifact_id ?? null;
        return frozenSet();
      },
      createOutlineCandidate: async () => ({
        request_artifact_id: "req-current-set",
        kind: "OutlineGenerate",
        status: "pending",
      }),
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    await session.generateOutline();
    expect(expectedSet).toBe(currentSet);
    session.dispose();
  });

  it("does not apply or generate while requirement compilation remains pending", async () => {
    let applied = 0;
    let generated = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope(),
      freezeDocumentSet: async () => frozenSet(),
      getRequirementSetCompilation: async () =>
        compiledRequirement({ status: "pending", result_identity: null }),
      applyRequirementProjection: async () => {
        applied += 1;
        return baseLists.applyRequirementProjection();
      },
      createOutlineCandidate: async () => {
        generated += 1;
        throw new Error("must not generate");
      },
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    let code = "";
    try {
      await session.generateOutline();
    } catch (error) {
      code = (error as { code?: string }).code ?? "";
    }
    expect(code).toBe("REQUIREMENT_COMPILE_PENDING");
    expect(applied).toBe(0);
    expect(generated).toBe(0);
    session.dispose();
  });

  it("does not apply or generate a failed or superseded requirement compilation", async () => {
    for (const request of [
      compiledRequirement({
        status: "failed",
        result_identity: null,
        error_code: "REQUIREMENT_COMPILE_FAILED",
      }),
      compiledRequirement({
        result_identity: {
          ...compiledRequirement().result_identity!,
          published_current: false,
          workspace_apply_required: false,
          requirement_projection_id: undefined,
          requirement_projection_sha256: undefined,
        },
      }),
    ]) {
      let applied = 0;
      let generated = 0;
      const api = mockApi({
        ...baseLists,
        getProjectWorkspace: async () => envelope(),
        freezeDocumentSet: async () => frozenSet(),
        getRequirementSetCompilation: async () => request,
        applyRequirementProjection: async () => {
          applied += 1;
          return baseLists.applyRequirementProjection();
        },
        createOutlineCandidate: async () => {
          generated += 1;
          throw new Error("must not generate");
        },
      });
      const session = createBidV2Session({
        api,
        clock: {
          now: () => 0,
          schedule: (fn) => {
            fn();
            return () => undefined;
          },
        },
      });
      await session.applyRoute({
        projectId: PROJECT,
        step: "authoring",
        nodeLineageId: ROOT,
      });
      try {
        await session.generateOutline();
      } catch {
        /* expected terminal refusal */
      }
      expect(applied).toBe(0);
      expect(generated).toBe(0);
      session.dispose();
    }
  });

  it("explicitly applies the exact compiled projection before outline generation and refreshes requirements", async () => {
    const events: string[] = [];
    let requirementLoads = 0;
    let sourceLoads = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope(),
      freezeDocumentSet: async () => {
        events.push("freeze");
        return frozenSet();
      },
      getRequirementSetCompilation: async () => {
        events.push("compile");
        return compiledRequirement();
      },
      applyRequirementProjection: async (_workspace, projection) => {
        events.push(`apply:${projection.artifact_id}`);
        return baseLists.applyRequirementProjection();
      },
      listRequirements: async () => {
        requirementLoads += 1;
        return [];
      },
      listSourceUnits: async () => {
        sourceLoads += 1;
        return [];
      },
      createOutlineCandidate: async (_workspace, body) => {
        events.push(`outline:${body.document_set_revision_id}`);
        return {
          request_artifact_id: "req-explicit",
          kind: "OutlineGenerate",
          status: "pending",
        };
      },
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    await session.generateOutline();
    expect(events).toEqual([
      "freeze",
      "compile",
      `apply:${FROZEN_PROJECTION}`,
      `outline:${FROZEN_SET}`,
    ]);
    expect(requirementLoads > 1).toBe(true);
    expect(sourceLoads > 1).toBe(true);
    session.dispose();
  });

  it("stops before outline generation when explicit projection apply conflicts", async () => {
    let generated = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope(),
      freezeDocumentSet: async () => frozenSet(),
      applyRequirementProjection: async () => {
        throw new ApiError(
          409,
          "workspace changed",
          "WORKSPACE_HEAD_CAS_MISMATCH",
        );
      },
      createOutlineCandidate: async () => {
        generated += 1;
        throw new Error("must not generate");
      },
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    let code = "";
    try {
      await session.generateOutline();
    } catch (error) {
      code = (error as { code?: string }).code ?? "";
    }
    expect(code).toBe("CAS_CONFLICT");
    expect(generated).toBe(0);
    session.dispose();
  });

  it("background hydration never applies a published requirement projection", async () => {
    let applied = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope(),
      applyRequirementProjection: async () => {
        applied += 1;
        return baseLists.applyRequirementProjection();
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
    await session.poll();
    expect(applied).toBe(0);
    session.dispose();
  });

  it("a deliberate terminal Generate click creates a new request for unchanged input", async () => {
    let creates = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () =>
        envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => frozenSet(),
      createOutlineCandidate: async () => {
        creates += 1;
        return {
          request_artifact_id: `req-regenerate-${creates}`,
          kind: "OutlineGenerate" as const,
          status: "succeeded" as const,
          result_identity: { artifact_id: `candidate-${creates}`, sha256: SHA },
        };
      },
      getCandidate: async (_workspaceId, candidateId) => ({
        candidate_id: candidateId,
        kind: "outline" as const,
        status: "proposed" as const,
        base_workspace_revision_id: "14141414-1414-1414-1414-141414141414",
        base_workspace_sha256: SHA,
        nodes: [
          {
            client_node_ref: "root",
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
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    await session.generateOutline();
    await session.generateOutline();
    expect(creates).toBe(2);
    session.dispose();
  });

  it("ignores a stale outline progress sequence while polling", async () => {
    const pending = {
      request_artifact_id: "req-progress",
      kind: "OutlineGenerate" as const,
      status: "pending" as const,
      progress: {
        stage: "generating",
        sequence: 5,
        detail: { phase: "drafting" as const, attempt: 2, max_attempts: 4 },
      },
    };
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => frozenSet(),
      createOutlineCandidate: async () => pending,
      getRequest: async () => ({
        ...pending,
        progress: {
          stage: "mapping",
          sequence: 4,
          detail: { phase: "mapping" as const, attempt: 1, max_attempts: 4 },
        },
      }),
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    await session.generateOutline();
    await session.poll();
    expect(session.getState().asyncRequests[0]?.progress?.sequence).toBe(5);
    expect(session.getState().asyncRequests[0]?.progress?.detail?.phase).toBe("drafting");
    session.dispose();
  });

  it("hydrates an outline candidate from a succeeded generate request", async () => {
    let workspaceLoads = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => {
        workspaceLoads += 1;
        return envelope({
          requirement_projection_revision_id:
            workspaceLoads > 1
              ? "19191919-1919-1919-1919-191919191919"
              : "15151515-1515-1515-1515-151515151515",
        });
      },
      freezeDocumentSet: async () => frozenSet(),
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
      clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } },
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

  it("restores the latest succeeded candidate when authoring is refreshed", async () => {
    let loadedCandidateId: string | null = null;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope(),
      listWorkspaceRequests: async () => [
        {
          request_artifact_id: "req-newer-failed",
          kind: "OutlineGenerate",
          status: "failed",
          result_identity: null,
          error_code: "AGENT_OUTPUT_INVALID",
        },
        {
          request_artifact_id: "req-latest-succeeded",
          kind: "OutlineGenerate",
          status: "succeeded",
          result_identity: { artifact_id: "cand-latest", sha256: SHA },
        },
        {
          request_artifact_id: "req-older-succeeded",
          kind: "OutlineGenerate",
          status: "succeeded",
          result_identity: { artifact_id: "cand-older", sha256: SHA },
        },
      ],
      getCandidate: async (_workspaceId, candidateId) => {
        loadedCandidateId = candidateId;
        return {
          candidate_id: candidateId,
          kind: "outline",
          status: "proposed",
          base_workspace_revision_id:
            "14141414-1414-1414-1414-141414141414",
          base_workspace_sha256: SHA,
          nodes: [
            {
              client_node_ref: "restored-node",
              parent_client_node_ref: null,
              ordinal: 0,
              title: "刷新后可见的大纲",
            },
          ],
          notices: [],
        };
      },
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: () => () => undefined,
      },
    });

    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });

    expect(loadedCandidateId).toBe("cand-latest");
    expect(session.getState().candidate?.candidate_id).toBe("cand-latest");
    expect(session.getState().selectedOutlineNodeRefs).toEqual([
      "restored-node",
    ]);
    session.dispose();
  });
  it("resolves a committed uncertain generation before starting a new workflow", async () => {
    const storage = installSessionStorage();
    const attempts: string[] = [];
    const payloads: unknown[] = [];
    const ifMatches: Array<string | null | undefined> = [];
    let calls = 0;
    let freezes = 0;
    let applies = 0;
    let getCalls = 0;
    let terminal = false;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => {
        freezes += 1;
        return frozenSet();
      },
      applyRequirementProjection: async () => {
        applies += 1;
        return baseLists.applyRequirementProjection();
      },
      getRequest: async () => {
        getCalls += 1;
        return terminal
          ? {
              request_artifact_id: "req-uncertain",
              kind: "OutlineGenerate",
              status: "failed",
              error_code: "AGENT_OUTPUT_INVALID",
            }
          : {
              request_artifact_id: "req-uncertain",
              kind: "OutlineGenerate",
              status: "pending",
            };
      },
      createOutlineCandidate: async (_workspace, body, opts) => {
        attempts.push(opts.attempt!.idempotencyKey);
        payloads.push(body);
        ifMatches.push(opts.ifMatch);
        calls += 1;
        if (calls === 1) {
          const error = new ApiError(503, "queue unavailable", "QUEUE_UNAVAILABLE") as ApiError & {
            requestArtifactId?: string;
            queueRequestIdentity?: { request_artifact_id: string; request_revision: number; frozen_input_sha256: string; retry_same_idempotency_key: boolean };
          };
          error.requestArtifactId = "req-uncertain";
          error.queueRequestIdentity = {
            request_artifact_id: "req-uncertain",
            request_revision: 1,
            frozen_input_sha256: SHA,
            retry_same_idempotency_key: true,
          };
          throw error;
        }
        return { request_artifact_id: "req-uncertain", kind: "OutlineGenerate", status: "pending" };
      },
    });
    const session = createBidV2Session({ api, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
    await session.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    try { await session.generateOutline(); } catch { /* uncertain request remains replayable */ }
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    expect(storage.length > 0).toBe(true);
    let code = "";
    try { await session.generateOutline(); } catch (error) {
      code = (error as { code?: string }).code ?? "";
    }
    expect(code).toBe("PENDING_REPLAY");
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    expect(attempts.length).toBe(2);
    expect(attempts[1]).toBe(attempts[0]);
    expect(payloads.length).toBe(2);
    expect(payloads[1]).toEqual(payloads[0]);
    expect(ifMatches).toEqual([SHA, SHA]);
    expect(storage.length > 0).toBe(true);
    expect(getCalls).toBe(0);
    terminal = true;
    await session.generateOutline();
    expect(getCalls).toBe(1);
    expect(freezes).toBe(2);
    expect(applies).toBe(2);
    expect(attempts.length).toBe(3);
    expect(attempts[2] === attempts[1]).toBe(false);
    expect(storage.length).toBe(0);
    session.dispose();
  });

  it("fingerprints the actual path, body, and If-Match and persists only transport uncertainty", async () => {
    const storage = installSessionStorage();
    const attempts: string[] = [];
    let calls = 0;
    let freezes = 0;
    let applies = 0;
    let getCalls = 0;
    let replayTerminal = false;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => {
        freezes += 1;
        return frozenSet();
      },
      applyRequirementProjection: async () => {
        applies += 1;
        return baseLists.applyRequirementProjection();
      },
      getRequest: async () => {
        getCalls += 1;
        if (replayTerminal) {
          return { request_artifact_id: "network-replay", kind: "OutlineGenerate", status: "failed", error_code: "AGENT_OUTPUT_INVALID" };
        }
        if (getCalls === 1) {
          throw new ApiError(503, "maintenance", "SERVICE_UNAVAILABLE");
        }
        return { request_artifact_id: "network-replay", kind: "OutlineGenerate", status: "pending" };
      },
      createOutlineCandidate: async (_workspace, _body, opts) => {
        attempts.push(opts.attempt!.idempotencyKey);
        calls += 1;
        if (calls === 1) throw new NetworkTransportError(new TypeError("offline"));
        return { request_artifact_id: "network-replay", kind: "OutlineGenerate", status: "pending" };
      },
    });
    const session = createBidV2Session({ api, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
    await session.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    try { await session.generateOutline(); } catch { /* persisted */ }
    const record = storedRecord(storage, "outline-generate");
    const descriptor = record.descriptor as Record<string, unknown>;
    expect(descriptor.path).toBe(`/api/v2/submission-workspaces/${WORKSPACE}/outline-candidates`);
    expect(descriptor.ifMatch).toBe(SHA);
    expect((descriptor.body as Record<string, unknown>).expected_workspace_revision_id)
      .toBe("24242424-2424-4242-8242-242424242424");
    let replayCode = "";
    try { await session.generateOutline(); } catch (error) {
      replayCode = (error as { code?: string }).code ?? "";
    }
    expect(replayCode).toBe("PENDING_REPLAY");
    expect(attempts[0]).toBe(attempts[1]);
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    expect(storage.length > 0).toBe(true);
    const storedBeforeLookup = storedRecord(storage, "outline-generate");
    let lookupCode = "";
    try { await session.generateOutline(); } catch (error) {
      lookupCode = (error as { code?: string }).code ?? "";
    }
    expect(lookupCode).toBe("SERVICE_UNAVAILABLE");
    expect(storage.length > 0).toBe(true);
    expect(storedRecord(storage, "outline-generate")).toEqual(storedBeforeLookup);
    expect(attempts.length).toBe(2);
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    let pendingCode = "";
    try { await session.generateOutline(); } catch (error) {
      pendingCode = (error as { code?: string }).code ?? "";
    }
    expect(pendingCode).toBe("PENDING_REPLAY");
    expect(storage.length > 0).toBe(true);
    expect(attempts.length).toBe(2);
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    replayTerminal = true;
    await session.refresh();
    expect(storage.length).toBe(0);
    session.dispose();

    for (const failure of [
      new Error("local programming failure"),
      new ApiError(503, "maintenance", "SERVICE_UNAVAILABLE"),
    ]) {
      const definitiveStorage = installSessionStorage();
      const definitiveApi = mockApi({
        ...baseLists,
        getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
        freezeDocumentSet: async () => frozenSet(),
        createOutlineCandidate: async () => { throw failure; },
      });
      const definitive = createBidV2Session({ api: definitiveApi, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
      await definitive.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
      try { await definitive.generateOutline(); } catch { /* definitive */ }
      expect(definitiveStorage.length).toBe(0);
      definitive.dispose();
    }
  });

  it("clears a definitive missing request and continues the deliberate new click", async () => {
    const storage = installSessionStorage();
    let freezes = 0;
    let applies = 0;
    let creates = 0;
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () =>
        envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => {
        freezes += 1;
        return frozenSet();
      },
      applyRequirementProjection: async () => {
        applies += 1;
        return baseLists.applyRequirementProjection();
      },
      getRequest: async () => {
        throw new ApiError(404, "missing", "REQUEST_NOT_FOUND");
      },
      createOutlineCandidate: async () => {
        creates += 1;
        return {
          request_artifact_id: "new-after-missing",
          kind: "OutlineGenerate",
          status: "pending",
        };
      },
    });
    const session = createBidV2Session({
      api,
      clock: {
        now: () => 0,
        schedule: (fn) => {
          fn();
          return () => undefined;
        },
      },
    });
    await session.applyRoute({
      projectId: PROJECT,
      step: "authoring",
      nodeLineageId: ROOT,
    });
    const payload = {
      expected_workspace_revision_id:
        "24242424-2424-4242-8242-242424242424",
      document_set_revision_id: FROZEN_SET,
      document_set_sha256: SHA,
    };
    storage.setItem(
      `kb.bid.v2.queued-attempt:outline-generate:${WORKSPACE}`,
      JSON.stringify({
        idempotencyKey: "missing-request-key",
        fingerprint: "missing-request-fingerprint",
        descriptor: {
          version: 1,
          method: "POST",
          path: `/api/v2/submission-workspaces/${WORKSPACE}/outline-candidates`,
          body: payload,
          ifMatch: SHA,
        },
        payload,
        status: "uncertain",
        requestIdentity: {
          request_artifact_id: "missing-request",
          retry_same_idempotency_key: false,
        },
      }),
    );
    await session.generateOutline();
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    expect(creates).toBe(1);
    expect(storage.length).toBe(0);
    session.dispose();
  });

  it("replays an old descriptor and defers changed intent while the old request is pending", async () => {
    const storage = installSessionStorage();
    let newer = false;
    let oldTerminal = false;
    let calls = 0;
    let freezes = 0;
    let applies = 0;
    const keys: string[] = [];
    const ifMatches: Array<string | null | undefined> = [];
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => newer
        ? { ...envelope({ document_set_revision_id: FROZEN_SET, revision_id: "24242424-2424-4242-8242-242424242424", sha256: "b".repeat(64) }), etag: "b".repeat(64) }
        : envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => {
        freezes += 1;
        return freezes === 1
          ? frozenSet()
          : {
              ...frozenSet(),
              artifact_id: "26262626-2626-4262-8262-262626262626",
              request_artifact_id: "27272727-2727-4272-8272-272727272727",
            };
      },
      applyRequirementProjection: async () => {
        applies += 1;
        return applies === 1
          ? baseLists.applyRequirementProjection()
          : envelope({
              document_set_revision_id: "26262626-2626-4262-8262-262626262626",
              requirement_projection_revision_id:
                "28282828-2828-4282-8282-282828282828",
            });
      },
      getRequest: async (_workspace, requestId) => oldTerminal
        ? { request_artifact_id: requestId, kind: "OutlineGenerate", status: "failed", error_code: "AGENT_OUTPUT_INVALID" }
        : { request_artifact_id: requestId, kind: "OutlineGenerate", status: "pending" },
      createOutlineCandidate: async (_workspace, _body, opts) => {
        calls += 1;
        keys.push(opts.attempt!.idempotencyKey);
        ifMatches.push(opts.ifMatch);
        if (calls === 1) throw new NetworkTransportError(new TypeError("offline"));
        return { request_artifact_id: `req-${calls}`, kind: "OutlineGenerate", status: "pending" };
      },
    });
    const session = createBidV2Session({ api, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
    await session.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    try { await session.generateOutline(); } catch { /* old descriptor persisted */ }
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    newer = true;
    await session.refresh();
    let code = "";
    try { await session.generateOutline(); } catch (error) {
      code = (error as { code?: string }).code ?? "";
    }
    expect(code).toBe("PENDING_REPLAY");
    expect(freezes).toBe(1);
    expect(applies).toBe(1);
    expect(calls).toBe(2);
    expect(keys[0]).toBe(keys[1]);
    expect(ifMatches).toEqual([SHA, SHA]);
    expect(storage.length > 0).toBe(true);
    oldTerminal = true;
    await session.refresh();
    expect(storage.length).toBe(0);
    session.dispose();
  });

  it("hydrates committed workspace request identity after session recreation", async () => {
    const storage = installSessionStorage();
    const queueError = new ApiError(503, "queue unavailable", "QUEUE_UNAVAILABLE") as ApiError & {
      queueRequestIdentity?: { request_artifact_id: string; request_revision: number; frozen_input_sha256: string; retry_same_idempotency_key: boolean };
    };
    queueError.queueRequestIdentity = {
      request_artifact_id: "req-hydrate",
      request_revision: 1,
      frozen_input_sha256: SHA,
      retry_same_idempotency_key: true,
    };
    const firstApi = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
      freezeDocumentSet: async () => frozenSet(),
      createOutlineCandidate: async () => { throw queueError; },
    });
    const first = createBidV2Session({ api: firstApi, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
    await first.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    try { await first.generateOutline(); } catch { /* committed identity persisted */ }
    first.dispose();

    let terminal = false;
    const secondApi = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
      getRequest: async () => terminal
        ? { request_artifact_id: "req-hydrate", kind: "OutlineGenerate", status: "succeeded", result_identity: { artifact_id: "cand-hydrate", sha256: SHA } }
        : { request_artifact_id: "req-hydrate", kind: "OutlineGenerate", status: "pending" },
      getCandidate: async () => ({
        candidate_id: "cand-hydrate", kind: "outline", status: "proposed",
        base_workspace_revision_id: "14141414-1414-1414-1414-141414141414",
        base_workspace_sha256: SHA, nodes: [], notices: [],
      }),
    });
    const second = createBidV2Session({ api: secondApi, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
    await second.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    expect(second.getState().asyncRequests[0]?.request_artifact_id).toBe("req-hydrate");
    terminal = true;
    await second.refresh();
    expect(storage.length).toBe(0);
    second.dispose();
  });

  it("guards storage failures without reclassifying confirmed HTTP success", async () => {
    for (const failure of ["get", "set", "remove"] as const) {
      installSessionStorage(new ThrowingStorage(failure));
      let calls = 0;
      const api = mockApi({
        ...baseLists,
        getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
        freezeDocumentSet: async () => frozenSet(),
        createOutlineCandidate: async () => {
          calls += 1;
          return { request_artifact_id: `storage-${failure}-${calls}`, kind: "OutlineGenerate", status: "pending" };
        },
      });
      const session = createBidV2Session({ api, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
      await session.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
      await session.generateOutline();
      await session.generateOutline();
      expect(calls).toBe(1);
      session.dispose();
    }
  });

  it("maps candidate obsolete before generic CAS and clears candidate selections after workspace refresh", async () => {
    installSessionStorage();
    let candidateLoads = 0;
    const candidate = {
      candidate_id: "cand-obsolete",
      kind: "outline" as const,
      status: "proposed" as const,
      base_workspace_revision_id: "14141414-1414-1414-1414-141414141414",
      base_workspace_sha256: SHA,
      nodes: [{ client_node_ref: "n1", parent_client_node_ref: null, ordinal: 0, title: "投标文件" }],
      notices: [],
    };
    const api = mockApi({
      ...baseLists,
      getProjectWorkspace: async () => envelope({ document_set_revision_id: FROZEN_SET }),
      getWorkspace: async () => envelope({
        document_set_revision_id: FROZEN_SET,
        revision_id: "24242424-2424-4242-8242-242424242424",
        sha256: "b".repeat(64),
      }),
      freezeDocumentSet: async () => frozenSet(),
      createOutlineCandidate: async () => ({
        request_artifact_id: "req-obsolete", kind: "OutlineGenerate", status: "succeeded",
        result_identity: { artifact_id: candidate.candidate_id, sha256: SHA },
      }),
      getCandidate: async () => { candidateLoads += 1; return candidate; },
      acceptCandidate: async () => { throw new ApiError(409, "obsolete", "CANDIDATE_OBSOLETE"); },
    });
    const session = createBidV2Session({ api, clock: { now: () => 0, schedule: (fn) => { fn(); return () => undefined; } } });
    await session.applyRoute({ projectId: PROJECT, step: "authoring", nodeLineageId: ROOT });
    await session.generateOutline();
    let code = "";
    try { await session.acceptCandidate(); } catch (error) {
      code = (error as { code?: string }).code ?? "";
    }
    expect(code).toBe("CANDIDATE_OBSOLETE");
    expect(candidateLoads).toBe(1);
    expect(session.getState().candidate).toBe(null);
    expect(session.getState().selectedOperationIndexes).toEqual([]);
    expect(session.getState().selectedOutlineNodeRefs).toEqual([]);
    session.dispose();
  });

});
