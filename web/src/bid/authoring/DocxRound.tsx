import { useEffect, useState, useSyncExternalStore } from "react";
import { guardHashNavigation } from "../../hash";
import { docxApi } from "../api/docx";
import { createDocxRound } from "./docxRoundSession";

export function DocxRound({ workspaceId, onCancel, onPublished, onUnsafeChange }: {
  workspaceId: string; onCancel: () => void; onPublished: () => void; onUnsafeChange: (value: boolean) => void;
}) {
  const [round] = useState(() => createDocxRound(docxApi, workspaceId));
  const state = useSyncExternalStore(round.subscribe, round.getState);
  useEffect(() => { void round.load(); return round.cancelRead; }, [round]);
  useEffect(() => { onUnsafeChange(round.unsafe()); }, [round, state, onUnsafeChange]);
  useEffect(() => {
    const originalHash = location.hash;
    const unload = (event: BeforeUnloadEvent) => {
      if (round.unsafe()) { event.preventDefault(); event.returnValue = ""; }
    };
    const click = (event: MouseEvent) => {
      if (round.unsafe() && event.target instanceof Element && event.target.closest("a[href]")) {
        event.preventDefault(); event.stopPropagation();
      }
    };
    const unguard = guardHashNavigation(() => {
      if (!round.unsafe()) return true;
      history.replaceState(null, "", `${location.pathname}${location.search}${originalHash}`);
      return false;
    });
    window.addEventListener("beforeunload", unload);
    document.addEventListener("click", click, true);
    return () => { unguard(); window.removeEventListener("beforeunload", unload); document.removeEventListener("click", click, true); };
  }, [round]);
  return <section data-testid="docx-round">
    <h2>从 DOCX 创建新轮</h2>
    <p>选择已准备好的投标初稿或指定模板。发布后以所选文件开始编制，旧轮保存稿保留，旧轮编辑会话失效。</p>
    {state.input && <p>{state.input.expected ? "本次将替换当前编制轮次。" : "本次将创建首份 DOCX 投标稿。"}</p>}
    <label>投标初稿或模板（DOCX） <input type="file" accept=".docx"
      disabled={round.unsafe() || state.phase === "published"}
      onChange={event => round.select(event.target.files?.[0] ?? null)} /></label>
    {state.file && <p>已选择：{state.file.name}</p>}
    {state.phase === "loading" && <p role="status">正在核对新轮依据…</p>}
    {state.phase === "sending" && <p role="status">正在发布，请等待结果确认…</p>}
    {state.error && <p role="alert">{state.error}</p>}
    {state.phase === "published" ? <>
      <p role="status">第 {state.receipt?.round_revision} 轮已发布。</p>
      <button className="btn" onClick={onPublished}>进入当前稿件</button>
    </> : <>
      <button className="btn" disabled={state.phase !== "uncertain" && (state.phase !== "ready" || !state.file)}
        onClick={() => void round.publish()}>{state.phase === "uncertain" ? "确认发布结果" : "确认创建新轮"}</button>
      <button className="btn ghost" disabled={round.unsafe() || state.phase === "loading"}
        onClick={() => void round.load()}>刷新依据</button>
      <button className="btn ghost" disabled={round.unsafe()} onClick={onCancel}>取消</button>
    </>}
  </section>;
}
