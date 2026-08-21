import type { BookletPart, Clause, MatchUnit } from "../api";

export const NIL = "00000000-0000-0000-0000-000000000000";

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

export function fileStage(doc: {
  parse_status: string;
  multimodal_status?: string;
  multimodal_error?: string;
  error_message?: string;
  extract_status?: string | null;
  extract_error?: string | null;
  clause_count?: number;
  file_name: string;
}): { label: string; tone: "pine" | "amber" | "rose" | "gray"; desc: string; retryable: boolean } {
  const err = explainFileError(doc.error_message || "", doc.file_name);
  if (doc.parse_status === "failed" || (doc.parse_status === "pending" && err)) {
    return { label: "解析失败", tone: "rose", desc: err || "解析失败，可重试或换格式。", retryable: true };
  }
  if (doc.parse_status === "pending") {
    return { label: "排队解析", tone: "gray", desc: "已入库，等待转换。", retryable: false };
  }
  if (doc.parse_status === "processing") {
    if (doc.multimodal_status === "running") {
      return { label: "处理图像", tone: "amber", desc: "正在识别文件中的图片。", retryable: false };
    }
    if (doc.multimodal_status === "failed") {
      return {
        label: "图像失败",
        tone: "rose",
        desc: explainFileError(doc.multimodal_error || "图像处理失败", doc.file_name),
        retryable: true,
      };
    }
    return { label: "解析中", tone: "amber", desc: "正在转成可抽取的文本。", retryable: false };
  }
  if ((doc.error_message || "").includes("conversion_quality=thin")) {
    return {
      label: "转换偏瘦",
      tone: "amber",
      desc: "转换文本过短或缺少标题，抽取可能漏条款。可去评估手补。",
      retryable: false,
    };
  }
  if ((doc.error_message || "").includes("conversion_quality=tables_flat")) {
    return {
      label: "表格可能丢失",
      tone: "amber",
      desc: "Word/Excel 转换后没有表，抽取可能漏条款。可去评估手补。",
      retryable: false,
    };
  }
  if (doc.extract_status === "pending" || doc.extract_status === "running") {
    return { label: "抽条款中", tone: "amber", desc: "文本已就绪，正在抽商务 / 技术条款。", retryable: false };
  }
  if (doc.extract_status === "failed") {
    return {
      label: "抽取失败",
      tone: "rose",
      desc: doc.extract_error || "条款抽取失败，可重试。",
      retryable: true,
    };
  }
  const n = doc.clause_count ?? 0;
  if ((doc.extract_error || "").includes("partial_failure") || (doc.extract_error || "").includes("fallback")) {
    return {
      label: n > 0 ? `需复核 ${n}` : "需复核",
      tone: "amber",
      desc: "抽取用了规则兜底或覆盖不全，请在评估里重点复核。",
      retryable: true,
    };
  }
  if (n > 0) {
    return { label: `已抽出 ${n} 条`, tone: "pine", desc: "解析完成，可去评估确认条款。", retryable: false };
  }
  return { label: "已解析", tone: "pine", desc: "文本已就绪，等待抽取或可去评估手补。", retryable: false };
}

export function suggestionLabel(s?: string): string {
  if (s === "cover") return "覆盖";
  if (s === "pending") return "待勾选";
  if (s === "need_rematch") return "需重配";
  if (s === "unmet" || s === "uncovered") return "未覆盖";
  return s || "—";
}

export function assessmentLabel(s?: string): string {
  if (s === "meet") return "满足";
  if (s === "partial") return "部分";
  if (s === "deviate") return "偏离";
  if (s === "fail") return "不响应";
  return "未评";
}

export function partTitle(key: string, units: MatchUnit[]): string {
  if (key === "1") return "① 扉页";
  if (key === "3") return "③ 偏离表";
  if (key === "4") return "④ 资格材料";
  if (key === "5") return "⑤ 商务缺件";
  if (key === "2:unsectioned") return "② 未归段";
  if (key.startsWith("2:")) {
    const id = key.slice(2);
    const u = units.find((x) => x.id === id);
    return `② ${u?.heading_path || "技术段"}`;
  }
  return key;
}

export function unitIdForView(view: string): string | null {
  if (view === "commercial" || view === "booklet" || view === "files") return null;
  if (view === "unsectioned") return NIL;
  return view;
}

export function bookletKeyFor(view: string, part: string, selected: string | null, clauses: Clause[]): string {
  if (view === "booklet") return part;
  if (view === "commercial") {
    const hit = clauses.find((c) => c.id === selected)?.hit_outcome;
    return hit === "miss" ? "5" : "4";
  }
  if (view === "unsectioned") return "2:unsectioned";
  return `2:${view}`;
}

export function liveClauses(clauses: Clause[], view: string): Clause[] {
  const open = clauses.filter((c) => c.status !== "superseded");
  if (view === "commercial") return open.filter((c) => c.family === "commercial");
  if (view === "booklet" || view === "files") return [];
  if (view === "unsectioned") return open.filter((c) => c.family === "technical" && (!c.unit_id || c.unit_id === NIL));
  return open.filter((c) => c.family === "technical" && c.unit_id === view);
}

export function catalogKeys(units: MatchUnit[], booklet: BookletPart[]): string[] {
  const tech = units.filter((u) => u.kind === "technical");
  const twos = [
    ...tech.map((u) => `2:${u.id}`),
    "2:unsectioned",
  ].filter((key) => booklet.some((p) => p.key === key));
  return ["1", ...twos, "3", "4", "5"];
}
