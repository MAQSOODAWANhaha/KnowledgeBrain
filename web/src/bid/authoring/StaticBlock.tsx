import type { ReactNode } from "react";
import type {
  ContentBlockV1,
  Inline,
  RichNode,
  TableContent,
} from "./contentBlock";

function inlineView(inline: Inline, key: number) {
  if (inline.kind === "hard_break") return <br key={key} />;
  let node: ReactNode = inline.text;
  for (const mark of inline.marks ?? []) {
    if (mark.kind === "bold") node = <strong>{node}</strong>;
    else if (mark.kind === "italic") node = <em>{node}</em>;
    else if (mark.kind === "underline") node = <u>{node}</u>;
    else if (mark.kind === "strike") node = <s>{node}</s>;
    else if (mark.kind === "code") node = <code>{node}</code>;
    else if (mark.kind === "link") node = <a href={mark.href}>{node}</a>;
  }
  return <span key={key}>{node}</span>;
}

function richView(nodes: RichNode[]) {
  return nodes.map((node, index) => {
    if (node.kind === "paragraph") {
      return <p key={index}>{node.content.map(inlineView)}</p>;
    }
    if (node.kind === "horizontal_rule") return <hr key={index} />;
    if (node.kind === "code_block") {
      return (
        <pre key={index}>
          <code>{node.text}</code>
        </pre>
      );
    }
    if (node.kind === "blockquote") {
      return <blockquote key={index}>{richView(node.content)}</blockquote>;
    }
    const Tag = node.kind === "bullet_list" ? "ul" : "ol";
    return (
      <Tag key={index}>
        {node.content.map((item, itemIndex) => (
          <li key={itemIndex}>{item.content.map((p) => richView([p]))}</li>
        ))}
      </Tag>
    );
  });
}

function tableView(content: TableContent) {
  const rows = [];
  for (let row = 0; row < content.row_count; row += 1) {
    const cells = content.cells.filter((cell) => cell.row === row);
    rows.push(
      <tr key={row}>
        {cells.map((cell) => (
          <td
            key={`${cell.row}:${cell.column}`}
            colSpan={cell.colspan}
            rowSpan={cell.rowspan}
          >
            {richView(cell.content)}
          </td>
        ))}
      </tr>,
    );
  }
  return (
    <table>
      <tbody>{rows}</tbody>
    </table>
  );
}

export function StaticBlock({ block }: { block: ContentBlockV1 }) {
  if (block.kind === "rich_text")
    return <div>{richView(block.content.nodes)}</div>;
  if (block.kind === "table") return tableView(block.content);
  if (block.kind === "image") {
    return (
      <p className="note">图片：{block.content.caption || block.content.alt}</p>
    );
  }
  if (block.kind === "attachment_ref") {
    return <p className="note">附件</p>;
  }
  if (block.kind === "structured_form") {
    return (
      <div>
        {block.content.field_values.map((field) => (
          <p key={field.field_id} className="note">
            {field.field_id}：{field.value}
          </p>
        ))}
      </div>
    );
  }
  if (block.kind === "page_break") return <p className="note">— 分页符 —</p>;
  return <p className="note">签章占位：{block.content.label}</p>;
}
