import { useEffect, useState, useSyncExternalStore } from "react";
import { Button } from "../../components/ui/button";
import { Alert } from "../../components/ui/alert";
import { guardHashNavigation } from "../../hash";
import { docxApi } from "../api/docx";
import { downloadExport, downloadExportReport, exportsApi } from "../api/exports";
import { createExportSession, type ExportAttempt } from "./exportSession";

export function ExportPane({ workspaceId, ended, onUnsafeChange }: {
  workspaceId: string; ended: boolean; onUnsafeChange(value: boolean): void;
}) {
  const [session] = useState(() => createExportSession({ ...exportsApi, current: docxApi.current }, workspaceId, {
    read() {
      const raw = sessionStorage.getItem(`kb.docx-export:${workspaceId}`);
      if (!raw) return null;
      try { return JSON.parse(raw) as ExportAttempt; }
      catch { return null; }
    },
    write(value) {
      if (value) sessionStorage.setItem(`kb.docx-export:${workspaceId}`, JSON.stringify(value));
      else sessionStorage.removeItem(`kb.docx-export:${workspaceId}`);
    },
  }, ended));
  const state = useSyncExternalStore(session.subscribe, session.getState);
  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  useEffect(() => { void session.load(); return session.cancelRead; }, [session]);
  useEffect(() => { onUnsafeChange(session.unsafe()); }, [session, state, onUnsafeChange]);
  useEffect(() => {
    if (state.phase !== "running") return;
    const timer = setInterval(() => void session.poll(), 3000);
    return () => clearInterval(timer);
  }, [session, state.phase]);
  useEffect(() => {
    const originalHash = location.hash;
    const unguard = guardHashNavigation(() => {
      if (!session.unsafe()) return true;
      history.replaceState(null, "", `${location.pathname}${location.search}${originalHash}`); return false;
    });
    const unload = (event: BeforeUnloadEvent) => { if (session.unsafe()) { event.preventDefault(); event.returnValue = ""; } };
    window.addEventListener("beforeunload", unload);
    return () => { unguard(); window.removeEventListener("beforeunload", unload); };
  }, [session]);
  const result = state.result;
  async function download(format: "docx" | "pdf" | "report") {
    if (!result || downloading) return;
    setDownloading(true); setDownloadError(null);
    try {
      const blob = format === "report" ? await downloadExportReport(workspaceId, result.manifest_id)
        : await downloadExport(workspaceId, result.outputs[format].artifact_id);
      const url = URL.createObjectURL(blob); const link = document.createElement("a");
      link.href = url; link.download = `投标稿-${result.source.version_id}${format === "report" ? "-校核报告.json" : `.${format}`}`;
      link.click(); URL.revokeObjectURL(url);
    } catch { setDownloadError(result.manifest_id); }
    finally { setDownloading(false); }
  }
  return <section className="stack" data-testid="docx-export">
    <h2>导出投标稿</h2>
    <p>以已保存的同一版本生成 DOCX、PDF 和校核报告。PDF 由该版 DOCX 转换；未保存、文件无效或转换失败会阻止导出。后续编辑需重新导出。</p>
    {state.current && <p>当前已保存版本 {state.current.revision}</p>}
    {state.phase === "loading" && <p role="status">正在读取稿件和导出记录…</p>}
    {state.phase === "sending" && <p role="status">正在提交导出请求…</p>}
    {state.phase === "running" && <div role="status"><p>正在转换 PDF 并整理导出文件…</p><p>任务在后台继续执行，可离开后返回查看。</p></div>}
    {state.error && <Alert>{state.error}</Alert>}
    {state.input && <p className="break-all text-sm text-muted-foreground">{state.phase === "ready" ? "待导出版本" : "本次请求版本"}：{state.input.version_id}</p>}
    {(state.phase === "ready" || state.phase === "uncertain") && <Button onClick={() => void session.start()}>
      {state.phase === "uncertain" ? "确认本次导出请求" : "生成导出文件"}</Button>}
    {(state.phase === "blocked" || state.phase === "failed" || state.phase === "completed") && <Button variant="outline" onClick={() => void session.load(true)}>
      {state.phase === "failed" || state.phase === "completed" ? "刷新稿件并重新确认" : "刷新状态"}</Button>}
    {result && <div className="stack">
      <h3>已生成的导出文件</h3>
      <p className="break-all">文件对应保存版本：{result.source.version_id}</p>
      {(!state.current || result.source.version_id !== state.current.version_id || result.source.docx_sha256 !== state.current.docx_sha256)
        && <p>此文件固定对应上述版本；请刷新稿件状态，确认是否包含最新修改。</p>}
      <p>校核报告为待复核状态，尚不能证明内容、附表对应关系和版式已经通过验收。</p>
      <div className="flex flex-wrap gap-2">
        <Button disabled={downloading} onClick={() => void download("docx")}>下载 DOCX</Button>
        <Button disabled={downloading} onClick={() => void download("pdf")}>下载 PDF</Button>
        <Button variant="outline" disabled={downloading} onClick={() => void download("report")}>下载校核报告</Button>
      </div>
      {downloading && <p role="status">正在下载…</p>}
      {downloadError === result.manifest_id && <Alert>下载失败，请重试。</Alert>}
    </div>}
  </section>;
}
