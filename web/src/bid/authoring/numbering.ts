import type { OutlineCandidateView } from "../api/types";
import type { OutlineNodeView } from "./tree";

type CandidateNode = OutlineCandidateView["nodes"][number];

function chineseOrdinal(value: number): string {
  const digits = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
  if (value <= 9) return digits[value] ?? String(value);
  if (value === 10) return "十";
  if (value < 20) return `十${digits[value % 10]}`;
  if (value < 100 && value % 10 === 0) return `${digits[Math.floor(value / 10)]}十`;
  if (value < 100)
    return `${digits[Math.floor(value / 10)]}十${digits[value % 10]}`;
  return String(value);
}

function isBodyNode(node: CandidateNode): boolean {
  return (
    node.semantic_role !== "cover" &&
    node.semantic_role !== "toc" &&
    node.render_role !== "front_matter" &&
    node.render_role !== "toc" &&
    node.render_role !== "hidden"
  );
}

export function outlineDisplayTitles(
  nodes: OutlineCandidateView["nodes"],
): Map<string, string> {
  const labels = new Map(nodes.map((node) => [node.client_node_ref, node.title]));
  if (
    nodes.some(
      (node) => node.semantic_role === undefined || node.render_role === undefined,
    )
  ) {
    return labels;
  }
  const byParent = new Map<string | null, CandidateNode[]>();
  for (const node of nodes) {
    const siblings = byParent.get(node.parent_client_node_ref) ?? [];
    siblings.push(node);
    byParent.set(node.parent_client_node_ref, siblings);
  }
  for (const siblings of byParent.values()) {
    siblings.sort(
      (left, right) =>
        left.ordinal - right.ordinal ||
        left.client_node_ref.localeCompare(right.client_node_ref),
    );
  }
  const cover = (byParent.get(null) ?? []).find(
    (node) => node.semantic_role === "cover" || node.render_role === "front_matter",
  );
  const topSections = (byParent.get(cover?.client_node_ref ?? null) ?? []).filter(
    isBodyNode,
  );

  function walkLower(parentRef: string, path: number[]): void {
    const children = (byParent.get(parentRef) ?? []).filter(isBodyNode);
    children.forEach((node, index) => {
      const childPath = [...path, index + 1];
      const prefix =
        childPath.length === 1
          ? `${childPath[0]}.`
          : childPath.join(".");
      labels.set(node.client_node_ref, `${prefix} ${node.title}`);
      walkLower(node.client_node_ref, childPath);
    });
  }

  topSections.forEach((node, index) => {
    labels.set(
      node.client_node_ref,
      `${chineseOrdinal(index + 1)}、${node.title}`,
    );
    walkLower(node.client_node_ref, []);
  });
  return labels;
}

export function workspaceOutlineDisplayTitles(
  nodes: OutlineNodeView[],
): Map<string, string> {
  const candidateNodes: OutlineCandidateView["nodes"] = nodes.map((node) => ({
    client_node_ref: node.lineage_id,
    parent_client_node_ref: node.parent_lineage_id,
    ordinal: node.ordinal,
    title: node.title,
    semantic_role: node.semantic_role,
    render_role: node.render_role,
    origin_source_unit_revision_ids: [],
  }));
  return outlineDisplayTitles(candidateNodes);
}
