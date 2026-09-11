import { ApiError, NetworkTransportError } from "../../api";
import { compositionApi, type CompositionStatus } from "../api/composition";
import { createCompositionSession, type CompositionAttempt } from "./compositionSession";
import { describe, expect, it } from "./harness";
function fixture() {
  const basis = { document_set_id: "documents", document_set_sha256: "a".repeat(64), requirement_set_id: "analysis", requirement_set_sha256: "b".repeat(64) };
  const job: CompositionStatus = { request_artifact_id: "request", request_revision: 1, frozen_input_sha256: "c".repeat(64), workspace_id: "workspace", basis, expected: null,
    status: "pending", progress: null, result_identity: null, error_code: null };
  const calls: CompositionAttempt[] = [];
  let stored: CompositionAttempt | null = null;
  const store = { read: () => structuredClone(stored), write(value: CompositionAttempt | null) { stored = structuredClone(value); } };
  const api = { basis: async () => basis, current: async () => null, latest: async (): Promise<CompositionStatus | null> => null,
    status: async () => job, start: async (_workspace: string, input: CompositionAttempt["input"], attempt: CompositionAttempt["attempt"]) => {
      calls.push(structuredClone({ input, attempt })); return job;
    } };
  return { api, store, job, basis, calls, session: createCompositionSession(api, "workspace", store) };
}
describe("complete template composition", () => {
  it("start freezes actual source basis and enters existing request polling", async () => {
    const f = fixture(); await f.session.load(); await f.session.start();
    expect(f.calls[0].input).toEqual({ basis: f.basis, expected: null });
    expect(f.session.getState().phase).toBe("running"); expect(f.session.unsafe()).toBe(false);
    await f.session.start(); expect(f.calls.length).toBe(1); expect(f.store.read()).toBe(null);
  });
  it("lost response retains exact intent/key across reload and source changes", async () => {
    const f = fixture(); const start = f.api.start;
    f.api.start = async (...args) => { await start(...args); throw new NetworkTransportError(new Error("lost ACK")); };
    await f.session.load(); await f.session.start(); expect(f.session.unsafe()).toBe(true);
    f.basis.document_set_id = "later-source"; f.api.start = start;
    const reopened = createCompositionSession(f.api, "workspace", f.store);
    await reopened.load(); expect(reopened.getState().phase).toBe("uncertain");
    await reopened.start(); expect(f.calls[1]).toEqual(f.calls[0]); expect(reopened.getState().phase).toBe("running");
  });
  it("known missing service configuration does not leave an ambiguous submission", async () => {
    const f = fixture(); f.api.start = async () => { throw new ApiError(503, "config missing", "AGENT_PROVIDER_UNAVAILABLE"); };
    await f.session.load(); await f.session.start(); expect(f.session.getState().phase).toBe("blocked");
    expect(f.session.unsafe()).toBe(false); expect(f.store.read()).toBe(null);
  });
  it("queue unavailable replays the original request instead of starting anew", async () => {
    const f = fixture(); const start = f.api.start;
    f.api.start = async (...args) => { await start(...args); throw new ApiError(503, "queue unavailable", "QUEUE_UNAVAILABLE"); };
    await f.session.load(); await f.session.start(); await f.session.load(true); f.api.start = start; await f.session.start();
    expect(f.calls[1]).toEqual(f.calls[0]);
  });
  it("reload resumes a pending server task without another POST", async () => {
    const f = fixture(); f.api.latest = async () => f.job;
    await f.session.load(true); expect(f.session.getState().phase).toBe("running"); await f.session.start(); expect(f.calls.length).toBe(0);
    f.job.status = "succeeded"; f.job.result_identity = { version_id: "generated", docx_sha256: "d".repeat(64), round_id: "round", round_revision: 1, composition_status: "reviewed_template" };
    await f.session.poll(); expect(f.session.getState().phase).toBe("completed");
  });
  it("poll failure retains original identity and never creates replacement work", async () => {
    const f = fixture(); await f.session.load(); await f.session.start();
    f.api.status = async () => { throw new NetworkTransportError(new Error("read failed")); };
    await f.session.poll(); expect(f.session.getState().phase).toBe("running"); expect(f.session.getState().request?.request_artifact_id).toBe("request");
    await f.session.load(true); expect(f.calls.length).toBe(1);
  });
  it("version conflict needs a fresh basis and a new explicit submit", async () => {
    const f = fixture(); const start = f.api.start;
    f.api.start = async (...args) => { await start(...args); throw new ApiError(409, "stale", "WORKSPACE_CAS_CONFLICT"); };
    await f.session.load(); await f.session.start(); expect(f.session.getState().phase).toBe("conflict");
    await f.session.start(); expect(f.calls.length).toBe(1);
    f.api.start = start; await f.session.load(true); expect(f.calls.length).toBe(1); await f.session.start();
    expect(f.calls[0].attempt.idempotencyKey === f.calls[1].attempt.idempotencyKey).toBe(false);
  });
  it("cannot submit without durable browser recovery information", async () => {
    const f = fixture(); const session = createCompositionSession(f.api, "workspace", { read: () => null, write: () => { throw new Error("storage unavailable"); } });
    await session.load(); await session.start(); expect(f.calls.length).toBe(0); expect(session.getState().phase).toBe("ready");
  });
  it("late source reads cannot revive a closed view", async () => {
    const f = fixture(); let finish!: (value: null) => void;
    f.api.latest = () => new Promise(resolve => { finish = resolve; });
    const loading = f.session.load(); f.session.cancelRead(); finish(null); await loading;
    expect(f.session.getState().input).toBe(null);
  });
  it("wire receipt validation preserves mutation ambiguity and exact key", async () => {
    const previous = globalThis.fetch; let sent: RequestInit | undefined;
    globalThis.fetch = async (_path, init) => { sent = init; return new Response("null", { status: 202 }); };
    try {
      let error: unknown;
      const f = fixture(); try { await compositionApi.start("workspace", { basis: f.basis, expected: null }, { idempotencyKey: "frozen-key" }); } catch (e) { error = e; }
      expect(error instanceof NetworkTransportError).toBe(true); expect(new Headers(sent?.headers).get("Idempotency-Key")).toBe("frozen-key");
    } finally { globalThis.fetch = previous; }
  });
});
