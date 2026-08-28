import { useEffect, useRef } from "react";
import { go } from "../../hash";
import { blocksForNode } from "./blocks";
import { authoringHref } from "./routes";
import { SectionBlocks } from "./SectionEditor";
import type { BidV2Session, BidV2State } from "./session";
import { flattenPreorder } from "./tree";

export function DocumentCanvas({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const nodes = flattenPreorder(session.tree());
  const focused = state.selectedNodeLineageId;
  const focusedNode = focused ? session.findNode(focused) : null;
  const focusedBlocks = blocksForNode(
    focusedNode,
    state.workspace?.blocks,
    state.drafts,
  );
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const navigating = useRef(false);
  const selectedRef = useRef(state.selectedNodeLineageId);
  selectedRef.current = state.selectedNodeLineageId;
  const projectId = state.route?.projectId ?? "";
  const nodeIds = nodes.map((node) => node.lineage_id).join(",");

  useEffect(() => {
    if (!focused) return;
    navigating.current = true;
    const el = document.getElementById(`canvas-section-${focused}`);
    const rootEl = scrollRef.current;
    if (el && rootEl) {
      const er = el.getBoundingClientRect();
      const rr = rootEl.getBoundingClientRect();
      const visible = er.top < rr.bottom && er.bottom > rr.top;
      if (!visible) el.scrollIntoView({ behavior: "smooth", block: "start" });
    }
    const timer = window.setTimeout(() => {
      navigating.current = false;
    }, 500);
    return () => window.clearTimeout(timer);
  }, [focused]);

  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (navigating.current) return;
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort(
            (a, b) => a.boundingClientRect.top - b.boundingClientRect.top,
          )[0];
        const id = visible?.target.getAttribute("data-node-id");
        if (!id || id === selectedRef.current) return;
        session.selectNode(id);
        if (projectId) go(authoringHref(projectId, "authoring", id));
      },
      { root, rootMargin: "-18% 0px -62% 0px", threshold: 0.05 },
    );
    for (const el of root.querySelectorAll("[data-node-id]"))
      observer.observe(el);
    return () => observer.disconnect();
  }, [nodeIds, projectId, session]);

  const pending = state.asyncRequests.find(
    (request) => request.status === "pending",
  );

  function select(id: string) {
    session.selectNode(id);
    if (projectId) go(authoringHref(projectId, "authoring", id));
  }

  return (
    <div className="ed-page" data-testid="document-canvas">
      <div className="ed-toolbar">
        <strong>{focusedNode?.title ?? "投标文件"}</strong>
        <span className="chip gray" data-testid="draft-status">
          {state.draftStatus}
        </span>
        <button
          type="button"
          className="btn ghost"
          disabled={state.ended || !focusedNode}
          onClick={() =>
            focusedNode &&
            session.insertRichTextBlock(
              focusedNode.lineage_id,
              focusedBlocks.length,
            )
          }
        >
          插入段落
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={state.ended || !focusedNode}
          onClick={() =>
            focusedNode &&
            session.insertTableBlock(
              focusedNode.lineage_id,
              focusedBlocks.length,
            )
          }
        >
          插入表格
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={state.ended || !focusedNode}
          onClick={() =>
            focusedNode &&
            session.insertPageBreak(
              focusedNode.lineage_id,
              focusedBlocks.length,
            )
          }
        >
          分页
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={state.ended || !focusedNode}
          onClick={() =>
            focusedNode &&
            session.insertSignature(
              focusedNode.lineage_id,
              focusedBlocks.length,
            )
          }
        >
          签章占位
        </button>
        <label className="chip gray" style={{ cursor: "pointer" }}>
          插入图片
          <input
            type="file"
            accept="image/*"
            hidden
            disabled={state.ended || !focusedNode}
            onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              if (file) void session.uploadAsset(file);
              event.currentTarget.value = "";
            }}
          />
        </label>
        <button
          type="button"
          className="btn"
          disabled={state.ended || state.draftStatus === "clean"}
          onClick={() => void session.save()}
        >
          保存
        </button>
      </div>
      {pending && (
        <div className="banner" data-testid="authoring-pending">
          正在{pending.kind === "OutlineGenerate" ? "生成大纲" : "填充内容"}
          …可继续改树和正文。
        </div>
      )}
      {state.conflict && (
        <div className="banner warn" data-testid="authoring-conflict">
          工作区已更新。
          <button
            type="button"
            className="btn"
            onClick={() => void session.resolveConflict("keep_local")}
          >
            保留本地
          </button>
          <button
            type="button"
            className="btn ghost"
            onClick={() => void session.resolveConflict("take_server")}
          >
            使用服务器
          </button>
        </div>
      )}
      <div className="ed-stage canvas-stage">
        <div className="ed-doc" ref={scrollRef} data-testid="section-editor">
          <div className="ed-sheet">
            {nodes.length === 0 && (
              <p className="note">
                还没有章节。在左侧添加根章节，或点「生成大纲」。
              </p>
            )}
            {nodes.map((node) => {
              const live = node.lineage_id === focused;
              const heading =
                node.depth === 0 ? "h1" : node.depth === 1 ? "h2" : "h3";
              const Heading = heading;
              return (
                <section
                  key={node.lineage_id}
                  id={`canvas-section-${node.lineage_id}`}
                  data-node-id={node.lineage_id}
                  data-testid={`canvas-section-${node.lineage_id}`}
                  className={`canvas-section${live ? " on" : ""}`}
                  onClick={() => {
                    if (!live) select(node.lineage_id);
                  }}
                >
                  <Heading className={`canvas-${heading}`}>
                    {node.title}
                  </Heading>
                  <SectionBlocks
                    session={session}
                    state={state}
                    node={node}
                    live={live}
                  />
                </section>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
