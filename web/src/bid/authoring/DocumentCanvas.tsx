import type { Content } from "@tiptap/core";
import Link from "@tiptap/extension-link";
import { Table } from "@tiptap/extension-table";
import { TableCell } from "@tiptap/extension-table-cell";
import { TableHeader } from "@tiptap/extension-table-header";
import { TableRow } from "@tiptap/extension-table-row";
import Underline from "@tiptap/extension-underline";
import {
  BubbleMenu,
  EditorContent,
  FloatingMenu,
  useEditor,
  type Editor,
} from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useRef } from "react";
import { go } from "../../hash";
import { outlineToDoc, type TiptapNode } from "./adapter";
import { blocksForNode } from "./blocks";
import {
  BidDocument,
  BidImage,
  Chapter,
  ChapterTitle,
  PageBreak,
  SignatureBlock,
} from "./chapter";
import { EvidenceRef } from "./EvidenceRef";
import { authoringHref } from "./routes";
import type { BidV2Session, BidV2State } from "./session";
import { flattenPreorder } from "./tree";

const EXTENSIONS = [
  StarterKit.configure({
    document: false,
    heading: false,
  }),
  BidDocument,
  Chapter,
  ChapterTitle,
  PageBreak,
  SignatureBlock,
  BidImage,
  Underline,
  Link.configure({ openOnClick: false }),
  Table.configure({ resizable: false }),
  TableRow,
  TableHeader,
  TableCell,
  EvidenceRef,
];

function FormatBar({ editor }: { editor: Editor }) {
  return (
    <div className="fmt-bar">
      <button type="button" onClick={() => editor.chain().focus().toggleBold().run()}>
        B
      </button>
      <button type="button" onClick={() => editor.chain().focus().toggleItalic().run()}>
        I
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleUnderline().run()}
      >
        U
      </button>
      <button type="button" onClick={() => editor.chain().focus().toggleStrike().run()}>
        S
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleBulletList().run()}
      >
        •
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleOrderedList().run()}
      >
        1.
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleBlockquote().run()}
      >
        “
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleCodeBlock().run()}
      >
        {"</>"}
      </button>
    </div>
  );
}

export function DocumentCanvas({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const nodes = flattenPreorder(session.tree());
  const focused = state.selectedNodeLineageId;
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const navigating = useRef(false);
  const selectedRef = useRef(state.selectedNodeLineageId);
  selectedRef.current = state.selectedNodeLineageId;
  const applying = useRef(false);
  const projectId = state.route?.projectId ?? "";
  const nodeKey = nodes.map((node) => `${node.lineage_id}:${node.depth}`).join("|");
  const revision = state.workspace?.revision_id ?? "";

  const editor = useEditor({
    extensions: EXTENSIONS,
    content: outlineToDoc(nodes, (node) =>
      blocksForNode(node, state.workspace?.blocks, state.drafts),
    ) as Content,
    immediatelyRender: false,
    editable: !state.ended,
    onUpdate: ({ editor: instance }) => {
      if (applying.current || !state.workspace) return;
      session.applyDocument(instance.getJSON() as TiptapNode);
    },
    onSelectionUpdate: ({ editor: instance }) => {
      const pos = instance.state.selection.from;
      const chapter = instance.state.doc.resolve(pos).node(1);
      const id = chapter?.type.name === "chapter" ? chapter.attrs.lineageId : null;
      if (typeof id === "string" && id && id !== selectedRef.current) {
        session.selectNode(id);
        if (projectId) go(authoringHref(projectId, "authoring", id));
      }
    },
  });

  useEffect(() => {
    if (!editor) return;
    applying.current = true;
    editor.setEditable(!state.ended);
    applying.current = false;
  }, [editor, state.ended]);

  useEffect(() => {
    if (!editor) return;
    if (Object.keys(state.drafts).length > 0) return;
    applying.current = true;
    editor.commands.setContent(
      outlineToDoc(nodes, (node) =>
        blocksForNode(node, state.workspace?.blocks, state.drafts),
      ) as Content,
    );
    applying.current = false;
    // nodeKey + revision cover tree identity; drafts skip this path.
  }, [editor, revision, nodeKey]);

  useEffect(() => {
    if (!focused || !editor) return;
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
    }, 400);
    return () => window.clearTimeout(timer);
  }, [focused, editor]);

  if (nodes.length === 0) {
    return (
      <div className="ed-page" data-testid="document-canvas">
        <div className="ed-stage canvas-stage">
          <div className="ed-doc">
            <p className="note">还没有章节。在左侧添加根章节，或点「生成大纲」。</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="ed-page" data-testid="document-canvas">
      <div className="ed-toolbar">
        <strong>投标文件</strong>
        <span className="chip gray" data-testid="draft-status">
          {state.draftStatus}
        </span>
      </div>
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
          {editor ? (
            <>
              <BubbleMenu editor={editor} className="fmt-bubble">
                <FormatBar editor={editor} />
              </BubbleMenu>
              <FloatingMenu editor={editor} className="fmt-bubble">
                <button
                  type="button"
                  onClick={() =>
                    editor
                      .chain()
                      .focus()
                      .insertTable({ rows: 2, cols: 3, withHeaderRow: true })
                      .run()
                  }
                >
                  表
                </button>
                <button
                  type="button"
                  onClick={() =>
                    editor.chain().focus().setHorizontalRule().run()
                  }
                >
                  —
                </button>
                <button
                  type="button"
                  onClick={() =>
                    editor.chain().focus().insertContent({ type: "pageBreak" }).run()
                  }
                >
                  分页
                </button>
                <button
                  type="button"
                  onClick={() =>
                    editor
                      .chain()
                      .focus()
                      .insertContent({
                        type: "signature",
                        attrs: { label: "签字" },
                      })
                      .run()
                  }
                >
                  签
                </button>
              </FloatingMenu>
              <EditorContent editor={editor} className="ed-sheet" />
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
}
