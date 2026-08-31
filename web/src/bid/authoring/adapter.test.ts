import {
  docToChapterPatches,
  outlineToDoc,
  tiptapToRichText,
  richTextToTiptap,
} from "./adapter";
import type { ContentBlockV1 } from "./contentBlock";
import { expect, it } from "./harness";
import type { FlattenedNode } from "./tree";

function block(partial: Partial<ContentBlockV1> & Pick<ContentBlockV1, "kind" | "content">): ContentBlockV1 {
  return {
    schema_version: 1,
    block_revision_id: "00000000-0000-4000-8000-000000000001",
    lineage_id: "00000000-0000-4000-8000-000000000002",
    revision: 1,
    origin: "human",
    content_sha256: "0".repeat(64),
    ...partial,
  };
}

it("rich text roundtrips quote, code, and rule", () => {
  const content = {
    type: "rich_text" as const,
    nodes: [
      { kind: "paragraph" as const, content: [{ kind: "text" as const, text: "hi" }] },
      {
        kind: "blockquote" as const,
        content: [{ kind: "paragraph" as const, content: [{ kind: "text" as const, text: "q" }] }],
      },
      { kind: "code_block" as const, language: "ts", text: "const x = 1" },
      { kind: "horizontal_rule" as const },
    ],
  };
  const back = tiptapToRichText(richTextToTiptap(content));
  expect(back).toEqual(content);
});

it("outlineToDoc / docToChapterPatches keep chapter identity", () => {
  const node: FlattenedNode = {
    lineage_id: "00000000-0000-4000-8000-0000000000aa",
    revision_id: "00000000-0000-4000-8000-0000000000ab",
    parent_lineage_id: null,
    ordinal: 0,
    title: "投标文件",
    semantic_role: "other",
    render_role: "section",
    stale: false,
    block_lineage_ids: ["00000000-0000-4000-8000-000000000002"],
    depth: 0,
  };
  const blocks = [
    block({
      kind: "rich_text",
      content: {
        type: "rich_text",
        nodes: [{ kind: "paragraph", content: [{ kind: "text", text: "正文" }] }],
      },
    }),
  ];
  const doc = outlineToDoc([node], () => blocks);
  const patches = docToChapterPatches(doc);
  expect(patches.length).toBe(1);
  expect(patches[0]?.lineageId).toBe(node.lineage_id);
  expect(patches[0]?.title).toBe("投标文件");
  expect(patches[0]?.blocks[0]?.kind).toBe("rich_text");
});
