import {
  ApiError,
  NetworkTransportError,
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
  FreezeDocumentSetResult,
  RequirementSetCompileRequestView,
  RequirementView,
  SourceUnitView,
  TenderDocumentView,
  TenderRelationView,
  WorkspaceAssetView,
  WorkspaceEnvelope,
  WorkspaceView,
} from "../api/types";
import {
  applyEditorModel,
  docToChapterPatches,
  type ChapterPatchBlock,
  type TiptapNode,
} from "./adapter";
import { blocksForNode } from "./blocks";
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
  type OutlineCandidateRequest,
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
  "requirements" | "evidence" | "assets" | "assessment";
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
  preparingOutline: boolean;
  outlineSourceKey: string | null;
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
  applyDocument: (doc: TiptapNode) => void;
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
    preparingOutline: false,
    outlineSourceKey: null,
    error: null,
    busy: false,
  };
}

function mapError(error: unknown): AuthoringLogicError {
  if (error instanceof AuthoringLogicError) return error;
  if (error instanceof NetworkTransportError)
    return new EnqueueUncertainError();
  if (error instanceof ApiError) {
    const requestArtifactId = (
      error as ApiError & { requestArtifactId?: string }
    ).requestArtifactId;
    if (error.code === "CANDIDATE_OBSOLETE")
      return new AuthoringLogicError(
        "CANDIDATE_OBSOLETE",
        "候选基于旧工作区版本，请刷新候选并重新生成。",
        true,
      );
    if (error.status === 409) return new CasConflictError();
    if (
      error.code === "QUEUE_UNAVAILABLE" &&
      queueRequestIdentity(error)?.retry_same_idempotency_key === true
    )
      return new EnqueueUncertainError(requestArtifactId);
    if (
      error.code === "TENDER_DOCUMENT_DUPLICATE" ||
      error.message.includes("TENDER_DOCUMENT_DUPLICATE")
    ) {
      return new AuthoringLogicError(
        "TENDER_DOCUMENT_DUPLICATE",
        "本项目已上传过这份文件",
        true,
        requestArtifactId,
      );
    }
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
  if (state.preparingOutline) return true;
  if (
    state.documents.some(
      (doc) =>
        doc.parse_status === "pending" || doc.parse_status === "processing",
    )
  )
    return true;
  return state.asyncRequests.some((request) => request.status === "pending");
}

type ImmutableQueuedRequestDescriptor = {
  version: 1;
  method: "POST";
  path: string;
  body: unknown;
  ifMatch: string | null;
  upload?: {
    fileName: string;
    mediaType: string;
    byteLength: number;
    sha256: string;
  };
};

type StoredQueuedAttempt = {
  idempotencyKey: string;
  fingerprint: string;
  descriptor: ImmutableQueuedRequestDescriptor;
  payload: unknown;
  status?: "uncertain" | "completed";
  requestIdentity?: {
    request_artifact_id: string;
    request_revision?: number;
    frozen_input_sha256?: string;
    retry_same_idempotency_key?: boolean;
  };
};

const QUEUED_ATTEMPT_PREFIX = "kb.bid.v2.queued-attempt:";
const queuedAttemptFallback = new Map<string, StoredQueuedAttempt>();

function queuedAttemptStorage(): Storage | null {
  try {
    return typeof sessionStorage === "undefined" ? null : sessionStorage;
  } catch {
    return null;
  }
}

function parseQueuedAttempt(raw: string | null): StoredQueuedAttempt | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Partial<StoredQueuedAttempt>;
    if (
      typeof value.idempotencyKey !== "string" ||
      typeof value.fingerprint !== "string" ||
      !value.descriptor ||
      !("payload" in value)
    )
      return null;
    return value as StoredQueuedAttempt;
  } catch {
    return null;
  }
}

function readQueuedAttempt(slot: string): StoredQueuedAttempt | null {
  const fallback = queuedAttemptFallback.get(slot);
  if (fallback?.status === "completed") {
    queuedAttemptFallback.delete(slot);
    try {
      queuedAttemptStorage()?.removeItem(`${QUEUED_ATTEMPT_PREFIX}${slot}`);
    } catch {
      // The completed in-memory tombstone still prevents stale replay this session.
    }
    return null;
  }
  if (fallback) return fallback;
  try {
    const value = parseQueuedAttempt(
      queuedAttemptStorage()?.getItem(`${QUEUED_ATTEMPT_PREFIX}${slot}`) ??
        null,
    );
    if (value?.status === "completed") {
      try {
        queuedAttemptStorage()?.removeItem(`${QUEUED_ATTEMPT_PREFIX}${slot}`);
      } catch {
        queuedAttemptFallback.set(slot, value);
      }
      return null;
    }
    return value;
  } catch {
    return null;
  }
}

