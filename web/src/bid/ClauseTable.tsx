import { useMemo, useState } from "react";
import type { Clause } from "../api";
import { bidHref, go } from "../hash";
import { assessmentLabel, suggestionLabel } from "./helpers";

export function ClauseTable({
  id,
  view,
  live,
  selected,
  ended,
  addText,
  addMust,
  onSelect,
  onConfirm,
  onReject,
  onMerge,
  onAddText,
  onAddMust,
  onAdd,
  hasFiles = true,
  filesReady = true,
  extractRunning = false,
  retryStatus,
  retryError,
  onGoFiles,
  onExtract,
}: {
  id: string;
  view: string;
  live: Clause[];
  selected: string | null;
  ended: boolean;
  addText: string;
  addMust: boolean;
  onSelect: (id: string) => void;
  onConfirm: (c: Clause) => void;
  onReject: (c: Clause) => void;
  onMerge?: () => void;
  onAddText: (v: string) => void;
  onAddMust: (v: boolean) => void;
  onAdd: () => void;
  hasFiles?: boolean;
  filesReady?: boolean;
  extractRunning?: boolean;
  retryStatus?: string;
  retryError?: string;
  onGoFiles?: () => void;
  onExtract?: () => void;
}) {
  const [filter, setFilter] = useState<"all" | "draft" | "rejected" | "miss">("all");
  const rows = useMemo(() => {
    return live.filter((c) => {
      if (filter === "draft") return c.status === "draft";
      if (filter === "rejected") return c.status === "rejected";
      if (filter === "miss") return c.hit_outcome === "miss";
      return true;
    });
  }, [live, filter]);
  const pending = live.filter((c) => c.status === "draft").length;
  const misses = live.filter((c) => c.hit_outcome === "miss").length;
  const commercial = view === "commercial";

  return (
    <div className="stack">
      {retryStatus && retryStatus !== "done" && (
        <div className={`banner ${retryStatus === "failed" ? "bad" : ""}`}>
          {retryStatus === "pending" ? "本段重抽已排队" : retryStatus === "running" ? "本段正在重抽" : `本段重抽失败：${retryError || "请重试"}`}
        </div>
      )}
      {!ended && (onMerge || (onExtract && hasFiles && filesReady && !extractRunning && live.length > 0)) && (
        <div className="row">
          {onMerge && (
            <button className="btn" type="button" onClick={onMerge}>
              并入上一段
            </button>
          )}
          {onExtract && hasFiles && filesReady && !extractRunning && live.length > 0 && (
            <button className="btn" type="button" onClick={onExtract}>
              {view === "commercial" || view === "unsectioned" ? "重新抽取" : "重抽本段"}
            </button>
          )}
        </div>
      )}
      <div className="card pad-0">
        <div className="toolbar">
          <input className="inp" placeholder="按条款过滤…" />
          <button className={`chip ${filter === "all" ? "iris" : ""}`} type="button" onClick={() => setFilter("all")}>
            全部
          </button>
          <button className={`chip ${filter === "draft" ? "iris" : ""}`} type="button" onClick={() => setFilter("draft")}>
            待确认
          </button>
          <button className={`chip ${filter === "rejected" ? "iris" : ""}`} type="button" onClick={() => setFilter("rejected")}>
            已驳回
          </button>
          {commercial && (
            <button className={`chip ${filter === "miss" ? "iris" : ""}`} type="button" onClick={() => setFilter("miss")}>
              缺件
            </button>
          )}
        </div>
        <div className="group-h">
          <span>{commercial ? "资格与业绩" : "本段条款"}</span>
          <span>
            待确认 {pending}
            {commercial ? ` · 缺件 ${misses}` : ""}
          </span>
        </div>
        {rows.length === 0 ? (
          <div className="empty">
            {!hasFiles ? (
              <>
                <h2>先上传招标文件</h2>
                <p className="note" style={{ margin: "0 0 16px" }}>
                  解析抽出商务 / 技术条款后，再在这里确认和勾选。
                </p>
                {onGoFiles && (
                  <button className="btn pri" type="button" onClick={onGoFiles}>
                    去上传
                  </button>
                )}
              </>
            ) : extractRunning || !filesReady ? (
              <>
                <h2>{extractRunning ? "正在抽条款" : "文件解析中"}</h2>
                <p className="note">抽出的草稿会出现在这张表里。不用手补完整份标。</p>
              </>
            ) : (
              <>
                <h2>{commercial ? "还没有商务条款" : "这一段还没有技术条款"}</h2>
                <p className="note" style={{ margin: "0 0 16px" }}>
                  {commercial ? "抽取没出条。可以再抽一次，或在下面手补。" : "漏抽可以手补进当前段。"}
                </p>
                {onExtract && !ended && (
                  <button className="btn" type="button" onClick={onExtract}>
                    再抽一次
                  </button>
                )}
              </>
            )}
          </div>
        ) : (
          rows.map((c) => {
            const long = (c.text || "").length >= 36 && !commercial;
            return (
              <div
                key={c.id}
                className={`item ${c.id === selected ? "on" : ""}`}
                style={{ gridTemplateColumns: "1fr auto auto", cursor: "pointer" }}
                onClick={() => onSelect(c.id)}
              >
                <div>
                  <div className="name">{c.text || "（空）"}</div>
                  <div className="desc">
                    {c.must ? "必须 · " : ""}
                    {c.status === "confirmed" ? "已确认" : c.status === "rejected" ? "已驳回 · 未进匹配" : "待确认 · 未进匹配"}
                    {c.family_conflict ? " · 分类冲突，请核对" : ""}
                    {commercial && c.hit_outcome === "hit" ? ` · 命中 ${c.hit_file || "材料"}` : ""}
                    {commercial && c.hit_outcome === "miss" ? " · 资料库未命中" : ""}
                    {!commercial && c.suggestion ? ` · ${suggestionLabel(c.suggestion)}` : ""}
                  </div>
                </div>
                {commercial ? (
                  c.hit_outcome === "hit" ? (
                    <span className="chip pine">
                      <i className="dot" />
                      命中
                    </span>
                  ) : c.hit_outcome === "miss" ? (
                    <span className="chip rose">
                      <i className="dot" />
                      缺件
                    </span>
                  ) : (
                    <span className="chip gray">{c.status === "confirmed" ? "待检索" : "待确认"}</span>
                  )
                ) : (
                  <span className={`chip ${c.assessment && c.assessment !== "unset" ? "" : "gray"}`}>{assessmentLabel(c.assessment)}</span>
                )}
                {c.status === "draft" && !ended ? (
                  <div className="row">
                    <button
                      className="btn sm"
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onReject(c);
                      }}
                    >
                      驳回
                    </button>
                    <button
                      className="btn pri sm"
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onConfirm(c);
                      }}
                    >
                      确认
                    </button>
                  </div>
                ) : c.status === "rejected" ? (
                  <button className="btn sm" type="button" disabled>
                    已驳回
                  </button>
                ) : commercial && c.hit_outcome === "miss" ? (
                  <a className="btn sm" href="#/library" onClick={(e) => e.stopPropagation()}>
                    去补证
                  </a>
                ) : long ? (
                  <button
                    className="btn sm"
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      go(bidHref(id, view, { pane: "detail", clause: c.id }));
                    }}
                  >
                    深挖
                  </button>
                ) : (
                  <button className="btn sm" type="button" disabled>
                    已确认
                  </button>
                )}
              </div>
            );
          })
        )}
        {!ended && (
          <div style={{ padding: "14px 18px", borderTop: "1px solid var(--line-soft)" }}>
            <label className="fld">手补一条到当前段</label>
            <div className="row">
              <input className="inp" placeholder="招标原文里漏抽的句子…" value={addText} onChange={(e) => onAddText(e.target.value)} />
              <label className="row" style={{ color: "var(--muted)", fontSize: 12.5, whiteSpace: "nowrap" }}>
                <input type="checkbox" checked={addMust} onChange={(e) => onAddMust(e.target.checked)} />
                必须
              </label>
              <button className="btn" type="button" onClick={onAdd}>
                补入并确认
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
