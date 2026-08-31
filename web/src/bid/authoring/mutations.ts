import type { ContentBlockV1 } from "./contentBlock";
import { AuthoringLogicError } from "./errors";

export const SEMANTIC_ROLES = [
  "cover",
  "toc",
  "qualification",
  "technical",
  "commercial",
  "quotation",
  "deviation",
  "implementation",
  "evidence_index",
  "attachment",
  "other",
] as const;
export type SemanticRole = (typeof SEMANTIC_ROLES)[number];

export const RENDER_ROLES = [
  "section",
  "front_matter",
  "toc",
  "appendix",
  "hidden",
] as const;
export type RenderRole = (typeof RENDER_ROLES)[number];

export const FULFILLMENT_CHANNELS = [
  "narrative_content",
  "response_table",
  "deviation_statement",
  "structured_form",
  "evidence_attachment",
  "quotation",
] as const;
export type FulfillmentChannel = (typeof FULFILLMENT_CHANNELS)[number];

export type InsertionAnchor = {
  node_revision_id: string;
  block_revision_id?: string | null;
};

export type DocumentSettings = {
  page_size: "A4";
  margins_mm: { top: number; right: number; bottom: number; left: number };
  body_font_pt: number;
  line_spacing: number;
  heading_numbering: "decimal" | "chinese" | "none";
  header: string;
  footer: string;
  page_number: "none" | "footer_center" | "footer_outside";
};

export const DEFAULT_DOCUMENT_SETTINGS: DocumentSettings = {
  page_size: "A4",
  margins_mm: { top: 25.4, right: 25.4, bottom: 25.4, left: 25.4 },
  body_font_pt: 12,
  line_spacing: 1.5,
  heading_numbering: "decimal",
  header: "",
  footer: "",
  page_number: "footer_center",
};

export type BindingTarget =
  | { kind: "outline_node"; node_lineage_id: string }
  | { kind: "response_table"; block_lineage_id: string }
  | { kind: "structured_form"; form_definition_revision_id: string }
  | { kind: "quote"; quote_snapshot_id: string };

export type InsertNodeOp = {
  kind: "insert_node";
  client_node_ref: string;
  parent_lineage_id: string | null;
  ordinal: number;
  title: string;
  semantic_role: SemanticRole;
  render_role: RenderRole;
};

export type RenameNodeOp = {
  kind: "rename_node";
  node_lineage_id: string;
  title: string;
};
export type MoveNodeOp = {
  kind: "move_node";
  node_lineage_id: string;
  parent_lineage_id: string | null;
  ordinal: number;
};
export type SplitNodeOp = {
  kind: "split_node";
  node_lineage_id: string;
  titles: string[];
};
export type MergeNodesOp = {
  kind: "merge_nodes";
  node_lineage_ids: string[];
  title: string;
};
export type DeleteNodeOp = { kind: "delete_node"; node_lineage_id: string };
export type InsertBlockOp = {
  kind: "insert_block";
  node_lineage_id: string;
  ordinal: number;
  block: ContentBlockV1;
};
export type UpdateBlockOp = {
  kind: "update_block";
  block_lineage_id: string;
  block: ContentBlockV1;
};
export type MoveBlockOp = {
  kind: "move_block";
  block_lineage_id: string;
  target_node_lineage_id: string;
  ordinal: number;
};
export type DeleteBlockOp = { kind: "delete_block"; block_lineage_id: string };
export type InsertAssetBlockOp = {
  kind: "insert_asset_block";
  node_lineage_id: string;
  asset_revision_id: string;
  ordinal: number;
};
export type UpdateDocumentSettingsOp = {
  kind: "update_document_settings";
  settings: DocumentSettings;
};
export type BindFulfillmentOp = {
  kind: "bind_fulfillment";
  need_occurrence_id: string;
  channel: FulfillmentChannel;
  requirement_projection_revision_id: string;
  requirement_projection_sha256: string;
  target: BindingTarget;
  reason: string;
};
export type RemapFulfillmentOp = {
  kind: "remap_fulfillment";
  binding_lineage_id: string;
  need_occurrence_id: string;
  channel: FulfillmentChannel;
  requirement_projection_revision_id: string;
  requirement_projection_sha256: string;
  target: BindingTarget;
  reason: string;
};
export type UnbindFulfillmentOp = {
  kind: "unbind_fulfillment";
  binding_lineage_id: string;
};

export type WorkspaceOperation =
  | InsertNodeOp
  | RenameNodeOp
  | MoveNodeOp
  | SplitNodeOp
  | MergeNodesOp
  | DeleteNodeOp
  | InsertBlockOp
  | UpdateBlockOp
  | MoveBlockOp
  | DeleteBlockOp
  | InsertAssetBlockOp
  | UpdateDocumentSettingsOp
  | BindFulfillmentOp
  | RemapFulfillmentOp
  | UnbindFulfillmentOp
;

