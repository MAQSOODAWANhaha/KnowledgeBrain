import { type MutationAttempt } from "../../api";
import type { BidProjectView } from "./types";
import type { DocxCurrent, DocxIdentity, DocxOpened, DocxSaveReceipt, DocxSavedReceipt } from "./docx";
import type { DocxSessionApi } from "../authoring/docxSession";
import type { ReviewIdentity } from "./analysisReview";
import { downloadResponse, v2Blob, v2Request } from "./http";

export type TenderIssue = { code: string; detail: string; issue_id?: string; chapter_id?: string | null; target_id?: string | null; rule_refs?: RuleRef[] };
// page is the 1-based physical page; heading_path is used when the source has no pages.
export type TenderEvidence = { range_id: string; quote: string; page?: number | null; heading_path?: string | null };
export type TenderItem = {
  item_id?: string; kind?: string; text: string; evidence?: TenderEvidence[];
  domains?: string[] | null; subject?: string; scope?: string | null; conditions?: string[] | null; exceptions?: string[] | null;
  numbers?: Array<{ value: string | number; unit: string; quote: string }>;
  scoring?: { method?: string; points?: number; max_points?: number };
  consequence?: { effect?: string; trigger?: string };
  submission?: { fields?: string[]; documents?: string[] };
};
export type RuleRef = { item_ref: { item_id: string; artifact_ref?: { sha256: string } }; decision_id?: string | null; rule_index?: number | null };
export type SourceExcerptSelector = { range_id: string; source_range_id: string; start: number; end: number };
export type SourceExcerptRef = {
  source_ref: { source_index_ref: { sha256: string; relative_path?: string; size?: number; media_type?: string }; unit_id: string };
  selectors: SourceExcerptSelector[];
};
export type ProjectContextOption = {
  option_id: string; range_id: string; document_id: string;
  page?: number | null; heading_path?: string | null; preview: string; excerpt: SourceExcerptRef;
};
export type ProjectContextSources = {
  available: boolean; reason: string | null; frozen_sha256: string | null; options: ProjectContextOption[];
};
export type OutlineSourceAddition = { target_id: string; project_context_excerpts: SourceExcerptRef[] };
export type TenderTarget = {
  target_id: string; kind: string; purpose: string; manual_fill?: boolean;
  rule_refs?: RuleRef[]; source_refs?: Array<{ unit_id: string }>; bookmark?: string | null;
  related_target_ids?: string[];
  support_origins?: string[]; template_refs?: unknown[]; response_table?: unknown | null;
  response_form_pending?: unknown | null; source_body_contract?: string | null;
  project_context_excerpts?: SourceExcerptRef[];
};
export type TenderChapter = {
  chapter_id: string; title: string; children: TenderChapter[]; targets: TenderTarget[];
  is_volume?: boolean; bookmark?: string | null; manual?: boolean;
  presentation_role?: string; writing_goal?: string;
  target_ids?: string[];
};
export type OutlineNode = Pick<TenderChapter, "chapter_id" | "title" | "is_volume" | "presentation_role" | "writing_goal"> & {
  children: OutlineNode[]; target_ids?: string[]; source_additions?: OutlineSourceAddition[];
};
export type OutlineHistoryEntry = { revision_id: string; revision: number; parent_id: string | null; created_at: string };
export type OutlineDocument = { chapters: TenderChapter[]; issues?: TenderIssue[]; authoring_input?: ReviewIdentity & { frozen_ref?: { sha256?: string } } };
export type OutlineRevision = {
  revision_id: string; revision: number; parent_id: string | null; execution_id: string; sha256: string;
  outline: OutlineDocument; sources_current: boolean; newer_analysis_identity?: ReviewIdentity | null;
};
export type OutlineView = {
  draft: OutlineRevision | null;
  candidate: { execution_id: string; sha256: string; outline: OutlineDocument } | null;
  generation: TenderExecution | null;
  project_context_sources?: ProjectContextSources | null;
};
export type EvaluationResolution = "pending" | "evaluation" | "reference" | "document_wide" | "target";
export type EvaluationDecision = { rule_ref: RuleRef; resolution: EvaluationResolution; target_id?: string };
export type OutlineSave = { expected_revision_id: string | null; source_revision_id?: string; source_execution_id?: string; chapters: OutlineNode[]; evaluation_decisions?: EvaluationDecision[] };
export type DocumentStart = { outline_revision_id?: string; outline_sha256?: string; knowledge_selections?: Array<{ product_id: string; version_id: string }>; previous_execution_id?: string; review_request?: { target_ids: string[]; reason: string } };
export type TenderDocument = {
  id: string; file_name: string; media_type: string; byte_length: number;
  parse_status?: string; content_sha256?: string; download_url?: string;
};
export type SavedWordOutline = { chapters: TenderChapter[]; bookmarks?: string[]; missing_chapter_ids?: string[] };
export type TenderDraft = {
  id: string; sha256: string; revision: number; source: "generated" | "manual" | "recovered";
  download_url: string; execution_id?: string | null; outline?: SavedWordOutline;
};
export type TenderDocxCurrent = DocxCurrent & { execution_id?: string | null; outline?: SavedWordOutline; source?: "generated" | "manual" | "recovered" };
export type TenderUnit = {
  task_id: string; chapter_id: string; target_ids: string[];
  blocks: Array<{ text: string; material_ids: string[]; source_refs: Array<{ unit_id: string }> }>;
  cell_fills: Array<{ text: string; material_ids: string[]; source_refs: Array<{ unit_id: string }> }>;
  attachments: Array<{ material_id: string; title: string }>;
};
export type TaskProgressView = {
  identity: { run_id?: string; source_version?: number; domain_revision?: number; execution_id?: string };
  state: string; current_phase: string | null;
  active_tasks: Array<{ phase: string; prepared: number; sent: number }>;
  latest_request_phase: string | null; last_completed_phase: string | null;
  accepted_units: number; accepted_by_phase: Array<{ phase: string; count: number; planned: number | null }>;
  request_states: { prepared: number; sent: number; uncertain: number; failed: number; received: number };
  rejected_responses: number;
};
export type PendingGapStatus = "pending" | "resolved" | "need_material" | "accepted_gap";
export type PendingQueueItem = {
  gap_id: string; code: string; detail: string;
  chapter_id: string | null; chapter_title: string | null;
  target_id: string | null; target_purpose: string | null; bookmark: string | null;
  status: PendingGapStatus; note: string;
  created_at: string | null; handled_at: string | null; handling_duration_ms: number | null;
};
export type PendingQueueSummary = {
  total: number; pending: number; handled: number;
  by_status: Record<PendingGapStatus, number>;
  handling_duration_ms: { observations: number; avg: number | null; max: number | null; p95: number | null };
};
export type ExecutionReportView = {
  execution_id: string; status: string; kind: "outline" | "document"; requests: number;
  stages: Record<string, { stage: string; logical_requests: number; requests: number; reported_tokens: { total_tokens?: number } }>;
  failures: Array<{ stage: string; task: string; kind: string; code: string; count: number; reported_tokens: number; unknown_usage: number }>;
  pending_queue?: PendingQueueSummary;
};
export type TenderExecution = {
  execution_id: string; status: string; error?: unknown;
  operation?: "outline" | "document";
  progress?: TaskProgressView;
  usage?: { sent_requests: number; retry_requests: number; total_tokens: number; unknown_usage_requests: number };
};
export type WorkbenchView = {
  project: BidProjectView;
  documents: TenderDocument[];
  analysis: null | {
    run_id: string; status: string; source_version: number; domain_revision: number;
    confirmed: boolean; summary?: string; items?: TenderItem[]; issues: TenderIssue[]; error?: unknown;
    sources_current?: boolean;
    progress?: TaskProgressView;
  };
  authoring: TenderExecution | null;
  executions?: TenderExecution[];
  context_execution_id?: string | null;
  units?: TenderUnit[];
  materials?: Array<{ material_id: string; title: string; text?: string; locator?: Record<string, unknown>; original_url?: string }>;
  outline: { chapters: TenderChapter[]; version_id?: string | null } | null;
  current_docx: TenderDraft | null;
  gaps: TenderIssue[];
  pending_queue?: PendingQueueItem[];
  coverage?: { state: "unknown" | "draft"; pending: number | null; unreviewed: number | null; headline: string;
    applies_to?: { execution_id: string; version_id: string; docx_sha256: string } };
  requirements?: { rules: Array<{ rule_ref: RuleRef; rule: TenderItem | null; target_ids: string[] }> } | null;
  versions: TenderDraft[];
  artifacts?: { execution_id?: string; package_url?: string | null };
};

