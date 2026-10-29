export const inProgress = (status?: string) => ["pending", "published", "running"].includes(status ?? "");

const labels: Record<string, string> = {
  pending: "等待执行", published: "排队中", running: "正在执行", failed: "执行失败",
  blocked: "需要处理", cancelled: "已取消", succeeded: "执行结束",
  "waiting-confirmation": "分析待确认", "analysis-ready": "分析已确认",
  waiting_confirmation: "分析待确认", analysis_ready: "分析已确认",
};

export const executionStatus = (status?: string) => status ? labels[status] ?? "状态待确认" : "尚未开始";
