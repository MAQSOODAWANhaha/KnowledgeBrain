import type {
  ContentBlockV1,
  ImageContent,
  Inline,
  Paragraph,
  RichNode,
  RichTextContent,
  SignatureContent,
  TableContent,
  TextMark,
} from "./contentBlock";
import type { FlattenedNode } from "./tree";
import { emptyRichText, validateTableGrid } from "./contentBlock";
import { EditorAdapterError } from "./errors";

export type TiptapMark = { type: string; attrs?: Record<string, unknown> };
export type TiptapNode = {
  type: string;
  text?: string;
  attrs?: Record<string, unknown> | null;
  marks?: TiptapMark[];
  content?: TiptapNode[];
};

export type EditorModel =
  | { kind: "rich_text"; doc: TiptapNode }
  | { kind: "table"; doc: TiptapNode }
  | { kind: "image"; content: ImageContent }
  | {
      kind: "attachment_ref";
      content: ContentBlockV1 extends infer _
        ? Extract<ContentBlockV1, { kind: "attachment_ref" }>["content"]
        : never;
    }
  | {
      kind: "structured_form";
      content: Extract<ContentBlockV1, { kind: "structured_form" }>["content"];
    }
  | { kind: "page_break" }
  | {
      kind: "signature_placeholder";
      content: Extract<
        ContentBlockV1,
        { kind: "signature_placeholder" }
      >["content"];
    };

const RICH_DOC_TYPES = new Set([
  "paragraph",
  "bulletList",
  "orderedList",
  "blockquote",
  "codeBlock",
  "horizontalRule",
]);

function fail(message: string): never {
  throw new EditorAdapterError(message);
}

function attrs(node: TiptapNode): Record<string, unknown> {
  return node.attrs ?? {};
}

function asString(value: unknown, field: string): string {
  if (typeof value !== "string") fail(`缺少字符串字段 ${field}`);
  return value;
}

function asNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value))
    fail(`缺少数字字段 ${field}`);
  return value;
}

function marksToTiptap(
  marks: TextMark[] | undefined,
): TiptapMark[] | undefined {
  if (!marks?.length) return undefined;
  return marks.map((mark) => {
    if (mark.kind === "link")
      return { type: "link", attrs: { href: mark.href } };
    if (mark.kind === "evidence_ref") {
      return {
        type: "evidenceRef",
        attrs: {
          evidenceBundleId: mark.evidence_bundle_id,
          evidenceItemId: mark.evidence_item_id,
          quoteStartOffset: mark.quote_start_offset,
          quoteEndOffset: mark.quote_end_offset,
        },
      };
    }
    return { type: mark.kind };
  });
}

function marksFromTiptap(
  marks: TiptapMark[] | undefined,
): TextMark[] | undefined {
  if (!marks?.length) return undefined;
  return marks.map((mark) => {
    if (
      mark.type === "bold" ||
      mark.type === "italic" ||
      mark.type === "underline" ||
      mark.type === "strike" ||
      mark.type === "code"
    ) {
      return { kind: mark.type };
    }
    if (mark.type === "link") {
      const href = asString(mark.attrs?.href, "link.href");
      if (!href || href.length > 2048) fail("非法链接");
      return { kind: "link", href };
    }
    if (mark.type === "evidenceRef") {
      const start = asNumber(
        mark.attrs?.quoteStartOffset,
        "evidenceRef.quoteStartOffset",
      );
      const end = asNumber(
        mark.attrs?.quoteEndOffset,
        "evidenceRef.quoteEndOffset",
      );
      if (start < 0 || end < 1 || end <= start) fail("evidence_ref 偏移非法");
      return {
        kind: "evidence_ref",
        evidence_bundle_id: asString(
          mark.attrs?.evidenceBundleId,
          "evidenceRef.evidenceBundleId",
        ),
        evidence_item_id: asString(
          mark.attrs?.evidenceItemId,
          "evidenceRef.evidenceItemId",
        ),
        quote_start_offset: start,
        quote_end_offset: end,
      };
    }
    fail(`未知 mark: ${mark.type}`);
  });
}

function inlineToTiptap(inline: Inline): TiptapNode {
  if (inline.kind === "hard_break") return { type: "hardBreak" };
  return {
    type: "text",
    text: inline.text,
    marks: marksToTiptap(inline.marks),
  };
}

function inlineFromTiptap(node: TiptapNode): Inline | null {
  if (node.type === "hardBreak") return { kind: "hard_break" };
  if (node.type !== "text") fail(`富文本不允许节点 ${node.type}`);
  if (!node.text) return null;
  if (node.text.length > 65536) fail("文本过长");
  return { kind: "text", text: node.text, marks: marksFromTiptap(node.marks) };
}

