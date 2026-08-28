import type { ContentBlockV1 } from "./contentBlock";
import type { DraftMap } from "./drafts";
import type { OutlineNodeView } from "./tree";

export function blocksForNode(
  node: OutlineNodeView | null,
  workspaceBlocks: ContentBlockV1[] | undefined,
  drafts: DraftMap,
): ContentBlockV1[] {
  if (!node) return [];
  const stored = workspaceBlocks ?? [];
  const blocks: ContentBlockV1[] = [];
  for (const lineageId of node.block_lineage_ids) {
    const drafted = drafts[lineageId]?.block;
    const current = stored.find((item) => item.lineage_id === lineageId);
    if (drafted) blocks.push(drafted);
    else if (current) blocks.push(current);
  }
  for (const draft of Object.values(drafts)) {
    if (
      draft.nodeLineageId === node.lineage_id &&
      draft.op === "insert" &&
      !blocks.some((block) => block.lineage_id === draft.blockLineageId)
    ) {
      blocks.push(draft.block);
    }
  }
  return blocks;
}
