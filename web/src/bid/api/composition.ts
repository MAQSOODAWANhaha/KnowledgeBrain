import { NetworkTransportError, type MutationAttempt } from "../../api";
import type { DocxRoundInput, DocxRoundReceipt } from "./docx";
import { v2Request } from "./http";

export type CompositionIdentity = { request_artifact_id: string; request_revision: number; frozen_input_sha256: string };
export type CompositionStatus = CompositionIdentity & {
  workspace_id: string; status: "pending" | "succeeded" | "failed";
  basis: DocxRoundInput["basis"]; expected: DocxRoundInput["expected"];
  error_code: string | null;
  result_identity: (DocxRoundReceipt & { composition_status: string }) | null;
  progress: { phase: string; sequence: number; attempt: number; detail: { turn?: number; sections?: number; reviewed?: boolean } } | null;
};
export type CompositionApi = {
  basis(workspace: string): Promise<DocxRoundInput["basis"] | null>;
  start(workspace: string, input: DocxRoundInput, attempt: MutationAttempt): Promise<CompositionIdentity>;
  latest(workspace: string): Promise<CompositionStatus | null>;
  status(workspace: string, request: string): Promise<CompositionStatus>;
};
const path = (workspace: string) => `/api/v2/submission-workspaces/${encodeURIComponent(workspace)}/docx-compositions`;
function identity(value: CompositionIdentity): boolean {
  return !!value && typeof value.request_artifact_id === "string" && !!value.request_artifact_id
    && Number.isSafeInteger(value.request_revision) && value.request_revision > 0
    && typeof value.frozen_input_sha256 === "string" && /^[0-9a-f]{64}$/.test(value.frozen_input_sha256);
}
function status(value: CompositionStatus, workspace: string): CompositionStatus {
  if (!identity(value) || value.workspace_id !== workspace || !["pending", "succeeded", "failed"].includes(value.status)
    || (value.status === "succeeded" && (!value.result_identity?.version_id || !value.result_identity.round_id
      || !/^[0-9a-f]{64}$/.test(value.result_identity.docx_sha256)))) {
    throw new NetworkTransportError(new Error("invalid composition status"));
  }
  return value;
}
export const compositionApi: CompositionApi = {
  async basis(workspace) { return (await v2Request<DocxRoundInput["basis"] | null>(`${path(workspace)}/basis`)).data; },
  async start(workspace, input, attempt) {
    const value = (await v2Request<CompositionIdentity>(path(workspace), { method: "POST", body: JSON.stringify(input) }, { attempt })).data;
    if (!identity(value)) throw new NetworkTransportError(new Error("invalid composition receipt"));
    return value;
  },
  async latest(workspace) {
    const value = (await v2Request<CompositionStatus | null>(`${path(workspace)}/latest`)).data;
    return value === null ? null : status(value, workspace);
  },
  async status(workspace, request) {
    const value = status((await v2Request<CompositionStatus>(`${path(workspace)}/${encodeURIComponent(request)}`)).data, workspace);
    if (value.request_artifact_id !== request) throw new NetworkTransportError(new Error("composition request changed"));
    return value;
  },
};
