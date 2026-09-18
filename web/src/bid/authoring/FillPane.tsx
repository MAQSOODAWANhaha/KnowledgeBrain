import { useEffect, useState, useSyncExternalStore } from "react";
import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import { docxApi } from "../api/docx";
import { fillApi } from "../api/fill";
import { createFillSession, fillProgressText, type FillAttempt } from "./fillSession";

/**
 * 阶段二的填章入口：一次点击填完整份。已有正文的章（用户手写或上一轮填的）原样
 * 保留，只填空着的章。
 */
export function FillPane({ workspaceId, onPublished }: { workspaceId: string; onPublished?: () => void }) {
  const [session] = useState(() => createFillSession({ ...fillApi, current: docxApi.current }, workspaceId, {
    read() {
      const raw = sessionStorage.getItem(`kb.docx-fill:${workspaceId}`);
      if (!raw) return null;
      try { return JSON.parse(raw) as FillAttempt; }
      catch { return null; }
    },
    write(value) {
      if (value) sessionStorage.setItem(`kb.docx-fill:${workspaceId}`, JSON.stringify(value));
      else sessionStorage.removeItem(`kb.docx-fill:${workspaceId}`);
    },
  }, onPublished));
  const state = useSyncExternalStore(session.subscribe, session.getState);
  useEffect(() => { void session.load(); return session.cancelRead; }, [session]);
  useEffect(() => {
    if (state.phase !== "running") return;
    const timer = setInterval(() => void session.poll(), 3000);
    return () => clearInterval(timer);
  }, [session, state.phase]);
  const starting = state.phase === "sending" || state.phase === "running";
  return <section className="card stack" data-testid="docx-fill">
    <h2 className="h3">AI 填充正文</h2>
    <p className="text-sm">
      按当前 Word 的章节结构逐章填写模板正文。只填空着的章，保留可重建的普通段落与表格正文。
    </p>
    <p className="text-sm">
      填充读取的是<strong>已保存</strong>的稿件，并会写出一个新版本；编辑器里尚未保存的改动不会进入这一版。
    </p>
    <p className="text-sm">
      含图片、签章、扫描件、自定义样式、页眉页脚、首章前正文或不支持的域时，会拒绝填充并提示位置；原文件仍可下载。
    </p>
    <p className="text-sm">
      新增章请使用标题样式（Heading 1/2/3）。用普通段落敲的标题不会被识别成章，它的文字会被当作上一章的正文。
    </p>
    <p role="status" aria-live="polite" className="text-sm" data-testid="docx-fill-progress">
      {state.phase === "loading" ? "正在读取填充状态…" : state.phase === "unchanged" ? "没有待填空章，当前版本未改变" : fillProgressText(state.job, state.stopping)}
    </p>
    {state.error && <Alert>{state.error}</Alert>}
    {state.phase === "uncertain"
      ? <Button disabled={false} onClick={() => void session.start()}>确认同一次填充</Button>
      : <Button disabled={starting || state.phase !== "ready"} onClick={() => void session.start()}>
        {starting ? "填充进行中…" : "一键填充整份"}
      </Button>}
    {state.phase === "running"
      && <Button variant="outline" disabled={state.stopping} data-testid="docx-fill-stop"
        onClick={() => void session.stop()}>
        {state.stopping ? "正在停止…" : "停止填充"}
      </Button>}
    {(state.phase === "completed" || state.phase === "failed" || state.phase === "blocked")
      && <Button variant="outline" onClick={() => void session.load(true)}>
        {state.phase === "completed" ? "继续填充剩余章" : "刷新状态"}
      </Button>}
  </section>;
}