const root = "/api/v3/tender-projects";
export const projectPath = (id: string) => `${root}/${encodeURIComponent(id)}`;
const docxPath = (id: string) => `${projectPath(id)}/docx`;

export const tenderApi = {
  async listProjects(signal?: AbortSignal): Promise<BidProjectView[]> {
    return (await v2Request<BidProjectView[]>(root, { signal })).data;
  },
  async createProject(body: { title: string }, attempt?: MutationAttempt): Promise<BidProjectView> {
    return (await v2Request<BidProjectView>(root, { method: "POST", body: JSON.stringify(body) }, { attempt })).data;
  },
  async getProject(id: string, signal?: AbortSignal): Promise<BidProjectView> {
    return (await v2Request<BidProjectView>(projectPath(id), { signal })).data;
  },
  async workbench(id: string, signal?: AbortSignal): Promise<WorkbenchView> {
    return (await v2Request<WorkbenchView>(`${projectPath(id)}/workbench`, { signal })).data;
  },
  async upload(id: string, file: File, attempt: MutationAttempt): Promise<TenderDocument> {
    const body = new FormData();
    body.append("file", file);
    return (await v2Request<TenderDocument>(`${projectPath(id)}/documents`, { method: "POST", body }, { attempt })).data;
  },
  async start(id: string, kind: "analysis" | "authoring", attempt: MutationAttempt, body: DocumentStart | Record<string, never> = {}): Promise<void> {
    await v2Request(`${projectPath(id)}/${kind}`, { method: "POST", body: JSON.stringify(body) }, { attempt });
  },
  async outline(id: string, signal?: AbortSignal): Promise<OutlineView> {
    return (await v2Request<OutlineView>(`${projectPath(id)}/outline`, { signal })).data;
  },
  async generateOutline(id: string, attempt: MutationAttempt): Promise<void> {
    await v2Request(`${projectPath(id)}/outline/generate`, { method: "POST", body: "{}" }, { attempt });
  },
  async outlineHistory(id: string, signal?: AbortSignal): Promise<OutlineHistoryEntry[]> {
    return (await v2Request<{ revisions: OutlineHistoryEntry[] }>(`${projectPath(id)}/outline/revisions`, { signal })).data.revisions;
  },
  async executionReport(id: string, execution: string, signal?: AbortSignal): Promise<ExecutionReportView | null> {
    return (await v2Request<{ report: ExecutionReportView | null }>(`${projectPath(id)}/executions/${encodeURIComponent(execution)}/report`, { signal })).data.report;
  },
  async updatePendingItem(
    id: string,
    execution: string,
    gapId: string,
    body: { status: PendingGapStatus; note?: string },
  ): Promise<{ item: PendingQueueItem; summary: PendingQueueSummary }> {
    return (await v2Request<{ item: PendingQueueItem; summary: PendingQueueSummary }>(
      `${projectPath(id)}/executions/${encodeURIComponent(execution)}/pending/${encodeURIComponent(gapId)}`,
      { method: "PUT", body: JSON.stringify(body) },
    )).data;
  },
  async outlineRevision(id: string, revision: string, signal?: AbortSignal): Promise<{ draft: OutlineRevision; available_targets: TenderTarget[] }> {
    return (await v2Request<{ draft: OutlineRevision; available_targets: TenderTarget[] }>(`${projectPath(id)}/outline/revisions/${encodeURIComponent(revision)}`, { signal })).data;
  },
  async saveOutline(id: string, body: OutlineSave, attempt: MutationAttempt): Promise<OutlineRevision> {
    return (await v2Request<{ draft: OutlineRevision }>(`${projectPath(id)}/outline`, { method: "PUT", body: JSON.stringify(body) }, { attempt })).data.draft;
  },
  async execution(id: string, execution: string, action: "resume" | "cancel", attempt: MutationAttempt): Promise<void> {
    await v2Request(`${projectPath(id)}/executions/${encodeURIComponent(execution)}/${action}`,
      { method: "POST", body: "{}" }, { attempt });
  },
  async selectDraft(id: string, draft: DocxIdentity, attempt: MutationAttempt): Promise<DocxCurrent> {
    return (await v2Request<DocxCurrent>(`${docxPath(id)}/current`,
      { method: "POST", body: JSON.stringify(draft) }, { attempt })).data;
  },
};

