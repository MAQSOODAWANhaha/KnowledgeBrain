import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import type { DocxApi } from "../api/docx";
import type { FillApi, FillIdentity, FillInput, FillStatus } from "../api/fill";

export type FillAttempt = { input: FillInput; attempt: MutationAttempt };
export type FillAttemptStore = { read(): FillAttempt | null; write(value: FillAttempt | null): void };
type State = {
  phase: "loading" | "ready" | "blocked" | "sending" | "uncertain" | "running" | "completed" | "unchanged" | "failed";
  input: FillInput | null; request: FillIdentity | null; job: FillStatus | null; error: string | null;
  /** 已经请求停止：任务仍在写当前章，写完就停并出稿。 */
  stopping: boolean;
};

/** 「已填 N/M 章 · 当前章」——数字全部来自 checkpoint 进度，前端不自己推算。 */
export function fillProgressText(job: FillStatus | null, stopping = false): string {
  const detail = job?.progress?.detail ?? null;
  if (!job) return "尚未填充";
  if (job.status === "succeeded") {
    return detail?.draft_stopped ? "已按你的要求停止，已填的章都已保存" : "填充已完成，新版本已保存";
  }
  if (job.status === "failed") return "本次填充未完成";
  const total = detail?.draft_chapters;
  const filled = detail?.draft_filled;
  const parts: string[] = [];
  parts.push(typeof total === "number" && typeof filled === "number" ? `已填 ${filled}/${total} 章` : "正在准备填充");
  if (stopping) parts.push("正在停止：当前章写完后收尾");
  else if (detail?.boundary === "prepared") parts.push("正在等待模型");
  if (detail?.draft_active_title) parts.push(`当前章：${detail.draft_active_title}`);
  return parts.join(" · ");
}

export function createFillSession(
  api: FillApi & Pick<DocxApi, "current">,
  workspace: string,
  store: FillAttemptStore,
  onPublished?: () => void,
) {
  let state: State = { phase: "loading", input: null, request: null, job: null, error: null, stopping: false };
  let flight: FillAttempt | null = null;
  let epoch = 0;
  let polling = false;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<State>) => { state = { ...state, ...patch }; listeners.forEach(fn => fn()); };
  const unsafe = () => state.phase === "sending" || state.phase === "uncertain";
  const received = (job: FillStatus) => {
    const phase = job.status === "pending" ? "running" : job.status === "succeeded" ? "completed" : "failed";
    set({
      job, request: job, phase,
      error: job.status === "failed"
        ? (job.error_code === "WORKSPACE_CAS_CONFLICT"
          ? "稿件在填充期间已被改动，本次填充没有替换当前稿件。"
          : "本次填充未完成，可在稿件保存并关闭编辑器后重试。")
        : null,
    });
    if (job.status === "succeeded") onPublished?.();
  };
  async function load(fresh = false) {
    if (unsafe() || state.phase === "running") return;
    const turn = ++epoch;
    set({ phase: "loading", input: null, error: null, stopping: false });
    try {
      flight = store.read();
      if (flight) {
        set({ phase: "uncertain", input: flight.input, error: "上次提交的填充尚未确认，请确认同一次请求，避免重复填充。" });
        return;
      }
      const latest = await api.latest(workspace);
      if (turn !== epoch) return;
      if (latest && (latest.status === "pending" || !fresh)) { received(latest); return; }
      const [basis, current] = await Promise.all([api.basis(workspace), api.current(workspace)]);
      if (turn !== epoch) return;
      // 出稿会写一个新版本，SQL 在编辑器会话未关闭时拒绝入稿，所以这里先挡住。
      if (!basis || !current) {
        set({ phase: "blocked", error: !basis ? "填充依据尚未就绪，请稍后重试。" : "工作区还没有可填充的稿件。" });
      } else if (current.editor.pending_save_id || current.editor.key) {
        set({ phase: "blocked", error: "请先保存并关闭编辑器，再开始填充。" });
      } else {
        set({
          phase: "ready", job: null, request: null,
          input: { basis, expected: { version_id: current.version_id, docx_sha256: current.docx_sha256 } },
        });
      }
    } catch { if (turn === epoch) set({ phase: "blocked", error: "无法读取填充依据，请刷新重试。" }); }
  }
  async function poll() {
    if (state.phase !== "running" || !state.request || polling) return;
    const turn = epoch; const request = state.request; polling = true;
    try {
      const job = await api.status(workspace, request.request_artifact_id);
      if (turn === epoch) {
        if (job.request_revision !== request.request_revision || job.frozen_input_sha256 !== request.frozen_input_sha256) {
          throw new Error("request identity changed");
        }
        received(job);
      }
    } catch { if (turn === epoch) set({ error: "暂时无法读取填充进度，任务仍在继续，请稍后刷新。" }); }
    finally { polling = false; }
  }
  async function start() {
    if (state.phase !== "ready" && state.phase !== "uncertain") return;
    if (!flight) {
      if (!state.input) return;
      flight = { input: structuredClone(state.input), attempt: createMutationAttempt() };
      try { store.write(flight); }
      catch { flight = null; set({ error: "无法保存请求恢复信息，请检查浏览器存储后重试。" }); return; }
    }
    set({ phase: "sending", error: null });
    try {
      const request = await api.start(workspace, flight.input, flight.attempt);
      store.write(null); flight = null;
      if ("status" in request && request.status === "unchanged") {
        set({ phase: "unchanged", request: null, job: null, stopping: false });
        return;
      }
      set({ phase: "running", request: request as FillIdentity, job: null, stopping: false });
      await poll();
    } catch (error) {
      if (error instanceof ApiError && (error.code === "AGENT_PROVIDER_UNAVAILABLE"
        || (error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429))) {
        try { store.write(null); } catch { /* retained attempt remains safe to replay */ }
        flight = null;
        set({
          phase: "blocked", input: null,
          error: error.code === "AGENT_PROVIDER_UNAVAILABLE" ? "填充服务配置尚未就绪，请联系管理员后重试。"
            : error.status === 409 ? "稿件已变化，请刷新后重新开始填充。"
              : "填充请求被拒绝，请确认稿件已保存关闭且章节大纲已生成。",
        });
      } else {
        set({ phase: "uncertain", error: "提交结果尚未确认，请确认同一次请求，避免重复填充。" });
      }
    }
  }
  /// 停止不是取消：任务会把当前章写完、照常出稿，已填的章一个不丢。所以这里不撤
  /// 回请求、也不清进度，只把意向发出去并如实告诉用户还要等本章写完。
  async function stop() {
    if (state.phase !== "running" || !state.request || state.stopping) return;
    const request = state.request;
    set({ stopping: true, error: null });
    try { await api.stop(workspace, request.request_artifact_id); }
    catch { set({ stopping: false, error: "停止请求没有送达，任务仍在继续，请稍后重试。" }); }
  }
  return {
    getState: () => state, load, start, poll, stop, unsafe,
    subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
    cancelRead() { ++epoch; },
  };
}
