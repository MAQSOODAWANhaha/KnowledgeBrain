import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import type { DocxApi, DocxCurrent, DocxIdentity, DocxOpened, DocxSave } from "../api/docx";

export type DocxMode = "view" | "edit";
export type DocxSessionApi = Omit<DocxApi, "open"> & {
  open(workspace: string, expected: DocxIdentity, attempt: MutationAttempt, options: { mode: DocxMode; language?: string }): Promise<DocxOpened>;
  version?(workspace: string, versionId: string): Promise<DocxCurrent>;
  close?(workspace: string, editorKey: string): Promise<void>;
};
export type DocxOpenOptions = { mode?: DocxMode; expected?: DocxIdentity };
export type DocxSessionOptions = {
  readOnly?: boolean;
  beforeClose?: () => void | Promise<void>;
  storage?: Pick<Storage, "getItem" | "setItem" | "removeItem">;
  storageKey?: string;
};

export type DocxState = {
  phase: "loading" | "connecting" | "ready" | "closed" | "stale" | "failed";
  current: DocxCurrent | null;
  opened: DocxOpened | null;
  mode: DocxMode;
  available: DocxCurrent | null;
  saved: DocxCurrent | null;
  dirty: boolean;
  syncing: boolean;
  saving: boolean;
  uncertain: boolean;
  confirmed: boolean;
  closing: boolean;
  closePending: boolean;
  openPending: boolean;
  recoveryRequired: boolean;
  error: string | null;
};
type Flight = { body: DocxSave; attempt: MutationAttempt; sequence: number; id?: string };
type OpenFlight = { expected: DocxIdentity; options: { mode: DocxMode; language?: string }; attempt: MutationAttempt };
const message = (error: unknown) => error instanceof ApiError && error.status === 403
  ? "没有访问这份稿件的权限。" : "暂时无法确认保存状态，请检查连接后重试。";

