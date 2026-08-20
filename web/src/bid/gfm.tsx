import type { ReactNode } from "react";
import { Text } from "@mantine/core";

function inline(s: string) {
  const parts = s.split(/(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`)/g);
  return parts.map((p, i) => {
    if (p.startsWith("**") && p.endsWith("**")) return <strong key={i}>{p.slice(2, -2)}</strong>;
    if (p.startsWith("*") && p.endsWith("*")) return <em key={i}>{p.slice(1, -1)}</em>;
    if (p.startsWith("`") && p.endsWith("`")) return <code key={i}>{p.slice(1, -1)}</code>;
    return <span key={i}>{p}</span>;
  });
}

export function GfmPreview({ markdown }: { markdown: string }) {
  const lines = markdown.replace(/<!--[\s\S]*?-->/g, "").split("\n");
  const nodes: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i += 1;
      continue;
    }
    if (line.startsWith("# ")) {
      nodes.push(
        <Text key={i} fw={600} fz={22} mt="sm">
          {inline(line.slice(2))}
        </Text>,
      );
      i += 1;
      continue;
    }
    if (line.startsWith("## ")) {
      nodes.push(
        <Text key={i} fw={600} fz={17} mt="sm">
          {inline(line.slice(3))}
        </Text>,
      );
      i += 1;
      continue;
    }
    if (line.startsWith("### ")) {
      nodes.push(
        <Text key={i} fw={600} fz={15} mt="sm">
          {inline(line.slice(4))}
        </Text>,
      );
      i += 1;
      continue;
    }
    if (line.startsWith("|")) {
      const rows: string[][] = [];
      while (i < lines.length && lines[i].startsWith("|")) {
        const cells = lines[i]
          .split("|")
          .slice(1, -1)
          .map((c) => c.trim());
        if (!cells.every((c) => /^[-:]+$/.test(c))) rows.push(cells);
        i += 1;
      }
      nodes.push(
        <table key={`t${i}`} style={{ width: "100%", borderCollapse: "collapse", margin: "8px 0" }}>
          <tbody>
            {rows.map((r, ri) => (
              <tr key={ri}>
                {r.map((c, ci) => (
                  <td key={ci} style={{ borderBottom: "1px solid rgba(60,60,67,0.12)", padding: "6px 8px", textAlign: "left" }}>
                    {inline(c)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>,
      );
      continue;
    }
    if (line.startsWith("- ") || line.startsWith("* ")) {
      const items: string[] = [];
      while (i < lines.length && (lines[i].startsWith("- ") || lines[i].startsWith("* "))) {
        items.push(lines[i].slice(2));
        i += 1;
      }
      nodes.push(
        <ul key={`u${i}`} style={{ margin: "8px 0 8px 1.2em" }}>
          {items.map((it, ii) => (
            <li key={ii}>
              <Text size="sm">{inline(it)}</Text>
            </li>
          ))}
        </ul>,
      );
      continue;
    }
    nodes.push(
      <Text key={i} size="sm" mt={6}>
        {inline(line)}
      </Text>,
    );
    i += 1;
  }
  return <div style={{ maxWidth: "65ch" }}>{nodes}</div>;
}
