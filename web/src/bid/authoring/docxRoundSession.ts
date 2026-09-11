import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import type { DocxApi, DocxRoundApi, DocxRoundInput, DocxRoundReceipt } from "../api/docx";

type RoundState = {
  phase: "loading" | "ready" | "blocked" | "sending" | "uncertain" | "conflict" | "failed" | "published";
  input: DocxRoundInput | null;
  file: File | null;
  receipt: DocxRoundReceipt | null;
  error: string | null;
};

export function createDocxRound(api: Pick<DocxApi, "current"> & DocxRoundApi, workspace: string) {
  let state: RoundState = { phase: "loading", input: null, file: null, receipt: null, error: null };
  let request: { input: DocxRoundInput; file: File; attempt: MutationAttempt } | null = null;
  let epoch = 0;
  const listeners = new Set<() => void>();
  const set = (patch: Partial<RoundState>) => { state = { ...state, ...patch }; listeners.forEach(fn => fn()); };
  const unsafe = () => state.phase === "sending" || state.phase === "uncertain";
  async function load() {
    if (unsafe() || state.phase === "published") return;
    const turn = ++epoch;
    request = null;
    set({ phase: "loading", input: null, error: null });
    try {
      const [basis, current] = await Promise.all([api.basis(workspace), api.current(workspace)]);
      if (turn !== epoch) return;
      if (!basis) {
        set({ phase: "blocked", error: "当前文件集的要求尚未就绪，请完成文件冻结与要求整理后刷新。" });
      } else if (current?.editor.pending_save_id) {
        // A clean status-4 disconnect retains a reconnectable key. The caller
        // closes its editor first; the key alone does not prove active editing.
        set({ phase: "blocked", error: "当前稿件还有保存待确认，请等待保存完成后刷新。" });
      } else {
        set({ phase: "ready", input: { basis, expected: current
          ? { version_id: current.version_id, docx_sha256: current.docx_sha256 } : null } });
      }
    } catch { if (turn === epoch) set({ phase: "failed", error: "无法读取新轮依据，请重试。" }); }
  }
  async function publish() {
    if (state.phase !== "ready" && state.phase !== "uncertain") return;
    if (!request) {
      if (!state.input || !state.file) return;
      request = { input: structuredClone(state.input), file: state.file, attempt: createMutationAttempt() };
    }
    const flight = request;
    set({ phase: "sending", error: null });
    try {
      const receipt = await api.publish(workspace, flight.input, flight.file, flight.attempt);
      set({ phase: "published", receipt });
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408) {
        request = null;
        set({ phase: error.status === 409 ? "conflict" : "failed", input: null,
          error: error.status === 409 ? "文件依据或当前稿件已变化，本次未发布。请刷新依据后重新确认。"
            : "发布被拒绝，请检查文件及项目权限后刷新重试。" });
      } else {
        set({ phase: "uncertain", error: "发布结果尚未确认。请确认同一次请求的结果，避免重复创建新轮。" });
      }
    }
  }
  return {
    getState: () => state, unsafe, load, publish,
    subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
    select(file: File | null) { if (!unsafe() && state.phase !== "published") set({ file }); },
    cancelRead() { ++epoch; },
  };
}
