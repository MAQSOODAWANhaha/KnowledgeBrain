import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack } from "@astryxdesign/core/HStack";
import { Link } from "@astryxdesign/core/Link";
import { List, ListItem } from "@astryxdesign/core/List";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Tab as TabItem, TabList } from "@astryxdesign/core/TabList";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { useEffect, useMemo, useState } from "react";
import { type DocChunk, type DocContent, api } from "../api";
import { GfmPreview, formatPlainLayout } from "../bid/gfm";
import { FilePreview } from "./FilePreview";

type Tab = "file" | "parse" | "body" | "images" | "questions" | "summary" | "wiki";

function isBody(c: DocChunk): boolean {
  return c.chunk_type === "text" || c.chunk_type === "parent" || c.chunk_type === "child";
}

function groupImages(chunks: DocChunk[]): { key: string; ocr?: DocChunk; caption?: DocChunk }[] {
  const map = new Map<string, { key: string; ocr?: DocChunk; caption?: DocChunk }>();
  for (const c of chunks) {
    if (c.chunk_type !== "image_ocr" && c.chunk_type !== "image_caption") continue;
    const key = c.context_header || c.id;
    const cur = map.get(key) ?? { key };
    if (c.chunk_type === "image_ocr") cur.ocr = c;
    else cur.caption = c;
    map.set(key, cur);
  }
  return [...map.values()];
}

export function DocumentDetail({
  docId,
  backHref,
}: {
  docId: string;
  backHref: string;
}) {
  const [tab, setTab] = useState<Tab>("file");
  const [data, setData] = useState<DocContent | null>(null);
  const [err, setErr] = useState("");

  useEffect(() => {
    let dead = false;
    setData(null);
    setErr("");
    setTab("file");
    api
      .documentContent(docId)
      .then((d) => {
        if (!dead) setData(d);
      })
      .catch((e) => {
        if (!dead) setErr(e instanceof Error ? e.message : "加载失败");
      });
    return () => {
      dead = true;
    };
  }, [docId]);

  const groups = useMemo(() => {
    const chunks = data?.chunks ?? [];
    const body = chunks.filter(isBody).sort((a, b) => a.start_at - b.start_at || a.id.localeCompare(b.id));
    return {
      body,
      images: groupImages(chunks),
      questions: chunks.filter((c) => c.chunk_type === "question"),
      summary: chunks.filter((c) => c.chunk_type === "summary"),
      wiki: chunks.filter((c) => c.chunk_type === "wiki_page"),
    };
  }, [data]);

  if (err) {
    return <Banner role="alert" status="error" title={err} endContent={<Link href={`#${backHref}`}>返回</Link>} />;
  }
  if (!data) {
    return <Card><VStack gap={3}><Skeleton height={44} /><Skeleton height={320} /></VStack></Card>;
  }
  const tabs: { key: Tab; label: string; n?: number; hideIfEmpty?: boolean }[] = [
    { key: "file", label: "原件" },
    { key: "parse", label: "解析" },
    { key: "body", label: "正文", n: groups.body.length },
    { key: "images", label: "图像", n: groups.images.length, hideIfEmpty: true },
    { key: "questions", label: "问句", n: groups.questions.length, hideIfEmpty: true },
    { key: "summary", label: "摘要", n: groups.summary.length, hideIfEmpty: true },
    { key: "wiki", label: "Wiki", n: groups.wiki.length, hideIfEmpty: true },
  ];
  return (
    <VStack gap={4}>
      {data.error_message ? <Banner status="error" title={data.error_message} /> : null}
      <TabList aria-label="文档视图" value={tab} onChange={(value) => setTab(value as Tab)}>
        {tabs.filter((item) => !item.hideIfEmpty || (item.n ?? 0) > 0).map((item) => (
          <TabItem key={item.key} value={item.key} label={item.n != null ? `${item.label} ${item.n}` : item.label} />
        ))}
      </TabList>
      {tab === "file" ? <Card padding={0}><FilePreview fileName={data.file_name} objectKey={data.object_ref} /></Card> : null}
      {tab === "parse" ? (
        <Card>{data.markdown.trim() ? <GfmPreview markdown={data.markdown} /> : <EmptyState isCompact title="还没有解析正文" />}</Card>
      ) : null}
      {tab === "body" ? (
        <ChunkList emptyTitle="还没有正文分片" rows={groups.body.map((c, i) => ({
          key: c.id, tag: c.chunk_type === "text" ? "正文" : c.chunk_type,
          meta: `${i + 1}/${groups.body.length}${c.start_at || c.end_at ? ` · 字 ${c.start_at}–${c.end_at}` : ""}`,
          parts: [{ text: formatPlainLayout(c.content) }],
        }))} />
      ) : null}
      {tab === "images" ? (
        <ChunkList emptyTitle="没有图像块" rows={groups.images.map((g, i) => ({
          key: g.key, tag: `图 ${i + 1}`, meta: g.key.replace(/^objects\//, "").slice(0, 12),
          parts: [...(g.ocr ? [{ title: "OCR", text: g.ocr.content }] : []), ...(g.caption ? [{ title: "配图说明", text: g.caption.content }] : [])],
        }))} />
      ) : null}
      {tab === "questions" ? <ChunkList emptyTitle="还没有检索问句" rows={plainRows(groups.questions, "问句")} /> : null}
      {tab === "summary" ? <ChunkList emptyTitle="还没有摘要" rows={plainRows(groups.summary, "摘要")} /> : null}
      {tab === "wiki" ? <ChunkList emptyTitle="还没有 Wiki" rows={plainRows(groups.wiki, "Wiki")} /> : null}
    </VStack>
  );
}

type ChunkRow = { key: string; tag: string; meta: string; parts: { title?: string; text: string }[] };

function plainRows(chunks: DocChunk[], tag: string): ChunkRow[] {
  return chunks.map((c, i) => ({ key: c.id, tag, meta: `${i + 1}/${chunks.length}${c.context_header ? ` · ${c.context_header}` : ""}`, parts: [{ text: c.content }] }));
}

// Chunk text keeps its line breaks: one block per line.
function Lines({ text }: { text: string }) {
  return <VStack gap={0}>{text.split("\n").map((line, index) => <Text key={index} display="block">{line || "\u00a0"}</Text>)}</VStack>;
}

function ChunkList({ rows, emptyTitle }: { rows: ChunkRow[]; emptyTitle: string }) {
  if (!rows.length) return <Card><EmptyState isCompact title={emptyTitle} /></Card>;
  return (
    <Card padding={0}>
      <List hasDividers>
        {rows.map((row) => (
          <ListItem key={row.key} label={<HStack gap={2} vAlign="center"><Badge variant="neutral" label={row.tag} /><Text type="supporting" hasTabularNumbers>{row.meta}</Text></HStack>}
            description={<VStack gap={2}>{row.parts.map((part, index) => <VStack key={index} gap={1}>{part.title && <Text weight="semibold">{part.title}</Text>}<Lines text={part.text} /></VStack>)}</VStack>} />
        ))}
      </List>
    </Card>
  );
}
