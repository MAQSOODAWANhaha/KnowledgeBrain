import { Button } from "@mantine/core";
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
import type { ContentBlockV1 } from "./contentBlock";
import { contentBlockToEditorModel, type TiptapNode } from "./adapter";
import { EvidenceRef } from "./EvidenceRef";
import type { BidV2Session, BidV2State } from "./session";

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
  onChange,
}: {
  block: ContentBlockV1;
  ended: boolean;
  onChange: (doc: TiptapNode) => void;
}) {
  const model = contentBlockToEditorModel(block);
  const doc = (
    model.kind === "rich_text" || model.kind === "table"
      ? model.doc
      : { type: "doc", content: [{ type: "paragraph" }] }
  ) as Content;
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
    editor.commands.setContent(doc);
    // Identity change only — typing updates drafts without resetting the cursor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, block.lineage_id, block.revision]);

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

export function SectionEditor({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const nodeId = state.selectedNodeLineageId;
  const node = nodeId ? session.findNode(nodeId) : null;
  const draftBlocks = useMemo(
    () =>
      Object.values(state.drafts).filter(
        (draft) => draft.nodeLineageId === nodeId,
      ),
    [state.drafts, nodeId],
  );

  const blocks: ContentBlockV1[] = [];
  if (node && state.workspace) {
    for (const lineageId of node.block_lineage_ids) {
      const drafted = state.drafts[lineageId]?.block;
      const stored = state.workspace.blocks.find(
        (item) => item.lineage_id === lineageId,
      );
      if (drafted) blocks.push(drafted);
      else if (stored) blocks.push(stored);
    }
    for (const draft of draftBlocks) {
      if (
        draft.op === "insert" &&
        !blocks.some((block) => block.lineage_id === draft.blockLineageId)
      ) {
        blocks.push(draft.block);
      }
    }
  }

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
      <div className="ed-toolbar">
        <strong>{node.title}</strong>
        <span className="chip gray">{state.draftStatus}</span>
        <Button
          size="compact-sm"
          variant="default"
          disabled={state.ended}
          onClick={() =>
            session.insertRichTextBlock(node.lineage_id, blocks.length)
          }
        >
          插入段落
        </Button>
        <Button
          size="compact-sm"
          variant="default"
          disabled={state.ended}
          onClick={() =>
            session.insertTableBlock(node.lineage_id, blocks.length)
          }
        >
          插入表格
        </Button>
        <Button
          size="compact-sm"
          variant="default"
          disabled={state.ended}
          onClick={() =>
            session.insertPageBreak(node.lineage_id, blocks.length)
          }
        >
          分页
        </Button>
        <Button
          size="compact-sm"
          variant="default"
          disabled={state.ended}
          onClick={() =>
            session.insertSignature(node.lineage_id, blocks.length)
          }
        >
          签章占位
        </Button>
        <Button
          size="compact-sm"
          disabled={state.ended || state.draftStatus === "clean"}
          onClick={() => void session.save()}
        >
          保存
        </Button>
      </div>
      {state.conflict && (
        <div className="banner warn" data-testid="authoring-conflict">
          工作区已更新。
          <Button
            size="compact-xs"
            ml="sm"
            onClick={() => void session.resolveConflict("keep_local")}
          >
            保留本地
          </Button>
          <Button
            size="compact-xs"
            variant="default"
            ml="sm"
            onClick={() => void session.resolveConflict("take_server")}
          >
            使用服务器
          </Button>
        </div>
      )}
      <div className="ed-stage">
        <div className="ed-doc">
          <div className="ed-sheet">
            {blocks.length === 0 && (
              <p className="note">
                这一章还是空的。插入段落，或点「生成本章」。
              </p>
            )}
            {blocks.map((block) => (
              <div
                key={block.lineage_id}
                className="block-card"
                data-testid={`content-block-${block.kind}`}
              >
                {block.kind === "rich_text" || block.kind === "table" ? (
                  <RichBlockEditor
                    block={block}
                    ended={state.ended}
                    onChange={(doc) =>
                      block.kind === "table"
                        ? session.editTable(block.lineage_id, doc)
                        : session.editRichText(block.lineage_id, doc)
                    }
                  />
                ) : block.kind === "page_break" ? (
                  <p className="note">— 分页符 —</p>
                ) : block.kind === "signature_placeholder" ? (
                  <p className="note">签章占位：{block.content.label}</p>
                ) : (
                  <p className="note">
                    {block.kind}
                    {block.stale ? " · stale" : ""}
                  </p>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
