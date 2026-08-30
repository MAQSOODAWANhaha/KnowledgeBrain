import { CandidateReview } from "./CandidateReview";
import { DocumentCanvas } from "./DocumentCanvas";
import type { BidV2Session, BidV2State } from "./session";

export function outlineProgressText(state: BidV2State): string {
  const outline = state.asyncRequests.find(
    (request) =>
      request.kind === "OutlineGenerate" && request.status === "pending",
  );
  const label =
    outline?.progress?.label ||
    (state.preparingOutline ? "正在生成大纲" : "");
  const detail = outline?.progress?.detail;
  const mapped = detail?.mapped_batches;
  const total = detail?.total_batches;
  const phase = detail?.phase;
  const attempt = detail?.attempt;
  const maxAttempts = detail?.max_attempts;
  if (!label) return "";
  if (
    phase === "retrying" &&
    typeof attempt === "number" &&
    typeof maxAttempts === "number"
  ) {
    return `自动重试 ${attempt}/${maxAttempts} · ${label}`;
  }
  if (typeof mapped === "number" && typeof total === "number") {
    return `${label} ${mapped}/${total}`;
  }
  if (phase === "collecting") {
    return "复核招标结构与冲突条款";
  }
  const requirementsDone = detail?.requirements_done;
  const requirementsTotal = detail?.requirements_total;
  if (
    phase === "routing" &&
    typeof requirementsDone === "number" &&
    typeof requirementsTotal === "number"
  ) {
    return `复核条款 ${requirementsDone}/${requirementsTotal}`;
  }
  const phaseLabels: Record<string, string> = {
    reducing: "正在汇总招标结构",
    drafting: "正在生成大纲章节",
    verifying: "正在校验大纲",
    repairing: "正在修复大纲结构",
    publishing: "正在发布大纲候选",
  };
  if (phase && phaseLabels[phase]) return phaseLabels[phase];
  if (typeof total === "number") return `${label} 0/${total}`;
  return label;
}

export function AuthoringShell({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const progress = outlineProgressText(state);
  const pending =
    state.preparingOutline ||
    state.asyncRequests.some(
      (request) =>
        request.status === "pending" &&
        (request.kind === "OutlineGenerate" ||
          request.kind === "ContentGenerate"),
    );
  return (
    <>
      {pending && (
        <div className="banner" data-testid="authoring-pending">
          {progress || "正在生成…"}，可继续改树和正文。
        </div>
      )}
      <CandidateReview session={session} state={state} />
      <DocumentCanvas session={session} state={state} />
    </>
  );
}
