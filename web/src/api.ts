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
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function req<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  if (init.body && !(init.body instanceof FormData) && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  const res = await fetch(path, { ...init, headers });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  const data = text ? (JSON.parse(text) as unknown) : null;
  if (!res.ok) {
    const obj = data && typeof data === "object" ? (data as { message?: string; error?: { message?: string } }) : null;
    const msg = obj?.error?.message || obj?.message || res.statusText;
    throw new ApiError(res.status, msg || "请求失败");
  }
  return data as T;
}

export type Project = {
  id: string;
  title: string;
  owner_name: string;
  expires_at: string | null;
  status: string;
  ended_at: string | null;
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
  text: string;
  raw_text: string;
  family: string;
  must: boolean;
  status: string;
  family_conflict?: boolean;
  deviate: boolean;
  deviate_note: string;
  section_id?: string | null;
  assessment?: string;
  unit_id?: string;
  suggestion?: string;
  hit_outcome?: string;
  hit_file?: string;
};

export type ExtractDiagnostics = {
  fallback_reasons?: string[];
  failed_spans?: string[];
  coverage?: {
    candidate_spans?: number;
    covered_spans?: number;
    uncovered_spans?: string[];
    ambiguous_clauses?: number;
  };
};

export type ExtractRun = {
  status: string;
  extractor_mode: string;
  error_message: string;
  failed_documents?: number;
  partial_failure?: boolean;
  diagnostics?: ExtractDiagnostics;
};

export type BookletPart = {
  key: string;
  markdown: string;
  stale: boolean;
  generated_at: string | null;
  edited_at: string | null;
};

export type MatchUnit = {
  id: string | null;
  heading_path: string;
  kind?: string;
  technical_count?: number;
  prev_id?: string | null;
  extract_status?: string;
  error_message?: string;
  retry_status?: string;
};

export type BidDoc = {
  id: string;
  file_name: string;
  parse_status: string;
  multimodal_status?: string;
  multimodal_error?: string;
  error_message?: string;
  object_key: string;
};

export type Candidate = {
  product_id: string;
  product_title: string;
  matched_version_id: string;
  matched_version_label: string;
  score: number;
  coverage: number;
  unmet_must: string[];
};

export type Pick = {
  product_id: string;
  unit_id?: string;
  version_id: string;
  score: number;
  coverage: number;
  clauses: unknown;
};

