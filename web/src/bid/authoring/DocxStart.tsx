import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { guardHashNavigation } from "../../hash";
import { compositionApi } from "../api/composition";
import { docxApi } from "../api/docx";
import { createCompositionSession, type CompositionAttempt } from "./compositionSession";
import { DocxRound } from "./DocxRound";

type Props = { workspaceId: string; onCancel: () => void; onPublished: () => void; onUnsafeChange: (value: boolean) => void };
export function DocxStart(props: Props) {
  const [mode, setMode] = useState<"generate" | "upload">("generate");
  const [unsafe, setUnsafe] = useState(false);
  const { onUnsafeChange } = props;
  const changeUnsafe = useCallback((value: boolean) => { setUnsafe(value); onUnsafeChange(value); }, [onUnsafeChange]);
  return <>
    <div className="toolbar">
      <button className="btn" disabled={unsafe || mode === "generate"} onClick={() => setMode("generate")}>按招标要求生成</button>
      <button className="btn ghost" disabled={unsafe || mode === "upload"} onClick={() => setMode("upload")}>上传已有 DOCX</button>
    </div>
    {mode === "generate" ? <Composition {...props} onUnsafeChange={changeUnsafe} /> : <DocxRound {...props} onUnsafeChange={changeUnsafe} />}
  </>;
}
function Composition({ workspaceId, onCancel, onPublished, onUnsafeChange }: Props) {
  const [session] = useState(() => createCompositionSession({ ...docxApi, ...compositionApi }, workspaceId, {
    read() { const raw = sessionStorage.getItem(`kb.docx-compose:${workspaceId}`); return raw ? JSON.parse(raw) as CompositionAttempt : null; },
    write(value) { if (value) sessionStorage.setItem(`kb.docx-compose:${workspaceId}`, JSON.stringify(value)); else sessionStorage.removeItem(`kb.docx-compose:${workspaceId}`); },
  }));
  const state = useSyncExternalStore(session.subscribe, session.getState);
  const [downloadError, setDownloadError] = useState(false);
  useEffect(() => { void session.load(); return session.cancelRead; }, [session]);
  useEffect(() => { onUnsafeChange(session.unsafe()); }, [session, state, onUnsafeChange]);
  useEffect(() => {
    if (state.phase !== "running") return;
    const timer = setInterval(() => { void session.poll(); }, 3000);
    return () => clearInterval(timer);
  }, [session, state.phase]);
  useEffect(() => {
    const originalHash = location.hash;
    const unguard = guardHashNavigation(() => {
      if (!session.unsafe()) return true;
      history.replaceState(null, "", `${location.pathname}${location.search}${originalHash}`); return false;
    });
    const unload = (event: BeforeUnloadEvent) => { if (session.unsafe()) { event.preventDefault(); event.returnValue = ""; } };
    const click = (event: MouseEvent) => { if (session.unsafe() && event.target instanceof Element && event.target.closest("a[href]")) { event.preventDefault(); event.stopPropagation(); } };
    window.addEventListener("beforeunload", unload); document.addEventListener("click", click, true);
    return () => { unguard(); window.removeEventListener("beforeunload", unload); document.removeEventListener("click", click, true); };
  }, [session]);
  const result = state.job?.result_identity;
  async function download() {
    if (!result) return;
    setDownloadError(false);
    try {
      const url = URL.createObjectURL(await docxApi.download(workspaceId, result.version_id));
      const link = document.createElement("a"); link.href = url; link.download = `投标模板-${result.round_revision}.docx`; link.click(); URL.revokeObjectURL(url);
    } catch { setDownloadError(true); }
  }
  const progress = state.job?.progress;
  const phase = progress?.phase;
  const progressText = phase === "verifying" ? "正在复核章节、附表及对应要求…"
    : phase === "publishing" ? "复核完成，正在保存完整稿件…"
    : phase === "retrying" ? "正在从已有进度恢复…" : "正在按招标要求分章编制…";
  return <section data-testid="docx-composition">
    <h2>生成完整投标模板</h2>
    <p>根据当前招标文件对投标文件的组成、顺序和格式要求，逐章生成内容与附表，并复核对应关系。投标人资料、实际响应、价格和证明材料保留待填。</p>
    {state.input?.expected && <p>生成成功后创建新轮。旧轮保存稿保留；若当前稿件或招标依据已变化，本次生成不会替换它。</p>}
    {state.phase === "loading" && <p role="status">正在读取招标分析和已有任务…</p>}
    {state.phase === "sending" && <p role="status">正在提交生成请求…</p>}
    {state.phase === "running" && <div role="status"><p>{progressText}</p>
      {typeof progress?.detail.sections === "number" && <p>已编制 {progress.detail.sections} 个章节，尚待整稿完成。</p>}
      <p>任务在后台继续执行，可离开后返回查看。</p></div>}
    {state.error && <p role="alert">{state.error}</p>}
    {state.phase === "completed" && result && <>
      <p role="status">第 {result.round_revision} 轮投标模板已生成。</p>
      {result.composition_status === "reviewed_template_with_open_items" && <p>招标来源仍有待确认项，请核对来源完整性后使用此稿。</p>}
      <button className="btn" onClick={onPublished}>进入当前稿件</button>
      <button className="btn ghost" onClick={() => void download()}>下载本次生成稿</button>
      {downloadError && <p role="alert">无法下载生成稿，请稍后重试。</p>}
    </>}
    {(state.phase === "ready" || state.phase === "uncertain") && <button className="btn" onClick={() => void session.start()}>
      {state.phase === "uncertain" ? "确认本次生成请求" : "生成完整投标模板"}</button>}
    {state.phase === "running" ? <button className="btn ghost" onClick={() => void session.poll()}>刷新生成进度</button>
      : <button className="btn ghost" disabled={session.unsafe() || state.phase === "loading"} onClick={() => void session.load(true)}>
        {state.phase === "completed" || state.phase === "failed" ? "准备重新生成" : "刷新编制依据"}</button>}
    <button className="btn ghost" disabled={session.unsafe()} onClick={onCancel}>返回</button>
  </section>;
}
