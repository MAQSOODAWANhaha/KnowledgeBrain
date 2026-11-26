import type { HumanVerdict } from "./api/analysisReview";
import type { TenderIssue } from "./api/workbench";

export type Decision = { verdict: HumanVerdict["verdict"] | ""; reason: string };
export type IssueStatusFilter = "all" | "pending" | "decided";
export type BatchScope = "undecided" | "all";
export type HumanIssueEntry = { id: string; index: number; issue: TenderIssue | undefined; code: string };
export type IssueCodeSummary = { code: string; label: string; total: number; decided: number };

export const EMPTY_DECISION: Decision = { verdict: "", reason: "" };
export const ISSUE_PAGE_SIZE = 20;
export const CORRECTION_PAGE_SIZE = 20;

export const ISSUE_CODE_LABELS: Record<string, string> = {
  unresolved_cross_reference: "跨处引用未定位",
  unresolved_source_range: "来源范围未定位",
  evidence_quote_mismatch: "引文与原文不一致",
  missing_referenced_text: "被引用原文缺失",
  extraction_disposition_unresolved: "抽取处置未决",
  extraction_omission: "抽取遗漏",
  extraction_relation_unresolved: "条款关系未决",
  semantic_ambiguity: "语义歧义",
  unknown: "其他问题",
};

/** Formats an issue code into a concise Chinese label with raw code context when mapped. */
export function issueCodeLabel(code: string): string {
  const mapped = ISSUE_CODE_LABELS[code];
  if (!mapped) return code;
  return code === "unknown" ? mapped : `${mapped}（${code}）`;
}

/** Returns true when a human verdict has both a selected verdict and a non-empty reason. */
export function isDecisionComplete(decision: Decision | undefined): boolean {
  return !!decision?.verdict && !!decision.reason.trim();
}

/** Pairs ordered human_issue_ids with their TenderIssue records in linear time. */
export function buildHumanIssueEntries(humanIssueIds: string[], issues: TenderIssue[]): HumanIssueEntry[] {
  const byId = new Map<string, TenderIssue>();
  for (const issue of issues) {
    if (issue.issue_id && !byId.has(issue.issue_id)) byId.set(issue.issue_id, issue);
  }
  return humanIssueIds.map((id, index) => {
    const issue = byId.get(id);
    return { id, index, issue, code: issue?.code || "unknown" };
  });
}

/** Summarizes total and decided counts grouped by issue code, ordered by frequency. */
export function summarizeIssueCodes(entries: HumanIssueEntry[], decisions: Record<string, Decision>): IssueCodeSummary[] {
  const counts = new Map<string, { total: number; decided: number }>();
  for (const entry of entries) {
    let bucket = counts.get(entry.code);
    if (!bucket) {
      bucket = { total: 0, decided: 0 };
      counts.set(entry.code, bucket);
    }
    bucket.total += 1;
    if (isDecisionComplete(decisions[entry.id])) bucket.decided += 1;
  }
  return Array.from(counts.entries())
    .sort(([codeA, statsA], [codeB, statsB]) => statsB.total - statsA.total || codeA.localeCompare(codeB))
    .map(([code, stats]) => ({ code, label: issueCodeLabel(code), total: stats.total, decided: stats.decided }));
}

/** Filters human issues by issue code and decision completeness. */
export function filterHumanIssues(
  entries: HumanIssueEntry[],
  decisions: Record<string, Decision>,
  codeFilter: string,
  statusFilter: IssueStatusFilter,
): HumanIssueEntry[] {
  return entries.filter(entry => {
    if (codeFilter !== "all" && entry.code !== codeFilter) return false;
    if (statusFilter === "all") return true;
    const complete = isDecisionComplete(decisions[entry.id]);
    return statusFilter === "decided" ? complete : !complete;
  });
}

/** Applies a verdict and reason across filtered issues, skipping complete decisions unless overwriting. */
export function applyBatchDecision(
  decisions: Record<string, Decision>,
  targets: HumanIssueEntry[],
  verdict: HumanVerdict["verdict"],
  reason: string,
  scope: BatchScope,
): { next: Record<string, Decision>; updated: number } {
  let updated = 0;
  let next: Record<string, Decision> | null = null;
  for (const entry of targets) {
    if (scope === "undecided" && isDecisionComplete(decisions[entry.id])) continue;
    if (!next) next = { ...decisions };
    next[entry.id] = { verdict, reason };
    updated += 1;
  }
  return { next: next ?? decisions, updated };
}

/** Returns a clamped 1-based page slice for an item list. */
export function paginate<T>(items: T[], page: number, pageSize: number): { page: number; totalPages: number; slice: T[] } {
  const safeSize = Math.max(1, pageSize);
  const totalPages = Math.max(1, Math.ceil(items.length / safeSize));
  const clamped = Math.min(Math.max(1, Math.trunc(page) || 1), totalPages);
  const start = (clamped - 1) * safeSize;
  return { page: clamped, totalPages, slice: items.slice(start, start + safeSize) };
}