function paragraphToTiptap(paragraph: Paragraph): TiptapNode {
  return { type: "paragraph", content: paragraph.content.map(inlineToTiptap) };
}

function paragraphFromTiptap(node: TiptapNode): Paragraph {
  if (node.type !== "paragraph") fail("list_item 只能包含 paragraph");
  const content = (node.content ?? [])
    .map(inlineFromTiptap)
    .filter((item): item is Inline => item !== null);
  return { kind: "paragraph", content };
}

function richNodeToTiptap(node: RichNode): TiptapNode {
  if (node.kind === "paragraph") return paragraphToTiptap(node);
  if (node.kind === "horizontal_rule") return { type: "horizontalRule" };
  if (node.kind === "code_block") {
    return {
      type: "codeBlock",
      attrs: { language: node.language },
      content: node.text ? [{ type: "text", text: node.text }] : [],
    };
  }
  if (node.kind === "blockquote") {
    return {
      type: "blockquote",
      content: node.content.map(paragraphToTiptap),
    };
  }
  return {
    type: node.kind === "bullet_list" ? "bulletList" : "orderedList",
    content: node.content.map((item) => ({
      type: "listItem",
      content: item.content.map(paragraphToTiptap),
    })),
  };
}

function richNodeFromTiptap(node: TiptapNode): RichNode {
  if (node.type === "paragraph") return paragraphFromTiptap(node);
  if (node.type === "horizontalRule") return { kind: "horizontal_rule" };
  if (node.type === "codeBlock") {
    const text = (node.content ?? [])
      .map((child) => child.text ?? "")
      .join("\n");
    const language =
      typeof node.attrs?.language === "string" ? node.attrs.language : "";
    if (language.length > 64 || text.length > 65536) fail("代码块过长");
    return { kind: "code_block", language, text };
  }
  if (node.type === "blockquote") {
    const children = node.content ?? [];
    if (children.length < 1) fail("引用不能为空");
    return {
      kind: "blockquote",
      content: children.map((child) => {
        if (child.type !== "paragraph") fail("引用只能包含段落");
        return paragraphFromTiptap(child);
      }),
    };
  }
  if (node.type === "bulletList" || node.type === "orderedList") {
    const items = node.content ?? [];
    if (items.length < 1) fail("列表不能为空");
    return {
      kind: node.type === "bulletList" ? "bullet_list" : "ordered_list",
      content: items.map((item) => {
        if (item.type !== "listItem") fail("列表只能包含 listItem");
        const paragraphs = item.content ?? [];
        if (paragraphs.length < 1) fail("listItem 至少一段");
        if (paragraphs.some((child) => child.type !== "paragraph")) {
          fail("listItem 不得嵌套列表或其他块");
        }
        return {
          kind: "list_item",
          content: paragraphs.map(paragraphFromTiptap),
        };
      }),
    };
  }
  fail(`未知富文本节点 ${node.type}`);
}

export function richTextToTiptap(content: RichTextContent): TiptapNode {
  return { type: "doc", content: content.nodes.map(richNodeToTiptap) };
}

export function tiptapToRichText(doc: TiptapNode): RichTextContent {
  if (doc.type !== "doc") fail("富文本根节点必须是 doc");
  const children = doc.content ?? [];
  for (const child of children) {
    if (!RICH_DOC_TYPES.has(child.type)) fail(`章节正文不允许 ${child.type}`);
  }
  if (children.length === 0) return emptyRichText();
  return { type: "rich_text", nodes: children.map(richNodeFromTiptap) };
}

export function tableToTiptap(content: TableContent): TiptapNode {
  validateTableGrid(content);
  const occupancy = Array.from({ length: content.row_count }, () =>
    Array<boolean>(content.column_count).fill(false),
  );
  const byPos = new Map<string, TableContent["cells"][number]>();
  for (const cell of content.cells)
    byPos.set(`${cell.row}:${cell.column}`, cell);
  const rows: TiptapNode[] = [];
  for (let row = 0; row < content.row_count; row += 1) {
    const cells: TiptapNode[] = [];
    for (let column = 0; column < content.column_count; column += 1) {
      if (occupancy[row][column]) continue;
      const cell = byPos.get(`${row}:${column}`);
      if (!cell) fail(`表格缺单元格 ${row}:${column}`);
      for (let r = row; r < row + cell.rowspan; r += 1) {
        for (let c = column; c < column + cell.colspan; c += 1)
          occupancy[r][c] = true;
      }
      const isHeader = row < content.repeat_header_rows;
      cells.push({
        type: isHeader ? "tableHeader" : "tableCell",
        attrs: { colspan: cell.colspan, rowspan: cell.rowspan },
        content: cell.content.map(richNodeToTiptap),
      });
    }
    rows.push({ type: "tableRow", content: cells });
  }
  return {
    type: "table",
    attrs: {
      widthsMm: content.widths_mm,
      repeatHeaderRows: content.repeat_header_rows,
    },
    content: rows,
  };
}

