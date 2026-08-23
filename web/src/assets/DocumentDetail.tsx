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
    return (
      <div className="card">
        <p className="note" style={{ color: "var(--rose)" }}>
          {err}
        </p>
        <a className="btn" href={`#${backHref}`}>
          返回
        </a>
      </div>
    );
  }
  if (!data) return <div className="card">正在打开文件…</div>;

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
    <div className="stack doc-detail">
      <div className="card pad-0">
        <p className="note" style={{ margin: "12px 18px 8px" }}>
          {data.index_ready ? "已可检索。" : data.error_message || "解析还在进行。"}
          「解析」是完整 Markdown；「正文」只含切块后的检索正文，不含问句、Wiki、配图。
        </p>
        <div className="toolbar" style={{ borderBottom: 0 }}>
          {tabs
            .filter((t) => !t.hideIfEmpty || (t.n ?? 0) > 0)
            .map((t) => (
              <button
                key={t.key}
                className={`chip ${tab === t.key ? "iris" : ""}`}
                type="button"
                onClick={() => setTab(t.key)}
              >
                {t.label}
                {t.n != null ? <span style={{ marginLeft: 6, opacity: 0.7 }}>{t.n}</span> : null}
              </button>
            ))}
        </div>
      </div>
      {tab === "file" ? (
        <div className="card pad-0 preview-card">
          <FilePreview fileName={data.file_name} objectKey={data.object_ref} />
        </div>
      ) : null}
      {tab === "parse" ? (
        <div className="card pad-0 preview-card">
          {data.markdown.trim() ? (
            <div className="md-wrap">
              <GfmPreview markdown={data.markdown} />
            </div>
          ) : (
            <p className="note" style={{ padding: 24 }}>
              还没有解析正文。等 convert 完成后会出现在这里。
            </p>
          )}
        </div>
      ) : null}
      {tab === "body" ? <BodyChunks chunks={groups.body} /> : null}
      {tab === "images" ? <ImageChunks groups={groups.images} /> : null}
      {tab === "questions" ? (
        <ListChunks
          emptyTitle="还没有检索问句"
          emptyNote="问句是后处理生成的，用来辅助检索，不是原文。"
          chunks={groups.questions}
          label="问句"
        />
      ) : null}
      {tab === "summary" ? (
        <ListChunks emptyTitle="还没有摘要" emptyNote="摘要来自后处理，不是切块正文。" chunks={groups.summary} label="摘要" />
      ) : null}
      {tab === "wiki" ? (
        <ListChunks emptyTitle="还没有 Wiki" emptyNote="Wiki 是从正文蒸馏的词条，单独存放。" chunks={groups.wiki} label="Wiki" />
      ) : null}
    </div>
  );
}

function BodyChunks({ chunks }: { chunks: DocChunk[] }) {
  if (chunks.length === 0) {
    return (
      <div className="card pad-0">
        <div className="empty">
          <h2>还没有正文分片</h2>
          <p className="note">切块完成后按文档顺序排列。完整连续文本看「解析」。问句和配图不在这里。</p>
        </div>
      </div>
    );
  }
  return (
    <div className="card pad-0">
      {chunks.map((c, i) => (
        <article key={c.id} className="chunk-row">
          <header>
            <span className="chip gray">{c.chunk_type === "text" ? "正文" : c.chunk_type}</span>
            <span className="muted">
              {i + 1}/{chunks.length}
              {c.start_at || c.end_at ? ` · 字 ${c.start_at}–${c.end_at}` : ""}
            </span>
          </header>
          <pre>{formatPlainLayout(c.content)}</pre>
        </article>
      ))}
    </div>
  );
}

function ImageChunks({ groups }: { groups: { key: string; ocr?: DocChunk; caption?: DocChunk }[] }) {
  if (groups.length === 0) {
    return (
      <div className="card pad-0">
        <div className="empty">
          <h2>没有图像块</h2>
          <p className="note">文中的图 OCR 和配图说明会按图归组。</p>
        </div>
      </div>
    );
  }
  return (
    <div className="card pad-0">
      {groups.map((g, i) => (
        <article key={g.key} className="chunk-row">
          <header>
            <span className="chip gray">图 {i + 1}</span>
            <span className="muted">{g.key.replace(/^objects\//, "").slice(0, 12)}</span>
          </header>
          {g.ocr ? (
            <>
              <div className="chunk-kicker">OCR</div>
              <pre>{g.ocr.content}</pre>
            </>
          ) : null}
          {g.caption ? (
            <>
              <div className="chunk-kicker">配图说明</div>
              <pre>{g.caption.content}</pre>
            </>
          ) : null}
        </article>
      ))}
    </div>
  );
}

function ListChunks({
  chunks,
  label,
  emptyTitle,
  emptyNote,
}: {
  chunks: DocChunk[];
  label: string;
  emptyTitle: string;
  emptyNote: string;
}) {
  if (chunks.length === 0) {
    return (
      <div className="card pad-0">
        <div className="empty">
          <h2>{emptyTitle}</h2>
          <p className="note">{emptyNote}</p>
        </div>
      </div>
    );
  }
  return (
    <div className="card pad-0">
      {chunks.map((c, i) => (
        <article key={c.id} className="chunk-row">
          <header>
            <span className="chip gray">{label}</span>
            <span className="muted">
              {i + 1}/{chunks.length}
              {c.context_header ? ` · ${c.context_header}` : ""}
            </span>
          </header>
          <pre>{c.content}</pre>
        </article>
      ))}
    </div>
  );
}
