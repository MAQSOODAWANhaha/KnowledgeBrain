import type { BidDoc, Clause, MatchUnit } from "../api";

export const NIL = "00000000-0000-0000-0000-000000000000";

export const KIND_LABEL: Record<string, string> = {
  technical: "技术",
  qualification: "资格",
  service: "服务",
  pricing: "报价结构",
  schedule_delivery: "交付",
  schedule_payment: "付款",
  evaluation: "评标",
  procedural: "程序",
};

export function kindLabel(kind: string): string {
  return KIND_LABEL[kind] || kind;
}

export function familyLabel(family: string | null): string {
  if (family === "technical") return "技术匹配";
  if (family === "commercial") return "商务匹配";
  return "不进匹配";
}

export function fileLabel(s: string): string {
  if (s === "completed") return "已解析";
  if (s === "processing") return "解析中";
  if (s === "pending") return "排队解析";
  if (s === "failed") return "解析失败";
  return s;
}

export function explainFileError(message: string, fileName?: string): string {
  const msg = message.trim();
  if (!msg) return "";
  const lower = msg.toLowerCase();
  if (lower.includes("failed to parse") || lower.includes("cannot parse")) {
    const name = fileName || msg.replace(/^failed to parse:\s*/i, "");
    if (/\.doc$/i.test(name) && !/\.docx$/i.test(name)) {
      return `无法解析 ${name}。请另存为 .docx 后再上传。`;
    }
    return `无法解析 ${name || "该文件"}。请换 PDF / Word(.docx) / Markdown 再试。`;
  }
  return msg;
}

export function fileStage(doc: BidDoc): {
  label: string;
  tone: "pine" | "amber" | "rose" | "gray";
  desc: string;
  retryable: boolean;
} {
  const err = explainFileError(doc.error_code || "", doc.file_name);
  if (doc.parse_status === "failed") {
    return { label: "解析失败", tone: "rose", desc: err || "解析失败，可重试或换格式。", retryable: true };
  }
  if (doc.parse_status === "pending") {
    return { label: "排队解析", tone: "gray", desc: "已入库，等待转换。", retryable: false };
  }
  if (doc.parse_status === "processing") {
    return { label: "解析中", tone: "amber", desc: "正在转成可抽取的文本。", retryable: false };
  }
  return { label: "已解析", tone: "pine", desc: "文本已就绪，可确认事实与条款。", retryable: false };
}

export function partTitle(key: string, units: MatchUnit[]): string {
  if (key === "1") return "① 项目概况";
  if (key === "3") return "③ 总体方案";
  if (key === "4") return "④ 公司资质";
  if (key === "5") return "⑤ 偏离与缺件";
  if (key === "6:letter") return "⑥ 投标函";
  if (key === "6:authorization") return "⑥ 授权材料";
  if (key === "6:quote") return "⑥ 报价表";
  if (key === "6:implementation_plan") return "⑥ 实施计划";
  if (key === "6:procedural") return "⑥ 程序检查";
  if (key === "2:unsectioned") return "② 未归段";
  if (key.startsWith("2:")) {
    const id = key.slice(2);
    const u = units.find((x) => x.id === id);
    return `② ${u?.heading_path || "技术单元"}`;
  }
  return key;
}

export function catalogKeys(required: string[], units: MatchUnit[]): string[] {
  if (required.length) return required;
  const twos = units
    .filter((u) => u.kind === "technical" || u.kind === "unsectioned")
    .map((u) => (u.id && u.id !== NIL ? `2:${u.id}` : "2:unsectioned"));
  return ["1", ...twos, "3", "4", "5", "6:letter", "6:authorization", "6:quote", "6:implementation_plan", "6:procedural"];
}

export function liveClauses(clauses: Clause[], view: string): Clause[] {
  const open = clauses.filter((c) => c.status !== "superseded");
  if (view === "pending") return open.filter((c) => c.status === "draft");
  if (view === "confirmed") return open.filter((c) => c.status === "confirmed");
  if (view === "commercial") return open.filter((c) => c.family === "commercial");
  if (view === "unsectioned") return open.filter((c) => c.kind === "technical");
  if (view === "procedural") return open.filter((c) => c.kind === "procedural");
  return open;
}

export function shanghaiEndOfDay(date: string): string {
  return `${date}T23:59:59+08:00`;
}
