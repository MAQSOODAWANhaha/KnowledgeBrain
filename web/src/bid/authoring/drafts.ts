import type { ContentBlockV1 } from "./contentBlock";
import { withContentSha256 } from "./contentBlock";
import { ops, type WorkspaceOperation } from "./mutations";

export type BlockDraft = {
  generation: number;
  nodeLineageId: string;
  blockLineageId: string;
  op: "insert" | "update";
  ordinal: number;
  block: ContentBlockV1;
  baseWorkspaceRevisionId: string;
};

export type DraftMap = Record<string, BlockDraft>;

export function upsertDraft(
  drafts: DraftMap,
  next: Omit<BlockDraft, "generation"> & { generation?: number },
): DraftMap {
  const previous = drafts[next.blockLineageId];
  const generation =
    next.generation ?? (previous ? previous.generation + 1 : 1);
  return { ...drafts, [next.blockLineageId]: { ...next, generation } };
}

export function clearAckedDrafts(
  drafts: DraftMap,
  acked: Record<string, number>,
): DraftMap {
  const remaining: DraftMap = {};
  for (const [id, draft] of Object.entries(drafts)) {
    const ack = acked[id];
    if (ack === undefined || draft.generation > ack) remaining[id] = draft;
  }
  return remaining;
}

export async function draftsToOperations(
  drafts: DraftMap,
): Promise<WorkspaceOperation[]> {
  const ordered = Object.values(drafts).sort(
    (a, b) => a.generation - b.generation,
  );
  const operations: WorkspaceOperation[] = [];
  for (const draft of ordered) {
    const block = await withContentSha256(draft.block);
    if (draft.op === "insert") {
      operations.push(
        ops.insertBlock({
          node_lineage_id: draft.nodeLineageId,
          ordinal: draft.ordinal,
          block,
        }),
      );
    } else {
      operations.push(ops.updateBlock(draft.blockLineageId, block));
    }
  }
  return operations;
}

export function hasDrafts(drafts: DraftMap): boolean {
  return Object.keys(drafts).length > 0;
}