function writeQueuedAttempt(slot: string, value: StoredQueuedAttempt): void {
  queuedAttemptFallback.set(slot, value);
  try {
    queuedAttemptStorage()?.setItem(
      `${QUEUED_ATTEMPT_PREFIX}${slot}`,
      JSON.stringify(value),
    );
  } catch {
    // The in-memory record preserves the key without reclassifying storage denial as a network error.
  }
}

function clearQueuedAttempt(slot: string, value: StoredQueuedAttempt): void {
  const completed = { ...value, status: "completed" as const };
  queuedAttemptFallback.set(slot, completed);
  try {
    const storage = queuedAttemptStorage();
    storage?.setItem(
      `${QUEUED_ATTEMPT_PREFIX}${slot}`,
      JSON.stringify(completed),
    );
    storage?.removeItem(`${QUEUED_ATTEMPT_PREFIX}${slot}`);
    queuedAttemptFallback.delete(slot);
  } catch {
    // The completed tombstone wins over an older uncertain storage record.
  }
}

function listQueuedAttempts(): Array<[string, StoredQueuedAttempt]> {
  const values = new Map<string, StoredQueuedAttempt>();
  for (const [slot, value] of queuedAttemptFallback) {
    if (value.status !== "completed") values.set(slot, value);
  }
  try {
    const storage = queuedAttemptStorage();
    if (storage) {
      for (let index = 0; index < storage.length; index += 1) {
        const key = storage.key(index);
        if (!key?.startsWith(QUEUED_ATTEMPT_PREFIX)) continue;
        const slot = key.slice(QUEUED_ATTEMPT_PREFIX.length);
        if (values.has(slot)) continue;
        const value = parseQueuedAttempt(storage.getItem(key));
        if (value && value.status !== "completed") values.set(slot, value);
      }
    }
  } catch {
    // In-memory records remain available when storage enumeration is denied.
  }
  return [...values.entries()];
}

function queueRequestIdentity(
  error: unknown,
): StoredQueuedAttempt["requestIdentity"] {
  if (!(error instanceof ApiError)) return undefined;
  return (
    error as ApiError & {
      queueRequestIdentity?: StoredQueuedAttempt["requestIdentity"];
    }
  ).queueRequestIdentity;
}

function isUncertainQueuedDispatch(error: unknown): boolean {
  if (error instanceof NetworkTransportError) return true;
  if (!(error instanceof ApiError)) return false;
  const identity = queueRequestIdentity(error);
  return (
    error.code === "QUEUE_UNAVAILABLE" &&
    identity?.retry_same_idempotency_key === true
  );
}

export function workspaceRequestMode(
  path: string,
  workspaceId: string,
): "candidate" | "evidence" | "export" | null {
  const prefix = `/api/v2/submission-workspaces/${workspaceId}/`;
  if (!path.startsWith(prefix)) return null;
  if (path.endsWith("/exports")) return "export";
  if (path.includes("evidence-matches")) return "evidence";
  if (path.includes("candidates")) return "candidate";
  return null;
}

export function asyncRequestMode(
  request: AsyncRequestView,
): "candidate" | "evidence" | "export" {
  if (request.kind === "SubmissionExport") return "export";
  if (
    request.kind === "ContentGenerate" &&
    request.operation === "match_only"
  ) {
    return "evidence";
  }
  return "candidate";
}

function asyncRequestResult(value: unknown): AsyncRequestView | null {
  if (!value || typeof value !== "object") return null;
  const request = value as Partial<AsyncRequestView>;
  return typeof request.request_artifact_id === "string" &&
    (request.status === "pending" ||
      request.status === "succeeded" ||
      request.status === "failed")
    ? (request as AsyncRequestView)
    : null;
}

type CanonicalFingerprintValue =
  | null
  | boolean
  | number
  | string
  | CanonicalFingerprintValue[]
  | { [key: string]: CanonicalFingerprintValue };