export type WorkspaceMutationRequestV1 = {
  schema_version: 1;
  workspace_id: string;
  expected_workspace_revision_id: string;
  expected_workspace_sha256: string;
  operations: WorkspaceOperation[];
};

function assertTitle(title: string): void {
  const trimmed = title.trim();
  if (trimmed.length < 1 || trimmed.length > 1024) {
    throw new AuthoringLogicError("NODE_TITLE", "标题长度必须为 1–1024");
  }
}

export const ops = {
  insertNode(input: Omit<InsertNodeOp, "kind">): InsertNodeOp {
    assertTitle(input.title);
    if (input.ordinal < 0)
      throw new AuthoringLogicError("NODE_ORDINAL", "ordinal 不能为负");
    return { kind: "insert_node", ...input, title: input.title.trim() };
  },
  renameNode(nodeLineageId: string, title: string): RenameNodeOp {
    assertTitle(title);
    return {
      kind: "rename_node",
      node_lineage_id: nodeLineageId,
      title: title.trim(),
    };
  },
  moveNode(
    nodeLineageId: string,
    parentLineageId: string | null,
    ordinal: number,
  ): MoveNodeOp {
    if (ordinal < 0)
      throw new AuthoringLogicError("NODE_ORDINAL", "ordinal 不能为负");
    return {
      kind: "move_node",
      node_lineage_id: nodeLineageId,
      parent_lineage_id: parentLineageId,
      ordinal,
    };
  },
  splitNode(nodeLineageId: string, titles: string[]): SplitNodeOp {
    if (titles.length < 2 || titles.length > 100) {
      throw new AuthoringLogicError("SPLIT_TITLES", "拆分至少两个标题");
    }
    titles.forEach(assertTitle);
    return {
      kind: "split_node",
      node_lineage_id: nodeLineageId,
      titles: titles.map((title) => title.trim()),
    };
  },
  mergeNodes(nodeLineageIds: string[], title: string): MergeNodesOp {
    const unique = [...new Set(nodeLineageIds)];
    if (unique.length < 2)
      throw new AuthoringLogicError("MERGE_NODES", "合并至少两个不同节点");
    assertTitle(title);
    return {
      kind: "merge_nodes",
      node_lineage_ids: unique,
      title: title.trim(),
    };
  },
  deleteNode(nodeLineageId: string): DeleteNodeOp {
    return { kind: "delete_node", node_lineage_id: nodeLineageId };
  },
  insertBlock(input: Omit<InsertBlockOp, "kind">): InsertBlockOp {
    if (input.ordinal < 0)
      throw new AuthoringLogicError("BLOCK_ORDINAL", "ordinal 不能为负");
    return { kind: "insert_block", ...input };
  },
  updateBlock(blockLineageId: string, block: ContentBlockV1): UpdateBlockOp {
    return { kind: "update_block", block_lineage_id: blockLineageId, block };
  },
  moveBlock(
    blockLineageId: string,
    targetNodeLineageId: string,
    ordinal: number,
  ): MoveBlockOp {
    if (ordinal < 0)
      throw new AuthoringLogicError("BLOCK_ORDINAL", "ordinal 不能为负");
    return {
      kind: "move_block",
      block_lineage_id: blockLineageId,
      target_node_lineage_id: targetNodeLineageId,
      ordinal,
    };
  },
  deleteBlock(blockLineageId: string): DeleteBlockOp {
    return { kind: "delete_block", block_lineage_id: blockLineageId };
  },
  insertAssetBlock(
    input: Omit<InsertAssetBlockOp, "kind">,
  ): InsertAssetBlockOp {
    return { kind: "insert_asset_block", ...input };
  },
  updateDocumentSettings(settings: DocumentSettings): UpdateDocumentSettingsOp {
    return { kind: "update_document_settings", settings };
  },
  bindFulfillment(input: Omit<BindFulfillmentOp, "kind">): BindFulfillmentOp {
    return { kind: "bind_fulfillment", ...input };
  },
  remapFulfillment(
    input: Omit<RemapFulfillmentOp, "kind">,
  ): RemapFulfillmentOp {
    return { kind: "remap_fulfillment", ...input };
  },
  unbindFulfillment(bindingLineageId: string): UnbindFulfillmentOp {
    return { kind: "unbind_fulfillment", binding_lineage_id: bindingLineageId };
  },
};

export function buildMutationRequest(
  workspaceId: string,
  expectedRevisionId: string,
  expectedSha256: string,
  operations: WorkspaceOperation[],
): WorkspaceMutationRequestV1 {
  if (operations.length < 1 || operations.length > 1000) {
    throw new AuthoringLogicError(
      "MUTATION_BATCH",
      "一次 mutation 必须包含 1–1000 条操作",
    );
  }
  return {
    schema_version: 1,
    workspace_id: workspaceId,
    expected_workspace_revision_id: expectedRevisionId,
    expected_workspace_sha256: expectedSha256,
    operations,
  };
}
