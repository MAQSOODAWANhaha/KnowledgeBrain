import type { MutationAttempt } from "../../api";
import { v2Request } from "./http";
import { projectPath, type TenderItem, type TenderIssue } from "./workbench";

export type HumanVerdict = { issue_id: string; verdict: "acknowledged" | "dismissed"; reason: string };
export type ReviewIdentity = { run_id: string; source_version: number; domain_revision: number };
export type AnalysisReviewView = ReviewIdentity & {
  status: string; checklist_digest: string | null; sources_current: boolean;
  human_issue_ids: string[]; issues: TenderIssue[]; hard_blocking: boolean;
  correction: null | { base_items_digest: string; items: Array<{ pack_id: string; item: TenderItem & { item_id: string } }> };
};
export type AnalysisConfirmation = ReviewIdentity & {
  checklist_digest: string; issue_resolutions: { schema_version: 1; resolutions: HumanVerdict[] };
};
export type AnalysisBusinessFields = {
  text?: string; kind?: string; domains?: string[] | null; subject?: string | null; scope?: string | null;
  conditions?: string[] | null; exceptions?: string[] | null;
  numbers?: Array<{ value: string | number; unit: string; quote: string }> | null;
  scoring?: { method?: string; points?: number; max_points?: number } | null;
  consequence?: { effect?: string; trigger?: string } | null;
  submission?: { fields?: string[]; documents?: string[] } | null;
};
export type AnalysisCorrection = {
  run_id: string; source_version: number; expected_revision: number; base_items_digest: string;
  reason: string; changes: Array<{ pack_id: string; item_id: string } & AnalysisBusinessFields>;
};

export const analysisReviewApi = {
  async read(project: string, signal?: AbortSignal): Promise<AnalysisReviewView | null> {
    return (await v2Request<{ review: AnalysisReviewView | null }>(`${projectPath(project)}/analysis/review`, { signal })).data.review;
  },
  async confirm(project: string, body: AnalysisConfirmation, attempt: MutationAttempt): Promise<void> {
    await v2Request(`${projectPath(project)}/analysis/confirm`, { method: "POST", body: JSON.stringify(body) }, { attempt });
  },
  async correct(project: string, body: AnalysisCorrection, attempt: MutationAttempt): Promise<void> {
    await v2Request(`${projectPath(project)}/analysis/correct`, { method: "POST", body: JSON.stringify(body) }, { attempt });
  },
};
