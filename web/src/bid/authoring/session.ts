import {
  ApiError,
  createMutationAttempt,
  type MutationAttempt,
} from "../../api";
import type { BidV2Api } from "../api/client";
import type {
  AsyncRequestView,
  BidProjectView,
  CandidateView,
  DocumentRelationKind,
  DocumentRole,
  EvidenceOverview,
  ExpectedPointer,
  ExportView,
  RequirementView,
  SourceUnitView,
  TenderDocumentView,
  TenderRelationView,
  WorkspaceAssetView,
  WorkspaceEnvelope,
  WorkspaceView,
} from "../api/types";
import { applyEditorModel, type TiptapNode } from "./adapter";
import type { CurrentAssessments } from "./assessment";
import { checkpointAllowed, exportAllowed } from "./assessment";
import type { ContentBlockV1 } from "./contentBlock";
import { emptyRichText, emptyTable } from "./contentBlock";
import {
  clearAckedDrafts,
  draftsToOperations,
  hasDrafts,
  upsertDraft,
  type DraftMap,
} from "./drafts";
import {
  AuthoringLogicError,
  CasConflictError,
  EnqueueUncertainError,
} from "./errors";
import {
  buildContentCandidateRequest,
  buildExportRequest,
  buildOutlineCandidateRequest,
  selectedOperationIndexes,
  selectedOutlineRefs,
  type EvidenceMode,
  type ExportFormat,
  type ExportMode,
  type FillPolicy,
  type GenerateTarget,
} from "./generation";
import { clientRef, fileSha256Hex, newUuid } from "./ids";
import {
  buildMutationRequest,
  ops,
  type DocumentSettings,
  type InsertionAnchor,
  type RenderRole,
  type SemanticRole,
  type WorkspaceOperation,
} from "./mutations";
import type { AuthoringRoute } from "./routes";
import {
  assertCanDelete,
  assertCanInsert,
  assertCanMerge,
  assertCanMove,
  assertCanRename,
  assertCanSplit,
  childrenOf,
  findNode,
  outlineIndex,
  subtreeLineageIds,
  type OutlineIndex,
  type OutlineNodeView,
} from "./tree";

export type InspectorTab =
  | "requirements"
  | "evidence"
  | "assets"
  | "assessment";
export type DraftStatus = "clean" | "dirty" | "saving" | "conflict";

export type ConflictState = {
  serverRevisionId: string;
  serverSha256: string;
};

export type BidV2State = {
  route: AuthoringRoute | null;
  project: BidProjectView | null;
  ended: boolean;
  documents: TenderDocumentView[];
  relations: TenderRelationView[];
  sourceUnits: SourceUnitView[];
  requirements: RequirementView[];
  workspace: WorkspaceView | null;
  etag: string | null;
  drafts: DraftMap;
  draftStatus: DraftStatus;
  conflict: ConflictState | null;
  inspectorTab: InspectorTab;
  evidenceMode: EvidenceMode;
  fillPolicy: FillPolicy;
  selectedNodeLineageId: string | null;
  candidate: CandidateView | null;
  selectedOperationIndexes: number[];
  selectedOutlineNodeRefs: string[];
  assessments: CurrentAssessments | null;
  previewHtml: string | null;
  exports: ExportView[];
  evidenceOverview: EvidenceOverview | null;
  assets: WorkspaceAssetView[];
  asyncRequests: AsyncRequestView[];
  pendingUploads: string[];
  error: { message: string; code: string; technical: boolean } | null;
  busy: boolean;
};

export type BidV2Clock = {
  now: () => number;
  schedule: (fn: () => void, ms: number) => () => void;
};

export type BidV2Deps = {
  api: BidV2Api;
  clock: BidV2Clock;
  autosaveDelayMs?: number;
};