export type Shot = {
  id: string;
  clause_id: string;
  product_id: string;
  source: string;
  object_key: string;
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

export type Doc = {
  id: string;
  title: string;
  file_name: string;
  parse_status: string | { [k: string]: unknown };
  index_ready: boolean;
  object_key: string;
};

export const api = {
  login: (email: string, password: string) =>
    req<{ token: string; user_id: string }>("/api/v1/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  me: () => req<{ id: string; email: string }>("/api/v1/me"),
  bids: () => req<Project[]>("/api/v1/bids"),
  createBid: (body: { title: string; owner_name: string; expires_at?: string | null }) =>
    req<Project>("/api/v1/bids", { method: "POST", body: JSON.stringify(body) }),
  bid: (id: string) => req<{ project: Project; derived: Derived; latest_extract?: ExtractRun | null }>(`/api/v1/bids/${id}`),
  endBid: (id: string) => req<void>(`/api/v1/bids/${id}`, { method: "POST" }),
  docs: (id: string) => req<{ documents: BidDoc[] }>(`/api/v1/bids/${id}/documents`),
  uploadDoc: (id: string, file: File) => {
    const fd = new FormData();
    fd.set("file", file);
    return req<{ id: string }>(`/api/v1/bids/${id}/documents`, { method: "POST", body: fd });
  },
  retryDoc: (id: string, did: string) =>
    req<void>(`/api/v1/bids/${id}/documents/${did}/retry`, { method: "POST" }),
  retrySection: (id: string, sectionId: string) =>
    req<void>(`/api/v1/bids/${id}/sections/${sectionId}/retry`, { method: "POST" }),
  deleteDoc: (id: string, did: string) =>
    req<void>(`/api/v1/bids/${id}/documents/${did}`, { method: "DELETE" }),
  clauses: (id: string, include = false) =>
    req<Clause[]>(`/api/v1/bids/${id}/clauses?include_superseded=${include}`),
  patchClause: (id: string, cid: string, body: Partial<Clause>) =>
    req<void>(`/api/v1/bids/${id}/clauses/${cid}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  addClause: (id: string, body: { text: string; family: string; must: boolean; section_id?: string | null }) =>
    req<{ id: string }>(`/api/v1/bids/${id}/clauses`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  reextract: (id: string) => req<void>(`/api/v1/bids/${id}/extract`, { method: "POST" }),
  rematch: (id: string) => req<void>(`/api/v1/bids/${id}/match`, { method: "POST" }),
  units: (id: string) => req<{ units: MatchUnit[] }>(`/api/v1/bids/${id}/units`),
  picks: (id: string, unitId: string) =>
    req<{ picks: Pick[]; candidates: Candidate[] }>(
      `/api/v1/bids/${id}/picks?unit_id=${encodeURIComponent(unitId)}`,
    ),
  pick: (id: string, product_id: string, unitId: string) =>
    req<void>(`/api/v1/bids/${id}/picks`, {
      method: "POST",
      body: JSON.stringify({ product_id, unit_id: unitId }),
    }),
  unpick: (id: string, pid: string, unitId: string) =>
    req<void>(`/api/v1/bids/${id}/picks/${pid}?unit_id=${encodeURIComponent(unitId)}`, {
      method: "DELETE",
    }),
  mergeSection: (id: string, sid: string, into: string) =>
    req<void>(`/api/v1/bids/${id}/sections/${sid}/merge`, {
      method: "POST",
      body: JSON.stringify({ into }),
    }),
  shots: (id: string) => req<{ shots: Shot[] }>(`/api/v1/bids/${id}/shots`),
  uploadShot: (id: string, fields: { clause_id: string; product_id: string; version_id?: string; file: File }) => {
    const fd = new FormData();
    fd.set("clause_id", fields.clause_id);
    fd.set("product_id", fields.product_id);
    if (fields.version_id) fd.set("version_id", fields.version_id);
    fd.set("file", fields.file);
    return req<{ id: string }>(`/api/v1/bids/${id}/shots`, { method: "POST", body: fd });
  },
  deleteShot: (id: string, sid: string) =>
    req<void>(`/api/v1/bids/${id}/shots/${sid}`, { method: "DELETE" }),
  booklet: (id: string) => req<{ parts: BookletPart[] }>(`/api/v1/bids/${id}/booklet`),
  saveBooklet: (id: string, key: string, markdown: string) =>
    req<void>(`/api/v1/bids/${id}/booklet/${encodeURIComponent(key)}`, {
      method: "PUT",
      body: JSON.stringify({ markdown }),
    }),
  regenBooklet: (id: string, key: string) =>
    req<BookletPart>(`/api/v1/bids/${id}/booklet/${encodeURIComponent(key)}/regenerate`, {
      method: "POST",
    }),
  workspaces: () => req<Workspace[]>("/api/v1/workspaces"),
  createWorkspace: (body: { name: string; slug: string; kind?: string }) =>
    req<Workspace>("/api/v1/workspaces", { method: "POST", body: JSON.stringify(body) }),
  products: (wid: string) => req<Product[]>(`/api/v1/workspaces/${wid}/products`),
  createProduct: (wid: string, body: { name: string; slug: string; kind?: string }) =>
    req<Product>(`/api/v1/workspaces/${wid}/products`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  createVersion: (pid: string, label: string) =>
    req<{ id: string }>(`/api/v1/products/${pid}/versions`, {
      method: "POST",
      body: JSON.stringify({ label, make_current: true }),
    }),
  documents: (pid: string, vid: string) =>
    req<Doc[]>(`/api/v1/products/${pid}/versions/${vid}/documents`),
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

export async function downloadExport(id: string, format: "docx" | "pdf", regenerateStale = false): Promise<void> {
  const headers = new Headers();
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  const q = new URLSearchParams({ format });
  if (regenerateStale) q.set("regenerate_stale", "true");
  const res = await fetch(`/api/v1/bids/${id}/export?${q}`, { headers });
  if (!res.ok) {
    let msg = "导出失败";
    try {
      const j = (await res.json()) as { message?: string; error?: { message?: string } };
      if (j.error?.message) msg = j.error.message;
      else if (j.message) msg = j.message;
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
  a.download = m?.[1] ?? (format === "pdf" ? "定稿.pdf" : "应答卷.docx");
  a.click();
  URL.revokeObjectURL(a.href);
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
