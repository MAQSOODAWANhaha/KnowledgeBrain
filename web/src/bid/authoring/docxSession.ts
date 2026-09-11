import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import type { DocxApi, DocxCurrent, DocxOpened, DocxSave } from "../api/docx";

export type DocxState = {
  phase: "loading" | "connecting" | "ready" | "closed" | "stale" | "failed";
  current: DocxCurrent | null;
  opened: DocxOpened | null;
  dirty: boolean;
  syncing: boolean;
  saving: boolean;
  uncertain: boolean;
  confirmed: boolean;
  error: string | null;
};
type Flight = { body: DocxSave; attempt: MutationAttempt; sequence: number; id?: string };
const message = (error: unknown) => error instanceof ApiError && error.status === 403
  ? "没有访问这份稿件的权限。" : "暂时无法确认保存状态，请检查连接后重试。";

export function createDocxSession(api: DocxApi, workspace: string) {
  let state: DocxState = { phase: "loading", current: null, opened: null, dirty: false,
    syncing: false, saving: false, uncertain: false, confirmed: false, error: null };
  const listeners = new Set<() => void>();
  let epoch = 0, sequence = 0;
  let disposed = false, sending = false;
  let reading: number | null = null;
  let readError: string | null = null;
  let flight: Flight | null = null;
  const set = (patch: Partial<DocxState>) => {
    if (disposed) return;
    state = { ...state, ...patch };
    listeners.forEach(listener => listener());
  };
  const identity = (value: DocxCurrent) => ({ version_id: value.version_id, docx_sha256: value.docx_sha256 });
  const unsafe = () => state.dirty || state.syncing || state.saving || state.uncertain || !!state.current?.editor.pending_save_id;
  async function start() {
    disposed = false;
    const turn = ++epoch;
    flight = null; sending = false; sequence = 0; readError = null;
    set({ phase: "loading", current: null, opened: null, dirty: false, syncing: false, saving: false,
      uncertain: false, confirmed: false, error: null });
    try {
      const current = await api.current(workspace);
      if (disposed || turn !== epoch) return;
      if (!current) {
        set({ phase: "failed", error: "尚未创建 DOCX 稿件。" });
        return;
      }
      const opened = await api.open(workspace, identity(current), createMutationAttempt());
      if (disposed || turn !== epoch) return;
      if (opened.session.round_id !== current.round_id) throw new Error("round changed");
      set({ current, opened, phase: "connecting" });
    } catch (error) {
      if (turn === epoch) set({ phase: "failed", error: message(error) });
    }
  }
  async function refresh() {
    if (reading === epoch || disposed || !state.opened || state.phase === "stale") return;
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
      if (!current || current.round_id !== opened.session.round_id || current.editor.key !== opened.session.editor_key) {
        set({ current, phase: "stale", saving: false, confirmed: false,
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
        set({ current, dirty: !clean, saving: false, uncertain: false, confirmed: clean, error: null });
      } else if (flight?.id && !current.editor.pending_save_id && !current.editor.save_error
        && current.version_id !== flight.body.expected.version_id) {
        set({ current, phase: "stale", saving: false, confirmed: false,
          error: "当前保存版本不对应本次保存请求。请保留当前修改后重新打开。" });
      } else if (current.editor.save_error && !current.editor.pending_save_id && (!flight || flight.id)) {
        flight = null;
        set({ current, saving: false, uncertain: false, confirmed: false, error: "保存未成功，当前修改尚未确认入稿。请重试保存。" });
      } else {
        set({ current, ...(readError && state.error === readError ? { error: null } : {}) });
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
    if (disposed || sending || state.phase !== "ready" || state.syncing || !state.current || !state.opened) return;
    if (!flight) {
      if (!state.dirty || state.current.editor.pending_save_id) return;
      flight = { sequence, attempt: createMutationAttempt(), body: {
        editor_key: state.opened.session.editor_key, expected: identity(state.current),
      } };
    } else if (!state.uncertain) return;
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
  return {
    getState: () => state,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    start, refresh, save, unsafe,
    ready() { if (state.phase === "connecting") set({ phase: "ready" }); },
    changed(value: boolean) {
      if (state.phase !== "ready" && state.phase !== "connecting") return;
      if (value) { sequence++; set({ dirty: true, syncing: true, confirmed: false }); }
      else set({ syncing: false });
    },
    editorError() { set({ error: "编辑器连接异常，保存状态尚未确认。", confirmed: false }); },
    close() { if (unsafe()) return false; ++epoch; set({ opened: null, phase: "closed", confirmed: false }); return true; },
    dispose() { disposed = true; ++epoch; listeners.clear(); },
  };
}

export function docxStatus(state: DocxState): string {
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