export function tiptapToTable(doc: TiptapNode): TableContent {
  const table = doc.type === "doc" ? doc.content?.[0] : doc;
  if (!table || table.type !== "table") fail("表格根节点必须是 table");
  const rows = table.content ?? [];
  if (rows.length < 1) fail("表格至少一行");
  const widthsMmRaw = attrs(table).widthsMm;
  const repeatHeaderRows =
    typeof attrs(table).repeatHeaderRows === "number"
      ? asNumber(attrs(table).repeatHeaderRows, "repeatHeaderRows")
      : 0;
  let columnCount = 0;
  const occupancy: boolean[][] = [];
  const cells: TableContent["cells"] = [];
  for (let row = 0; row < rows.length; row += 1) {
    const rowNode = rows[row];
    if (rowNode.type !== "tableRow") fail("表格只能包含 tableRow");
    occupancy[row] ??= [];
    let column = 0;
    for (const cellNode of rowNode.content ?? []) {
      if (cellNode.type !== "tableCell" && cellNode.type !== "tableHeader")
        fail(`未知单元格 ${cellNode.type}`);
      while (occupancy[row][column]) column += 1;
      const colspan =
        typeof attrs(cellNode).colspan === "number"
          ? asNumber(attrs(cellNode).colspan, "colspan")
          : 1;
      const rowspan =
        typeof attrs(cellNode).rowspan === "number"
          ? asNumber(attrs(cellNode).rowspan, "rowspan")
          : 1;
      if (colspan < 1 || rowspan < 1) fail("非法合并单元格");
      for (let r = row; r < row + rowspan; r += 1) {
        occupancy[r] ??= [];
        for (let c = column; c < column + colspan; c += 1) {
          if (occupancy[r][c]) fail("表格单元格重叠");
          occupancy[r][c] = true;
        }
      }
      cells.push({
        row,
        column,
        rowspan,
        colspan,
        content: (cellNode.content ?? []).map(richNodeFromTiptap),
      });
      column += colspan;
    }
    columnCount = Math.max(columnCount, occupancy[row].length);
  }
  const rowCount = rows.length;
  const widthsMm = Array.isArray(widthsMmRaw)
    ? widthsMmRaw.map((value, i) => asNumber(value, `widthsMm[${i}]`))
    : Array.from({ length: columnCount }, () => 160 / Math.max(columnCount, 1));
  const content: TableContent = {
    type: "table",
    row_count: rowCount,
    column_count: columnCount,
    cells,
    widths_mm: widthsMm,
    repeat_header_rows: repeatHeaderRows,
  };
  validateTableGrid(content);
  return content;
}

export function contentBlockToEditorModel(block: ContentBlockV1): EditorModel {
  switch (block.kind) {
    case "rich_text":
      return { kind: "rich_text", doc: richTextToTiptap(block.content) };
    case "table":
      return { kind: "table", doc: tableToTiptap(block.content) };
    case "image":
      return { kind: "image", content: block.content };
    case "attachment_ref":
      return { kind: "attachment_ref", content: block.content };
    case "structured_form":
      return { kind: "structured_form", content: block.content };
    case "page_break":
      return { kind: "page_break" };
    case "signature_placeholder":
      return { kind: "signature_placeholder", content: block.content };
  }
}

export function applyEditorModel(
  block: ContentBlockV1,
  model: EditorModel,
): ContentBlockV1 {
  if (model.kind !== block.kind) fail("编辑模型与当前块类型不一致");
  if (model.kind === "rich_text" && block.kind === "rich_text") {
    return { ...block, content: tiptapToRichText(model.doc), origin: "human" };
  }
  if (model.kind === "table" && block.kind === "table") {
    return { ...block, content: tiptapToTable(model.doc), origin: "human" };
  }
  if (model.kind === "image" && block.kind === "image")
    return { ...block, content: model.content, origin: "human" };
  if (model.kind === "attachment_ref" && block.kind === "attachment_ref") {
    return { ...block, content: model.content, origin: "human" };
  }
  if (model.kind === "structured_form" && block.kind === "structured_form") {
    return { ...block, content: model.content, origin: "human" };
  }
  if (
    model.kind === "signature_placeholder" &&
    block.kind === "signature_placeholder"
  ) {
    return { ...block, content: model.content, origin: "human" };
  }
  return block;
}

