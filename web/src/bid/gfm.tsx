import { Markdown } from "@astryxdesign/core/Markdown";
import { VStack } from "@astryxdesign/core/VStack";
import { useEffect, useRef, useState } from "react";
import { fileBlob } from "../api";

function slug(text: string): string {
  return text
    .replace(/\s+/g, "-")
    .replace(/[^\w\u4e00-\u9fff.-]+/g, "")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 80);
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

const IMAGE = /!\[([^\]]*)\]\(([^)\s]+)\)/g;

// Parsed documents reference images by object key or inline data; both become blob URLs the renderer accepts.
function useImageUrls(markdown: string): Map<string, string> {
  const [urls, setUrls] = useState(new Map<string, string>());
  useEffect(() => {
    const sources = [...new Set([...markdown.matchAll(IMAGE)].map((match) => match[2]).filter((src) => objectKey(src) || src.startsWith("data:")))];
    let alive = true;
    const created: string[] = [];
    void Promise.all(sources.map(async (src) => {
      try {
        const url = src.startsWith("data:") ? URL.createObjectURL(await (await fetch(src)).blob()) : await fileBlob(objectKey(src)!);
        created.push(url);
        return [src, url] as const;
      } catch { return null; }
    })).then((pairs) => {
      if (alive) setUrls(new Map(pairs.filter((pair): pair is readonly [string, string] => !!pair)));
      else created.forEach((url) => URL.revokeObjectURL(url));
    });
    return () => { alive = false; created.forEach((url) => URL.revokeObjectURL(url)); };
  }, [markdown]);
  return urls;
}

export function GfmPreview({ markdown }: { markdown: string }) {
  const source = rewriteTocLinks(isolatePdfLayout(markdown.replace(/<!--[\s\S]*?-->/g, "")));
  const urls = useImageUrls(source);
  const resolved = source.replace(IMAGE, (whole, alt: string, src: string) => {
    if (!objectKey(src) && !src.startsWith("data:")) return whole;
    const url = urls.get(src);
    return url ? `![${alt}](${url})` : `*${alt ? `图：${alt}` : "图"}*`;
  });
  const container = useRef<HTMLElement>(null);
  return (
    <VStack ref={container} gap={0}>
      <Markdown autolink="gfm" contentWidth="100%"
        onLinkClick={(href) => {
          if (!href.startsWith("#") && !href.startsWith("_Toc")) return;
          const id = href.replace(/^#/, "");
          const headings = [...(container.current?.querySelectorAll("h1, h2, h3, h4, h5, h6") ?? [])];
          headings.find((heading) => heading.id === id || slug(heading.textContent ?? "") === id)?.scrollIntoView({ block: "start", behavior: "smooth" });
          return false;
        }}>
        {resolved}
      </Markdown>
    </VStack>
  );
}
