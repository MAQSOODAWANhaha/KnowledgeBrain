import type { TenderDocumentView } from "./api/types";

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
    return `无法解析 ${name || "该文件"}。请换 PDF / Word(.docx) / Excel / 图片再试。`;
  }
  return msg;
}

export function fileStage(
  doc: Pick<TenderDocumentView, "parse_status" | "error_code" | "file_name">,
): {
  label: string;
  tone: "pine" | "amber" | "rose" | "gray";
  desc: string;
  retryable: boolean;
} {
  const err = explainFileError(doc.error_code || "", doc.file_name);
  if (doc.parse_status === "failed") {
    return {
      label: "解析失败",
      tone: "rose",
      desc: err || "解析失败，可重试或换格式。",
      retryable: true,
    };
  }
  if (doc.parse_status === "pending") {
    return {
      label: "排队解析",
      tone: "gray",
      desc: "已入库，等待转换。",
      retryable: false,
    };
  }
  if (doc.parse_status === "processing") {
    return {
      label: "解析中",
      tone: "amber",
      desc: "正在抽取来源结构。",
      retryable: false,
    };
  }
  return {
    label: "已解析",
    tone: "pine",
    desc: "可冻结文件集并进入要求台账。",
    retryable: false,
  };
}

export function shanghaiEndOfDay(date: string): string {
  return `${date}T23:59:59+08:00`;
}

export const DOCUMENT_ROLE_LABEL: Record<string, string> = {
  primary_tender: "主招标文件",
  bid_format: "投标文件格式",
  technical_specification: "技术规范",
  commercial_requirement: "商务要求",
  bill_of_quantities: "工程量清单",
  contract: "合同",
  drawing: "图纸",
  clarification: "澄清",
  amendment: "补遗/修改",
  other_attachment: "其他附件",
};
