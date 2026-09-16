import { NetworkTransportError, type MutationAttempt } from "../../api";
import { v2Blob, v2Request } from "./http";

export type ExportInput = { version_id: string; docx_sha256: string };
export type ExportOutput = { artifact_id: string; sha256: string; byte_length: number };
export type ExportPackage = {
  manifest_id: string; source: ExportInput;
  outputs: { docx: ExportOutput; pdf: ExportOutput };
  assessment_report_id: string; assessment_report_sha256: string;
};
export type ExportStatus = {
  request_artifact_id: string; request_revision: number; frozen_input_sha256: string;
  kind: "SubmissionExport"; status: "pending" | "succeeded" | "failed";
  result_identity: ExportPackage | null; error_code: string | null;
};
export type ExportApi = {
  start(workspace: string, input: ExportInput, attempt: MutationAttempt): Promise<ExportStatus>;
  status(workspace: string, request: string): Promise<ExportStatus>;
  list(workspace: string): Promise<ExportPackage[]>;
};
const path = (workspace: string) => `/api/v2/submission-workspaces/${encodeURIComponent(workspace)}`;
const sha = (value: unknown) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
export function validExportPackage(value: ExportPackage): boolean {
  const output = (item: ExportOutput) => !!item?.artifact_id && sha(item.sha256) && Number.isSafeInteger(item.byte_length) && item.byte_length > 0;
  return !!value?.manifest_id && !!value.source?.version_id && sha(value.source.docx_sha256)
    && output(value.outputs?.docx) && output(value.outputs?.pdf)
    && value.outputs.docx.sha256 === value.source.docx_sha256
    && !!value.assessment_report_id && sha(value.assessment_report_sha256);
}
function status(value: ExportStatus): ExportStatus {
  if (!value?.request_artifact_id || value.kind !== "SubmissionExport"
    || !Number.isSafeInteger(value.request_revision) || value.request_revision < 1 || !sha(value.frozen_input_sha256)
    || !["pending", "succeeded", "failed"].includes(value.status)
    || (value.status === "succeeded" && (!value.result_identity || !validExportPackage(value.result_identity)))) {
    throw new NetworkTransportError(new Error("invalid export receipt"));
  }
  return value;
}
export const exportsApi: ExportApi = {
  async start(workspace, input, attempt) {
    return status((await v2Request<ExportStatus>(`${path(workspace)}/exports`, {
      method: "POST", body: JSON.stringify({ version_id: input.version_id }),
    }, { attempt, ifMatch: input.docx_sha256 })).data);
  },
  async status(workspace, request) {
    const value = status((await v2Request<ExportStatus>(`${path(workspace)}/requests/${encodeURIComponent(request)}`)).data);
    if (value.request_artifact_id !== request) throw new NetworkTransportError(new Error("export request changed"));
    return value;
  },
  async list(workspace) {
    const values = (await v2Request<ExportPackage[]>(`${path(workspace)}/exports`)).data;
    if (!Array.isArray(values) || values.some(value => !validExportPackage(value))) throw new Error("invalid export packages");
    return values;
  },
};
export const downloadExport = (workspace: string, output: string) => v2Blob(`${path(workspace)}/exports/${encodeURIComponent(output)}/download`);
export const downloadExportReport = (workspace: string, manifest: string) => v2Blob(`${path(workspace)}/exports/${encodeURIComponent(manifest)}/assessment-report`);