export type BidV2Session = {
  getState: () => BidV2State;
  subscribe: (listener: () => void) => () => void;
  dispose: () => void;
  applyRoute: (route: AuthoringRoute) => Promise<void>;
  refresh: () => Promise<void>;
  poll: () => Promise<void>;
  uploadTenderDocuments: (files: File[]) => Promise<void>;
  retryTenderDocument: (
    documentId: string,
    expectedGeneration: number,
  ) => Promise<void>;
  setDocumentRole: (
    documentId: string,
    role: DocumentRole,
    expected: ExpectedPointer,
  ) => Promise<void>;
  addDocumentRelation: (
    fromDocumentId: string,
    toDocumentId: string,
    relationKind: DocumentRelationKind,
  ) => Promise<void>;
  freezeDocumentSet: (
    documentIds: string[],
    expected: ExpectedPointer | null,
  ) => Promise<void>;
  selectNode: (nodeLineageId: string | null) => void;
  setInspectorTab: (tab: InspectorTab) => void;
  setEvidenceMode: (mode: EvidenceMode) => void;
  setFillPolicy: (policy: FillPolicy) => void;
  tree: () => OutlineIndex;
  findNode: (lineageId: string) => OutlineNodeView | null;
  childrenOf: (parentLineageId: string | null) => OutlineNodeView[];
  subtreeIds: (lineageId: string) => string[];
  insertNode: (input: {
    parentLineageId: string | null;
    ordinal: number;
    title: string;
    semanticRole?: SemanticRole;
    renderRole?: RenderRole;
  }) => Promise<void>;
  renameNode: (nodeLineageId: string, title: string) => Promise<void>;
  moveNode: (
    nodeLineageId: string,
    parentLineageId: string | null,
    ordinal: number,
  ) => Promise<void>;
  deleteNode: (nodeLineageId: string) => Promise<void>;
  splitNode: (nodeLineageId: string, titles: string[]) => Promise<void>;
  mergeNodes: (nodeLineageIds: string[], title: string) => Promise<void>;
  editRichText: (blockLineageId: string, doc: TiptapNode) => void;
  editTable: (blockLineageId: string, doc: TiptapNode) => void;
  insertRichTextBlock: (nodeLineageId: string, ordinal: number) => void;
  insertTableBlock: (nodeLineageId: string, ordinal: number) => void;
  insertPageBreak: (nodeLineageId: string, ordinal: number) => void;
  insertSignature: (nodeLineageId: string, ordinal: number) => void;
  insertAssetBlock: (
    nodeLineageId: string,
    assetRevisionId: string,
    ordinal: number,
  ) => Promise<void>;
  uploadAsset: (file: File) => Promise<void>;
  save: () => Promise<void>;
  resolveConflict: (choice: "keep_local" | "take_server") => Promise<void>;
  generateOutline: () => Promise<void>;
  generateContent: (
    target: GenerateTarget,
    nodeLineageId?: string,
    insertionAnchor?: InsertionAnchor,
  ) => Promise<void>;
  matchEvidence: (nodeLineageId: string) => Promise<void>;
  toggleCandidateOperation: (index: number, selected: boolean) => void;
  toggleOutlineNode: (clientNodeRef: string, selected: boolean) => void;
  acceptCandidate: () => Promise<void>;
  rejectCandidate: () => Promise<void>;
  confirmOutlineCheckpoint: () => Promise<void>;
  loadPreview: () => Promise<void>;
  exportDocument: (mode: ExportMode, format: ExportFormat) => Promise<void>;
  updateDocumentSettings: (settings: DocumentSettings) => Promise<void>;
};

function emptyState(): BidV2State {
  return {
    route: null,
    project: null,
    ended: false,
    documents: [],
    relations: [],
    sourceUnits: [],
    requirements: [],
    workspace: null,
    etag: null,
    drafts: {},
    draftStatus: "clean",
    conflict: null,
    inspectorTab: "evidence",
    evidenceMode: "system_proposed",
    fillPolicy: "empty_only",
    selectedNodeLineageId: null,
    candidate: null,
    selectedOperationIndexes: [],
    selectedOutlineNodeRefs: [],
    assessments: null,
    previewHtml: null,
    exports: [],
    evidenceOverview: null,
    assets: [],
    asyncRequests: [],
    pendingUploads: [],
    error: null,
    busy: false,
  };
}

function mapError(error: unknown): AuthoringLogicError {
  if (error instanceof AuthoringLogicError) return error;
  if (error instanceof ApiError) {
    const requestArtifactId = (
      error as ApiError & { requestArtifactId?: string }
    ).requestArtifactId;
    if (error.status === 409) return new CasConflictError();
    if (error.status === 503)
      return new EnqueueUncertainError(requestArtifactId);
    return new AuthoringLogicError(
      error.code || `HTTP_${error.status}`,
      error.message,
      true,
      requestArtifactId,
    );
  }
  return new AuthoringLogicError(
    "UNKNOWN",
    error instanceof Error ? error.message : String(error),
  );
}

function requireBlock(
  workspace: WorkspaceView,
  drafts: DraftMap,
  blockLineageId: string,
): ContentBlockV1 {
  const drafted = drafts[blockLineageId]?.block;
  if (drafted) return drafted;
  const block = workspace.blocks.find(
    (item) => item.lineage_id === blockLineageId,
  );
  if (!block) throw new AuthoringLogicError("BLOCK_MISSING", "内容块不存在");
  return block;
}

export function shouldPoll(state: BidV2State): boolean {
  if (
    state.documents.some(
      (doc) =>
        doc.parse_status === "pending" || doc.parse_status === "processing",
    )
  )
    return true;
  return state.asyncRequests.some((request) => request.status === "pending");
}

