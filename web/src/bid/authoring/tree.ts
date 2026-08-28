import { AuthoringLogicError } from "./errors";
import type { RenderRole, SemanticRole } from "./mutations";

export type OutlineNodeView = {
  lineage_id: string;
  revision_id: string;
  parent_lineage_id: string | null;
  ordinal: number;
  title: string;
  semantic_role: SemanticRole;
  render_role: RenderRole;
  stale: boolean;
  block_lineage_ids: string[];
};

export type OutlineIndex = {
  nodes: OutlineNodeView[];
  byId: Map<string, OutlineNodeView>;
  children: Map<string | null, OutlineNodeView[]>;
  roots: OutlineNodeView[];
};

function byOrdinal(a: OutlineNodeView, b: OutlineNodeView): number {
  return a.ordinal - b.ordinal;
}

export function outlineIndex(nodes: OutlineNodeView[]): OutlineIndex {
  const byId = new Map(nodes.map((node) => [node.lineage_id, node]));
  const children = new Map<string | null, OutlineNodeView[]>();
  for (const node of nodes) {
    const key = node.parent_lineage_id;
    const list = children.get(key);
    if (list) list.push(node);
    else children.set(key, [node]);
  }
  for (const list of children.values()) list.sort(byOrdinal);
  const roots = children.get(null) ?? [];
  return { nodes, byId, children, roots };
}

export function findNode(
  index: OutlineIndex,
  lineageId: string,
): OutlineNodeView | null {
  return index.byId.get(lineageId) ?? null;
}

export function childrenOf(
  index: OutlineIndex,
  parentLineageId: string | null,
): OutlineNodeView[] {
  return index.children.get(parentLineageId) ?? [];
}

export function nodePath(
  index: OutlineIndex,
  lineageId: string,
): OutlineNodeView[] {
  const path: OutlineNodeView[] = [];
  let current = findNode(index, lineageId);
  const seen = new Set<string>();
  while (current) {
    if (seen.has(current.lineage_id))
      throw new AuthoringLogicError("TREE_CYCLE", "大纲树存在环");
    seen.add(current.lineage_id);
    path.unshift(current);
    current = current.parent_lineage_id
      ? findNode(index, current.parent_lineage_id)
      : null;
  }
  return path;
}

export function subtreeLineageIds(
  index: OutlineIndex,
  lineageId: string,
): string[] {
  const ids = [lineageId];
  for (const child of childrenOf(index, lineageId)) {
    ids.push(...subtreeLineageIds(index, child.lineage_id));
  }
  return ids;
}

export function isDescendant(
  index: OutlineIndex,
  ancestorId: string,
  maybeChildId: string,
): boolean {
  if (ancestorId === maybeChildId) return true;
  return subtreeLineageIds(index, ancestorId).includes(maybeChildId);
}

export function validateTree(index: OutlineIndex): void {
  if (index.roots.length > 1)
    throw new AuthoringLogicError("TREE_MULTI_ROOT", "大纲必须单根");
  const seen = new Set<string>();
  function walk(parentId: string | null): void {
    const siblings = childrenOf(index, parentId);
    siblings.forEach((node, i) => {
      if (seen.has(node.lineage_id))
        throw new AuthoringLogicError("TREE_CYCLE", "大纲树存在环或重复节点");
      seen.add(node.lineage_id);
      if (node.parent_lineage_id !== parentId) {
        throw new AuthoringLogicError("TREE_PARENT", "节点父子关系不一致");
      }
      if (node.ordinal !== i)
        throw new AuthoringLogicError(
          "TREE_ORDINAL",
          "同级 ordinal 必须从 0 连续",
        );
      walk(node.lineage_id);
    });
  }
  walk(null);
  if (seen.size !== index.nodes.length) {
    throw new AuthoringLogicError("TREE_ORPHAN", "存在无法从根到达的大纲节点");
  }
}

export function assertCanInsert(
  index: OutlineIndex,
  parentLineageId: string | null,
  ordinal: number,
): void {
  if (ordinal < 0)
    throw new AuthoringLogicError("NODE_ORDINAL", "ordinal 不能为负");
  if (parentLineageId === null) {
    if (index.roots.length > 0)
      throw new AuthoringLogicError(
        "TREE_MULTI_ROOT",
        "已有根节点，不能再插入根",
      );
    return;
  }
  if (!findNode(index, parentLineageId))
    throw new AuthoringLogicError("TREE_PARENT", "父节点不存在");
}

