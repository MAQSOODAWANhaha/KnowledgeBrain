import { Button, TextInput } from "@mantine/core";
import { useState } from "react";
import { go } from "../../hash";
import type { BidV2Session, BidV2State } from "./session";
import { authoringHref } from "./routes";
import type { OutlineNodeView } from "./tree";

function NodeRow({
  session,
  state,
  node,
  depth,
  siblings,
}: {
  session: BidV2Session;
  state: BidV2State;
  node: OutlineNodeView;
  depth: number;
  siblings: OutlineNodeView[];
}) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(node.title);
  const selected = state.selectedNodeLineageId === node.lineage_id;
  const index = siblings.findIndex(
    (item) => item.lineage_id === node.lineage_id,
  );
  const children = session.childrenOf(node.lineage_id);
  const projectId = state.route?.projectId ?? "";
  const ended = state.ended;

  function href() {
    return `#${authoringHref(projectId, "authoring", node.lineage_id)}`;
  }

  return (
    <>
      <div
        className={`outline-row${selected ? " on" : ""}`}
        data-testid={`outline-node-${node.lineage_id}`}
        style={{ paddingLeft: 8 + depth * 14 }}
      >
        {editing ? (
          <TextInput
            size="xs"
            value={title}
            data-testid="outline-rename-input"
            onChange={(event) => setTitle(event.currentTarget.value)}
            onBlur={() => {
              setEditing(false);
              if (title.trim() && title.trim() !== node.title)
                void session.renameNode(node.lineage_id, title);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter")
                (event.currentTarget as HTMLInputElement).blur();
              if (event.key === "Escape") {
                setTitle(node.title);
                setEditing(false);
              }
            }}
            autoFocus
          />
        ) : (
          <a
            className={selected ? "flt" : undefined}
            href={href()}
            onClick={(event) => {
              event.preventDefault();
              session.selectNode(node.lineage_id);
              go(authoringHref(projectId, "authoring", node.lineage_id));
            }}
          >
            <em>{node.title}</em>
            {node.stale && <span className="chip amber">stale</span>}
          </a>
        )}
        {!ended && (
          <span className="outline-ops">
            <button
              type="button"
              title="上移"
              disabled={index <= 0}
              onClick={() =>
                void session.moveNode(
                  node.lineage_id,
                  node.parent_lineage_id,
                  index - 1,
                )
              }
            >
              ↑
            </button>
            <button
              type="button"
              title="下移"
              disabled={index < 0 || index >= siblings.length - 1}
              onClick={() =>
                void session.moveNode(
                  node.lineage_id,
                  node.parent_lineage_id,
                  index + 1,
                )
              }
            >
              ↓
            </button>
            <button
              type="button"
              title="降级为上一节的子节点"
              disabled={index <= 0}
              onClick={() => {
                const prev = siblings[index - 1];
                if (!prev) return;
                void session.moveNode(
                  node.lineage_id,
                  prev.lineage_id,
                  session.childrenOf(prev.lineage_id).length,
                );
              }}
            >
              →
            </button>
            <button
              type="button"
              title="升级到与父节点同级"
              disabled={!node.parent_lineage_id}
              onClick={() => {
                const parent = session.findNode(node.parent_lineage_id ?? "");
                if (!parent) return;
                const parentSiblings = session.childrenOf(
                  parent.parent_lineage_id,
                );
                const parentIndex = parentSiblings.findIndex(
                  (item) => item.lineage_id === parent.lineage_id,
                );
                void session.moveNode(
                  node.lineage_id,
                  parent.parent_lineage_id,
                  parentIndex + 1,
                );
              }}
            >
              ←
            </button>
            <button
              type="button"
              data-testid="outline-rename"
              title="改名"
              onClick={() => setEditing(true)}
            >
              改
            </button>
            <button
              type="button"
              data-testid="outline-add-child"
              title="子节点"
              onClick={() =>
                void session.insertNode({
                  parentLineageId: node.lineage_id,
                  ordinal: children.length,
                  title: "新章节",
                })
              }
            >
              +
            </button>
            <button
              type="button"
              title="拆成两节"
              onClick={() => {
                const second = window.prompt(
                  "第二节标题",
                  `${node.title}（续）`,
                );
                if (!second?.trim()) return;
                void session.splitNode(node.lineage_id, [
                  node.title,
                  second.trim(),
                ]);
              }}
            >
              拆
            </button>
            <button
              type="button"
              title="与上一节合并"
              disabled={index <= 0}
              onClick={() => {
                const prev = siblings[index - 1];
                if (!prev) return;
                void session.mergeNodes(
                  [prev.lineage_id, node.lineage_id],
                  prev.title,
                );
              }}
            >
              合
            </button>
            <button
              type="button"
              data-testid="outline-delete"
              title="删除"
              onClick={() => void session.deleteNode(node.lineage_id)}
            >
              ×
            </button>
          </span>
        )}
      </div>
      {children.map((child) => (
        <NodeRow
          key={child.lineage_id}
          session={session}
          state={state}
          node={child}
          depth={depth + 1}
          siblings={children}
        />
      ))}
    </>
  );
}

export function OutlineTree({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const roots = session.tree().roots;
  return (
    <div data-testid="outline-tree">
      <div className="side-sec">大纲</div>
      <nav className="sidenav">
        {roots.map((node) => (
          <NodeRow
            key={node.lineage_id}
            session={session}
            state={state}
            node={node}
            depth={0}
            siblings={roots}
          />
        ))}
      </nav>
      {!state.ended && (
        <div className="wrap" style={{ paddingTop: 8 }}>
          <Button
            size="compact-sm"
            variant="default"
            data-testid="outline-add-root"
            disabled={roots.length > 0}
            onClick={() =>
              void session.insertNode({
                parentLineageId: null,
                ordinal: 0,
                title: "投标文件",
              })
            }
          >
            添加根章节
          </Button>
        </div>
      )}
    </div>
  );
}
