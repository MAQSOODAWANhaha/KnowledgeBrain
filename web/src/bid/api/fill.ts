import { NetworkTransportError, type MutationAttempt } from "../../api";
import type { DocxIdentity, DocxRoundBasis } from "./docx";
import { v2Request } from "./http";

/** 填章请求：填的是**当前这一版**，所以 expected 不可为空。 */
export type FillInput = { basis: DocxRoundBasis; expected: DocxIdentity };
export type FillIdentity = { request_artifact_id: string; request_revision: number; frozen_input_sha256: string };
/** 进度只取宿主写在 checkpoint 里的三项，不在前端另算一套章数。 */
export type FillProgress = {
  phase: string; sequence: number; attempt: number;
  detail: {
    draft_stage?: string; boundary?: string; turn?: number;
    draft_chapters?: number; draft_filled?: number; draft_active_title?: string | null;
    /** 本次是用户叫停的，不是填完了。 */
    draft_stopped?: boolean;
  } | null;
};
export type FillStatus = FillIdentity & {
  workspace_id: string; status: "pending" | "succeeded" | "failed";
  error_code: string | null;
  result_identity: (DocxIdentity & { round_id: string; round_revision: number }) | null;
  progress: FillProgress | null;
};
export type FillApi = {
  basis(workspace: string): Promise<DocxRoundBasis | null>;
  start(workspace: string, input: FillInput, attempt: MutationAttempt): Promise<FillIdentity>;
  latest(workspace: string): Promise<FillStatus | null>;
  status(workspace: string, request: string): Promise<FillStatus>;
  /** 只记一次「停」的意向：任务在当前章收尾后自己停下并照常出稿。 */
  stop(workspace: string, request: string): Promise<void>;
};
const workspacePath = (workspace: string) => `/api/v2/submission-workspaces/${encodeURIComponent(workspace)}`;
function identity(value: FillIdentity): boolean {
  return !!value && typeof value.request_artifact_id === "string" && !!value.request_artifact_id
    && Number.isSafeInteger(value.request_revision) && value.request_revision > 0
    && typeof value.frozen_input_sha256 === "string" && /^[0-9a-f]{64}$/.test(value.frozen_input_sha256);
}
function status(value: FillStatus, workspace: string): FillStatus {
  if (!identity(value) || value.workspace_id !== workspace || !["pending", "succeeded", "failed"].includes(value.status)
    || (value.status === "succeeded" && !/^[0-9a-f]{64}$/.test(value.result_identity?.docx_sha256 ?? ""))) {
    throw new NetworkTransportError(new Error("invalid fill status"));
  }
  return value;
}
export const fillApi: FillApi = {
  async basis(workspace) {
    return (await v2Request<DocxRoundBasis | null>(`${workspacePath(workspace)}/docx-compositions/basis`)).data;
  },
  async start(workspace, input, attempt) {
    const value = (await v2Request<FillIdentity>(`${workspacePath(workspace)}/docx-fills`,
      { method: "POST", body: JSON.stringify(input) }, { attempt })).data;
    if (!identity(value)) throw new NetworkTransportError(new Error("invalid fill receipt"));
    return value;
  },
  async latest(workspace) {
    const value = (await v2Request<FillStatus | null>(`${workspacePath(workspace)}/docx-compositions/latest`)).data;
    return value === null ? null : status(value, workspace);
  },
  async status(workspace, request) {
    const value = status((await v2Request<FillStatus>(
      `${workspacePath(workspace)}/docx-compositions/${encodeURIComponent(request)}`)).data, workspace);
    if (value.request_artifact_id !== request) throw new NetworkTransportError(new Error("fill request changed"));
    return value;
  },
  async stop(workspace, request) {
    await v2Request(`${workspacePath(workspace)}/docx-fills/${encodeURIComponent(request)}/stop`, { method: "POST" });
  },
};
