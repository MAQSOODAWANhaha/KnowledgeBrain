const TOKEN = "kb.token";

export function token(): string | null {
  return localStorage.getItem(TOKEN);
}

export function setToken(t: string | null): void {
  if (t) localStorage.setItem(TOKEN, t);
  else localStorage.removeItem(TOKEN);
}

export class ApiError extends Error {
  status: number;
  code?: string;
  constructor(status: number, message: string, code?: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export type MutationAttempt = Readonly<{
  idempotencyKey: string;
}>;

export function createMutationAttempt(): MutationAttempt {
  return { idempotencyKey: crypto.randomUUID() };
}

async function req<T>(path: string, init: RequestInit = {}, attempt?: MutationAttempt): Promise<T> {
  const headers = new Headers(init.headers);
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  const method = (init.method || "GET").toUpperCase();
  if (["POST", "PUT", "PATCH", "DELETE"].includes(method) && !headers.has("Idempotency-Key")) {
    headers.set("Idempotency-Key", attempt?.idempotencyKey ?? createMutationAttempt().idempotencyKey);
  }
  if (init.body && !(init.body instanceof FormData) && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  const res = await fetch(path, { ...init, headers });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  const data = text ? (JSON.parse(text) as unknown) : null;
  if (!res.ok) {
    const obj =
      data && typeof data === "object"
        ? (data as { message?: string; error?: { message?: string; code?: string } })
        : null;
    const msg = obj?.error?.message || obj?.message || res.statusText;
    throw new ApiError(res.status, msg || "请求失败", obj?.error?.code);
  }
  return data as T;
}

export const CLAUSE_KINDS = [
  "technical",
  "qualification",
  "service",
  "pricing",
  "schedule_delivery",
  "schedule_payment",
  "evaluation",
  "procedural",
] as const;

export type ClauseKind = (typeof CLAUSE_KINDS)[number];

export type Project = {
  id: string;
  title: string;
  owner_user_id: string;
  ends_at: string;
  expires_at: string | null;
  status: string;
  ended_at: string | null;
  fact_revision: number;
  fact_sha256: string;
  budget_amount: string | null;
  ceiling_price: string | null;
  ceiling_basis: string;
  ceiling_revision: number;
  ceiling_identity_sha256: string;
  bid_open_at: string | null;
  bid_valid_until: string | null;
  bid_valid_days: number | null;
};

export type Derived = {
  has_files: boolean;
  files_ready: boolean;
  extract_running: boolean;
  unconfirmed_drafts: number;
  match_running: boolean;
  has_picks: boolean;
  files_not_in_clauses: number;
};

export type Clause = {
  id: string;
  project_id: string;
  publication_id: string | null;
  provenance: string;
  status: string;
  kind: string;
  family: string | null;
  text: string;
  must: boolean;
  revision: number;
  current_source_span_v2: unknown;
  extracted_origin_source_span_v2: unknown;
  confirmation_required_reason: string | null;
  confirmation_required_router_generation: number | null;
};

export type BidDoc = {
  id: string;
  project_id: string;
  file_name: string;
  media_type: string;
  byte_length: number;
  original_object_ref: string;
  original_sha256: string;
  conversion_generation: number;
  parse_status: string;
  current_converted_source_artifact_id: string | null;
  parsed_at: string | null;
  error_code: string | null;
};

export type FactSuggestion = {
  id: string;
  field: string;
  typed_value: unknown;
  raw_quote: string;
  confidence: string;
};

export type MatchUnit = {
  id: string | null;
  route_id?: string | null;
  heading_path: string;
  kind?: string;
  technical_count?: number;
};

export type Candidate = {
  requirement_artifact_id: string;
  candidate_artifact_id: string;
  product_id: string;
  product_version_id: string;
  recommended: boolean;
  retrieval_rank?: number;
  retrieval_raw_score?: string;
};

export type RoutePickSet = {
  route_id: string;
  route_kind: string;
  unit_id: string | null;
  source_report_artifact_id?: string;
  report_sha256?: string;
  report_generation?: number;
  revision: number;
  items: Array<{
    requirement_artifact_id: string;
    candidate_artifact_id: string;
    product_id?: string;
    product_version_id?: string;
    unit_id?: string;
  }>;
  supported_candidates: Candidate[];
};

export type QuoteLine = {
  id: string;
  ordinal: number;
  description: string;
  pricing_mode: string;
  complete: boolean;
  quantity: string | null;
  unit: string | null;
  unit_price: string | null;
  entered_amount: string | null;
  tax_rate: string;
  basis_amount: string | null;
  net_amount: string | null;
  tax_amount: string | null;
  gross_amount: string | null;
  user_confirmed: boolean;
};

export type QuoteState = {
  exists: boolean;
  pointer?: string;
  quote_id?: string;
  revision_id?: string;
  snapshot_id?: string;
  revision?: number;
  edit_version?: number;
  status?: string;
  tax_mode?: string;
  title?: string;
  notes?: string | null;
  eligibility?: string;
  lines?: QuoteLine[];
  net_total?: string;
  tax_total?: string;
  gross_total?: string;
  active_finalized_snapshot_id?: string | null;
};

export type CompanyProfile = {
  revision?: number;
  legal_name?: string;
  unified_social_credit_code?: string;
  registered_address?: string;
  legal_representative?: string;
  contact_name?: string;
  contact_phone?: string;
  contact_email?: string;
};

export type SubmissionProfile = {
  revision?: number;
  buyer_name?: string;
  project_code?: string;
  authorized_representative?: string;
  submission_date?: string;
  submission_place?: string;
  seal_confirmed?: boolean;
  signature_confirmed?: boolean;
};

export type AttachmentKind = "bid_bond" | "authorization_support" | "seal_sample" | "procedural_support";

export type ProceduralRequirementKind = AttachmentKind | "confirmation";

export type ProceduralResolution = "confirmed_by_user" | "satisfied_by_attachment" | "not_applicable";

export type AttachmentAction = "validate" | "invalidate" | "confirm" | "reject" | "delete";

export type ProceduralClassification = {
  id: string;
  segment_text: string;
  effective_requirement_kind: ProceduralRequirementKind;
  router_requirement_kind: ProceduralRequirementKind;
  router_result_status: string;
};

export type ProceduralAttachment = {
  id: string;
  kind: AttachmentKind;
  status: "draft" | "confirmed" | "rejected" | "superseded";
  validation_status: "pending" | "valid" | "invalid";
  revision: number;
};

export type GateIssue = {
  code: string;
  part_key?: string | null;
  entity_locator?: unknown;
  remediation?: { action?: string };
};

export type PartStatus = {
  part_key: string;
  stale?: boolean;
  stale_reason_codes?: string[];
  content_revision?: number;
  markdown?: string;
  dependency_sha256?: string;
  typed_input_identities?: unknown;
};

export type BidDetail = {
  project: Project;
  documents: BidDoc[];
  quote: QuoteState;
  facts?: {
    revision: number;
    suggestions: FactSuggestion[];
    budget_amount: string | null;
    ceiling_price: string | null;
    ceiling_basis: string;
    ceiling_revision: number;
    ceiling_identity_sha256: string;
    expires_at: string | null;
    bid_open_at: string | null;
    bid_valid_until: string | null;
    bid_valid_days: number | null;
  };
  clause_sets?: Array<{ set_kind: string; revision: number; content_sha256: string }>;
  matching?: {
    routes: Array<{ route_id: string; route_kind: string; unit_id: string | null }>;
    reports: unknown[];
    commercial_decisions: Array<{
      clause_id?: string;
      system_decision?: string;
      final_support?: string;
      reason_code?: string;
      frozen_document_display_name?: string;
    }>;
    technical_candidates: Candidate[];
    project_pick_set?: { payload?: { items?: unknown[] } } | null;
  };
  parts?: PartStatus[];
  outputs?: Array<{ id: string; manifest_id: string; format: string; content_sha256: string }>;
  derived: Derived;
};

export type Workspace = {
  id: string;
  name: string;
  slug: string;
  kind: string;
};

export type Product = {
  id: string;
  workspace_id: string;
  kind: string;
  name: string;
  slug: string;
  current_version_id: string | null;
};

export type Version = {
  id: string;
  product_id: string;
  label: string;
  status: string;
  current: boolean;
};

export type Doc = {
  id: string;
  title: string;
  file_name: string;
  parse_status: string | { [k: string]: unknown };
  index_ready: boolean;
  object_ref: string;
  error_message?: string;
};

export type DocChunk = {
  id: string;
  chunk_type: string;
  content: string;
  context_header: string;
  start_at: number;
  end_at: number;
  generated_questions: string[];
};

export type DocContent = {
  id: string;
  title: string;
  file_name: string;
  object_ref: string;
  file_hash: string;
  parse_status: string | { [k: string]: unknown };
  index_ready: boolean;
  error_message: string;
  description: string;
  markdown: string;
  chunks: DocChunk[];
};

export const api = {
  login: (email: string, password: string) =>
    req<{ token: string; user_id: string }>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  me: () => req<{ id: string; email: string }>("/api/v1/me"),
  bids: () => req<Project[]>("/api/v1/bids"),
  createBid: (body: { title: string; ends_at: string; expires_at?: string | null }) =>
    req<{ id: string }>("/api/v1/bids", { method: "POST", body: JSON.stringify(body) }),
  bid: (id: string) => req<BidDetail>(`/api/v1/bids/${id}`),
  endBid: (id: string, expected_fact_revision: number) =>
    req<void>(`/api/v1/bids/${id}`, {
      method: "POST",
      body: JSON.stringify({ expected_fact_revision }),
    }),
  docs: (id: string) => req<{ documents: BidDoc[] }>(`/api/v1/bids/${id}/documents`),
  uploadDoc: (id: string, file: File, attempt: MutationAttempt) => {
    const fd = new FormData();
    fd.set("file", file);
    return req<{ id: string }>(`/api/v1/bids/${id}/documents`, { method: "POST", body: fd }, attempt);
  },
  retryDoc: (id: string, did: string, expected_generation: number) =>
    req<void>(`/api/v1/bids/${id}/documents/${did}/retry`, {
      method: "POST",
      body: JSON.stringify({ expected_generation }),
    }),
  clauses: (id: string, history = false) =>
    req<{ clauses: Clause[] }>(`/api/v1/bids/${id}/clauses?include_history=${history}`),
  addClause: (id: string, body: { text: string; kind: string; must: boolean }) =>
    req<{ id: string; revision: number }>(`/api/v1/bids/${id}/clauses`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  mutateClause: (
    id: string,
    cid: string,
    body: { action: "patch" | "confirm" | "unconfirm" | "reject" | "delete"; expected_revision: number; patch?: Record<string, unknown> },
  ) =>
    req<void>(`/api/v1/bids/${id}/clauses/${cid}`, {
      method: "PATCH",
      body: JSON.stringify({ action: body.action, expected_revision: body.expected_revision, patch: body.patch ?? {} }),
    }),
  facts: (id: string) =>
    req<{
      project_facts: BidDetail["facts"];
      suggestions: FactSuggestion[];
      history: unknown[];
    }>(`/api/v1/bids/${id}/facts`),
  mutateFact: (
    id: string,
    body: {
      action: "accept" | "reject" | "set" | "clear";
      expected_fact_revision: number;
      candidate_id?: string;
      field?: string;
      typed_value?: unknown;
      reason?: string;
      override_reason?: string;
    },
  ) => req<void>(`/api/v1/bids/${id}/facts`, { method: "POST", body: JSON.stringify(body) }),
  rematch: (id: string) => req<{ job_id: string | null }>(`/api/v1/bids/${id}/matching/schedule`, { method: "POST" }),
  matching: (id: string) => req<NonNullable<BidDetail["matching"]>>(`/api/v1/bids/${id}/matching`),
  units: (id: string) => req<{ units: MatchUnit[] }>(`/api/v1/bids/${id}/units`),
  routePickSet: (id: string, routeId: string) => req<RoutePickSet>(`/api/v1/bids/${id}/matching/routes/${routeId}/pick-set`),
  replaceRoutePickSet: (
    id: string,
    routeId: string,
    body: {
      source_report_artifact_id: string;
      report_sha256: string;
      expected_revision: number;
      items: Array<{ requirement_artifact_id: string; candidate_artifact_id: string }>;
    },
  ) =>
    req(`/api/v1/bids/${id}/matching/routes/${routeId}/pick-set`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),
  quote: (id: string) => req<QuoteState>(`/api/v1/bids/${id}/quote`),
  createQuoteDraft: (id: string, body: { tax_mode: string; title: string; notes?: string | null }) =>
    req<QuoteState>(`/api/v1/bids/${id}/quote/draft`, { method: "POST", body: JSON.stringify(body) }),
  patchQuote: (
    id: string,
    body: { expected_edit_version: number; tax_mode: string; title: string; notes?: string | null },
  ) => req<QuoteState>(`/api/v1/bids/${id}/quote`, { method: "PATCH", body: JSON.stringify(body) }),
  upsertQuoteLine: (
    id: string,
    lineId: string,
    body: {
      expected_edit_version: number;
      ordinal: number;
      description: string;
      pricing_mode: string;
      quantity?: string | null;
      unit?: string | null;
      unit_price?: string | null;
      entered_amount?: string | null;
      tax_rate: string;
      user_confirmed: boolean;
    },
  ) => req(`/api/v1/bids/${id}/quote/lines/${lineId}`, { method: "PUT", body: JSON.stringify(body) }),
  deleteQuoteLine: (id: string, lineId: string, expected_edit_version: number) =>
    req(`/api/v1/bids/${id}/quote/lines/${lineId}`, {
      method: "DELETE",
      body: JSON.stringify({ expected_edit_version }),
    }),
  previewQuote: (id: string) => req<{ net_total?: string; tax_total?: string; gross_total?: string }>(`/api/v1/bids/${id}/quote/preview`),
  finalizeQuote: (
    id: string,
    body: {
      expected_edit_version: number;
      expected_fact_revision: number;
      expected_ceiling_revision: number;
      expected_ceiling_identity_sha256: string;
      expected_pricing_revision: number;
      expected_pricing_set_sha256: string;
      no_ceiling_reviewed?: boolean;
      no_ceiling_reason?: string;
    },
  ) => req(`/api/v1/bids/${id}/quote/finalize`, { method: "POST", body: JSON.stringify(body) }),
  reopenQuote: (
    id: string,
    body: { expected_snapshot_id: string; expected_fact_revision: number; expected_pricing_revision: number },
  ) => req(`/api/v1/bids/${id}/quote/reopen`, { method: "POST", body: JSON.stringify(body) }),
  companyProfile: (id: string) => req<CompanyProfile | null>(`/api/v1/bids/${id}/company-profile`),
  updateCompanyProfile: (id: string, body: CompanyProfile & { expected_revision: number }) =>
    req(`/api/v1/bids/${id}/company-profile`, { method: "PUT", body: JSON.stringify(body) }),
  submissionProfile: (id: string) => req<SubmissionProfile | null>(`/api/v1/bids/${id}/submission-profile`),
  updateSubmissionProfile: (id: string, body: SubmissionProfile & { expected_revision: number }) =>
    req(`/api/v1/bids/${id}/submission-profile`, { method: "PUT", body: JSON.stringify(body) }),
  procedural: (id: string) => req<{ classifications: ProceduralClassification[] }>(`/api/v1/bids/${id}/procedural-requirements`),
  overrideClassification: (id: string, cid: string, body: { effective_kind: ProceduralRequirementKind; reason: string }) =>
    req(`/api/v1/bids/${id}/procedural-classifications/${cid}/override`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  resolveRequirement: (
    id: string,
    cid: string,
    body: { resolution: ProceduralResolution; attachment_id?: string | null; reason?: string },
  ) =>
    req(`/api/v1/bids/${id}/procedural-requirements/${cid}/resolve`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  attachments: (id: string) => req<{ attachments: ProceduralAttachment[] }>(`/api/v1/bids/${id}/attachments`),
  uploadAttachment: (id: string, kind: AttachmentKind, file: File, attempt: MutationAttempt) => {
    const fd = new FormData();
    fd.set("kind", kind);
    fd.set("file", file);
    return req<{ id: string; revision: number }>(
      `/api/v1/bids/${id}/attachments`,
      { method: "POST", body: fd },
      attempt,
    );
  },
  mutateAttachment: (id: string, aid: string, action: AttachmentAction, expected_revision: number, reason?: string) =>
    req(`/api/v1/bids/${id}/attachments/${aid}/${action}`, {
      method: "POST",
      body: JSON.stringify({ expected_revision, reason }),
    }),
  parts: (id: string) => req<{ required_part_keys: string[]; parts: PartStatus[] }>(`/api/v1/bids/${id}/parts`),
  part: (id: string, key: string) => req<PartStatus>(`/api/v1/bids/${id}/parts/${encodeURIComponent(key)}`),
  updatePart: (id: string, key: string, expected_content_revision: number, markdown: string) =>
    req(`/api/v1/bids/${id}/parts/${encodeURIComponent(key)}`, {
      method: "PUT",
      body: JSON.stringify({ expected_content_revision, markdown }),
    }),
  regeneratePart: (
    id: string,
    key: string,
    body: {
      expected_content_revision: number;
      expected_dependency_sha256?: string | null;
    },
  ) =>
    req(`/api/v1/bids/${id}/parts/${encodeURIComponent(key)}/regenerate`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  gateIssues: (id: string, format: "docx" | "pdf") =>
    req<{ format: string; status: string; issues: GateIssue[]; required_part_keys: string[] }>(
      `/api/v1/bids/${id}/gate-issues?format=${format}`,
    ),
  createManifest: (id: string, format: "docx" | "pdf", attempt: MutationAttempt) =>
    req<{ manifest_id: string; content_sha256: string; format: string }>(`/api/v1/bids/${id}/submission/manifests`, {
      method: "POST",
      body: JSON.stringify({ format }),
    }, attempt),
  renderManifest: (id: string, mid: string, expected_manifest_sha256: string, attempt: MutationAttempt) =>
    req<{ render_job_id: string; manifest_id: string; status: "queued" }>(`/api/v1/bids/${id}/submission/manifests/${mid}/render`, {
      method: "POST",
      body: JSON.stringify({ expected_manifest_sha256 }),
    }, attempt),
  renderJob: (id: string, jobId: string) =>
    req<{
      render_job_id: string;
      manifest_id: string;
      status: "pending" | "running" | "completed" | "failed";
      attempt_count: number;
      max_attempts: number;
      output_id?: string;
      error_code?: string;
    }>(`/api/v1/bids/${id}/submission/render-jobs/${jobId}`),
  outputs: (id: string) =>
    req<{ outputs: Array<{ id: string; manifest_id: string; format: string }> }>(`/api/v1/bids/${id}/submission/outputs`),
  workspaces: () => req<Workspace[]>("/api/v1/workspaces"),
  createWorkspace: (body: { name: string; slug: string; kind?: string }) =>
    req<Workspace>("/api/v1/workspaces", { method: "POST", body: JSON.stringify(body) }),
  products: (wid: string) => req<Product[]>(`/api/v1/workspaces/${wid}/products`),
  createProduct: (wid: string, body: { name: string; slug: string; kind?: string }) =>
    req<Product>(`/api/v1/workspaces/${wid}/products`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  versions: (pid: string) => req<Version[]>(`/api/v1/products/${pid}/versions`),
  createVersion: (pid: string, label: string, makeCurrent = true) =>
    req<Version>(`/api/v1/products/${pid}/versions`, {
      method: "POST",
      body: JSON.stringify({ label, make_current: makeCurrent }),
    }),
  documents: (pid: string, vid: string) =>
    req<Doc[]>(`/api/v1/products/${pid}/versions/${vid}/documents`),
  documentContent: (id: string) => req<DocContent>(`/api/v1/documents/${id}/content`),
  ingest: (pid: string, vid: string, file: File) => {
    const fd = new FormData();
    fd.set("file", file);
    return req<Doc>(`/api/v1/products/${pid}/versions/${vid}/documents/file`, {
      method: "POST",
      body: fd,
    });
  },
};

export async function fileBlob(key: string): Promise<string> {
  const headers = new Headers();
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  const res = await fetch(`/api/v1/files?key=${encodeURIComponent(key)}`, { headers });
  if (!res.ok) throw new ApiError(res.status, "读文件失败");
  return URL.createObjectURL(await res.blob());
}

export async function downloadSubmission(projectId: string, outputId: string): Promise<void> {
  const headers = new Headers();
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  const res = await fetch(`/api/v1/bids/${projectId}/submission/artifacts/${outputId}`, { headers });
  if (!res.ok) {
    let msg = "下载失败";
    try {
      const j = (await res.json()) as { message?: string; error?: { message?: string } };
      msg = j.error?.message || j.message || msg;
    } catch {
      /* keep */
    }
    throw new ApiError(res.status, msg);
  }
  const blob = await res.blob();
  const cd = res.headers.get("content-disposition") ?? "";
  const m = /filename="([^"]+)"/.exec(cd);
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = m?.[1] ?? `submission-${outputId}`;
  a.click();
  URL.revokeObjectURL(a.href);
}

export type SubmissionExportAttempt = Readonly<{
  createManifest: MutationAttempt;
  renderManifest: MutationAttempt;
}>;

export function createSubmissionExportAttempt(): SubmissionExportAttempt {
  return {
    createManifest: createMutationAttempt(),
    renderManifest: createMutationAttempt(),
  };
}

const SUBMISSION_OUTPUT_POLL_ATTEMPTS = 60;
const SUBMISSION_OUTPUT_POLL_INTERVAL_MS = 500;

async function waitForSubmissionOutput(projectId: string, manifestId: string, renderJobId: string): Promise<string> {
  for (let poll = 0; poll < SUBMISSION_OUTPUT_POLL_ATTEMPTS; poll += 1) {
    const job = await api.renderJob(projectId, renderJobId);
    if (job.render_job_id !== renderJobId || job.manifest_id !== manifestId) {
      throw new ApiError(409, "渲染任务身份不匹配", "SUBMISSION_RENDER_JOB_IDENTITY_MISMATCH");
    }
    if (job.status === "completed") {
      if (!job.output_id) {
        throw new ApiError(409, "渲染任务完成但缺少产物", "SUBMISSION_RENDER_OUTPUT_MISSING");
      }
      return job.output_id;
    }
    if (job.status === "failed") {
      throw new ApiError(409, "渲染失败，请重试", job.error_code || "SUBMISSION_RENDER_FAILED");
    }
    if (poll + 1 < SUBMISSION_OUTPUT_POLL_ATTEMPTS) {
      await new Promise((resolve) => window.setTimeout(resolve, SUBMISSION_OUTPUT_POLL_INTERVAL_MS));
    }
  }
  throw new ApiError(504, "渲染任务超时，请重试", "SUBMISSION_RENDER_TIMEOUT");
}

export async function exportSubmission(
  id: string,
  format: "docx" | "pdf",
  attempt: SubmissionExportAttempt,
): Promise<void> {
  const manifest = await api.createManifest(id, format, attempt.createManifest);
  const sha = manifest.content_sha256;
  const manifestId = manifest.manifest_id;
  if (!sha || !manifestId) throw new ApiError(409, "manifest 未返回 manifest_id/content_sha256");
  const rendered = await api.renderManifest(id, manifestId, sha, attempt.renderManifest);
  if (rendered.status !== "queued" || rendered.manifest_id !== manifestId || !rendered.render_job_id) {
    throw new ApiError(409, "render 未返回匹配的 queued 任务", "SUBMISSION_RENDER_RESPONSE_INVALID");
  }
  const outputId = await waitForSubmissionOutput(id, manifestId, rendered.render_job_id);
  await downloadSubmission(id, outputId);
}

export function slugify(s: string): string {
  const n = s
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9\u4e00-\u9fff]+/g, "-")
    .replace(/^-|-$/g, "");
  return n || `item-${Date.now().toString(36)}`;
}

export function statusLabel(d: Derived): string {
  if (d.extract_running) return "正在抽条款";
  if (d.match_running) return "正在匹配";
  if (d.unconfirmed_drafts > 0) return `${d.unconfirmed_drafts} 条待确认`;
  if (d.has_files && !d.files_ready) return "文件解析中";
  if (d.has_picks) return "已勾选产品";
  if (d.files_ready) return "待审条款";
  return "待上传";
}
