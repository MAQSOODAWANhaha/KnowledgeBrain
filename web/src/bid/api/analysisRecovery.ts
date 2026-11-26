import type { MutationAttempt } from "../../api";
import { v2Request } from "./http";
import { projectPath } from "./workbench";

export type AnalysisRecoveryView = {
  run_id: string; source_version: number; domain_revision: number; status: string;
  failed_command_id: string | null; can_retry: boolean; blocker: string | null;
  notice: string; saved_responses: number;
  /** Receipt reason of the failed command, and the recorded model-call errors it ran into. */
  failure_reason?: string | null;
  failure_causes?: Array<{ error_type: string; category: FailureCategory; count: number }>;
};
export type FailureCategory = "uncertain" | "infrastructure" | "configuration" | "semantic" | "other";
export type AnalysisRetry = {
  run_id: string; source_version: number; domain_revision: number; failed_command_id: string; reason: string;
};
export const analysisRecoveryApi = {
  async read(project: string, signal?: AbortSignal): Promise<AnalysisRecoveryView | null> {
    return (await v2Request<{ recovery: AnalysisRecoveryView | null }>(`${projectPath(project)}/analysis/recovery`, { signal })).data.recovery ?? null;
  },
  async retry(project: string, body: AnalysisRetry, attempt: MutationAttempt): Promise<void> {
    await v2Request(`${projectPath(project)}/analysis/retry`, { method: "POST", body: JSON.stringify(body) }, { attempt });
  },
};
