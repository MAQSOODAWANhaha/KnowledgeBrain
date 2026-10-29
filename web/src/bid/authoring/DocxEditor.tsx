import { useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import { randomUuid } from "../../uuid";
import { guardHashNavigation } from "../../hash";
import { docxApi, type DocxCurrent, type DocxIdentity } from "../api/docx";
import { createDocxSession, docxStatus, type DocxMode, type DocxSessionApi } from "./docxSession";
import { connectWordNavigation, type WordNavigationRequest, type WordNavigationResult } from "./wordNavigation";
import "./docx.css";

type EditorInstance = { destroyEditor(): void };
declare global {
  interface Window { DocsAPI?: { DocEditor: new (id: string, config: Record<string, unknown>) => EditorInstance }; }
}
const scripts = new Map<string, Promise<void>>();
let installedUrl: string | null = null;
const recoveryStorage = {
  getItem: (key: string) => window.sessionStorage.getItem(key),
  setItem: (key: string, value: string) => window.sessionStorage.setItem(key, value),
  removeItem: (key: string) => window.sessionStorage.removeItem(key),
};
function recoveryOwner(fallback?: string): string | undefined {
  try {
    const payload = localStorage.getItem("kb.token")?.split(".")[1];
    const subject: unknown = payload && JSON.parse(atob(payload.replace(/-/g, "+").replace(/_/g, "/"))).sub;
    if (typeof subject === "string" && subject) return `user:${subject}`;
  } catch { /* Fall back to the immutable login identity when the token has no subject. */ }
  return fallback ? `account:${fallback}` : undefined;
}
function loadScript(url: string): Promise<void> {
  let address: URL;
  try { address = new URL(url); }
  catch { return Promise.reject(new Error("invalid script URL")); }
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

export type DocxOpenRequest = { id: string; mode: DocxMode; expected?: DocxIdentity };
export type DocxEditorHandle = { close(): Promise<boolean> };

export function DocxEditor({ workspaceId, api = docxApi, openRequest, controlRef, navigationRequest, onNavigationResult, onUnsafeChange, onCurrentChange, onSaved, onCreateRound, readOnly = false, ownerIdentity }: {
  workspaceId: string;
  api?: DocxSessionApi;
  openRequest?: DocxOpenRequest;
  controlRef?: Ref<DocxEditorHandle>;
  navigationRequest?: WordNavigationRequest;
  onNavigationResult?: (result: WordNavigationResult) => void;
  onUnsafeChange?: (unsafe: boolean) => void;
  onCurrentChange?: (current: DocxCurrent | null) => void;
  onSaved?: (current: DocxCurrent) => void;
  onCreateRound?: () => void;
  readOnly?: boolean;
  ownerIdentity?: string;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [session] = useState(() => {
    const owner = recoveryOwner(ownerIdentity);
    return createDocxSession(api, workspaceId, { readOnly,
      ...(owner ? { storage: recoveryStorage, storageKey: `kb.docx-open:${encodeURIComponent(owner)}:${workspaceId}` } : {}),
      beforeClose: () => {
        const host = hostRef.current;
        if (host) {
          host.inert = true;
          const focused = document.activeElement;
          if (focused instanceof HTMLElement && host.contains(focused)) focused.blur();
        }
        // Drain document-state events already queued before the input lock.
        return new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      },
    });
  });
  const [state, setState] = useState(session.getState);
  const [notice, setNotice] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [elementId] = useState(() => `docx-${randomUuid()}`);
  const requestRef = useRef(openRequest);
  requestRef.current = openRequest;
  const savedVersion = useRef<string | null>(null);
  const navigation = useRef<ReturnType<typeof connectWordNavigation> | null>(null);
  const navigationRef = useRef(navigationRequest);
  const navigationResultRef = useRef(onNavigationResult);
  const deliveredNavigation = useRef<string | null>(null);
  navigationRef.current = navigationRequest;
  navigationResultRef.current = onNavigationResult;
  const currentDocx = state.current;
  const editorKey = state.opened?.session.editor_key;
  const currentVersion = state.current?.version_id;
  const downloadUnconfirmed = session.unsafe();
  useImperativeHandle(controlRef, () => ({ close: session.close }), [session]);
  useEffect(() => {
    const unsubscribe = session.subscribe(() => setState(session.getState()));
    return () => { unsubscribe(); session.dispose(); };
  }, [session]);
  useEffect(() => { session.setReadOnly(readOnly); }, [session, readOnly]);
  useEffect(() => {
    const request = requestRef.current;
    void session.start({ mode: request?.mode ?? "view", expected: request?.expected }).then((opened) => {
      if (opened === false && session.unsafe()) setNotice("当前修改或保存结果尚未确认，请先保存后再切换稿件。");
    });
  }, [session, openRequest?.id]);
  useEffect(() => { onUnsafeChange?.(session.unsafe()); }, [session, state, onUnsafeChange]);
  useEffect(() => { onCurrentChange?.(currentDocx); }, [currentDocx, onCurrentChange]);
  useEffect(() => {
    if (!state.saved || savedVersion.current === state.saved.version_id) return;
    savedVersion.current = state.saved.version_id;
    onSaved?.(state.saved);
  }, [state.saved, onSaved]);
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
    if (!editorKey || !currentVersion) return;
    const adapter = connectWordNavigation({ elementId: `${elementId}-host`, editorKey, versionId: currentVersion,
      onResult: result => navigationResultRef.current?.(result) });
    navigation.current = adapter;
    const request = navigationRef.current;
    if (request && request.id !== deliveredNavigation.current) {
      deliveredNavigation.current = request.id;
      adapter.navigate(request);
    }
    return () => { adapter.dispose(); navigation.current = null; };
  }, [elementId, editorKey, currentVersion]);
  useEffect(() => {
    const request = navigationRef.current;
    if (!request || !navigation.current || request.id === deliveredNavigation.current) return;
    deliveredNavigation.current = request.id;
    navigation.current.navigate(request);
  }, [navigationRequest?.id]);
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
      const blob = await api.download(workspaceId, state.current.version_id);
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url; link.download = `投标文件-${state.current.revision}.docx`;
      // This is explicitly the persisted version, even if editing has continued.
      link.click(); URL.revokeObjectURL(url);
    } catch { setNotice("已保存稿下载失败，请重试。"); }
    finally { setDownloading(false); }
  }
  function reopen(mode = readOnly ? "view" : state.mode) {
    if (session.unsafe()) { setNotice("当前修改或保存结果尚未确认，请先保存后再切换。"); return; }
    setNotice(null); void session.start({ mode, expected: state.current ?? undefined });
  }
  return <section className="docx-pane" data-testid="docx-editor">
    <div className="docx-toolbar">
      <strong>{state.mode === "view" ? "Word 阅读" : "Word 编辑"}</strong>
      {state.current && <span data-testid="docx-version">保存版本 {state.current.revision}</span>}
      <span role="status" data-testid="docx-status">{docxStatus(state)}</span>
      <div className="spacer" />
      {state.mode === "edit" && <button className="btn" data-testid="docx-save" disabled={state.closing || state.closePending || state.recoveryRequired || state.phase !== "ready" || state.syncing ||
        (!state.uncertain && !state.saving && (!state.dirty || !!state.current?.editor.pending_save_id))}
        onClick={() => { setNotice(null); void session.save(); }}>{state.uncertain || state.saving ? "确认保存结果" : "保存"}</button>}
      <button className="btn ghost" disabled={!state.current || downloading} aria-describedby={downloadUnconfirmed ? `${elementId}-download-boundary` : undefined} onClick={() => void download()}>下载已保存稿</button>
      {state.phase === "ready" && <button className="btn ghost" disabled={session.unsafe() || readOnly && state.mode === "view"}
        data-testid="docx-mode" onClick={() => reopen(state.mode === "view" ? "edit" : "view")}>
        {state.mode === "view" ? "编辑 Word" : "返回阅读"}</button>}
      {state.opened && state.phase !== "stale"
        ? <button className="btn ghost" data-testid="docx-close" disabled={session.unsafe()} onClick={() => session.close()}>关闭编辑器</button>
        : <button className="btn ghost" data-testid="docx-reopen" disabled={state.openPending || state.closing} onClick={() => reopen()}>重新打开</button>}
      {state.phase === "closed" && onCreateRound && <button className="btn ghost" onClick={onCreateRound}>生成或导入新轮</button>}
    </div>
    {state.current && downloadUnconfirmed && <p className="docx-version-notice" role="status" id={`${elementId}-download-boundary`} data-testid="docx-download-boundary">下载仅包含已保存的 Word v{state.current.revision}，不包含当前尚未确认保存的修改。</p>}
    {state.openPending && state.phase === "failed" && <div className="docx-version-notice" role="status">
      上次打开文档的结果待确认。将恢复本标签页的原请求，不会关闭其他标签页的编辑会话。
      <button data-testid="docx-confirm-open" onClick={() => void session.confirmOpen()}>确认同一次打开请求</button>
    </div>}
    {state.closePending && !state.recoveryRequired && <div className="docx-version-notice" role="status">
      当前文档暂时锁定，确认原会话已关闭后才能重新打开。
      <button data-testid="docx-confirm-close" disabled={state.closing} onClick={() => void session.confirmClose()}>确认同一次关闭请求</button>
    </div>}
    {state.recoveryRequired && <div className="docx-version-notice" role="alert" data-testid="docx-recovery">
      平台尚未保存这些修改。请在当前 Word 中使用“文件 → 下载为 → DOCX”导出恢复稿，确认文件已下载后再关闭。
      <button disabled={state.closing || state.saving || state.uncertain || state.syncing || !!state.current?.editor.pending_save_id}
        onClick={() => void session.confirmRecoveredClose()}>已导出恢复稿，关闭编辑器</button>
    </div>}
    {(state.error || notice) && <div className="banner warn" role="alert">{state.error || notice}
      <button className="btn ghost" onClick={() => void session.refresh()}>刷新状态</button>
      {!state.openPending && (state.phase === "connecting" || state.phase === "failed") && <button className="btn ghost" onClick={() => reopen()}>重试打开</button>}
    </div>}
    {state.available && <div className="docx-version-notice" role="status">
      另有保存版本 {state.available.revision}。当前文稿保持打开，可从稿件版本中选择切换。
    </div>}
    {state.opened && <div ref={hostRef} inert={state.closing || state.closePending && !state.recoveryRequired} id={`${elementId}-host`} className="docx-editor-frame" key={state.opened.session.editor_key + elementId}><div id={elementId} /></div>}
  </section>;
}
