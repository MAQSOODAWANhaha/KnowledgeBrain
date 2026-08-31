import {
  ApiError,
  NetworkTransportError,
  createMutationAttempt,
  token,
  type MutationAttempt,
} from "../../api";

export type V2RequestOptions = {
  attempt?: MutationAttempt;
  ifMatch?: string | null;
  signal?: AbortSignal;
};

export type V2Response<T> = {
  data: T;
  etag: string | null;
  status: number;
};

function stripEtag(value: string | null): string | null {
  if (!value) return null;
  return value.replace(/^W\//, "").replaceAll('"', "");
}

function readError(status: number, data: unknown, fallback: string): ApiError {
  const obj =
    data && typeof data === "object"
      ? (data as {
          message?: string;
          error?: {
            message?: string;
            code?: string;
            request_artifact_id?: string;
            details?: {
              request_artifact_id?: string;
              request_revision?: number;
              frozen_input_sha256?: string;
              retry_same_idempotency_key?: boolean;
            };
          };
          request_artifact_id?: string;
        })
      : null;
  const msg = obj?.error?.message || obj?.message || fallback;
  const code = obj?.error?.code;
  const requestArtifactId =
    obj?.error?.details?.request_artifact_id ||
    obj?.error?.request_artifact_id ||
    obj?.request_artifact_id;
  const error = new ApiError(status, msg || "请求失败", code);
  if (requestArtifactId) {
    const queueIdentity = {
      request_artifact_id: requestArtifactId,
      request_revision: obj?.error?.details?.request_revision,
      frozen_input_sha256: obj?.error?.details?.frozen_input_sha256,
      retry_same_idempotency_key:
        obj?.error?.details?.retry_same_idempotency_key === true,
    };
    (
      error as ApiError & {
        requestArtifactId?: string;
        queueRequestIdentity?: typeof queueIdentity;
      }
    ).requestArtifactId = requestArtifactId;
    (
      error as ApiError & { queueRequestIdentity?: typeof queueIdentity }
    ).queueRequestIdentity = queueIdentity;
  }
  return error;
}

export async function v2Request<T>(
  path: string,
  init: RequestInit = {},
  opts: V2RequestOptions = {},
): Promise<V2Response<T>> {
  const headers = new Headers(init.headers);
  const auth = token();
  if (auth) headers.set("Authorization", `Bearer ${auth}`);
  const method = (init.method || "GET").toUpperCase();
  if (
    ["POST", "PUT", "PATCH", "DELETE"].includes(method) &&
    !headers.has("Idempotency-Key")
  ) {
    headers.set(
      "Idempotency-Key",
      opts.attempt?.idempotencyKey ?? createMutationAttempt().idempotencyKey,
    );
  }
  if (opts.ifMatch) headers.set("If-Match", opts.ifMatch);
  if (
    init.body &&
    !(init.body instanceof FormData) &&
    !headers.has("Content-Type")
  ) {
    headers.set("Content-Type", "application/json");
  }
  let res: Response;
  try {
    res = await fetch(path, {
      ...init,
      headers,
      signal: opts.signal ?? init.signal,
    });
  } catch (error) {
    throw new NetworkTransportError(error);
  }
  const etag = stripEtag(res.headers.get("ETag"));
  if (res.status === 204) return { data: undefined as T, etag, status: 204 };
  let text: string;
  try {
    text = await res.text();
  } catch (error) {
    throw new NetworkTransportError(error);
  }
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch (error) {
      if (["POST", "PUT", "PATCH", "DELETE"].includes(method)) {
        throw new NetworkTransportError(error);
      }
      throw new ApiError(res.status, res.statusText || "响应不是 JSON");
    }
  }
  if (!res.ok) throw readError(res.status, data, res.statusText);
  return { data: data as T, etag, status: res.status };
}

export async function v2Blob(
  path: string,
  opts: V2RequestOptions = {},
): Promise<Blob> {
  const headers = new Headers();
  const auth = token();
  if (auth) headers.set("Authorization", `Bearer ${auth}`);
  if (opts.ifMatch) headers.set("If-Match", opts.ifMatch);
  let res: Response;
  try {
    res = await fetch(path, { headers, signal: opts.signal });
  } catch (error) {
    throw new NetworkTransportError(error);
  }
  if (!res.ok) {
    let text: string;
    try {
      text = await res.text();
    } catch (error) {
      throw new NetworkTransportError(error);
    }
    let data: unknown = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      data = null;
    }
    throw readError(res.status, data, "下载失败");
  }
  return res.blob();
}