function canonicalFingerprint(value: unknown): string {
  const normalize = (item: unknown): CanonicalFingerprintValue => {
    if (item === null) return null;
    if (typeof item === "boolean" || typeof item === "string") return item;
    if (typeof item === "number" && Number.isFinite(item)) return item;
    if (Array.isArray(item)) return item.map(normalize);
    if (typeof item === "object") {
      return Object.fromEntries(
        Object.entries(item as Record<string, unknown>)
          .sort(([left], [right]) => left.localeCompare(right))
          .map(([key, child]) => [key, normalize(child)]),
      );
    }
    throw new TypeError("queued request descriptor must be finite JSON");
  };
  return JSON.stringify(normalize(value));
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

  async function dispatchQueued<T, P>(
    slot: string,
    descriptor: ImmutableQueuedRequestDescriptor,
    proposedPayload: P,
    dispatch: (
      payload: P,
      attempt: MutationAttempt,
      descriptor: ImmutableQueuedRequestDescriptor,
    ) => Promise<T>,
  ): Promise<T> {
    const proposedFingerprint = canonicalFingerprint(descriptor);
    let stored = readQueuedAttempt(slot);
    if (stored && stored.fingerprint !== proposedFingerprint) {
      const workspaceMode = state.workspace
        ? workspaceRequestMode(
            stored.descriptor.path,
            state.workspace.workspace_id,
          )
        : null;
      if (stored.requestIdentity && state.workspace && workspaceMode) {
        const latest = await deps.api.getRequest(
          state.workspace.workspace_id,
          stored.requestIdentity.request_artifact_id,
        );
        await rememberRequest(latest, workspaceMode);
        if (latest.status === "pending") {
          throw new AuthoringLogicError(
            "PENDING_REPLAY",
            "原请求仍在处理中；完成后将使用新幂等键提交当前更改。",
            true,
            latest.request_artifact_id,
          );
        }
        clearQueuedAttempt(slot, stored);
        stored = null;
      } else if (stored.descriptor.upload) {
        throw new AuthoringLogicError(
          "PENDING_UPLOAD_RESELECT",
          "上次上传结果尚未确认。请刷新文件状态；如仍未出现，请重新选择原文件后再提交新文件。",
          false,
          stored.requestIdentity?.request_artifact_id,
        );
      } else {
        const oldRecord = stored;
        try {
          const replayed = await dispatch(
            oldRecord.payload as P,
            { idempotencyKey: oldRecord.idempotencyKey },
            oldRecord.descriptor,
          );
          const request = asyncRequestResult(replayed);
          if (request) {
            const replayRecord = {
              ...oldRecord,
              status: "uncertain" as const,
              requestIdentity: {
                request_artifact_id: request.request_artifact_id,
              },
            };
            writeQueuedAttempt(slot, replayRecord);
            const mode = state.workspace
              ? workspaceRequestMode(
                  oldRecord.descriptor.path,
                  state.workspace.workspace_id,
                )
              : null;
            if (mode) await rememberRequest(request, mode);
            if (request.status === "pending") {
              throw new AuthoringLogicError(
                "PENDING_REPLAY",
                "原请求仍在处理中；完成后将使用新幂等键提交当前更改。",
                true,
                request.request_artifact_id,
              );
            }
          }
          clearQueuedAttempt(slot, oldRecord);
          stored = null;
        } catch (error) {
          if (
            error instanceof AuthoringLogicError &&
            error.code === "PENDING_REPLAY"
          ) {
            throw error;
          }
          if (isUncertainQueuedDispatch(error)) {
            writeQueuedAttempt(slot, {
              ...oldRecord,
              status: "uncertain",
              requestIdentity:
                queueRequestIdentity(error) ?? oldRecord.requestIdentity,
            });
            throw error;
          }
          clearQueuedAttempt(slot, oldRecord);
          stored = null;
        }
      }
    }
    const record: StoredQueuedAttempt = stored ?? {
      idempotencyKey: createMutationAttempt().idempotencyKey,
      fingerprint: proposedFingerprint,
      descriptor,
      payload: proposedPayload,
      status: "uncertain",
    };
    writeQueuedAttempt(slot, record);
    try {
      const result = await dispatch(
        record.payload as P,
        { idempotencyKey: record.idempotencyKey },
        record.descriptor,
      );
      clearQueuedAttempt(slot, record);
      return result;
    } catch (error) {
      if (isUncertainQueuedDispatch(error)) {
        const identity = queueRequestIdentity(error);
        writeQueuedAttempt(slot, {
          ...record,
          status: "uncertain",
          requestIdentity: identity ?? record.requestIdentity,
        });
      } else {
        clearQueuedAttempt(slot, record);
      }
      throw error;
    }
  }

  async function resolveStoredOutlineGenerate(workspaceId: string): Promise<void> {
    const slot = `outline-generate:${workspaceId}`;
    const stored = readQueuedAttempt(slot);
    if (!stored) return;
    const expectedPath = `/api/v2/submission-workspaces/${workspaceId}/outline-candidates`;
    if (
      stored.descriptor.method !== "POST" ||
      stored.descriptor.path !== expectedPath ||
      workspaceRequestMode(stored.descriptor.path, workspaceId) !== "candidate"
    ) {
      throw new AuthoringLogicError(
        "QUEUED_OUTLINE_IDENTITY_INVALID",
        "待恢复的大纲请求身份无效",
        true,
      );
    }

    const rememberResolved = async (
      request: AsyncRequestView,
      record: StoredQueuedAttempt,
    ): Promise<void> => {
      const resolvedRecord = {
        ...record,
        status: "uncertain" as const,
        requestIdentity: {
          ...record.requestIdentity,
          request_artifact_id: request.request_artifact_id,
        },
      };
      writeQueuedAttempt(slot, resolvedRecord);
      await rememberRequest(request, "candidate");
      if (request.status === "pending") {
        throw new AuthoringLogicError(
          "PENDING_REPLAY",
          "原请求仍在处理中；完成后将使用新幂等键提交当前更改。",
          true,
          request.request_artifact_id,
        );
      }
      clearQueuedAttempt(slot, resolvedRecord);
    };

    const replayStoredPost = async (): Promise<void> => {
      try {
        const request = await deps.api.createOutlineCandidate(
          workspaceId,
          stored.payload as OutlineCandidateRequest,
          {
            attempt: { idempotencyKey: stored.idempotencyKey },
            ifMatch: stored.descriptor.ifMatch,
          },
        );
        await rememberResolved(request, {
          ...stored,
          requestIdentity: stored.requestIdentity
            ? {
                ...stored.requestIdentity,
                retry_same_idempotency_key: false,
              }
            : undefined,
        });
      } catch (error) {
        if (
          error instanceof AuthoringLogicError &&
          error.code === "PENDING_REPLAY"
        ) {
          throw error;
        }
        if (isUncertainQueuedDispatch(error)) {
          writeQueuedAttempt(slot, {
            ...stored,
            status: "uncertain",
            requestIdentity:
              queueRequestIdentity(error) ?? stored.requestIdentity,
          });
        } else {
          clearQueuedAttempt(slot, stored);
        }
        throw error;
      }
    };

    if (stored.requestIdentity?.retry_same_idempotency_key === true) {
      await replayStoredPost();
      return;
    }
    if (!stored.requestIdentity) {
      await replayStoredPost();
      return;
    }

    let request: AsyncRequestView;
    try {
      request = await deps.api.getRequest(
        workspaceId,
        stored.requestIdentity.request_artifact_id,
      );
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) {
        clearQueuedAttempt(slot, stored);
        return;
      }
      writeQueuedAttempt(slot, stored);
      throw error;
    }
    await rememberResolved(request, stored);
  }

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
  const requestModes = new Map<string, "candidate" | "evidence" | "export">();

  function isReadyDocument(
    status: TenderDocumentView["parse_status"],
  ): boolean {
    return status === "ready" || status === "completed";
  }

  async function waitForRequirementCompilation(
    projectId: string,
    frozen: FreezeDocumentSetResult,
  ): Promise<RequirementSetCompileRequestView> {
    for (let attempt = 0; attempt < 750; attempt += 1) {
      const request = await deps.api.getRequirementSetCompilation(
        projectId,
        frozen.request_artifact_id,
      );
      if (
        request.request_artifact_id !== frozen.request_artifact_id ||
        request.document_set_revision_id !== frozen.artifact_id ||
        request.document_set_sha256 !== frozen.sha256 ||
        request.frozen_input_sha256 !== frozen.frozen_input_sha256
      ) {
        throw new AuthoringLogicError(
          "REQUIREMENT_COMPILE_IDENTITY_MISMATCH",
          "条款编译结果与冻结输入不匹配",
          true,
        );
      }
      if (request.status !== "pending") return request;
      await new Promise<void>((resolve) => {
        deps.clock.schedule(() => resolve(), 400);
      });
    }
    throw new AuthoringLogicError(
      "REQUIREMENT_COMPILE_PENDING",
      "条款仍在编译中，请稍后重试生成大纲",
      false,
      frozen.request_artifact_id,
    );
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

  function withMonotonicProgress(
    previous: AsyncRequestView | undefined,
    next: AsyncRequestView,
  ): AsyncRequestView {
    const previousSequence = previous?.progress?.sequence;
    const nextSequence = next.progress?.sequence;
    if (
      typeof previousSequence === "number" &&
      typeof nextSequence === "number" &&
      nextSequence < previousSequence
    ) {
      return { ...next, progress: previous?.progress };
    }
    return next;
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

  async function hydrateStoredWorkspaceRequests(
    workspaceId: string,
  ): Promise<void> {
    const prefix = `/api/v2/submission-workspaces/${workspaceId}/`;
    for (const [slot, record] of listQueuedAttempts()) {
      const requestId = record.requestIdentity?.request_artifact_id;
      if (!requestId || !record.descriptor.path.startsWith(prefix)) continue;
      const mode = workspaceRequestMode(record.descriptor.path, workspaceId);
      if (!mode) continue;
      try {
        const request = await deps.api.getRequest(workspaceId, requestId);
        await rememberRequest(request, mode);
        if (request.status !== "pending") clearQueuedAttempt(slot, record);
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) {
          clearQueuedAttempt(slot, record);
        }
      }
    }
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
              asyncRequestMode(latest);
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
          nextRequests.push(withMonotonicProgress(request, latest));
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
        const [envelope, sourceUnits, requirements] = await Promise.all([
          deps.api.getProjectWorkspace(route.projectId, signal),
          deps.api
            .listSourceUnits(route.projectId, signal)
            .catch(() => state.sourceUnits),
          deps.api
            .listRequirements(route.projectId, signal)
            .catch(() => state.requirements),
        ]);
        if (signal.aborted) return;
        if (hasDrafts(state.drafts)) {
          setState({ route, sourceUnits, requirements });
        } else {
          applyWorkspace(envelope, {
            selectedNodeLineageId: route.nodeLineageId,
            route,
            sourceUnits,
            requirements,
          });
        }
      }
      if (route.step === "authoring" && state.workspace) {
        const workspaceId = state.workspace.workspace_id;
        const [assessments, evidenceOverview, assets, requests] =
          await Promise.all([
            deps.api.getAssessments(workspaceId, signal).catch(() => null),
            deps.api.getEvidenceOverview(workspaceId, signal).catch(() => null),
            deps.api
              .listAssets(workspaceId, signal)
              .catch(() => [] as WorkspaceAssetView[]),
            deps.api
              .listWorkspaceRequests(workspaceId, signal)
              .catch(() => [] as AsyncRequestView[]),
          ]);
        if (signal.aborted) return;
        for (const request of requests) {
          requestModes.set(
            request.request_artifact_id,
            asyncRequestMode(request),
          );
        }
        const monotonicRequests = requests.map((request) =>
          withMonotonicProgress(
            state.asyncRequests.find(
              (current) =>
                current.request_artifact_id === request.request_artifact_id,
            ),
            request,
          ),
        );
        setState({
          assessments,
          evidenceOverview,
          assets,
          asyncRequests: monotonicRequests,
        });
        await hydrateStoredWorkspaceRequests(workspaceId);
        if (signal.aborted) return;
        const latestCandidateRequest = monotonicRequests.find(
          (request) =>
            (request.kind === "OutlineGenerate" ||
              (request.kind === "ContentGenerate" &&
                request.operation !== "match_only")) &&
            request.status === "succeeded" &&
            request.result_identity,
        );
        if (latestCandidateRequest?.result_identity) {
          await hydrateCandidate(
            workspaceId,
            latestCandidateRequest.result_identity.artifact_id,
          ).catch(() => undefined);
        }
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
        await hydrateStoredWorkspaceRequests(state.workspace.workspace_id);
        if (signal.aborted) return;
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
    | "content_sha256"
  > {
    return {
      schema_version: 1,
      block_revision_id: newUuid(),
      lineage_id: newUuid(),
      revision: 1,
      origin: "human",
      content_sha256: "0".repeat(64),
    };
  }

  function materialize(
    incoming: ChapterPatchBlock,
    previous: ContentBlockV1 | null,
  ): ContentBlockV1 {
    const base =
      previous && previous.kind === incoming.kind
        ? previous
        : { ...newBlockBase() };
    if (incoming.kind === "rich_text") {
      return {
        ...base,
        kind: "rich_text",
        origin: "human",
        content: { type: "rich_text", nodes: incoming.nodes },
      };
    }
    if (incoming.kind === "table") {
      return {
        ...base,
        kind: "table",
        origin: "human",
        content: incoming.content,
      };
    }
    if (incoming.kind === "image") {
      return {
        ...base,
        kind: "image",
        origin: "human",
        content: incoming.content,
      };
    }
    if (incoming.kind === "page_break") {
      return {
        ...base,
        kind: "page_break",
        origin: "human",
        content: { type: "page_break" },
      };
    }
    return {
      ...base,
      kind: "signature_placeholder",
      origin: "human",
      content: incoming.content,
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
          const payload = {
            projectId,
            fileName: file.name,
            mediaType: file.type,
            size: file.size,
            sha256: digest,
          };
          await dispatchQueued(
            `upload:${key}`,
            {
              version: 1,
              method: "POST",
              path: `/api/v2/bid-projects/${projectId}/tender-documents`,
              body: null,
              ifMatch: null,
              upload: {
                fileName: file.name,
                mediaType: file.type,
                byteLength: file.size,
                sha256: digest,
              },
            },
            payload,
            (_payload, attempt) =>
              deps.api.uploadTenderDocument(projectId, file, attempt),
          );
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
        await dispatchQueued(
          `tender-retry:${state.route.projectId}:${documentId}`,
          {
            version: 1,
            method: "POST",
            path: `/api/v2/bid-projects/${state.route.projectId}/tender-documents/${documentId}/retry`,
            body: { expected_generation: expectedGeneration },
            ifMatch: null,
          },
          { expectedGeneration },
          (payload, attempt) =>
            deps.api.retryTenderDocument(
              state.route!.projectId,
              documentId,
              payload.expectedGeneration,
              attempt,
            ),
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
        await dispatchQueued(
          `requirement-compile:${state.route.projectId}`,
          {
            version: 1,
            method: "POST",
            path: `/api/v2/bid-projects/${state.route.projectId}/document-set-revisions`,
            body: {
              document_ids: documentIds,
              expected_artifact_id: expected?.artifact_id ?? null,
              expected_sha256: expected?.sha256 ?? null,
            },
            ifMatch: null,
          },
          { documentIds, expected },
          (payload, attempt) =>
            deps.api.freezeDocumentSet(
              state.route!.projectId,
              payload.documentIds,
              payload.expected,
              attempt,
            ),
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
    applyDocument(doc) {
      if (!state.workspace || !state.etag || state.ended) return;
      const { workspace } = head();
      const patches = docToChapterPatches(doc);
      let drafts = { ...state.drafts };
      let dirty = false;
      const titles: Array<{ id: string; title: string }> = [];
      for (const patch of patches) {
        const node = findNode(tree(), patch.lineageId);
        if (!node) continue;
        if (patch.title.trim() && patch.title.trim() !== node.title) {
          titles.push({ id: patch.lineageId, title: patch.title.trim() });
        }
        const existing = blocksForNode(node, workspace.blocks, drafts);
        const next = patch.blocks.map((incoming, index) => {
          const prev = existing[index];
          return materialize(
            incoming,
            prev && prev.kind === incoming.kind ? prev : null,
          );
        });
        const kept = new Set(next.map((block) => block.lineage_id));
        for (const extra of existing) {
          if (kept.has(extra.lineage_id)) continue;
          drafts = upsertDraft(drafts, {
            nodeLineageId: node.lineage_id,
            blockLineageId: extra.lineage_id,
            op: "delete",
            ordinal: 0,
            block: extra,
            baseWorkspaceRevisionId: workspace.revision_id,
          });
          dirty = true;
        }
        next.forEach((block, ordinal) => {
          const prev = existing.find(
            (item) => item.lineage_id === block.lineage_id,
          );
          if (
            prev &&
            prev.kind === block.kind &&
            JSON.stringify(prev.content) === JSON.stringify(block.content)
          ) {
            return;
          }
          drafts = upsertDraft(drafts, {
            nodeLineageId: node.lineage_id,
            blockLineageId: block.lineage_id,
            op: prev ? "update" : "insert",
            ordinal,
            block,
            baseWorkspaceRevisionId: workspace.revision_id,
          });
          dirty = true;
        });
      }
      if (dirty) {
        setState({
          drafts,
          draftStatus: state.conflict ? "conflict" : "dirty",
          error: null,
        });
        if (!state.conflict) scheduleSave();
      }
      for (const item of titles) void session.renameNode(item.id, item.title);
    },
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
          setState({ preparingOutline: true, error: null });
          assertEditable();
          if (!state.route)
            throw new AuthoringLogicError("NO_ROUTE", "未选择项目");
          let current = head();
          await resolveStoredOutlineGenerate(current.workspace.workspace_id);
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
          const sourceKey = ready
            .map(
              (doc) =>
                `${doc.id}:${doc.original_sha256}:${doc.conversion_generation}`,
            )
            .sort()
            .join("|");
          const pendingOutline = state.asyncRequests.find(
            (request) =>
              request.kind === "OutlineGenerate" &&
              request.status === "pending",
          );
          if (pendingOutline) {
            return;
          }
          const documentSets = await deps.api.listDocumentSets(
            state.route.projectId,
          );
          const documentSetHead = documentSets[0];
          if (!documentSetHead) {
            throw new AuthoringLogicError(
              "NO_DOCUMENT_SET",
              "项目缺少当前 DocumentSet，无法冻结",
            );
          }
          const compilePayload = {
            documentIds: ready.map((doc) => doc.id),
            expected: {
              artifact_id: documentSetHead.artifact_id,
              sha256: documentSetHead.sha256,
            },
          };
          const frozen = await dispatchQueued(
            `requirement-compile:${state.route.projectId}`,
            {
              version: 1,
              method: "POST",
              path: `/api/v2/bid-projects/${state.route.projectId}/document-set-revisions`,
              body: {
                document_ids: compilePayload.documentIds,
                expected_artifact_id: compilePayload.expected.artifact_id,
                expected_sha256: compilePayload.expected.sha256,
              },
              ifMatch: null,
            },
            compilePayload,
            (payload, attempt) =>
              deps.api.freezeDocumentSet(
                state.route!.projectId,
                payload.documentIds,
                payload.expected,
                attempt,
              ),
          );
          const compilation = await waitForRequirementCompilation(
            state.route.projectId,
            frozen,
          );
          if (compilation.status === "failed") {
            throw new AuthoringLogicError(
              compilation.error_code ?? "REQUIREMENT_COMPILE_FAILED",
              "条款编译失败，无法生成大纲",
              true,
              compilation.request_artifact_id,
            );
          }
          const result = compilation.result_identity;
          if (!result || !result.published_current) {
            throw new AuthoringLogicError(
              "REQUIREMENT_COMPILE_SUPERSEDED",
              "条款编译结果已被更新输入取代，请重新生成",
              false,
              compilation.request_artifact_id,
            );
          }
          if (
            !result.workspace_apply_required ||
            result.document_set_revision_id !== frozen.artifact_id ||
            result.document_set_sha256 !== frozen.sha256 ||
            !result.requirement_projection_id ||
            !result.requirement_projection_sha256
          ) {
            throw new AuthoringLogicError(
              "REQUIREMENT_COMPILE_RESULT_INVALID",
              "条款编译结果缺少可信投影身份",
              true,
              compilation.request_artifact_id,
            );
          }
          const latest = await deps.api.getProjectWorkspace(
            state.route.projectId,
          );
          applyWorkspace(latest);
          current = head();
          const applied = await deps.api.applyRequirementProjection(
            current.workspace.workspace_id,
            {
              artifact_id: result.requirement_projection_id,
              sha256: result.requirement_projection_sha256,
            },
            {
              artifact_id: current.workspace.revision_id,
              sha256: current.workspace.sha256,
            },
            { attempt: createMutationAttempt(), ifMatch: current.etag },
          );
          applyWorkspace(applied);
          current = head();
          if (
            current.workspace.requirement_projection_revision_id !==
              result.requirement_projection_id ||
            current.workspace.requirement_projection_sha256 !==
              result.requirement_projection_sha256 ||
            current.workspace.document_set_revision_id !== frozen.artifact_id ||
            current.workspace.document_set_sha256 !== frozen.sha256
          ) {
            throw new AuthoringLogicError(
              "REQUIREMENT_PROJECTION_APPLY_INVALID",
              "应用后的工作区与冻结条款投影不匹配",
              true,
            );
          }
          const [sourceUnits, requirements] = await Promise.all([
            deps.api
              .listSourceUnits(state.route.projectId)
              .catch(() => state.sourceUnits),
            deps.api
              .listRequirements(state.route.projectId)
              .catch(() => state.requirements),
          ]);
          setState({ sourceUnits, requirements });
          const body = buildOutlineCandidateRequest({
            expected_workspace_revision_id: current.workspace.revision_id,
            document_set_revision_id: frozen.artifact_id,
            document_set_sha256: frozen.sha256,
          });
          const request = await dispatchQueued(
            `outline-generate:${current.workspace.workspace_id}`,
            {
              version: 1,
              method: "POST",
              path: `/api/v2/submission-workspaces/${current.workspace.workspace_id}/outline-candidates`,
              body,
              ifMatch: current.etag,
            },
            body,
            (payload, attempt, descriptor) =>
              deps.api.createOutlineCandidate(
                current.workspace.workspace_id,
                payload,
                { attempt, ifMatch: descriptor.ifMatch },
              ),
          );
          await rememberRequest(request, "candidate");
          setState({ outlineSourceKey: sourceKey });
        } catch (error) {
          fail(error);
        } finally {
          setState({ preparingOutline: false });
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
          const request = await dispatchQueued(
            `content-generate:${current.workspace.workspace_id}:${nodeLineageId ?? "workspace"}`,
            {
              version: 1,
              method: "POST",
              path: `/api/v2/submission-workspaces/${current.workspace.workspace_id}/content-candidates`,
              body,
              ifMatch: current.etag,
            },
            body,
            (payload, attempt, descriptor) =>
              deps.api.createContentCandidate(
                current.workspace.workspace_id,
                payload,
                { attempt, ifMatch: descriptor.ifMatch },
              ),
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
          const evidencePayload = {
            workspaceRevisionId: workspace.revision_id,
          };
          const request = await dispatchQueued(
            `evidence-match:${workspace.workspace_id}:${nodeLineageId}`,
            {
              version: 1,
              method: "POST",
              path: `/api/v2/submission-workspaces/${workspace.workspace_id}/nodes/${nodeLineageId}/evidence-matches`,
              body: { expected_workspace_revision_id: workspace.revision_id },
              ifMatch: etag,
            },
            evidencePayload,
            (payload, attempt, descriptor) =>
              deps.api.matchEvidence(
                workspace.workspace_id,
                nodeLineageId,
                payload.workspaceRevisionId,
                { attempt, ifMatch: descriptor.ifMatch },
              ),
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
          if (
            error instanceof ApiError &&
            error.code === "CANDIDATE_OBSOLETE"
          ) {
            setState({
              candidate: null,
              selectedOperationIndexes: [],
              selectedOutlineNodeRefs: [],
            });
            const latestWorkspace = await deps.api
              .getWorkspace(workspace.workspace_id)
              .catch(() => null);
            if (latestWorkspace) {
              applyWorkspace(latestWorkspace, {
                candidate: null,
                selectedOperationIndexes: [],
                selectedOutlineNodeRefs: [],
              });
            }
          }
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
          const request = await dispatchQueued(
            `submission-export:${workspace.workspace_id}:${mode}:${format}`,
            {
              version: 1,
              method: "POST",
              path: `/api/v2/submission-workspaces/${workspace.workspace_id}/exports`,
              body,
              ifMatch: etag,
            },
            body,
            (payload, attempt, descriptor) =>
              deps.api.createExport(workspace.workspace_id, payload, {
                attempt,
                ifMatch: descriptor.ifMatch,
              }),
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