// Keep the editor's save-receipt contract while moving its scope to the project API.
export const tenderDocxApi: DocxSessionApi = {
  async current(scope) {
    return (await v2Request<DocxCurrent | null>(`${docxPath(scope)}/current`)).data;
  },
  async version(scope, version) {
    return (await v2Request<DocxCurrent>(`${docxPath(scope)}/versions/${encodeURIComponent(version)}`)).data;
  },
  async open(scope, expected, attempt, options?: { mode: "view" | "edit"; language?: string }) {
    return (await v2Request<DocxOpened>(`${docxPath(scope)}/editor`, { method: "POST",
      body: JSON.stringify({ ...expected, mode: options?.mode ?? "view", language: options?.language ?? (document.documentElement.lang || "zh-CN") }) }, { attempt })).data;
  },
  async save(scope, body, attempt) {
    return (await v2Request<DocxSaveReceipt>(`${docxPath(scope)}/editor/save`,
      { method: "POST", body: JSON.stringify(body) }, { attempt })).data;
  },
  async saved(scope, key, save) {
    return (await v2Request<DocxSavedReceipt | null>(`${docxPath(scope)}/editor/${encodeURIComponent(key)}/saves/${encodeURIComponent(save)}`)).data;
  },
  download(scope, version) {
    return v2Blob(`${docxPath(scope)}/versions/${encodeURIComponent(version)}/download`);
  },
  async close(scope, key) {
    await v2Request(`${docxPath(scope)}/editor/${encodeURIComponent(key)}/close`, { method: "POST", body: "{}" });
  },
};

