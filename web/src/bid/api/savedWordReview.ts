import type { MutationAttempt } from "../../api";
import { v2Request } from "./http";
import { projectPath, type TaskProgressView } from "./workbench";

export type SavedWordVersion = { version_id: string; docx_sha256: string; revision: number };
export type SavedReviewAnalysisIdentity = { run_id: string; source_version: number; domain_revision: number };
export type SavedWordReviewReport = {
  version_id: string; docx_sha256: string; reason: string;
  analysis_identity?: SavedReviewAnalysisIdentity;
  findings: Array<{ code: string; detail: string; task_ids: string[]; content_ids: string[]; suggestion: string; can_revise: false }>;
  gaps: Array<{ code: string; detail: string; chapter_id?: string | null }>;
  records: Array<{ task_id: string; chapter_id: string; title: string }>;
  record_count: number; reviewed_record_count: number;
};
export type SavedWordReview = {
  execution_id: string; status: string; operation: "saved_review"; error?: unknown;
  version_id: string; docx_sha256: string; report?: SavedWordReviewReport | null;
  analysis_identity?: SavedReviewAnalysisIdentity; sources_current?: boolean | null;
  source_status_issue?: { code: string; detail: string };
  progress?: TaskProgressView;
  usage?: { sent_requests: number; retry_requests: number; total_tokens: number; unknown_usage_requests: number };
};
export type SavedWordReviewBody = { docx_sha256: string; reason: string };
type Receipt = { review: SavedWordReview | null };
const reviewPath = (project: string, version: string) => `${projectPath(project)}/docx/versions/${encodeURIComponent(version)}/review`;

export const savedWordReviewApi = {
  async read(project: string, version: string, signal?: AbortSignal): Promise<SavedWordReview | null> {
    return (await v2Request<Receipt>(reviewPath(project, version), { signal })).data.review;
  },
  async start(project: string, version: string, body: SavedWordReviewBody, attempt: MutationAttempt): Promise<SavedWordReview | null> {
    return (await v2Request<Receipt>(reviewPath(project, version), { method: "POST", body: JSON.stringify(body) }, { attempt })).data.review;
  },
};
