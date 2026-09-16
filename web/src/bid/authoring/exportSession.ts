import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import type { DocxApi, DocxCurrent } from "../api/docx";
import { validExportPackage, type ExportApi, type ExportInput, type ExportPackage, type ExportStatus } from "../api/exports";

export type ExportAttempt = { input: ExportInput; attempt: MutationAttempt; request: ExportStatus | null };
type Store = { read(): ExportAttempt | null; write(value: ExportAttempt | null): void };
type State = {
  phase: "loading" | "ready" | "blocked" | "sending" | "uncertain" | "running" | "completed" | "failed";
  current: DocxCurrent | null; input: ExportInput | null; request: ExportStatus | null;
  result: ExportPackage | null; error: string | null;
};
export function createExportSession(api: ExportApi & Pick<DocxApi, "current">, workspace: string, store: Store, readOnly = false) {
  let state: State = { phase: "loading", current: null, input: null, request: null, result: null, error: null };
  let flight: ExportAttempt | null = null;
  let epoch = 0; let polling = false;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<State>) => { state = { ...state, ...patch }; listeners.forEach(fn => fn()); };
  const unsafe = () => state.phase === "sending" || state.phase === "uncertain";
  function received(request: ExportStatus) {
    const result = request.result_identity;
    if (request.status === "succeeded" && (!result || !validExportPackage(result)
      || result.source.version_id !== flight?.input.version_id || result.source.docx_sha256 !== flight.input.docx_sha256)) {
      throw new Error("export source changed");
    }
    set({ request, result: request.status === "succeeded" ? result : null,
      phase: request.status === "pending" ? "running" : request.status === "succeeded" ? "completed" : "failed",
      error: request.status === "failed" ? "本次导出失败。请检查稿件和转换服务后，重新确认导出。" : null });
  }
  async function load(fresh = false) {
    if (unsafe() || state.phase === "running") return;
    const turn = ++epoch;
    set({ phase: "loading", error: null, result: null, request: null, input: null });
    try {
      flight = store.read();
      if (flight && (!flight.input?.version_id || !/^[0-9a-f]{64}$/.test(flight.input.docx_sha256)
        || !flight.attempt?.idempotencyKey || (flight.request && (!flight.request.request_artifact_id
          || flight.request.kind !== "SubmissionExport" || !["pending", "succeeded", "failed"].includes(flight.request.status))))) {
        throw new Error("invalid saved export intent");
      }
      if (fresh && flight?.request && flight.request.status !== "pending") { store.write(null); flight = null; }
      if (flight) {
        set({ input: flight.input });
        if (flight.request) { received(flight.request); if (flight.request.status === "pending") await poll(); }
        else set({ phase: "uncertain", error: "上次提交结果尚未确认，请确认同一次导出请求。" });
        return;
      }
      const [current, packages] = await Promise.all([api.current(workspace), api.list(workspace)]);
      if (turn !== epoch) return;
      const result = packages.find(value => value.source.version_id === current?.version_id && value.source.docx_sha256 === current.docx_sha256) ?? packages[0] ?? null;
      const blocked = readOnly || !current || !!current.editor.pending_save_id || !!current.editor.save_error;
      set({ current, input: current ? { version_id: current.version_id, docx_sha256: current.docx_sha256 } : null,
        result, phase: blocked ? "blocked" : "ready",
        error: readOnly ? "项目已结束，可下载已有导出文件。" : !current ? "尚未创建投标稿。"
          : current.editor.pending_save_id || current.editor.save_error ? "稿件尚未成功保存，请返回编制页面完成保存后再导出。" : null });
    } catch { if (turn === epoch) set({ phase: "blocked", error: "无法读取稿件或恢复导出请求，请刷新重试。" }); }
  }
  async function poll() {
    if (state.phase !== "running" || !state.request || polling) return;
    const turn = epoch; const expected = state.request; polling = true;
    try {
      const request = await api.status(workspace, expected.request_artifact_id);
      if (turn !== epoch) return;
      if (request.request_artifact_id !== expected.request_artifact_id || request.request_revision !== expected.request_revision
        || request.frozen_input_sha256 !== expected.frozen_input_sha256) throw new Error("export identity changed");
      received(request);
      if (flight) { flight = { ...flight, request }; store.write(flight); }
    } catch { if (turn === epoch) set({ error: "暂时无法确认导出进度或文件版本，已保留原任务，请稍后重试。" }); }
    finally { polling = false; }
  }
  async function start() {
    if (state.phase !== "ready" && state.phase !== "uncertain") return;
    if (!flight) {
      if (!state.input || readOnly) return;
      flight = { input: structuredClone(state.input), attempt: createMutationAttempt(), request: null };
      try { store.write(flight); } catch { flight = null; set({ error: "无法保存请求恢复信息，请检查浏览器存储后重试。" }); return; }
    }
    set({ phase: "sending", result: null, error: null });
    try {
      const request = await api.start(workspace, flight.input, flight.attempt);
      received(request);
      flight = { ...flight, request }; store.write(flight);
      if (request.status === "pending") await poll();
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && ![408, 429].includes(error.status)) {
        try { store.write(null); flight = null; } catch { /* replaying the same rejected intent is safe */ }
        set({ phase: "blocked", error: error.status === 409 || error.status === 412
          ? "稿件版本或保存状态已变化，请刷新后重新确认导出。" : "导出请求被拒绝，请检查项目权限和稿件状态。" });
      } else set({ phase: "uncertain", result: null, error: "提交结果尚未确认，请确认同一次导出请求，避免重复生成文件。" });
    }
  }
  return { getState: () => state, load, start, poll, unsafe,
    subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
    cancelRead() { ++epoch; },
  };
}
