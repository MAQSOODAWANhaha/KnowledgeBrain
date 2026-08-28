import { AuthoringLogicError } from "./errors";
import type { InsertionAnchor } from "./mutations";

export type GenerateTarget = "node" | "subtree" | "workspace";
export type FillPolicy =
  | "empty_only"
  | "append_candidate"
  | "missing_requirements_only";
export type EvidenceMode = "system_proposed" | "user_pick_set";
export type ExportMode = "review_draft" | "submission";
export type ExportFormat = "docx" | "pdf";

export type OutlineCandidateRequest = {
  expected_workspace_revision_id: string;
  document_set_revision_id: string;
  document_set_sha256: string;
};

export type ContentCandidateRequest = {
  target: GenerateTarget;
  node_lineage_id?: string;
  fill_policy: FillPolicy;
  insertion_anchor?: InsertionAnchor | null;
  selection_mode: EvidenceMode;
  pick_set_artifact_id?: string;
  expected_workspace_revision_id: string;
};

export type AcceptCandidateRequest = {
  expected_workspace_revision_id: string;
  expected_workspace_sha256: string;
  operation_indexes?: number[];
  client_node_refs?: string[];
};

export type ExportRequest = {
  mode: ExportMode;
  format: ExportFormat;
  expected_workspace_revision_id: string;
  watermark?: { text: string } | null;
  include_risk_notices?: boolean;
  include_knowledge_provenance?: boolean;
};

export function buildOutlineCandidateRequest(
  input: OutlineCandidateRequest,
): OutlineCandidateRequest {
  if (
    !input.expected_workspace_revision_id ||
    !input.document_set_revision_id ||
    !input.document_set_sha256
  ) {
    throw new AuthoringLogicError(
      "OUTLINE_GENERATE_INPUT",
      "生成大纲需要冻结 DocumentSet 与当前 WorkspaceRevision",
    );
  }
  return input;
}

export function buildContentCandidateRequest(
  input: ContentCandidateRequest,
): ContentCandidateRequest {
  if (
    (input.target === "node" || input.target === "subtree") &&
    !input.node_lineage_id
  ) {
    throw new AuthoringLogicError(
      "CONTENT_TARGET",
      "生成本章/子树必须指定节点",
    );
  }
  if (input.target === "workspace" && input.node_lineage_id) {
    throw new AuthoringLogicError(
      "CONTENT_TARGET",
      "生成全部空章节不得携带 node_lineage_id",
    );
  }
  if (input.selection_mode === "user_pick_set" && !input.pick_set_artifact_id) {
    throw new AuthoringLogicError(
      "EVIDENCE_PICK_SET",
      "人工选证模式必须提供 PickSet",
    );
  }
  return input;
}

export function buildExportRequest(input: ExportRequest): ExportRequest {
  if (input.mode === "submission") {
    if (input.watermark)
      throw new AuthoringLogicError("EXPORT_WATERMARK", "正式提交不得带水印");
    if (input.include_risk_notices || input.include_knowledge_provenance) {
      throw new AuthoringLogicError(
        "EXPORT_OPTIONS",
        "正式提交不得包含风险提示或知识来源",
      );
    }
    return {
      mode: "submission",
      format: input.format,
      expected_workspace_revision_id: input.expected_workspace_revision_id,
      watermark: null,
      include_risk_notices: false,
      include_knowledge_provenance: false,
    };
  }
  return input;
}

export function selectedOperationIndexes(
  total: number,
  indexes: number[],
): number[] {
  const unique = [...new Set(indexes)].sort((a, b) => a - b);
  for (const index of unique) {
    if (!Number.isInteger(index) || index < 0 || index >= total) {
      throw new AuthoringLogicError("CANDIDATE_OP_INDEX", "候选操作下标不合法");
    }
  }
  if (unique.length === 0)
    throw new AuthoringLogicError("CANDIDATE_OP_EMPTY", "至少选择一个候选操作");
  return unique;
}