export function createDocxSession(api: DocxSessionApi, workspace: string, options: DocxSessionOptions = {}) {
  const recoveryClaim = createMutationAttempt().idempotencyKey;
  let pendingOpen: OpenFlight | null = null;
  let storageError = false;
  try {
    const raw = options.storageKey && options.storage?.getItem(options.storageKey);
    if (raw) {
      const value = JSON.parse(raw) as OpenFlight;
      if (!value.expected?.version_id || !/^[a-f0-9]{64}$/.test(value.expected.docx_sha256)
        || !["view", "edit"].includes(value.options?.mode) || !value.attempt?.idempotencyKey
        || value.options.language !== undefined && typeof value.options.language !== "string") throw new Error("invalid opening recovery");
      pendingOpen = value;
    }
  } catch { storageError = true; }
  let state: DocxState = { phase: "loading", current: null, opened: null, dirty: false,
    mode: pendingOpen?.options.mode ?? "edit", available: null, saved: null, syncing: false, saving: false, uncertain: false,
    confirmed: false, closing: false, closePending: false, openPending: !!pendingOpen, recoveryRequired: false, error: null };
  const listeners = new Set<() => void>();
  let epoch = 0, sequence = 0;
  let disposed = false, sending = false, closeSending = false;
  let reading: number | null = null;
  let readError: string | null = null;
  let flight: Flight | null = null;
  let readOnly = options.readOnly === true;
  const ownsRecovery = () => {
    if (!options.storageKey || !options.storage) return true;
    const raw = options.storage.getItem(options.storageKey);
    return !!raw && JSON.parse(raw).claim === recoveryClaim;
  };
  const persistOpen = (value: OpenFlight | null) => {
    if (!options.storageKey || !options.storage) return;
    if (value) options.storage.setItem(options.storageKey, JSON.stringify({ ...value, claim: recoveryClaim }));
    else if (ownsRecovery()) options.storage.removeItem(options.storageKey);
  };
  const set = (patch: Partial<DocxState>) => {
    if (disposed) return;
    state = { ...state, ...patch };
    listeners.forEach(listener => listener());
  };
  const identity = (value: DocxCurrent) => ({ version_id: value.version_id, docx_sha256: value.docx_sha256 });
  const contentUnsafe = () => state.dirty || state.syncing || state.saving || state.uncertain || state.recoveryRequired || !!state.current?.editor.pending_save_id;
  const unsafe = () => contentUnsafe() || state.closing || state.closePending || state.openPending;
  async function closeOpened(turn: number, exported = false) {
    const opened = state.opened;
    if (!opened) return true;
    set({ closing: true, error: null });
    const lockedSequence = sequence;
    let sent = false;
    try {
      await options.beforeClose?.();
      if (disposed || turn !== epoch) return false;
      if (exported ? sequence !== lockedSequence : contentUnsafe()) {
        set({ closing: false, error: "关闭前发现新修改，请先保存并确认入稿。" });
        return false;
      }
      const before = sequence;
      sent = true;
      closeSending = true;
      await api.close?.(workspace, opened.session.editor_key);
      if (disposed || turn !== epoch) return false;
      if (sequence !== before || !exported && contentUnsafe()) {
        set({ closing: false, closePending: false, recoveryRequired: true, confirmed: false,
          error: "关闭确认期间检测到新修改，当前文档已保留；请先导出恢复稿。" });
        return false;
      }
      set({ closing: false, closePending: false });
      return true;
    } catch (error) {
      if (!disposed && turn === epoch) {
        const unknown = sent && !(error instanceof ApiError && error.status >= 400 && error.status < 500 && ![408, 429].includes(error.status));
        set({ closing: false, closePending: unknown, recoveryRequired: state.recoveryRequired || unknown && state.dirty,
          error: unknown ? "关闭结果尚未确认，文档已锁定；请确认同一次关闭请求。" : "文档会话未能关闭，当前文档保留，请稍后重试。" });
      }
      return false;
    } finally { if (sent) closeSending = false; }
  }
  async function start(openOptions: DocxOpenOptions = {}, confirmPending = false) {
    const restarting = disposed;
    if (!restarting && (contentUnsafe() || state.closing || state.closePending)) return false;
    if (storageError) { set({ phase: "failed", error: "无法读取文档会话恢复信息，请恢复浏览器存储后刷新页面重试。" }); return false; }
    if (pendingOpen && !confirmPending && !restarting && (openOptions.mode !== pendingOpen.options.mode
      || openOptions.expected && (openOptions.expected.version_id !== pendingOpen.expected.version_id
        || openOptions.expected.docx_sha256 !== pendingOpen.expected.docx_sha256))) {
      set({ phase: "failed", openPending: true, error: "上次打开文档的结果尚未确认，请先确认同一次打开请求。" });
      return false;
    }
    const mode = pendingOpen?.options.mode ?? openOptions.mode ?? "edit";
    if (readOnly && mode === "edit" && !pendingOpen) {
      set({ phase: state.opened ? state.phase : "failed", error: "项目已结束，只能阅读已保存的 Word；已有会话仍可保存或关闭。" });
      return false;
    }
    disposed = false;
    const turn = ++epoch;
    if (!restarting && !await closeOpened(turn)) return false;
    if (disposed || turn !== epoch) return false;
    flight = null; sending = false; sequence = 0; readError = null;
    set({ phase: "loading", mode, current: null, opened: null, available: null, saved: null, dirty: false, syncing: false, saving: false,
      uncertain: false, confirmed: false, closing: false, closePending: false, recoveryRequired: false, error: null });
    let request = pendingOpen;
    try {
      const head = await api.current(workspace);
      if (disposed || turn !== epoch) return false;
      const expected = request?.expected ?? openOptions.expected;
      const current = expected && expected.version_id !== head?.version_id && api.version
        ? await api.version(workspace, expected.version_id) : head;
      if (disposed || turn !== epoch) return;
      if (!current) {
        set({ phase: "failed", error: "尚未创建 DOCX 稿件。" });
        return false;
      }
      if (expected && (current.version_id !== expected.version_id || current.docx_sha256 !== expected.docx_sha256)) {
        throw new Error("requested version changed");
      }
      request ??= { expected: identity(current), attempt: createMutationAttempt(), options: { mode,
        ...(typeof document !== "undefined" ? { language: document.documentElement.lang || "zh-CN" } : {}) } };
      try { persistOpen(request); }
      catch { set({ phase: "failed", error: "无法保留文档会话恢复信息，本次尚未发送。请检查浏览器存储后重试。" }); return false; }
      pendingOpen = request;
      set({ openPending: true });
      const opened = await api.open(workspace, request.expected, request.attempt, request.options);
      if (disposed || turn !== epoch) {
        if (state.opened?.session.editor_key !== opened.session.editor_key && !(pendingOpen === request && !disposed && !state.opened)) {
          try {
            if (!ownsRecovery()) return;
            await api.close?.(workspace, opened.session.editor_key);
            if (pendingOpen === request && disposed) { persistOpen(null); pendingOpen = null; }
          } catch { /* Keep the opening request so this tab can recover its own session. */ }
        }
        return;
      }
      if (opened.session.round_id !== current.round_id || opened.session.version_id !== current.version_id) throw new Error("opening version changed");
      if (mode === "view") {
        const config = opened.config as { editorConfig?: { mode?: string }; document?: { permissions?: { edit?: boolean } } };
        if (config.editorConfig?.mode !== "view" || config.document?.permissions?.edit !== false) {
          set({ current, phase: "failed", error: "文档服务未返回只读配置，请重试或联系管理员。" });
          return false;
        }
      }
      persistOpen(null); pendingOpen = null;
      set({ current, opened, openPending: false, available: head?.version_id !== current.version_id ? head : null, phase: "connecting" });
      return true;
    } catch (error) {
      if (disposed || turn !== epoch) return false;
      if (pendingOpen && error instanceof ApiError && error.status >= 400 && error.status < 500 && ![408, 429].includes(error.status)) {
        try { persistOpen(null); pendingOpen = null; } catch { /* Preserve recovery until storage is available. */ }
      }
      set({ phase: "failed", openPending: !!pendingOpen, error: pendingOpen
        ? "打开结果尚未确认，请确认同一次打开请求；不会创建另一编辑会话。" : message(error) });
      return false;
    }
  }
  async function refresh() {
    if (reading === epoch || disposed || state.closing || state.closePending || state.recoveryRequired || !state.opened || state.phase === "stale") return;
    const turn = epoch;
    const observedFlight = flight, observedSaveId = flight?.id;
    reading = turn;
    try {
      const current = await api.current(workspace);
      // Read the publication receipt after current: a callback can commit while
      // polling, and a pre-publication null must not invalidate its new version.
      const receipt = observedSaveId && observedFlight
        ? await api.saved(workspace, observedFlight.body.editor_key, observedSaveId) : null;
      if (disposed || turn !== epoch || flight !== observedFlight || flight?.id !== observedSaveId) return;
      const opened = state.opened;
      if (!opened) return;
      if (state.mode === "view") {
        const pinned = state.current;
        let metadata = current?.version_id === pinned?.version_id ? current : null;
        if (!metadata && pinned?.editor.pending_save_id && api.version) metadata = await api.version(workspace, pinned.version_id);
        if (disposed || turn !== epoch || flight !== observedFlight || flight?.id !== observedSaveId) return;
        if (metadata && pinned && (metadata.version_id !== pinned.version_id || metadata.docx_sha256 !== pinned.docx_sha256
          || metadata.round_id !== pinned.round_id || metadata.project_id !== pinned.project_id || metadata.workspace_id !== pinned.workspace_id)) {
          throw new Error("reader metadata identity mismatch");
        }
        set({ ...(metadata && pinned ? { current: { ...pinned, editor: metadata.editor } } : {}),
          available: current?.version_id !== pinned?.version_id ? current : null,
          ...(readError && state.error === readError ? { error: null } : {}) });
        readError = null;
        return;
      }
      if (!current || current.round_id !== opened.session.round_id || current.editor.key !== opened.session.editor_key) {
        set({ available: current, phase: "stale", saving: false, confirmed: false,
          error: "稿件或编辑会话已更新。请保留当前修改后重新打开。" });
        return;
      }
      if (receipt && observedFlight && (receipt.save_id !== observedSaveId
        || receipt.editor_key !== observedFlight.body.editor_key || receipt.round_id !== opened.session.round_id
        || receipt.parent_version_id !== observedFlight.body.expected.version_id)) {
        throw new Error("save receipt identity mismatch");
      }
      const saved = receipt && !current.editor.pending_save_id && !current.editor.save_error
        && current.version_id === receipt.version_id && current.docx_sha256 === receipt.docx_sha256;
      if (saved && flight) {
        const clean = sequence === flight.sequence;
        flight = null;
        set({ current, saved: current, available: null, dirty: !clean, saving: false, uncertain: false, confirmed: clean, error: null });
      } else if (flight?.id && !current.editor.pending_save_id && !current.editor.save_error
        && current.version_id !== flight.body.expected.version_id) {
        set({ available: current, phase: "stale", saving: false, confirmed: false,
          error: "当前保存版本不对应本次保存请求。请保留当前修改后重新打开。" });
      } else if (current.editor.save_error && !current.editor.pending_save_id && (!flight || flight.id)) {
        flight = null;
        set({ current, saving: false, uncertain: false, confirmed: false, error: "保存未成功，当前修改尚未确认入稿。请重试保存。" });
      } else {
        const sameVersion = current.version_id === state.current?.version_id && current.docx_sha256 === state.current.docx_sha256;
        set({ ...(sameVersion ? { current } : { available: current }),
          ...(readError && state.error === readError ? { error: null } : {}) });
      }
      readError = null;
    } catch (error) {
      if (turn === epoch && flight === observedFlight && flight?.id === observedSaveId) {
        readError = message(error);
        set({ error: readError, confirmed: false });
      }
    } finally { if (reading === turn) reading = null; }
  }
  async function save() {
    if (disposed || sending || state.closing || state.closePending || state.recoveryRequired || state.mode !== "edit" || state.phase !== "ready" || state.syncing || !state.current || !state.opened) return;
    if (!flight) {
      if (!state.dirty || state.current.editor.pending_save_id) return;
      flight = { sequence, attempt: createMutationAttempt(), body: {
        editor_key: state.opened.session.editor_key, expected: identity(state.current),
      } };
    }
    const request = flight, turn = epoch;
    sending = true;
    set({ saving: true, uncertain: false, confirmed: false, error: null });
    try {
      const receipt = await api.save(workspace, request.body, request.attempt);
      if (disposed || turn !== epoch || flight !== request) return;
      request.id = receipt.save_id;
      await refresh();
    } catch (error) {
      if (disposed || turn !== epoch) return;
      if (error instanceof ApiError && (error.code === "ONLYOFFICE_COMMAND_REJECTED" || error.status === 409)) {
        flight = null;
        set({ saving: false, error: "保存未成功，正在核对当前稿件状态。" });
        await refresh();
      } else {
        // Retain the exact request and idempotency key after an uncertain send.
        set({ saving: false, uncertain: true, error: "保存结果尚未确认，可重试确认同一次请求。" });
      }
    } finally { if (turn === epoch) sending = false; }
  }
  async function finishClose(exported = false, confirmPending = false) {
    if (disposed || state.closing || state.openPending) return false;
    if (exported ? !state.recoveryRequired || state.saving || state.uncertain || state.syncing || !!state.current?.editor.pending_save_id
      : contentUnsafe() || state.closePending && !confirmPending) return false;
    const turn = ++epoch;
    if (!await closeOpened(turn, exported) || disposed || turn !== epoch) return false;
    set({ opened: null, phase: "closed", dirty: false, syncing: false, recoveryRequired: false, closePending: false, confirmed: false, error: null });
    return true;
  }
  return {
    getState: () => state,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    start, refresh, save, unsafe,
    confirmOpen: () => start({}, true),
    setReadOnly(value: boolean) { readOnly = value; },
    confirmRecoveredClose: () => finishClose(true, true),
    confirmClose: () => finishClose(false, true),
    ready() { if (state.phase === "connecting") set({ phase: "ready" }); },
    changed(value: boolean) {
      if (state.mode !== "edit") return;
      if (state.phase !== "ready" && state.phase !== "connecting") return;
      if (value) { sequence++; set({ dirty: true, syncing: true, confirmed: false, ...(state.closePending ? { recoveryRequired: true } : {}) }); }
      else set({ syncing: false });
    },
    editorError() { set({ error: "编辑器连接异常，保存状态尚未确认。", confirmed: false }); },
    close: () => finishClose(),
    dispose() {
      const editorKey = !contentUnsafe() && !state.openPending && !state.closePending && !closeSending ? state.opened?.session.editor_key : null;
      disposed = true; ++epoch; listeners.clear();
      // A later explicit selection can retry a failed close using server state.
      if (editorKey) void api.close?.(workspace, editorKey).catch(() => {});
    },
  };
}

export function docxStatus(state: DocxState): string {
  if (state.closing) return "正在安全关闭编辑器…";
  if (state.recoveryRequired) return "修改待导出恢复";
  if (state.closePending) return "关闭结果待确认";
  if (state.openPending) return "打开结果待确认";
  if (state.phase === "loading") return "正在读取稿件…";
  if (state.phase === "connecting") return "正在打开编辑器…";
  if (state.phase === "stale") return "当前会话已失效";
  if (state.phase === "failed") return "稿件打开失败";
  if (state.phase === "closed") return "编辑器已关闭";
  if (state.uncertain) return "保存结果待确认";
  if (state.saving || state.current?.editor.pending_save_id) return "正在保存，等待入稿确认…";
  if (state.syncing) return "正在同步修改…";
  if (state.dirty) return "有修改尚未保存";
  if (state.error) return "保存状态待确认";
  if (state.confirmed && !state.error) return "已保存";
  return "当前保存版本已载入";
}