export function assertCanRename(
  index: OutlineIndex,
  nodeLineageId: string,
): OutlineNodeView {
  const node = findNode(index, nodeLineageId);
  if (!node) throw new AuthoringLogicError("TREE_NODE", "节点不存在");
  return node;
}

export function assertCanMove(
  index: OutlineIndex,
  nodeLineageId: string,
  parentLineageId: string | null,
  ordinal: number,
): OutlineNodeView {
  const node = assertCanRename(index, nodeLineageId);
  if (ordinal < 0)
    throw new AuthoringLogicError("NODE_ORDINAL", "ordinal 不能为负");
  if (parentLineageId === nodeLineageId) {
    throw new AuthoringLogicError("TREE_MOVE_SELF", "不能把节点移动到自身之下");
  }
  if (parentLineageId && isDescendant(index, nodeLineageId, parentLineageId)) {
    throw new AuthoringLogicError(
      "TREE_MOVE_DESCENDANT",
      "不能把节点移动到其子孙之下",
    );
  }
  if (parentLineageId === null) {
    const otherRoots = index.roots.filter(
      (root) => root.lineage_id !== nodeLineageId,
    );
    if (otherRoots.length > 0)
      throw new AuthoringLogicError("TREE_MULTI_ROOT", "已有其他根节点");
  } else if (!findNode(index, parentLineageId)) {
    throw new AuthoringLogicError("TREE_PARENT", "目标父节点不存在");
  }
  return node;
}

export function assertCanDelete(
  index: OutlineIndex,
  nodeLineageId: string,
): OutlineNodeView {
  return assertCanRename(index, nodeLineageId);
}

export function assertCanSplit(
  index: OutlineIndex,
  nodeLineageId: string,
  titles: string[],
): OutlineNodeView {
  if (titles.length < 2)
    throw new AuthoringLogicError("SPLIT_TITLES", "拆分至少两个标题");
  return assertCanRename(index, nodeLineageId);
}

export function assertCanMerge(
  index: OutlineIndex,
  nodeLineageIds: string[],
): OutlineNodeView[] {
  const unique = [...new Set(nodeLineageIds)];
  if (unique.length < 2)
    throw new AuthoringLogicError("MERGE_NODES", "合并至少两个不同节点");
  return unique.map((id) => assertCanRename(index, id));
}

export type FlattenedNode = OutlineNodeView & { depth: number };

export function flattenPreorder(index: OutlineIndex): FlattenedNode[] {
  const out: FlattenedNode[] = [];
  function walk(parentId: string | null, depth: number): void {
    for (const node of childrenOf(index, parentId)) {
      out.push({ ...node, depth });
      walk(node.lineage_id, depth + 1);
    }
  }
  walk(null, 0);
  return out;
}

export type DropPlacement = "before" | "after" | "child";

export type DropMove = {
  parentLineageId: string | null;
  ordinal: number;
};

export function dropPlacementFromRatio(yRatio: number): DropPlacement {
  if (yRatio < 0.28) return "before";
  if (yRatio > 0.72) return "after";
  return "child";
}

export function dropMove(
  index: OutlineIndex,
  draggedId: string,
  targetId: string,
  placement: DropPlacement,
): DropMove {
  if (draggedId === targetId) {
    const current = assertCanRename(index, draggedId);
    return {
      parentLineageId: current.parent_lineage_id,
      ordinal: current.ordinal,
    };
  }
  const target = assertCanRename(index, targetId);
  let parentLineageId: string | null;
  let ordinal: number;
  if (placement === "child") {
    parentLineageId = target.lineage_id;
    ordinal = childrenOf(index, parentLineageId).filter(
      (item) => item.lineage_id !== draggedId,
    ).length;
  } else {
    parentLineageId = target.parent_lineage_id;
    const siblings = childrenOf(index, parentLineageId).filter(
      (item) => item.lineage_id !== draggedId,
    );
    const targetIndex = siblings.findIndex(
      (item) => item.lineage_id === targetId,
    );
    if (targetIndex < 0) {
      throw new AuthoringLogicError("TREE_NODE", "放置目标不在同级列表中");
    }
    ordinal = placement === "before" ? targetIndex : targetIndex + 1;
  }
  assertCanMove(index, draggedId, parentLineageId, ordinal);
  return { parentLineageId, ordinal };
}
