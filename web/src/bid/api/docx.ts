import { NetworkTransportError, type MutationAttempt } from "../../api";
import { v2Blob, v2Request } from "./http";

export type DocxIdentity = { version_id: string; docx_sha256: string };
export type DocxRoundBasis = {
  document_set_id: string; document_set_sha256: string;
  requirement_set_id: string; requirement_set_sha256: string;
};
export type DocxRoundInput = { basis: DocxRoundBasis; expected: DocxIdentity | null };
export type DocxRoundReceipt = DocxIdentity & { round_id: string; round_revision: number };
export type DocxRoundApi = {
  basis(workspace: string): Promise<DocxRoundBasis | null>;
  publish(workspace: string, input: DocxRoundInput, file: File, attempt: MutationAttempt): Promise<DocxRoundReceipt>;
};
export type DocxCurrent = DocxIdentity & {
  project_id: string;
  workspace_id: string;
  round_id: string;
  revision: number;
  editor: {
    key: string | null;
    base_version_id: string | null;
    pending_save_id: string | null;
    save_error: { kind: "callback" | "command"; code: number } | null;
  };
};
export type DocxOpened = {
  config: Record<string, unknown>;
  api_script_url: string;
  session: { editor_key: string; round_id: string; version_id: string };
};
export type DocxSave = { editor_key: string; expected: DocxIdentity };
export type DocxSaveReceipt = { save_id: string; dispatch: boolean; pending: boolean };
export type DocxSavedReceipt = DocxIdentity & {
  save_id: string; editor_key: string; round_id: string; parent_version_id: string; revision: number;
};
export type DocxApi = {
  current(workspace: string): Promise<DocxCurrent | null>;
  open(workspace: string, expected: DocxIdentity, attempt: MutationAttempt): Promise<DocxOpened>;
  save(workspace: string, body: DocxSave, attempt: MutationAttempt): Promise<DocxSaveReceipt>;
  saved(workspace: string, editorKey: string, saveId: string): Promise<DocxSavedReceipt | null>;
  download(workspace: string, version: string): Promise<Blob>;
};
const path = (workspace: string) => `/api/v2/submission-workspaces/${encodeURIComponent(workspace)}/docx`;
export const docxApi: DocxApi & DocxRoundApi = {
  async basis(workspace) { return (await v2Request<DocxRoundBasis | null>(`${path(workspace)}-rounds/basis`)).data; },
  async publish(workspace, input, file, attempt) {
    const body = new FormData();
    body.append("metadata", JSON.stringify(input));
    body.append("file", file);
    const receipt = (await v2Request<DocxRoundReceipt>(`${path(workspace)}-rounds`, { method: "POST", body }, { attempt })).data;
    if (!receipt || typeof receipt.round_id !== "string" || !receipt.round_id
      || typeof receipt.version_id !== "string" || !receipt.version_id
      || typeof receipt.docx_sha256 !== "string" || !/^[0-9a-f]{64}$/.test(receipt.docx_sha256)
      || !Number.isSafeInteger(receipt.round_revision) || receipt.round_revision < 1) {
      throw new NetworkTransportError(new Error("invalid DOCX round receipt"));
    }
    return receipt;
  },
  async current(workspace) { return (await v2Request<DocxCurrent | null>(`${path(workspace)}/current`)).data; },
  async open(workspace, expected, attempt) {
    const language = document.documentElement.lang || navigator.language;
    return (await v2Request<DocxOpened>(`${path(workspace)}/editor`,
      { method: "POST", body: JSON.stringify({ ...expected, ...(language ? { language } : {}) }) }, { attempt })).data;
  },
  async save(workspace, body, attempt) {
    return (await v2Request<DocxSaveReceipt>(`${path(workspace)}/editor/save`,
      { method: "POST", body: JSON.stringify(body) }, { attempt })).data;
  },
  async saved(workspace, editorKey, saveId) {
    return (await v2Request<DocxSavedReceipt | null>(`${path(workspace)}/editor/${encodeURIComponent(editorKey)}/saves/${encodeURIComponent(saveId)}`)).data;
  },
  download(workspace, version) { return v2Blob(`${path(workspace)}/versions/${encodeURIComponent(version)}/download`); },
};
