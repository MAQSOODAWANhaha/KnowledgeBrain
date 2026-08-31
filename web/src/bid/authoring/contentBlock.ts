import { AuthoringLogicError } from "./errors";
import { sha256Hex } from "./ids";

export const BLOCK_KINDS = [
  "rich_text",
  "table",
  "image",
  "attachment_ref",
  "structured_form",
  "page_break",
  "signature_placeholder",
] as const;
export type BlockKind = (typeof BLOCK_KINDS)[number];

export const BLOCK_ORIGINS = [
  "human",
  "agent_candidate",
  "deterministic",
] as const;
export type BlockOrigin = (typeof BLOCK_ORIGINS)[number];

export type TextMark =
  | { kind: "bold" | "italic" | "underline" | "strike" | "code" }
  | { kind: "link"; href: string }
  | {
      kind: "evidence_ref";
      evidence_bundle_id: string;
      evidence_item_id: string;
      quote_start_offset: number;
      quote_end_offset: number;
    };

export type Inline =
  | { kind: "text"; text: string; marks?: TextMark[] }
  | { kind: "hard_break" };

export type Paragraph = { kind: "paragraph"; content: Inline[] };
export type ListItem = { kind: "list_item"; content: Paragraph[] };
export type RichNode =
  | Paragraph
  | { kind: "bullet_list" | "ordered_list"; content: ListItem[] }
  | { kind: "blockquote"; content: Paragraph[] }
  | { kind: "code_block"; language: string; text: string }
  | { kind: "horizontal_rule" };

export type RichTextContent = { type: "rich_text"; nodes: RichNode[] };

export type TableCell = {
  row: number;
  column: number;
  rowspan: number;
  colspan: number;
  content: RichNode[];
};

export type TableContent = {
  type: "table";
  row_count: number;
  column_count: number;
  cells: TableCell[];
  widths_mm: number[];
  repeat_header_rows: number;
};

export type ImageContent = {
  type: "image";
  asset_revision_id: string;
  width_mm: number;
  alignment: "left" | "center" | "right";
  crop: { left: number; top: number; right: number; bottom: number };
  caption?: string;
  alt: string;
};

export type AttachmentContent = {
  type: "attachment_ref";
  asset_revision_id: string;
  preparation_revision_id?: string | null;
  render_mode: "embedded_pages" | "file_reference";
  start_new_page: boolean;
};

export type StructuredFormContent = {
  type: "structured_form";
  form_definition_revision_id: string;
  field_values: Array<{ field_id: string; value: string }>;
};

export type PageBreakContent = { type: "page_break" };

export type SignatureContent = {
  type: "signature_placeholder";
  signature_kind: "signature" | "seal" | "date";
  width_mm: number;
  height_mm: number;
  label: string;
};

type BlockBase = {
  schema_version: 1;
  block_revision_id: string;
  lineage_id: string;
  revision: number;
  origin: BlockOrigin;
  content_sha256: string;
};

export type ContentBlockV1 = BlockBase &
  (
    | { kind: "rich_text"; content: RichTextContent }
    | { kind: "table"; content: TableContent }
    | { kind: "image"; content: ImageContent }
    | { kind: "attachment_ref"; content: AttachmentContent }
    | { kind: "structured_form"; content: StructuredFormContent }
    | { kind: "page_break"; content: PageBreakContent }
    | { kind: "signature_placeholder"; content: SignatureContent }
  );

export function emptyRichText(): RichTextContent {
  return { type: "rich_text", nodes: [{ kind: "paragraph", content: [] }] };
}

function emptyCell(row: number, column: number) {
  return {
    row,
    column,
    rowspan: 1,
    colspan: 1,
    content: [{ kind: "paragraph" as const, content: [] }],
  };
}

export function emptyTable(rows = 2, columns = 3): TableContent {
  const cells = [];
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1)
      cells.push(emptyCell(row, column));
  }
  return {
    type: "table",
    row_count: rows,
    column_count: columns,
    cells,
    widths_mm: Array.from({ length: columns }, () => 160 / columns),
    repeat_header_rows: 1,
  };
}

export function canonicalContentJson(
  content: ContentBlockV1["content"],
): string {
  return JSON.stringify(content);
}

export async function withContentSha256<T extends ContentBlockV1>(
  block: T,
): Promise<T> {
  return {
    ...block,
    content_sha256: await sha256Hex(canonicalContentJson(block.content)),
  };
}

export function validateTableGrid(content: TableContent): void {
  if (content.widths_mm.length !== content.column_count) {
    throw new AuthoringLogicError("TABLE_WIDTHS", "表格列宽数量必须等于列数");
  }
  if (content.repeat_header_rows > content.row_count) {
    throw new AuthoringLogicError("TABLE_HEADER", "表头行数不能超过总行数");
  }
  const cover = Array.from({ length: content.row_count }, () =>
    Array<boolean>(content.column_count).fill(false),
  );
  for (const cell of content.cells) {
    if (cell.rowspan < 1 || cell.colspan < 1) {
      throw new AuthoringLogicError("TABLE_SPAN", "rowspan/colspan 必须 ≥ 1");
    }
    if (
      cell.row < 0 ||
      cell.column < 0 ||
      cell.row + cell.rowspan > content.row_count ||
      cell.column + cell.colspan > content.column_count
    ) {
      throw new AuthoringLogicError("TABLE_BOUNDS", "单元格超出表格网格");
    }
    for (let row = cell.row; row < cell.row + cell.rowspan; row += 1) {
      for (
        let column = cell.column;
        column < cell.column + cell.colspan;
        column += 1
      ) {
        if (cover[row][column])
          throw new AuthoringLogicError("TABLE_OVERLAP", "表格单元格重叠");
        cover[row][column] = true;
      }
    }
  }
}

export function assertBlockKind(block: ContentBlockV1): void {
  if (block.kind === "table") validateTableGrid(block.content);
  if (
    block.kind !== block.content.type &&
    block.kind !== "page_break" &&
    block.kind !== "signature_placeholder"
  ) {
    if (block.kind === "rich_text" && block.content.type === "rich_text")
      return;
    if (
      block.kind === "attachment_ref" &&
      block.content.type === "attachment_ref"
    )
      return;
    if (
      block.kind === "structured_form" &&
      block.content.type === "structured_form"
    )
      return;
    if (block.kind === "image" && block.content.type === "image") return;
    throw new AuthoringLogicError(
      "BLOCK_KIND_MISMATCH",
      "ContentBlock kind 与 content.type 不一致",
    );
  }
}
