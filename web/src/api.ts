import { randomUuid } from "./uuid";

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
  return { idempotencyKey: randomUuid() };
}

async function req<T>(
  path: string,
  init: RequestInit = {},
  attempt?: MutationAttempt,
): Promise<T> {
  const headers = new Headers(init.headers);
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  const method = (init.method || "GET").toUpperCase();
  if (
    ["POST", "PUT", "PATCH", "DELETE"].includes(method) &&
    !headers.has("Idempotency-Key")
  ) {
    headers.set(
      "Idempotency-Key",
      attempt?.idempotencyKey ?? createMutationAttempt().idempotencyKey,
    );
  }
  if (
    init.body &&
    !(init.body instanceof FormData) &&
    !headers.has("Content-Type")
  ) {
    headers.set("Content-Type", "application/json");
  }
  const res = await fetch(path, { ...init, headers });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      throw new ApiError(res.status, res.statusText || "响应不是 JSON");
    }
  }
  if (!res.ok) {
    const obj =
      data && typeof data === "object"
        ? (data as {
            message?: string;
            error?: { message?: string; code?: string };
          })
        : null;
    const msg = obj?.error?.message || obj?.message || res.statusText;
    throw new ApiError(res.status, msg || "请求失败", obj?.error?.code);
  }
  return data as T;
}

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
  quote: (id: string) => req<QuoteState>(`/api/v1/bids/${id}/quote`),
  createQuoteDraft: (
    id: string,
    body: { tax_mode: string; title: string; notes?: string | null },
  ) =>
    req<QuoteState>(`/api/v1/bids/${id}/quote/draft`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  patchQuote: (
    id: string,
    body: {
      expected_edit_version: number;
      tax_mode: string;
      title: string;
      notes?: string | null;
    },
  ) =>
    req<QuoteState>(`/api/v1/bids/${id}/quote`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
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
  ) =>
    req(`/api/v1/bids/${id}/quote/lines/${lineId}`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),
  deleteQuoteLine: (
    id: string,
    lineId: string,
    expected_edit_version: number,
  ) =>
    req(`/api/v1/bids/${id}/quote/lines/${lineId}`, {
      method: "DELETE",
      body: JSON.stringify({ expected_edit_version }),
    }),
  previewQuote: (id: string, signal?: AbortSignal) =>
    req<{ net_total?: string; tax_total?: string; gross_total?: string }>(
      `/api/v1/bids/${id}/quote/preview`,
      { signal },
    ),
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
  ) =>
    req(`/api/v1/bids/${id}/quote/finalize`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  reopenQuote: (
    id: string,
    body: {
      expected_snapshot_id: string;
      expected_fact_revision: number;
      expected_pricing_revision: number;
    },
  ) =>
    req(`/api/v1/bids/${id}/quote/reopen`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  workspaces: () => req<Workspace[]>("/api/v1/workspaces"),
  createWorkspace: (body: { name: string; slug: string; kind?: string }) =>
    req<Workspace>("/api/v1/workspaces", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  products: (wid: string) =>
    req<Product[]>(`/api/v1/workspaces/${wid}/products`),
  createProduct: (
    wid: string,
    body: { name: string; slug: string; kind?: string },
  ) =>
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
  documentContent: (id: string) =>
    req<DocContent>(`/api/v1/documents/${id}/content`),
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
  const res = await fetch(`/api/v1/files?key=${encodeURIComponent(key)}`, {
    headers,
  });
  if (!res.ok) throw new ApiError(res.status, "读文件失败");
  return URL.createObjectURL(await res.blob());
}

export function slugify(s: string): string {
  const n = s
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9\u4e00-\u9fff]+/g, "-")
    .replace(/^-|-$/g, "");
  return n || `item-${Date.now().toString(36)}`;
}
