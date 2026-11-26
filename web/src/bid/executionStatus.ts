export const inProgress = (status?: string) => ["pending", "published", "running"].includes(status ?? "");

const labels: Record<string, string> = {
  pending: "等待执行", published: "排队中", running: "正在执行", failed: "执行失败",
  blocked: "需要处理", cancelled: "已取消", succeeded: "执行结束",
  "waiting-confirmation": "分析待确认", "analysis-ready": "分析已确认",
  waiting_confirmation: "分析待确认", analysis_ready: "分析已确认",
};

type Tone = "success" | "warning" | "error" | "info" | "neutral";
// One reading of the analysis step for the page header and project navigation.
export function analysisBadge(analysis?: { status: string; confirmed: boolean } | null): { label: string; tone: Tone } {
  if (!analysis) return { label: "尚未分析", tone: "neutral" };
  if (analysis.confirmed) return { label: "已确认", tone: "success" };
  if (inProgress(analysis.status)) return { label: "分析中", tone: "info" };
  if (analysis.status === "failed") return { label: "分析失败", tone: "error" };
  if (analysis.status === "cancelled") return { label: "已取消", tone: "neutral" };
  if (analysis.status === "blocked") return { label: "需要处理", tone: "warning" };
  return { label: "待人工确认", tone: "warning" };
}

export const executionStatus = (status?: string) => status ? labels[status] ?? "状态待确认" : "尚未开始";
