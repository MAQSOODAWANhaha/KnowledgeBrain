import { type ReactNode, useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { fileBlob } from "../api";

function slug(text: string): string {
  return text
    .replace(/\s+/g, "-")
    .replace(/[^\w\u4e00-\u9fff.-]+/g, "")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 80);
}

function flatten(node: ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(flatten).join("");
  if (typeof node === "object" && "props" in node) {
    return flatten(
      (node as { props?: { children?: ReactNode } }).props?.children,
    );
  }
  return "";
}

const LEADER = /[\u00b7\u2022\u22c5\u2027\uff0e.]{4,}/;

function collapseLeaders(line: string): string {
  return line
    .replace(LEADER, " · ")
    .replace(/\s{2,}/g, " ")
    .replace(/\s+(\d+)\s*$/, "  $1")
    .trim();
}

/** Body-tab plain text: keep PDF line breaks, collapse TOC leader dots. */
export function formatPlainLayout(text: string): string {
  return text
    .split("\n")
    .map((line) => {
      const trimmed = line.trimEnd();
      if (LEADER.test(trimmed) && /\d+\s*$/.test(trimmed.trim()))
        return collapseLeaders(trimmed);
      return trimmed;
    })
    .join("\n");
}

/** PDF TOC is one visual line per entry; GFM joins those into one wrapping paragraph. */
function isolatePdfLayout(markdown: string): string {
  const out: string[] = [];
  for (const line of markdown.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (out.length && out[out.length - 1] !== "") out.push("");
      continue;
    }
    if (
      /^#/.test(trimmed) ||
      trimmed.startsWith("|") ||
      trimmed.startsWith("```")
    ) {
      out.push(line);
      continue;
    }
    if (LEADER.test(trimmed) && /\d+\s*$/.test(trimmed)) {
      const item = collapseLeaders(trimmed);
      if (out.length && out[out.length - 1] !== "") out.push("");
      out.push(`- ${item}`);
      continue;
    }
    if (/^\u00a9/.test(trimmed) || /^\(c\)/i.test(trimmed)) {
      if (out.length && out[out.length - 1] !== "") out.push("");
      out.push(`*${trimmed}*`);
      out.push("");
      continue;
    }
    out.push(line);
  }
  return out.join("\n");
}

/** Word TOC `](_Toc123)` → `#1-引言` so in-page jump works. */
function rewriteTocLinks(markdown: string): string {
  return markdown.replace(/\[([^\]]+)\]\((_Toc\d+)\)/g, (_, label: string) => {
    const text = label
      .replace(/\*+/g, "")
      .replace(/\s+\d+\s*$/g, "")
      .replace(/\s+/g, " ")
      .trim();
    return `[${text}](#${slug(text)})`;
  });
}

function objectKey(src: string): string | null {
  const t = src.trim();
  if (t.startsWith("objects/")) return t;
  if (/^[a-f0-9]{32,}$/i.test(t)) return `objects/${t}`;
  return null;
}

function MdImage({ src, alt }: { src?: string; alt?: string }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!src) return;
    if (/^https?:\/\//i.test(src) || src.startsWith("data:")) {
      setUrl(src);
      return;
    }
    const key = objectKey(src);
    if (!key) return;
    let alive = true;
    let created: string | null = null;
    fileBlob(key)
      .then((u) => {
        created = u;
        if (alive) setUrl(u);
        else URL.revokeObjectURL(u);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      if (created) URL.revokeObjectURL(created);
    };
  }, [src]);
  if (!url) {
    return <span className="note">{alt ? `图：${alt}` : "图"}</span>;
  }
  return <img className="md-img" src={url} alt={alt || ""} />;
}

function heading(tag: "h1" | "h2" | "h3" | "h4") {
  return function Heading({ children }: { children?: ReactNode }) {
    const Tag = tag;
    const id = slug(flatten(children));
    return (
      <Tag id={id} className={`md-${tag}`}>
        {children}
      </Tag>
    );
  };
}

export function GfmPreview({ markdown }: { markdown: string }) {
  const src = rewriteTocLinks(
    isolatePdfLayout(markdown.replace(/<!--[\s\S]*?-->/g, "")),
  );
  return (
    <div className="md-doc">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          img: ({ src, alt }) => <MdImage src={src} alt={alt} />,
          a: ({ href, children }) => {
            const raw = href || "";
            const inPage = raw.startsWith("#") || raw.startsWith("_Toc");
            if (inPage) {
              return (
                <a
                  href={
                    typeof location !== "undefined" ? location.hash || "#" : "#"
                  }
                  onClick={(e) => {
                    e.preventDefault();
                    const id = raw.replace(/^#/, "");
                    const el =
                      document.getElementById(id) ||
                      document.getElementById(slug(flatten(children)));
                    el?.scrollIntoView({ block: "start", behavior: "smooth" });
                  }}
                >
                  {children}
                </a>
              );
            }
            if (href && /^https?:\/\//i.test(href)) {
              return (
                <a href={href} target="_blank" rel="noreferrer">
                  {children}
                </a>
              );
            }
            return <a href={href}>{children}</a>;
          },
          h1: heading("h1"),
          h2: heading("h2"),
          h3: heading("h3"),
          h4: heading("h4"),
        }}
      >
        {src}
      </Markdown>
    </div>
  );
}
