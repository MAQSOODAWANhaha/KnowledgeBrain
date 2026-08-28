import type { Content } from "@tiptap/core";
import Link from "@tiptap/extension-link";
import { Table } from "@tiptap/extension-table";
import { TableCell } from "@tiptap/extension-table-cell";
import { TableHeader } from "@tiptap/extension-table-header";
import { TableRow } from "@tiptap/extension-table-row";
import Underline from "@tiptap/extension-underline";
import { EditorContent, useEditor, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useMemo } from "react";
import { contentBlockToEditorModel, type TiptapNode } from "./adapter";
import { blocksForNode } from "./blocks";
import type { ContentBlockV1 } from "./contentBlock";
import { EvidenceRef } from "./EvidenceRef";
import type { BidV2Session, BidV2State } from "./session";
import { StaticBlock } from "./StaticBlock";
import type { OutlineNodeView } from "./tree";

const RICH_EXTENSIONS = [
  StarterKit.configure({
    heading: false,
    codeBlock: false,
    blockquote: false,
    horizontalRule: false,
  }),
  Underline,
  Link.configure({ openOnClick: false }),
  EvidenceRef,
];

const TABLE_EXTENSIONS = [
  StarterKit.configure({
    heading: false,
    codeBlock: false,
    blockquote: false,
    horizontalRule: false,
  }),
  Table.configure({ resizable: false }),
  TableRow,
  TableHeader,
  TableCell,
  Underline,
  Link.configure({ openOnClick: false }),
  EvidenceRef,
];

function RichBlockEditor({
  block,
  ended,
  drafted,
  onChange,
}: {
  block: ContentBlockV1;
  ended: boolean;
  drafted: boolean;
  onChange: (doc: TiptapNode) => void;
}) {
  const doc = useMemo(() => {
    const model = contentBlockToEditorModel(block);
    return (
      model.kind === "rich_text" || model.kind === "table"
        ? model.doc
        : { type: "doc", content: [{ type: "paragraph" }] }
    ) as Content;
  }, [block]);
  const editor = useEditor({
    extensions: block.kind === "table" ? TABLE_EXTENSIONS : RICH_EXTENSIONS,
    content: doc,
    immediatelyRender: false,
    editable: !ended,
    onUpdate: ({ editor: instance }) =>
      onChange(instance.getJSON() as TiptapNode),
  });

  useEffect(() => {
    if (!editor) return;
    editor.setEditable(!ended);
  }, [editor, ended]);

  useEffect(() => {
    if (!editor) return;
    if (drafted) return;
    const current = JSON.stringify(editor.getJSON());
    const next = JSON.stringify(doc);
    if (current === next) return;
    editor.commands.setContent(doc);
  }, [editor, block.lineage_id, drafted, block.content_sha256, doc]);

  if (!editor) return null;
  return (
    <div>
      <FormatBar editor={editor} table={block.kind === "table"} />
      <EditorContent
        editor={editor}
        data-testid={`block-editor-${block.lineage_id}`}
      />
    </div>
  );
}

function FormatBar({ editor, table }: { editor: Editor; table: boolean }) {
  return (
    <div className="fmt-bar">
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleBold().run()}
      >
        B
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleItalic().run()}
      >
        I
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleUnderline().run()}
      >
        U
      </button>
      <button
        type="button"
        onClick={() => editor.chain().focus().toggleStrike().run()}
      >
        S
      </button>
      {!table && (
        <>
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
        </>
      )}
      {table && (
        <>
          <button
            type="button"
            onClick={() => editor.chain().focus().addColumnAfter().run()}
          >
            +列
          </button>
          <button
            type="button"
            onClick={() => editor.chain().focus().addRowAfter().run()}
          >
            +行
          </button>
        </>
      )}
    </div>
  );
}

export function SectionBlocks({
  session,
  state,
  node,
  live,
}: {
  session: BidV2Session;
  state: BidV2State;
  node: OutlineNodeView;
  live: boolean;
}) {
  const blocks = blocksForNode(node, state.workspace?.blocks, state.drafts);
  return (
    <>
      {blocks.length === 0 && live && (
        <p className="note">这一章还是空的。插入段落，或点「生成本章」。</p>
      )}
      {blocks.map((block) => (
        <div
          key={block.lineage_id}
          className="block-card"
          data-testid={`content-block-${block.kind}`}
        >
          {live && (block.kind === "rich_text" || block.kind === "table") ? (
            <RichBlockEditor
              block={block}
              ended={state.ended}
              drafted={Boolean(state.drafts[block.lineage_id])}
              onChange={(doc) =>
                block.kind === "table"
                  ? session.editTable(block.lineage_id, doc)
                  : session.editRichText(block.lineage_id, doc)
              }
            />
          ) : (
            <StaticBlock block={block} />
          )}
        </div>
      ))}
    </>
  );
}

export function SectionEditor({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const nodeId = state.selectedNodeLineageId;
  const node = nodeId ? session.findNode(nodeId) : null;
  if (!node) {
    return (
      <div className="card">
        <h3 className="h3">选择一个章节</h3>
        <p className="note">在左侧大纲中点选，或添加根章节后开始编辑。</p>
      </div>
    );
  }
  return (
    <div className="ed-page" data-testid="section-editor">
      <div className="ed-stage canvas-stage">
        <div className="ed-doc">
          <div className="ed-sheet">
            <SectionBlocks session={session} state={state} node={node} live />
          </div>
        </div>
      </div>
    </div>
  );
}
