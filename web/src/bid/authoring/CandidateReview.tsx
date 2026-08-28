import type { ChangeEvent } from "react";
import type { ContentBlockV1, Inline, RichNode } from "./contentBlock";
import type { BidV2Session, BidV2State } from "./session";
import type { OutlineCandidateView } from "../api/types";

function inlineText(inline: Inline): string {
  return inline.kind === "text" ? inline.text : " ";
}

function richSnippet(nodes: RichNode[]): string {
  const parts: string[] = [];
  for (const node of nodes) {
    if (node.kind === "paragraph") {
      parts.push(node.content.map(inlineText).join(""));
    } else {
      for (const item of node.content) {
        parts.push(item.content.flatMap((p) => p.content.map(inlineText)).join(""));
      }
    }
  }
  return parts.join(" ").replace(/\s+/g, " ").trim().slice(0, 80);
}

function blockSummary(block: ContentBlockV1): string {
  if (block.kind === "rich_text") {
    return richSnippet(block.content.nodes) || "空段落";
  }
  if (block.kind === "table") {
    return `表格 ${block.content.row_count}×${block.content.column_count}`;
  }
  if (block.kind === "image") {
    return `图片：${block.content.caption || block.content.alt || block.content.asset_revision_id}`;
  }
  if (block.kind === "attachment_ref") return "附件";
  if (block.kind === "structured_form") {
    return `表单 ${block.content.field_values.length} 项`;
  }
  if (block.kind === "page_break") return "分页符";
  return `签章占位：${block.content.label}`;
}

function OutlineCandidateTree({
  nodes,
  selected,
  obsolete,
  onToggle,
}: {
  nodes: OutlineCandidateView["nodes"];
  selected: string[];
  obsolete: boolean;
  onToggle: (event: ChangeEvent<HTMLInputElement>, ref: string) => void;
}) {
  const byParent = new Map<string | null, OutlineCandidateView["nodes"]>();
  for (const node of [...nodes].sort((a, b) => a.ordinal - b.ordinal)) {
    const key = node.parent_client_node_ref;
    const list = byParent.get(key);
    if (list) list.push(node);
    else byParent.set(key, [node]);
  }
  const rows: Array<{ node: OutlineCandidateView["nodes"][number]; depth: number }> =
    [];
  function walk(parent: string | null, depth: number): void {
    for (const node of byParent.get(parent) ?? []) {
      rows.push({ node, depth });
      walk(node.client_node_ref, depth + 1);
    }
  }
  walk(null, 0);
  return (
    <>
      {rows.map(({ node, depth }) => (
        <label
          key={node.client_node_ref}
          className="note"
          style={{ display: "block", paddingLeft: 8 + depth * 14 }}
        >
          <input
            type="checkbox"
            checked={selected.includes(node.client_node_ref)}
            disabled={obsolete}
            onChange={(event) => onToggle(event, node.client_node_ref)}
          />{" "}
          {node.title}
        </label>
      ))}
    </>
  );
}

export function CandidateReview({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const candidate = state.candidate;
  if (!candidate || candidate.status === "rejected") return null;
  const stale =
    candidate.base_workspace_revision_id !== state.workspace?.revision_id;
  const obsolete = candidate.status === "obsolete" || stale;

  function onOutline(event: ChangeEvent<HTMLInputElement>, ref: string) {
    session.toggleOutlineNode(ref, event.currentTarget.checked);
  }

  function onOperation(event: ChangeEvent<HTMLInputElement>, index: number) {
    session.toggleCandidateOperation(index, event.currentTarget.checked);
  }

  return (
    <div className="card candidate-review" data-testid="candidate-review">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3 className="h3">
          {candidate.kind === "outline" ? "大纲候选" : "内容候选"}
        </h3>
        <span className="chip gray">{candidate.status}</span>
      </div>
      {obsolete && (
        <p className="note" data-testid="candidate-stale">
          候选已过期。人改的树和正文保留，可重新生成。
        </p>
      )}
      {candidate.kind === "outline" && (
        <OutlineCandidateTree
          nodes={candidate.nodes}
          selected={state.selectedOutlineNodeRefs}
          obsolete={obsolete}
          onToggle={onOutline}
        />
      )}
      {candidate.kind === "content" &&
        candidate.operations.map((op, index) => (
          <label key={op.client_operation_ref} className="note">
            <input
              type="checkbox"
              checked={state.selectedOperationIndexes.includes(index)}
              disabled={obsolete}
              onChange={(event) => onOperation(event, index)}
            />{" "}
            {op.kind} · {blockSummary(op.block)}
          </label>
        ))}
      <div className="row" style={{ marginTop: 12 }}>
        <button
          type="button"
          className="btn"
          data-testid="candidate-accept"
          disabled={state.ended || obsolete}
          onClick={() => void session.acceptCandidate()}
        >
          接受所选
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={state.ended}
          onClick={() => void session.rejectCandidate()}
        >
          拒绝
        </button>
      </div>
    </div>
  );
}