export function executionError(value: unknown): string | null {
  if (!value) return null;
  if (typeof value === "string") return value;
  if (typeof value === "object") {
    const error = value as { detail?: unknown; message?: unknown; type?: unknown };
    if (typeof error.detail === "string") return error.detail;
    if (typeof error.message === "string") return error.message;
    if (typeof error.type === "string") return `执行中断（${error.type}），已保留任务记录。`;
  }
  return "执行中断，详细记录暂不可读取。";
}

function saveTenderFile(file: Blob, name: string): void {
  const objectUrl = URL.createObjectURL(file);
  const link = document.createElement("a");
  link.href = objectUrl;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
}

export async function downloadTenderFile(url: string, name: string): Promise<void> {
  if (!url.startsWith(`${root}/`) || url.startsWith("//")) throw new Error("文件地址不属于投标项目");
  saveTenderFile(await v2Blob(url), name);
}

export async function downloadTenderPdf(project: string, version: Pick<TenderDraft, "id" | "sha256" | "revision">, signal: AbortSignal): Promise<void> {
  const { id, sha256, revision } = version;
  const response = await downloadResponse(`${docxPath(project)}/versions/${encodeURIComponent(id)}/pdf`, {
    method: "POST", headers: { "Content-Type": "application/json", Accept: "application/pdf" },
    body: JSON.stringify({ docx_sha256: sha256 }), signal,
  });
  const contentType = response.headers.get("content-type")?.split(";", 1)[0].trim().toLowerCase();
  if (contentType !== "application/pdf" || response.headers.get("x-kb-render-source-version") !== id
      || response.headers.get("x-kb-render-source-sha256") !== sha256
      || !/^[a-f0-9]{64}$/.test(response.headers.get("x-kb-render-pdf-sha256") ?? "")) {
    throw new Error("PDF 响应无法与此已保存版本核对，请重试。");
  }
  const file = await response.blob();
  signal.throwIfAborted();
  saveTenderFile(file, `投标文件-v${revision}.pdf`);
}
