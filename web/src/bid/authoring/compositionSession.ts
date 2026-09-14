import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import type { CompositionApi, CompositionIdentity, CompositionStatus } from "../api/composition";
import type { DocxApi, DocxRoundApi, DocxRoundInput } from "../api/docx";
export type CompositionAttempt = { input: DocxRoundInput; attempt: MutationAttempt };
export type AttemptStore = { read(): CompositionAttempt | null; write(value: CompositionAttempt | null): void };
type State = {
  phase: "loading" | "ready" | "blocked" | "sending" | "uncertain" | "running" | "completed" | "failed" | "conflict";
  input: DocxRoundInput | null; request: CompositionIdentity | null; job: CompositionStatus | null; error: string | null;
};
export function createCompositionSession(api: CompositionApi & Pick<DocxApi, "current"> & Pick<DocxRoundApi, "basis">, workspace: string, store: AttemptStore) {
  let state: State = { phase: "loading", input: null, request: null, job: null, error: null };
  let flight: CompositionAttempt | null = null;
  let epoch = 0; let polling = false;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<State>) => { state = { ...state, ...patch }; listeners.forEach(fn => fn()); };
  const unsafe = () => state.phase === "sending" || state.phase === "uncertain";
  const received = (job: CompositionStatus) => set({ job, request: job, phase: job.status === "pending" ? "running" : job.status === "succeeded" ? "completed" : "failed",
    error: job.status === "failed" ? (job.error_code === "WORKSPACE_CAS_CONFLICT" ? "招标依据或稿件已变化，本次生成未替换当前稿件。" : "本次生成未完成，请查看来源和服务状态后重新发起。") : null });
  async function load(fresh = false) {
    if (unsafe() || state.phase === "running") return;
    const turn = ++epoch;
    set({ phase: "loading", input: null, error: null });
    try {
      flight = store.read();
      if (flight) { set({ phase: "uncertain", input: flight.input, error: "上次提交的结果尚未确认，请确认同一次生成请求。" }); return; }
      const latest = await api.latest(workspace);
      if (turn !== epoch) return;
      if (latest && (latest.status === "pending" || !fresh)) { received(latest); return; }
      const [basis, current] = await Promise.all([api.basis(workspace), api.current(workspace)]);
      if (turn !== epoch) return;
      if (!basis || current?.editor.pending_save_id) {
        set({ phase: "blocked", error: !basis ? "编制数据尚未就绪，请稍后重试。" : "当前稿件仍在保存，请等待保存完成。" });
      } else {
        set({ phase: "ready", job: null, request: null, input: { basis, expected: current ? { version_id: current.version_id, docx_sha256: current.docx_sha256 } : null } });
      }
    } catch { if (turn === epoch) set({ phase: "blocked", error: "无法读取编制依据或恢复信息，请刷新重试。" }); }
  }
  async function poll() {
    if (state.phase !== "running" || !state.request || polling) return;
    const turn = epoch; const request = state.request; polling = true;
    try {
      const job = await api.status(workspace, request.request_artifact_id);
      if (turn === epoch) {
        if (job.request_revision !== request.request_revision || job.frozen_input_sha256 !== request.frozen_input_sha256) throw new Error("request identity changed");
        received(job);
      }
    } catch { if (turn === epoch) set({ error: "暂时无法读取生成进度，正在保留当前任务，请稍后刷新。" }); }
    finally { polling = false; }
  }
  async function start() {
    if (state.phase !== "ready" && state.phase !== "uncertain") return;
    if (!flight) {
      if (!state.input) return;
      flight = { input: structuredClone(state.input), attempt: createMutationAttempt() };
      try { store.write(flight); } catch { flight = null; set({ error: "无法保存请求恢复信息，请检查浏览器存储后重试。" }); return; }
    }
    set({ phase: "sending", error: null });
    try {
      const request = await api.start(workspace, flight.input, flight.attempt);
      store.write(null); flight = null;
      set({ phase: "running", request, job: null });
      await poll();
    } catch (error) {
      if (error instanceof ApiError && (error.code === "AGENT_PROVIDER_UNAVAILABLE"
        || (error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429))) {
        try { store.write(null); } catch { /* retained attempt remains safe to replay */ }
        flight = null;
        set({ phase: error.status === 409 ? "conflict" : "blocked", input: null,
          error: error.code === "AGENT_PROVIDER_UNAVAILABLE" ? "编制服务配置尚未就绪，请联系管理员后重试。"
            : error.status === 409 ? "编制依据已变化，请刷新后重新确认生成。" : "生成请求被拒绝，请检查招标分析和项目权限。" });
      } else {
        set({ phase: "uncertain", error: "提交结果或队列投递尚未确认，请确认同一次请求，避免重复生成。" });
      }
    }
  }
  return { getState: () => state, load, start, poll, unsafe,
    subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
    cancelRead() { ++epoch; },
  };
}
