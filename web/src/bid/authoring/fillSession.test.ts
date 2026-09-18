import { ApiError, NetworkTransportError } from "../../api";
import type { DocxCurrent } from "../api/docx";
import type { FillStatus } from "../api/fill";
import { createFillSession, fillProgressText, type FillAttempt } from "./fillSession";
import { describe, expect, it } from "./harness";

function current(editor: Partial<DocxCurrent["editor"]> = {}): DocxCurrent {
  return {
    project_id: "project", workspace_id: "workspace", round_id: "round", revision: 3,
    version_id: "version", docx_sha256: "d".repeat(64),
    editor: { key: null, base_version_id: null, pending_save_id: null, save_error: null, ...editor },
  };
}
function fixture(overrides: { current?: DocxCurrent | null; latest?: FillStatus | null } = {}) {
  const basis = {
    document_set_id: "documents", document_set_sha256: "a".repeat(64),
    requirement_set_id: "analysis", requirement_set_sha256: "b".repeat(64),
  };
  const job: FillStatus = {
    request_artifact_id: "request", request_revision: 1, frozen_input_sha256: "c".repeat(64),
    workspace_id: "workspace", status: "pending", progress: null, result_identity: null, error_code: null,
  };
  const calls: FillAttempt[] = [];
  let stored: FillAttempt | null = null;
  const store = { read: () => structuredClone(stored), write(value: FillAttempt | null) { stored = structuredClone(value); } };
  const stops: string[] = [];
  const api = {
    basis: async () => basis,
    current: async () => ("current" in overrides ? overrides.current! : current()),
    latest: async () => overrides.latest ?? null,
    status: async () => job,
    start: async (_workspace: string, input: FillAttempt["input"], attempt: FillAttempt["attempt"]) => {
      calls.push(structuredClone({ input, attempt }));
      return job;
    },
    stop: async (_workspace: string, request: string) => { stops.push(request); },
  };
  return { api, store, job, basis, calls, stops, session: createFillSession(api, "workspace", store) };
}
describe("user triggered chapter filling", () => {
  it("freezes the saved version it fills and starts polling", async () => {
    const f = fixture();
    await f.session.load();
    expect(f.session.getState().phase).toBe("ready");
    await f.session.start();
    expect(f.calls[0].input).toEqual({ basis: f.basis, expected: { version_id: "version", docx_sha256: "d".repeat(64) } });
    expect(f.session.getState().phase).toBe("running");
    await f.session.start();
    expect(f.calls.length).toBe(1);
    expect(f.store.read()).toBe(null);
  });
  it("refuses to start while an editor session is still open", async () => {
    const f = fixture({ current: current({ key: "editor" }) });
    await f.session.load();
    expect(f.session.getState().phase).toBe("blocked");
    await f.session.start();
    expect(f.calls.length).toBe(0);
    expect(f.session.getState().error).toBe("请先保存并关闭编辑器，再开始填充。");
  });
  it("keeps one intent across a lost response so a retry cannot fill twice", async () => {
    const f = fixture();
    const start = f.api.start;
    f.api.start = async (...args) => { await start(...args); throw new NetworkTransportError(new Error("lost ACK")); };
    await f.session.load();
    await f.session.start();
    expect(f.session.getState().phase).toBe("uncertain");
    expect(f.session.unsafe()).toBe(true);
    f.api.start = start;
    await f.session.start();
    expect(f.calls.length).toBe(2);
    expect(f.calls[0].attempt).toEqual(f.calls[1].attempt);
    expect(f.session.getState().phase).toBe("running");
  });
  it("reports a rejected request without keeping a replayable attempt", async () => {
    const f = fixture();
    f.api.start = async () => { throw new ApiError(422, "rejected", "DOCX_COMPOSITION_INPUT_INVALID"); };
    await f.session.load();
    await f.session.start();
    expect(f.session.getState().phase).toBe("blocked");
    expect(f.store.read()).toBe(null);
  });
  it("stopping asks once and keeps the run until the current chapter is done", async () => {
    const f = fixture();
    await f.session.load(); await f.session.start();
    await f.session.stop(); await f.session.stop();
    expect(f.stops).toEqual(["request"]);
    expect(f.session.getState().phase).toBe("running");
    expect(f.session.getState().stopping).toBe(true);
    expect(fillProgressText(f.session.getState().job, true)).toBe("正在准备填充 · 正在停止：当前章写完后收尾");
  });
  it("a failed stop leaves the run alone and says so", async () => {
    const f = fixture();
    f.api.stop = async () => { throw new NetworkTransportError(new Error("lost")); };
    await f.session.load(); await f.session.start(); await f.session.stop();
    expect(f.session.getState().stopping).toBe(false);
    expect(f.session.getState().phase).toBe("running");
    expect(f.session.getState().error).toBe("停止请求没有送达，任务仍在继续，请稍后重试。");
  });
  it("shows chapter counts and the active chapter from the checkpoint only", () => {
    expect(fillProgressText(null)).toBe("尚未填充");
    const job = { ...fixture().job };
    expect(fillProgressText(job)).toBe("正在准备填充");
    job.progress = {
      phase: "main", sequence: 4, attempt: 1,
      detail: { draft_chapters: 12, draft_filled: 5, draft_active_title: "施工组织设计" },
    };
    expect(fillProgressText(job)).toBe("已填 5/12 章 · 当前章：施工组织设计");
    expect(fillProgressText({ ...job, status: "succeeded" })).toBe("填充已完成，新版本已保存");
    const stopped = { ...job, status: "succeeded" as const, progress: { ...job.progress!, detail: { ...job.progress!.detail!, draft_stopped: true } } };
    expect(fillProgressText(stopped)).toBe("已按你的要求停止，已填的章都已保存");
  });
});


describe("fill without empty chapters", () => {
  it("returns unchanged without polling or reporting a new version", async () => {
    const f = fixture();
    let polls = 0;
    const session = createFillSession({ ...f.api,
      start: async (_workspace, input) => ({ status: "unchanged" as const, current: input.expected }),
      status: async () => { polls++; return f.job; },
    }, "workspace", f.store);
    await session.load();
    await session.start();
    expect(session.getState().phase).toBe("unchanged");
    expect(polls).toBe(0);
    expect(f.store.read()).toBe(null);
  });
});
