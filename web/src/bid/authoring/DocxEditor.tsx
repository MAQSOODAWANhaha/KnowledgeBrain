import { useEffect, useState } from "react";
import { randomUuid } from "../../uuid";
import { guardHashNavigation } from "../../hash";
import { docxApi } from "../api/docx";
import { createDocxSession, docxStatus } from "./docxSession";
import "./docx.css";

type EditorInstance = { destroyEditor(): void };
declare global {
  interface Window { DocsAPI?: { DocEditor: new (id: string, config: Record<string, unknown>) => EditorInstance }; }
}
const scripts = new Map<string, Promise<void>>();
let installedUrl: string | null = null;
function loadScript(url: string): Promise<void> {
  const address = new URL(url);
  if (!["http:", "https:"].includes(address.protocol)) return Promise.reject(new Error("invalid script URL"));
  if (installedUrl && installedUrl !== url) return Promise.reject(new Error("document service changed; reload required"));
  if (!scripts.has(url)) {
    scripts.set(url, new Promise<void>((resolve, reject) => {
      const script = document.createElement("script");
      script.src = url;
      script.async = true;
      script.onload = () => {
        if (window.DocsAPI) { installedUrl = url; resolve(); }
        else { scripts.delete(url); script.remove(); reject(new Error("editor API unavailable")); }
      };
      script.onerror = () => { scripts.delete(url); script.remove(); reject(new Error("editor script unavailable")); };
      document.head.append(script);
    }));
  }
  return scripts.get(url)!;
}

export function DocxEditor({ workspaceId, onUnsafeChange, onCreateRound }: {
  workspaceId: string;
  onUnsafeChange?: (unsafe: boolean) => void;
  onCreateRound?: () => void;
}) {
  const [session] = useState(() => createDocxSession(docxApi, workspaceId));
  const [state, setState] = useState(session.getState);
  const [notice, setNotice] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [elementId] = useState(() => `docx-${randomUuid()}`);
  useEffect(() => {
    const unsubscribe = session.subscribe(() => setState(session.getState()));
    void session.start();
    return () => { unsubscribe(); session.dispose(); };
  }, [session]);
  useEffect(() => { onUnsafeChange?.(session.unsafe()); }, [session, state, onUnsafeChange]);
  useEffect(() => {
    if (!state.opened) return;
    let canceled = false;
    let instance: EditorInstance | undefined;
    const opened = state.opened;
    void loadScript(opened.api_script_url).then(() => {
      if (canceled) return;
      instance = new window.DocsAPI!.DocEditor(elementId, {
        ...opened.config,
        events: {
          onDocumentReady: () => { if (!canceled) session.ready(); },
          onDocumentStateChange: (event: { data: boolean }) => { if (!canceled) session.changed(event.data); },
          onError: () => { if (!canceled) session.editorError(); },
        },
      });
    }).catch(() => { if (!canceled) session.editorError(); });
    return () => { canceled = true; instance?.destroyEditor(); };
  }, [state.opened, session, elementId]);
  useEffect(() => {
    if (!state.opened || state.phase === "stale") return;
    // UI refresh cadence, not a save deadline or a business rule.
    const timer = window.setInterval(() => void session.refresh(), 1000);
    return () => window.clearInterval(timer);
  }, [state.opened, state.phase, session]);
  useEffect(() => {
    const originalHash = window.location.hash;
    const warn = () => setNotice("请先保存并确认入稿，再离开编辑页。");
    const unload = (event: BeforeUnloadEvent) => {
      if (session.unsafe()) { event.preventDefault(); event.returnValue = ""; }
    };
    const click = (event: MouseEvent) => {
      if (session.unsafe() && event.target instanceof Element && event.target.closest("a[href]")) {
        event.preventDefault(); event.stopPropagation(); warn();
      }
    };
    const unguard = guardHashNavigation(() => {
      if (session.unsafe() && window.location.hash !== originalHash) {
        history.replaceState(null, "", `${window.location.pathname}${window.location.search}${originalHash}`);
        warn();
        return false;
      }
      return true;
    });
    window.addEventListener("beforeunload", unload);
    document.addEventListener("click", click, true);
    return () => {
      window.removeEventListener("beforeunload", unload);
      document.removeEventListener("click", click, true);
      unguard();
    };
  }, [session]);
  async function download() {
    if (!state.current || downloading) return;
    setDownloading(true);
    try {
      const blob = await docxApi.download(workspaceId, state.current.version_id);
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url; link.download = `投标稿-${state.current.revision}.docx`;
      // This is explicitly the persisted version, even if editing has continued.
      link.click(); URL.revokeObjectURL(url);
    } catch { setNotice("已保存稿下载失败，请重试。"); }
    finally { setDownloading(false); }
  }
  function reopen() {
    if (session.unsafe() && !window.confirm("当前修改尚未确认保存。重新打开会关闭当前编辑器，是否继续？")) return;
    setNotice(null); void session.start();
  }
  return <section className="docx-pane" data-testid="docx-editor">
    <div className="docx-toolbar">
      <strong>投标稿</strong>
      {state.current && <span data-testid="docx-version">保存版本 {state.current.revision}</span>}
      <span role="status" data-testid="docx-status">{docxStatus(state)}</span>
      <div className="spacer" />
      <button className="btn" data-testid="docx-save" disabled={state.phase !== "ready" || state.syncing ||
        (!state.uncertain && (!state.dirty || state.saving || !!state.current?.editor.pending_save_id))}
        onClick={() => { setNotice(null); void session.save(); }}>{state.uncertain ? "确认保存结果" : "保存"}</button>
      <button className="btn ghost" disabled={!state.current || downloading} onClick={() => void download()}>下载已保存稿</button>
      {state.opened && state.phase !== "stale"
        ? <button className="btn ghost" data-testid="docx-close" disabled={session.unsafe()} onClick={() => session.close()}>关闭编辑器</button>
        : <button className="btn ghost" data-testid="docx-reopen" onClick={reopen}>重新打开</button>}
      {state.phase === "closed" && onCreateRound && <button className="btn ghost" onClick={onCreateRound}>生成或导入新轮</button>}
    </div>
    {(state.error || notice) && <div className="banner warn" role="alert">{state.error || notice}
      <button className="btn ghost" onClick={() => void session.refresh()}>刷新状态</button>
      {(state.phase === "connecting" || state.phase === "failed") && <button className="btn ghost" onClick={reopen}>重试打开</button>}
    </div>}
    {state.opened && <div className="docx-editor-frame" key={state.opened.session.editor_key + elementId}><div id={elementId} /></div>}
  </section>;
}
