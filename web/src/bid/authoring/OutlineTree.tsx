import { useState } from "react";
import { ActionIcon, Menu } from "@mantine/core";
import { IconDots } from "@tabler/icons-react";
import { go } from "../../hash";
import { workspaceOutlineDisplayTitles } from "./numbering";
import { authoringHref } from "./routes";
import type { BidV2Session, BidV2State } from "./session";
import {
  dropMove,
  dropPlacementFromRatio,
  type DropPlacement,
  type OutlineNodeView,
} from "./tree";

type DropHint = { targetId: string; placement: DropPlacement };

function NodeRow({
  session,
  state,
  node,
  depth,
  siblings,
  displayTitles,
  dropHint,
  setDropHint,
  onSplit,
}: {
  session: BidV2Session;
  state: BidV2State;
  node: OutlineNodeView;
  depth: number;
  siblings: OutlineNodeView[];
  displayTitles: Map<string, string>;
  dropHint: DropHint | null;
  setDropHint: (hint: DropHint | null) => void;
  onSplit: (node: OutlineNodeView) => void;
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
  const hintClass =
    dropHint?.targetId === node.lineage_id ? ` drop-${dropHint.placement}` : "";

  function href() {
    return `#${authoringHref(projectId, "authoring", node.lineage_id)}`;
  }

  return (
    <>
      <div
        className={`outline-row${selected ? " on" : ""}${hintClass}`}
        data-testid={`outline-node-${node.lineage_id}`}
        style={{ paddingLeft: 8 + depth * 14 }}
        draggable={!ended && !editing}
        onDragStart={(event) => {
          event.dataTransfer.setData("text/plain", node.lineage_id);
          event.dataTransfer.effectAllowed = "move";
        }}
        onDragOver={(event) => {
          event.preventDefault();
          const rect = event.currentTarget.getBoundingClientRect();
          const ratio = (event.clientY - rect.top) / Math.max(rect.height, 1);
          setDropHint({
            targetId: node.lineage_id,
            placement: dropPlacementFromRatio(ratio),
          });
        }}
        onDragLeave={() => setDropHint(null)}
        onDrop={(event) => {
          event.preventDefault();
          const dragged = event.dataTransfer.getData("text/plain");
          const placement =
            dropHint?.targetId === node.lineage_id
              ? dropHint.placement
              : "child";
          setDropHint(null);
          try {
            const move = dropMove(
              session.tree(),
              dragged,
              node.lineage_id,
              placement,
            );
            void session.moveNode(dragged, move.parentLineageId, move.ordinal);
          } catch {
            /* illegal drop is ignored */
          }
        }}
      >
        {editing ? (
          <input
            className="in"
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
            title={node.title}
            onDoubleClick={(event) => {
              event.preventDefault();
              if (!ended) setEditing(true);
            }}
            onClick={(event) => {
              event.preventDefault();
              session.selectNode(node.lineage_id);
              go(authoringHref(projectId, "authoring", node.lineage_id));
              document
                .getElementById(`canvas-section-${node.lineage_id}`)
                ?.scrollIntoView({ behavior: "smooth", block: "start" });
            }}
          >
            <em>{displayTitles.get(node.lineage_id) ?? node.title}</em>
          </a>
        )}
        {!ended && (
          <span className="outline-ops">
            <Menu shadow="md" width={160} position="bottom-end" withinPortal>
              <Menu.Target>
                <ActionIcon
                  variant="subtle"
                  size="sm"
                  aria-label="章节操作"
                  onClick={(event) => event.stopPropagation()}
                  onDoubleClick={(event) => event.stopPropagation()}
                >
                  <IconDots size={16} />
                </ActionIcon>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Item
                  data-testid="outline-rename"
                  onClick={() => setEditing(true)}
                >
                  改名
                </Menu.Item>
                <Menu.Item
                  data-testid="outline-add-child"
                  onClick={() =>
                    void session.insertNode({
                      parentLineageId: node.lineage_id,
                      ordinal: children.length,
                      title: "新章节",
                    })
                  }
                >
                  添加子章节
                </Menu.Item>
                <Menu.Item
                  disabled={index <= 0}
                  onClick={() =>
                    void session.moveNode(
                      node.lineage_id,
                      node.parent_lineage_id,
                      index - 1,
                    )
                  }
                >
                  上移
                </Menu.Item>
                <Menu.Item
                  disabled={index < 0 || index >= siblings.length - 1}
                  onClick={() =>
                    void session.moveNode(
                      node.lineage_id,
                      node.parent_lineage_id,
                      index + 1,
                    )
                  }
                >
                  下移
                </Menu.Item>
                <Menu.Item
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
                  降为子章节
                </Menu.Item>
                <Menu.Item
                  disabled={!node.parent_lineage_id}
                  onClick={() => {
                    const parent = session.findNode(
                      node.parent_lineage_id ?? "",
                    );
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
                  升为同级
                </Menu.Item>
                <Menu.Item onClick={() => onSplit(node)}>拆分</Menu.Item>
                <Menu.Item
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
                  与上一节合并
                </Menu.Item>
                <Menu.Item
                  color="red"
                  data-testid="outline-delete"
                  onClick={() => void session.deleteNode(node.lineage_id)}
                >
                  删除
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
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
          displayTitles={displayTitles}
          dropHint={dropHint}
          setDropHint={setDropHint}
          onSplit={onSplit}
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
  const tree = session.tree();
  const roots = tree.roots;
  const displayTitles = workspaceOutlineDisplayTitles(tree.nodes);
  const [dropHint, setDropHint] = useState<DropHint | null>(null);
  const [splitFor, setSplitFor] = useState<OutlineNodeView | null>(null);
  const [splitTitle, setSplitTitle] = useState("");

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
            displayTitles={displayTitles}
            dropHint={dropHint}
            setDropHint={setDropHint}
            onSplit={(current) => {
              setSplitFor(current);
              setSplitTitle(`${current.title}（续）`);
            }}
          />
        ))}
      </nav>
      {splitFor && (
        <div className="card" data-testid="outline-split-dialog">
          <p className="lbl">拆成两节</p>
          <input
            className="in"
            value={splitTitle}
            onChange={(event) => setSplitTitle(event.currentTarget.value)}
          />
          <div className="row" style={{ marginTop: 8 }}>
            <button
              type="button"
              className="btn"
              onClick={() => {
                if (!splitTitle.trim()) return;
                void session.splitNode(splitFor.lineage_id, [
                  splitFor.title,
                  splitTitle.trim(),
                ]);
                setSplitFor(null);
              }}
            >
              拆分
            </button>
            <button
              type="button"
              className="btn ghost"
              onClick={() => setSplitFor(null)}
            >
              取消
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