export function createBidV2Session(deps: BidV2Deps): BidV2Session {
  const autosaveDelayMs = deps.autosaveDelayMs ?? 800;
  let state = emptyState();
  const listeners = new Set<() => void>();
  let disposed = false;
  let loadAbort: AbortController | null = null;
  let cancelAutosave: (() => void) | null = null;
  let mutationTail = Promise.resolve();
  let workspaceAttempt: MutationAttempt | null = null;
  const uploadAttempts = new Map<string, MutationAttempt[]>();

  function emit(): void {
    if (disposed) return;
    for (const listener of listeners) listener();
  }

  function setState(patch: Partial<BidV2State>): void {
    state = { ...state, ...patch };
    emit();
  }

  function tree(): OutlineIndex {
    return outlineIndex(state.workspace?.nodes ?? []);
  }

  function assertEditable(): void {
    if (state.ended)
      throw new AuthoringLogicError("PROJECT_ENDED", "项目已结束");
  }

  function head(): { workspace: WorkspaceView; etag: string } {
    if (!state.workspace || !state.etag)
      throw new AuthoringLogicError("NO_WORKSPACE", "工作区未加载");
    return { workspace: state.workspace, etag: state.etag };
  }

  function applyWorkspace(
    envelope: WorkspaceEnvelope,
    extra: Partial<BidV2State> = {},
  ): void {
    const selected =
      extra.selectedNodeLineageId ??
      state.selectedNodeLineageId ??
      extra.route?.nodeLineageId ??
      state.route?.nodeLineageId ??
      null;
    const requested =
      extra.selectedNodeLineageId !== undefined &&
      extra.selectedNodeLineageId !== null
        ? extra.selectedNodeLineageId
        : selected;
    const exists = envelope.workspace.nodes.some(
      (node) => node.lineage_id === requested,
    );
    const { selectedNodeLineageId: _ignored, ...rest } = extra;
    setState({
      workspace: envelope.workspace,
      etag: envelope.etag,
      ended: state.project?.status === "ended",
      selectedNodeLineageId: exists
        ? requested
        : (envelope.workspace.nodes[0]?.lineage_id ?? null),
      ...rest,
    });
  }

  function fail(error: unknown): never {
    const mapped = mapError(error);
    setState({
      error: {
        message: mapped.message,
        code: mapped.code,
        technical: mapped.technical,
      },
      busy: false,
    });
    throw mapped;
  }

  function enqueue<T>(fn: () => Promise<T>): Promise<T> {
    const run = mutationTail.then(fn, fn);
    mutationTail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  async function hydrateCandidate(
    workspaceId: string,
    candidateId: string,
  ): Promise<void> {
    const candidate = await deps.api.getCandidate(workspaceId, candidateId);
    if (candidate.kind === "content") {
      setState({
        candidate,
        selectedOperationIndexes:
          candidate.status === "proposed"
            ? candidate.operations.map((_, index) => index)
            : [],
        selectedOutlineNodeRefs: [],
      });
      return;
    }
    setState({
      candidate,
      selectedOutlineNodeRefs:
        candidate.status === "proposed"
          ? candidate.nodes.map((node) => node.client_node_ref)
          : [],
      selectedOperationIndexes: [],
    });
  }

  const downloadedExports = new Set<string>();
  const requestModes = new Map<
    string,
    "candidate" | "evidence" | "export"
  >();

  function isReadyDocument(status: TenderDocumentView["parse_status"]): boolean {
    return status === "ready" || status === "completed";
  }

  function triggerDownload(blob: Blob, filename: string): void {
    if (typeof document === "undefined") return;
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    anchor.click();
    URL.revokeObjectURL(url);
  }

  async function fulfillExport(request: AsyncRequestView): Promise<void> {
    const exportId = request.result_identity?.artifact_id;
    if (!exportId || !state.workspace || downloadedExports.has(exportId))
      return;
    const exports = await deps.api.listExports(state.workspace.workspace_id);
    setState({ exports, error: null });
    const blob = await deps.api.downloadExport(
      state.workspace.workspace_id,
      exportId,
    );
    const listed = exports.find((item) => item.export_id === exportId);
    const filename = `${listed?.mode ?? "submission"}.${listed?.format ?? "docx"}`;
    triggerDownload(blob, filename);
    downloadedExports.add(exportId);
  }

  async function rememberRequest(
    request: AsyncRequestView,
    mode: "candidate" | "evidence" | "export",
  ): Promise<void> {
    requestModes.set(request.request_artifact_id, mode);
    setState({
      asyncRequests: [
        ...state.asyncRequests.filter(
          (item) => item.request_artifact_id !== request.request_artifact_id,
        ),
        request,
      ],
      error: null,
    });
    if (request.status !== "succeeded" || !request.result_identity) return;
    if (!state.workspace) return;
    if (mode === "candidate") {
      await hydrateCandidate(
        state.workspace.workspace_id,
        request.result_identity.artifact_id,
      );
      return;
    }
    if (mode === "evidence") {
      const evidenceOverview = await deps.api
        .getEvidenceOverview(state.workspace.workspace_id)
        .catch(() => state.evidenceOverview);
      setState({ evidenceOverview });
      return;
    }
    await fulfillExport(request);
  }

  async function pollJobs(): Promise<void> {
    if (disposed || !state.route) return;
    const projectId = state.route.projectId;
    try {
      if (
        state.route.step === "files" ||
        state.route.step === "authoring" ||
        state.documents.some(
          (doc) =>
            doc.parse_status === "pending" || doc.parse_status === "processing",
        )
      ) {
        const documents = await deps.api.listTenderDocuments(projectId);
        setState({ documents });
      }
      const workspaceId = state.workspace?.workspace_id;
      if (!workspaceId) return;
      const nextRequests: AsyncRequestView[] = [];
      for (const request of state.asyncRequests) {
        if (request.status !== "pending") {
          nextRequests.push(request);
          continue;
        }
        try {
          const latest = await deps.api.getRequest(
            workspaceId,
            request.request_artifact_id,
          );
          if (latest.status === "succeeded" && latest.result_identity) {
            const mode =
              requestModes.get(latest.request_artifact_id) ??
              (latest.kind === "SubmissionExport"
                ? "export"
                : latest.kind === "OutlineGenerate" ||
                    latest.kind === "ContentGenerate"
                  ? "candidate"
                  : "evidence");
            if (mode === "candidate") {
              await hydrateCandidate(
                workspaceId,
                latest.result_identity.artifact_id,
              );
            } else if (mode === "export") {
              await fulfillExport(latest);
            } else if (mode === "evidence") {
              const evidenceOverview = await deps.api
                .getEvidenceOverview(workspaceId)
                .catch(() => state.evidenceOverview);
              setState({ evidenceOverview });
            }
          }
          nextRequests.push(latest);
        } catch {
          nextRequests.push(request);
        }
      }
      setState({ asyncRequests: nextRequests });
    } catch {
      /* polling is best-effort and must not clobber drafts */
    }
  }

  function takeAttempt(reuseOnUncertain: boolean): MutationAttempt {
    if (reuseOnUncertain && workspaceAttempt) return workspaceAttempt;
    workspaceAttempt = createMutationAttempt();
    return workspaceAttempt;
  }

  async function commitOperations(
    operations: WorkspaceOperation[],
    reuseOnUncertain = false,
  ): Promise<void> {
    assertEditable();
    const current = head();
    const body = buildMutationRequest(
      current.workspace.workspace_id,
      current.workspace.revision_id,
      current.workspace.sha256,
      operations,
    );
    const attempt = takeAttempt(reuseOnUncertain);
    try {
      const envelope = await deps.api.mutateWorkspace(
        current.workspace.workspace_id,
        body,
        {
          attempt,
          ifMatch: current.etag,
        },
      );
      workspaceAttempt = null;
      applyWorkspace(envelope, { error: null });
    } catch (error) {
      const mapped = mapError(error);
      if (mapped instanceof CasConflictError) {
        const envelope = await deps.api.getWorkspace(
          current.workspace.workspace_id,
        );
        workspaceAttempt = null;
        applyWorkspace(envelope, {
          conflict: {
            serverRevisionId: envelope.workspace.revision_id,
            serverSha256: envelope.workspace.sha256,
          },
          draftStatus: hasDrafts(state.drafts) ? "conflict" : state.draftStatus,
          error: {
            message: mapped.message,
            code: mapped.code,
            technical: true,
          },
        });
      }
      throw mapped;
    }
  }

  async function load(route: AuthoringRoute): Promise<void> {
    loadAbort?.abort();
    const controller = new AbortController();
    loadAbort = controller;
    const { signal } = controller;
    setState({ busy: true, error: null, route });
    try {
      const project = await deps.api.getProject(route.projectId, signal);
      if (signal.aborted) return;
      const ended = project.status === "ended";
      setState({ project, ended });
      if (route.step === "files" || route.step === "authoring") {
        const [documents, relations] = await Promise.all([
          deps.api.listTenderDocuments(route.projectId, signal),
          deps.api
            .listRelations(route.projectId, signal)
            .catch(() => [] as TenderRelationView[]),
        ]);
        if (signal.aborted) return;
        setState({ documents, relations });
      }
      if (route.step === "authoring" || route.step === "export") {
        const envelope = await deps.api.getProjectWorkspace(
          route.projectId,
          signal,
        );
        if (signal.aborted) return;
        if (hasDrafts(state.drafts)) {
          setState({ route });
        } else {
          applyWorkspace(envelope, {
            selectedNodeLineageId: route.nodeLineageId,
            route,
          });
        }
      }
      if (route.step === "authoring" && state.workspace) {
        const [assessments, evidenceOverview, assets] = await Promise.all([
          deps.api
            .getAssessments(state.workspace.workspace_id, signal)
            .catch(() => null),
          deps.api
            .getEvidenceOverview(state.workspace.workspace_id, signal)
            .catch(() => null),
          deps.api
            .listAssets(state.workspace.workspace_id, signal)
            .catch(() => [] as WorkspaceAssetView[]),
        ]);
        if (signal.aborted) return;
        setState({ assessments, evidenceOverview, assets });
      }
      if (route.step === "export" && state.workspace) {
        const [exports, assessments] = await Promise.all([
          deps.api.listExports(state.workspace.workspace_id, signal),
          deps.api
            .getAssessments(state.workspace.workspace_id, signal)
            .catch(() => null),
        ]);
        if (signal.aborted) return;
        setState({ exports, assessments });
      }
      setState({ busy: false });
    } catch (error) {
      if (signal.aborted) return;
      fail(error);
    } finally {
      if (loadAbort === controller) loadAbort = null;
    }
  }

  function scheduleSave(): void {
    cancelAutosave?.();
    cancelAutosave = deps.clock.schedule(() => {
      cancelAutosave = null;
      void session.save().catch(() => undefined);
    }, autosaveDelayMs);
  }

  function editModel(
    blockLineageId: string,
    model: Parameters<typeof applyEditorModel>[1],
  ): void {
    assertEditable();
    const { workspace } = head();
    const current = requireBlock(workspace, state.drafts, blockLineageId);
    const node = workspace.nodes.find((item) =>
      item.block_lineage_ids.includes(blockLineageId),
    );
    const existing = state.drafts[blockLineageId];
    const nodeLineageId = existing?.nodeLineageId ?? node?.lineage_id;
    if (!nodeLineageId)
      throw new AuthoringLogicError("BLOCK_NODE", "找不到内容块所属节点");
    const ordinal =
      existing?.ordinal ?? node?.block_lineage_ids.indexOf(blockLineageId) ?? 0;
    const next = applyEditorModel(current, model);
    setState({
      drafts: upsertDraft(state.drafts, {
        nodeLineageId,
        blockLineageId,
        op: existing?.op ?? "update",
        ordinal,
        block: next,
        baseWorkspaceRevisionId: workspace.revision_id,
      }),
      draftStatus: state.conflict ? "conflict" : "dirty",
      error: null,
    });
    if (!state.conflict) scheduleSave();
  }

  function insertDraftBlock(
    nodeLineageId: string,
    ordinal: number,
    block: ContentBlockV1,
  ): void {
    assertEditable();
    const { workspace } = head();
    if (!findNode(tree(), nodeLineageId))
      throw new AuthoringLogicError("TREE_NODE", "节点不存在");
    setState({
      drafts: upsertDraft(state.drafts, {
        nodeLineageId,
        blockLineageId: block.lineage_id,
        op: "insert",
        ordinal,
        block,
        baseWorkspaceRevisionId: workspace.revision_id,
      }),
      draftStatus: "dirty",
    });
    scheduleSave();
  }

  function newBlockBase(): Pick<
    ContentBlockV1,
    | "schema_version"
    | "block_revision_id"
    | "lineage_id"
    | "revision"
    | "origin"
    | "dependency_sha256"
    | "content_sha256"
  > {
    return {
      schema_version: 1,
      block_revision_id: newUuid(),
      lineage_id: newUuid(),
      revision: 1,
      origin: "human",
      dependency_sha256: null,
      content_sha256: "0".repeat(64),
    };
  }

  const session: BidV2Session = {
    getState: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    dispose() {
      disposed = true;
      cancelAutosave?.();
      loadAbort?.abort();
      listeners.clear();
    },
    applyRoute: (route) => enqueue(() => load(route)),
    refresh: () =>
      enqueue(() => (state.route ? load(state.route) : Promise.resolve())),
    poll: () => enqueue(() => pollJobs()),
    async uploadTenderDocuments(files) {
      assertEditable();
      if (!state.route) throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
      const projectId = state.route.projectId;
      setState({ pendingUploads: files.map((file) => file.name), busy: true });
      try {
        for (const file of files) {
          const digest = await fileSha256Hex(file);
          const key = `${projectId}:${file.name}:${file.size}:${digest}`;
          const queued = uploadAttempts.get(key) ?? [];
          const attempt = queued.shift() ?? createMutationAttempt();
          if (queued.length === 0) uploadAttempts.delete(key);
          try {
            await deps.api.uploadTenderDocument(projectId, file, attempt);
          } catch (error) {
            if (!(error instanceof ApiError)) {
              const pending = uploadAttempts.get(key) ?? [];
              pending.push(attempt);
              uploadAttempts.set(key, pending);
            }
            throw error;
          }
        }
        const documents = await deps.api.listTenderDocuments(projectId);
        setState({ documents, pendingUploads: [], busy: false, error: null });
      } catch (error) {
        setState({ pendingUploads: [], busy: false });
        fail(error);
      }
    },
    async retryTenderDocument(documentId, expectedGeneration) {
      assertEditable();
      if (!state.route) throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
      try {
        await deps.api.retryTenderDocument(
          state.route.projectId,
          documentId,
          expectedGeneration,
          createMutationAttempt(),
        );
        const documents = await deps.api.listTenderDocuments(
          state.route.projectId,
        );
        setState({ documents, error: null });
      } catch (error) {
        fail(error);
      }
    },
    async setDocumentRole(documentId, role, expected) {
      assertEditable();
      if (!state.route) throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
      try {
        await deps.api.patchDocumentRole(
          state.route.projectId,
          documentId,
          role,
          expected,
          createMutationAttempt(),
        );
        const documents = await deps.api.listTenderDocuments(
          state.route.projectId,
        );
        setState({ documents, error: null });
      } catch (error) {
        fail(error);
      }
    },
    async addDocumentRelation(fromDocumentId, toDocumentId, relationKind) {
      assertEditable();
      if (!state.route) throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
      try {
        await deps.api.upsertDocumentRelation(
          state.route.projectId,
          {
            from_document_id: fromDocumentId,
            to_document_id: toDocumentId,
            relation_kind: relationKind,
            applicability: {},
          },
          createMutationAttempt(),
        );
        const relations = await deps.api.listRelations(state.route.projectId);
        setState({ relations, error: null });
      } catch (error) {
        fail(error);
      }
    },
    async freezeDocumentSet(documentIds, expected) {
      assertEditable();
      if (!state.route) throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
      try {
        await deps.api.freezeDocumentSet(
          state.route.projectId,
          documentIds,
          expected,
          createMutationAttempt(),
        );
        setState({ error: null });
        await session.refresh();
      } catch (error) {
        fail(error);
      }
    },
    selectNode(nodeLineageId) {
      setState({ selectedNodeLineageId: nodeLineageId });
    },
    setInspectorTab(tab) {
      setState({ inspectorTab: tab });
    },
    setEvidenceMode(mode) {
      setState({ evidenceMode: mode });
    },
    setFillPolicy(policy) {
      setState({ fillPolicy: policy });
    },
    tree,
    findNode: (lineageId) => findNode(tree(), lineageId),
    childrenOf: (parentLineageId) => childrenOf(tree(), parentLineageId),
    subtreeIds: (lineageId) => subtreeLineageIds(tree(), lineageId),
    insertNode: (input) =>
      enqueue(async () => {
        assertCanInsert(tree(), input.parentLineageId, input.ordinal);
        await commitOperations([
          ops.insertNode({
            client_node_ref: clientRef("n"),
            parent_lineage_id: input.parentLineageId,
            ordinal: input.ordinal,
            title: input.title,
            semantic_role: input.semanticRole ?? "other",
            render_role: input.renderRole ?? "section",
          }),
        ]);
      }).catch(fail),
    renameNode: (nodeLineageId, title) =>
      enqueue(async () => {
        assertCanRename(tree(), nodeLineageId);
        await commitOperations([ops.renameNode(nodeLineageId, title)]);
      }).catch(fail),
    moveNode: (nodeLineageId, parentLineageId, ordinal) =>
      enqueue(async () => {
        assertCanMove(tree(), nodeLineageId, parentLineageId, ordinal);
        await commitOperations([
          ops.moveNode(nodeLineageId, parentLineageId, ordinal),
        ]);
      }).catch(fail),
    deleteNode: (nodeLineageId) =>
      enqueue(async () => {
        assertCanDelete(tree(), nodeLineageId);
        await commitOperations([ops.deleteNode(nodeLineageId)]);
      }).catch(fail),
    splitNode: (nodeLineageId, titles) =>
      enqueue(async () => {
        assertCanSplit(tree(), nodeLineageId, titles);
        await commitOperations([ops.splitNode(nodeLineageId, titles)]);
      }).catch(fail),
    mergeNodes: (nodeLineageIds, title) =>
      enqueue(async () => {
        assertCanMerge(tree(), nodeLineageIds);
        await commitOperations([ops.mergeNodes(nodeLineageIds, title)]);
      }).catch(fail),
    editRichText(blockLineageId, doc) {
      editModel(blockLineageId, { kind: "rich_text", doc });
    },
    editTable(blockLineageId, doc) {
      editModel(blockLineageId, { kind: "table", doc });
    },
    insertRichTextBlock(nodeLineageId, ordinal) {
      insertDraftBlock(nodeLineageId, ordinal, {
        ...newBlockBase(),
        kind: "rich_text",
        content: emptyRichText(),
      });
    },
    insertTableBlock(nodeLineageId, ordinal) {
      insertDraftBlock(nodeLineageId, ordinal, {
        ...newBlockBase(),
        kind: "table",
        content: emptyTable(),
      });
    },
    insertPageBreak(nodeLineageId, ordinal) {
      insertDraftBlock(nodeLineageId, ordinal, {
        ...newBlockBase(),
        kind: "page_break",
        content: { type: "page_break" },
      });
    },
    insertSignature(nodeLineageId, ordinal) {
      insertDraftBlock(nodeLineageId, ordinal, {
        ...newBlockBase(),
        kind: "signature_placeholder",
        content: {
          type: "signature_placeholder",
          signature_kind: "signature",
          width_mm: 40,
          height_mm: 20,
          label: "签字",
        },
      });
    },
    insertAssetBlock: (nodeLineageId, assetRevisionId, ordinal) =>
      enqueue(async () => {
        await commitOperations([
          ops.insertAssetBlock({
            node_lineage_id: nodeLineageId,
            asset_revision_id: assetRevisionId,
            ordinal,
          }),
        ]);
      }).catch(fail),
    async uploadAsset(file) {
      assertEditable();
      const { workspace } = head();
      try {
        const asset = await deps.api.uploadAsset(
          workspace.workspace_id,
          file,
          createMutationAttempt(),
        );
        const assets = await deps.api.listAssets(workspace.workspace_id);
        setState({ assets, error: null });
        const nodeId = state.selectedNodeLineageId;
        if (!nodeId) return;
        const node = findNode(tree(), nodeId);
        await session.insertAssetBlock(
          nodeId,
          asset.asset_revision_id,
          node?.block_lineage_ids.length ?? 0,
        );
      } catch (error) {
        fail(error);
      }
    },
    save: () =>
      enqueue(async () => {
        if (state.conflict) throw new CasConflictError();
        if (!hasDrafts(state.drafts)) {
          setState({ draftStatus: "clean" });
          return;
        }
        const acked = Object.fromEntries(
          Object.entries(state.drafts).map(([id, draft]) => [
            id,
            draft.generation,
          ]),
        );
        setState({ draftStatus: "saving", busy: true });
        try {
          const operations = await draftsToOperations(state.drafts);
          await commitOperations(operations, true);
          setState({
            drafts: clearAckedDrafts(state.drafts, acked),
            draftStatus: hasDrafts(clearAckedDrafts(state.drafts, acked))
              ? "dirty"
              : "clean",
            busy: false,
            error: null,
          });
        } catch (error) {
          const mapped = mapError(error);
          setState({
            draftStatus:
              mapped instanceof CasConflictError ? "conflict" : "dirty",
            busy: false,
            error: {
              message: mapped.message,
              code: mapped.code,
              technical: mapped.technical,
            },
          });
          throw mapped;
        }
      }),
    async resolveConflict(choice) {
      if (!state.conflict) return;
      if (choice === "take_server") {
        setState({
          drafts: {},
          conflict: null,
          draftStatus: "clean",
          error: null,
        });
        return;
      }
      setState({
        conflict: null,
        draftStatus: hasDrafts(state.drafts) ? "dirty" : "clean",
        error: null,
      });
      await session.save();
    },
    generateOutline: () =>
      enqueue(async () => {
        try {
          assertEditable();
          if (!state.route)
            throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
          let current = head();
          let docs = state.documents;
          if (docs.length === 0) {
            docs = await deps.api.listTenderDocuments(state.route.projectId);
            setState({ documents: docs });
          }
          const ready = docs.filter((doc) => isReadyDocument(doc.parse_status));
          if (ready.length === 0) {
            throw new AuthoringLogicError(
              "NO_READY_DOCUMENTS",
              "请等待招标文件解析完成后再生成大纲",
            );
          }
          if (
            !current.workspace.document_set_revision_id ||
            !current.workspace.document_set_sha256
          ) {
            throw new AuthoringLogicError(
              "NO_DOCUMENT_SET",
              "工作区缺少 DocumentSet 指针，无法冻结",
            );
          }
          const pointer = await deps.api.freezeDocumentSet(
            state.route.projectId,
            ready.map((doc) => doc.id),
            {
              artifact_id: current.workspace.document_set_revision_id,
              sha256: current.workspace.document_set_sha256,
            },
            createMutationAttempt(),
          );
          const envelope = await deps.api.getProjectWorkspace(
            state.route.projectId,
          );
          applyWorkspace(envelope);
          current = head();
          const setId =
            current.workspace.document_set_revision_id ?? pointer.artifact_id;
          const setSha =
            current.workspace.document_set_sha256 ?? pointer.sha256;
          const body = buildOutlineCandidateRequest({
            expected_workspace_revision_id: current.workspace.revision_id,
            document_set_revision_id: setId,
            document_set_sha256: setSha,
          });
          const request = await deps.api.createOutlineCandidate(
            current.workspace.workspace_id,
            body,
            {
              attempt: createMutationAttempt(),
              ifMatch: current.etag,
            },
          );
          await rememberRequest(request, "candidate");
        } catch (error) {
          fail(error);
        }
      }),
    generateContent: (target, nodeLineageId, insertionAnchor) =>
      enqueue(async () => {
        assertEditable();
        let current = head();
        if (!current.workspace.outline_checkpoint_id) {
          await deps.api.createOutlineCheckpoint(
            current.workspace.workspace_id,
            {
              artifact_id: current.workspace.revision_id,
              sha256: current.workspace.sha256,
            },
            createMutationAttempt(),
          );
          const envelope = await deps.api.getWorkspace(
            current.workspace.workspace_id,
          );
          applyWorkspace(envelope);
          current = head();
        }
        const body = buildContentCandidateRequest({
          target,
          node_lineage_id: nodeLineageId,
          fill_policy: state.fillPolicy,
          insertion_anchor: insertionAnchor ?? null,
          selection_mode: state.evidenceMode,
          expected_workspace_revision_id: current.workspace.revision_id,
        });
        try {
          const request = await deps.api.createContentCandidate(
            current.workspace.workspace_id,
            body,
            {
              attempt: createMutationAttempt(),
              ifMatch: current.etag,
            },
          );
          await rememberRequest(request, "candidate");
        } catch (error) {
          fail(error);
        }
      }),
    matchEvidence: (nodeLineageId) =>
      enqueue(async () => {
        assertEditable();
        const { workspace, etag } = head();
        try {
          const request = await deps.api.matchEvidence(
            workspace.workspace_id,
            nodeLineageId,
            workspace.revision_id,
            {
              attempt: createMutationAttempt(),
              ifMatch: etag,
            },
          );
          await rememberRequest(request, "evidence");
        } catch (error) {
          fail(error);
        }
      }),
    toggleCandidateOperation(index, selected) {
      const candidate = state.candidate;
      if (!candidate || candidate.kind !== "content") return;
      const next = new Set(state.selectedOperationIndexes);
      if (selected) next.add(index);
      else next.delete(index);
      setState({ selectedOperationIndexes: [...next].sort((a, b) => a - b) });
    },
    toggleOutlineNode(clientNodeRef, selected) {
      const candidate = state.candidate;
      if (!candidate || candidate.kind !== "outline") return;
      const next = new Set(state.selectedOutlineNodeRefs);
      if (selected) next.add(clientNodeRef);
      else next.delete(clientNodeRef);
      setState({ selectedOutlineNodeRefs: [...next] });
    },
    acceptCandidate: () =>
      enqueue(async () => {
        assertEditable();
        if (hasDrafts(state.drafts))
          throw new AuthoringLogicError(
            "SAVE_DRAFTS_FIRST",
            "请先保存本地草稿再接受候选",
          );
        const candidate = state.candidate;
        if (!candidate || candidate.status !== "proposed")
          throw new AuthoringLogicError("NO_CANDIDATE", "没有可接受的候选");
        const { workspace, etag } = head();
        const body =
          candidate.kind === "content"
            ? {
                expected_workspace_revision_id: workspace.revision_id,
                expected_workspace_sha256: workspace.sha256,
                operation_indexes: selectedOperationIndexes(
                  candidate.operations.length,
                  state.selectedOperationIndexes,
                ),
              }
            : {
                expected_workspace_revision_id: workspace.revision_id,
                expected_workspace_sha256: workspace.sha256,
                client_node_refs: selectedOutlineRefs(
                  candidate.nodes,
                  state.selectedOutlineNodeRefs,
                ),
              };
        try {
          const envelope = await deps.api.acceptCandidate(
            workspace.workspace_id,
            candidate.candidate_id,
            body,
            {
              attempt: createMutationAttempt(),
              ifMatch: etag,
            },
          );
          applyWorkspace(envelope, {
            candidate: null,
            selectedOperationIndexes: [],
            selectedOutlineNodeRefs: [],
            error: null,
          });
        } catch (error) {
          fail(error);
        }
      }),
    rejectCandidate: () =>
      enqueue(async () => {
        assertEditable();
        const candidate = state.candidate;
        if (!candidate)
          throw new AuthoringLogicError("NO_CANDIDATE", "没有可拒绝的候选");
        const { workspace } = head();
        try {
          const rejected = await deps.api.rejectCandidate(
            workspace.workspace_id,
            candidate.candidate_id,
            {
              attempt: createMutationAttempt(),
            },
          );
          setState({
            candidate: rejected,
            selectedOperationIndexes: [],
            selectedOutlineNodeRefs: [],
            error: null,
          });
        } catch (error) {
          fail(error);
        }
      }),
    confirmOutlineCheckpoint: () =>
      enqueue(async () => {
        assertEditable();
        const { workspace } = head();
        checkpointAllowed(state.assessments?.outline ?? null);
        try {
          await deps.api.createOutlineCheckpoint(
            workspace.workspace_id,
            { artifact_id: workspace.revision_id, sha256: workspace.sha256 },
            createMutationAttempt(),
          );
          setState({ error: null });
          await session.refresh();
        } catch (error) {
          fail(error);
        }
      }),
    async loadPreview() {
      const { workspace } = head();
      try {
        const previewHtml = await deps.api.getPreviewHtml(
          workspace.workspace_id,
        );
        setState({ previewHtml, error: null });
      } catch (error) {
        fail(error);
      }
    },
    exportDocument: (mode, format) =>
      enqueue(async () => {
        assertEditable();
        const { workspace, etag } = head();
        const allowed = exportAllowed({
          assessment: state.assessments?.submission ?? null,
          technicalReady: true,
        });
        if (!allowed.allowed)
          throw new AuthoringLogicError(
            "EXPORT_TECHNICAL",
            "存在技术错误，不能导出",
          );
        const body = buildExportRequest({
          mode,
          format,
          expected_workspace_revision_id: workspace.revision_id,
        });
        try {
          const request = await deps.api.createExport(
            workspace.workspace_id,
            body,
            {
              attempt: createMutationAttempt(),
              ifMatch: etag,
            },
          );
          await rememberRequest(request, "export");
        } catch (error) {
          fail(error);
        }
      }),
    updateDocumentSettings: (settings) =>
      enqueue(async () => {
        await commitOperations([ops.updateDocumentSettings(settings)]);
      }).catch(fail),
  };

  return session;
}