function blocksToChapterContent(blocks: ContentBlockV1[]): TiptapNode[] {
  const content: TiptapNode[] = [];
  for (const block of blocks) {
    if (block.kind === "rich_text") {
      content.push(...block.content.nodes.map(richNodeToTiptap));
    } else if (block.kind === "table") {
      content.push(tableToTiptap(block.content));
    } else if (block.kind === "image") {
      content.push({
        type: "bidImage",
        attrs: {
          assetRevisionId: block.content.asset_revision_id,
          widthMm: block.content.width_mm,
          alignment: block.content.alignment,
          alt: block.content.alt,
          caption: block.content.caption ?? "",
          cropLeft: block.content.crop.left,
          cropTop: block.content.crop.top,
          cropRight: block.content.crop.right,
          cropBottom: block.content.crop.bottom,
        },
      });
    } else if (block.kind === "page_break") {
      content.push({ type: "pageBreak" });
    } else if (block.kind === "signature_placeholder") {
      content.push({
        type: "signature",
        attrs: {
          signatureKind: block.content.signature_kind,
          widthMm: block.content.width_mm,
          heightMm: block.content.height_mm,
          label: block.content.label,
        },
      });
    } else {
      content.push({
        type: "paragraph",
        content: [{ type: "text", text: `[${block.kind}]` }],
      });
    }
  }
  if (content.length === 0) content.push({ type: "paragraph" });
  return content;
}

export function outlineToDoc(
  nodes: FlattenedNode[],
  blocksOf: (node: FlattenedNode) => ContentBlockV1[],
): TiptapNode {
  return {
    type: "doc",
    content: nodes.map((node) => ({
      type: "chapter",
      attrs: {
        lineageId: node.lineage_id,
        depth: node.depth,
      },
      content: [
        {
          type: "chapterTitle",
          attrs: { depth: node.depth },
          content: node.title ? [{ type: "text", text: node.title }] : [],
        },
        ...blocksToChapterContent(blocksOf(node)),
      ],
    })),
  };
}

export type ChapterPatchBlock =
  | { kind: "rich_text"; nodes: RichNode[] }
  | { kind: "table"; content: TableContent }
  | { kind: "image"; content: ImageContent }
  | { kind: "page_break" }
  | { kind: "signature_placeholder"; content: SignatureContent };

export type ChapterPatch = {
  lineageId: string;
  title: string;
  blocks: ChapterPatchBlock[];
};

export function docToChapterPatches(doc: TiptapNode): ChapterPatch[] {
  if (doc.type !== "doc") fail("根节点必须是 doc");
  const patches: ChapterPatch[] = [];
  for (const chapter of doc.content ?? []) {
    if (chapter.type !== "chapter") continue;
    const lineageId = asString(chapter.attrs?.lineageId, "lineageId");
    const children = [...(chapter.content ?? [])];
    let title = "";
    if (children[0]?.type === "chapterTitle") {
      const heading = children.shift();
      title = (heading?.content ?? [])
        .map((part) => part.text ?? "")
        .join("")
        .trim();
    }
    const blocks: ChapterPatchBlock[] = [];
    let rich: RichNode[] = [];
    const flush = () => {
      if (!rich.length) return;
      blocks.push({ kind: "rich_text", nodes: rich });
      rich = [];
    };
    for (const child of children) {
      if (RICH_DOC_TYPES.has(child.type)) {
        rich.push(richNodeFromTiptap(child));
        continue;
      }
      flush();
      if (child.type === "table") {
        blocks.push({ kind: "table", content: tiptapToTable(child) });
      } else if (child.type === "pageBreak") {
        blocks.push({ kind: "page_break" });
      } else if (child.type === "signature") {
        const kind = child.attrs?.signatureKind;
        blocks.push({
          kind: "signature_placeholder",
          content: {
            type: "signature_placeholder",
            signature_kind:
              kind === "seal" || kind === "date" ? kind : "signature",
            width_mm: Number(child.attrs?.widthMm ?? 40),
            height_mm: Number(child.attrs?.heightMm ?? 20),
            label: String(child.attrs?.label ?? "签字"),
          },
        });
      } else if (child.type === "bidImage") {
        blocks.push({
          kind: "image",
          content: {
            type: "image",
            asset_revision_id: asString(
              child.attrs?.assetRevisionId,
              "assetRevisionId",
            ),
            width_mm: Number(child.attrs?.widthMm ?? 80),
            alignment:
              child.attrs?.alignment === "left" ||
              child.attrs?.alignment === "right"
                ? child.attrs.alignment
                : "center",
            crop: {
              left: Number(child.attrs?.cropLeft ?? 0),
              top: Number(child.attrs?.cropTop ?? 0),
              right: Number(child.attrs?.cropRight ?? 0),
              bottom: Number(child.attrs?.cropBottom ?? 0),
            },
            caption: String(child.attrs?.caption ?? "") || undefined,
            alt: String(child.attrs?.alt ?? ""),
          },
        });
      }
    }
    flush();
    patches.push({ lineageId, title, blocks });
  }
  return patches;
}
