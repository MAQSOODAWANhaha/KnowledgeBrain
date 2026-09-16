import { ApiError, NetworkTransportError } from "../../api";
import type { DocxCurrent } from "../api/docx";
import { exportsApi, downloadExport, downloadExportReport, type ExportPackage, type ExportStatus } from "../api/exports";
import { createExportSession, type ExportAttempt } from "./exportSession";
import { describe, expect, it } from "./harness";

function fixture() {
  const current: DocxCurrent = { workspace_id: "workspace", project_id: "project", version_id: "version", docx_sha256: "a".repeat(64),
    round_id: "round", revision: 1, editor: { key: "editor", base_version_id: "version", pending_save_id: null, save_error: null } };
  const result: ExportPackage = { manifest_id: "manifest", source: { version_id: current.version_id, docx_sha256: current.docx_sha256 },
    outputs: { docx: { artifact_id: "docx", sha256: current.docx_sha256, byte_length: 123 }, pdf: { artifact_id: "pdf", sha256: "b".repeat(64), byte_length: 124 } },
    assessment_report_id: "report", assessment_report_sha256: "c".repeat(64) };
  const job: ExportStatus = { request_artifact_id: "request", request_revision: 1, frozen_input_sha256: "d".repeat(64),
    kind: "SubmissionExport", status: "pending", result_identity: null, error_code: null };
  const calls: ExportAttempt[] = []; let stored: ExportAttempt | null = null;
  const store = { read: () => structuredClone(stored), write(value: ExportAttempt | null) { stored = structuredClone(value); } };
  const api = { current: async () => current, list: async (): Promise<ExportPackage[]> => [], status: async () => structuredClone(job),
    start: async (_workspace: string, input: ExportAttempt["input"], attempt: ExportAttempt["attempt"]) => {
      calls.push(structuredClone({ input, attempt, request: null })); return structuredClone(job);
    } };
  return { current, result, job, calls, api, store, session: createExportSession(api, "workspace", store) };
}
describe("same-version export package", () => {
  it("prevents duplicate submit and resumes pending request after reload", async () => {
    const f = fixture(); await f.session.load(); await f.session.start(); await f.session.start();
    expect(f.calls.length).toBe(1); expect(f.session.getState().phase).toBe("running");
    const reopened = createExportSession(f.api, "workspace", f.store); await reopened.load();
    expect(reopened.getState().phase).toBe("running"); expect(f.calls.length).toBe(1);
    f.job.status = "succeeded"; f.job.result_identity = f.result; await reopened.poll();
    expect(reopened.getState().result).toEqual(f.result); expect(reopened.getState().phase).toBe("completed");
  });
  it("lost receipt and queue uncertainty preserve exact intent and key across reload", async () => {
    for (const error of [new NetworkTransportError(new Error("lost")), new ApiError(503, "queue unavailable", "QUEUE_UNAVAILABLE")]) {
      const f = fixture(); const start = f.api.start;
      f.api.start = async (...args) => { await start(...args); throw error; };
      await f.session.load(); await f.session.start(); expect(f.session.getState().phase).toBe("uncertain");
      f.current.version_id = "new-version"; f.api.start = start;
      const reopened = createExportSession(f.api, "workspace", f.store); await reopened.load(); await reopened.load(true); await reopened.start();
      expect(f.calls.length).toBe(2); expect(f.calls[1]).toEqual(f.calls[0]);
    }
  });
  it("pending save or failed save blocks export", async () => {
    for (const condition of ["pending", "error"]) {
      const f = fixture(); if (condition === "pending") f.current.editor.pending_save_id = "save";
      else f.current.editor.save_error = { kind: "callback", code: 1 };
      await f.session.load(); await f.session.start(); expect(f.session.getState().phase).toBe("blocked"); expect(f.calls.length).toBe(0);
    }
  });
  it("poll failure and identity mismatch keep the original task without downloads", async () => {
    const f = fixture(); await f.session.load(); await f.session.start();
    const status = f.api.status; f.api.status = async () => { throw new Error("offline"); }; await f.session.poll();
    expect(f.session.getState().phase).toBe("running"); expect(f.session.getState().request?.request_artifact_id).toBe("request");
    f.api.status = status; f.job.request_revision += 1; await f.session.poll();
    expect(f.session.getState().result).toBe(null); expect(f.session.getState().phase).toBe("running");
  });
  it("completed package must match the frozen DOCX and cannot represent a newer draft", async () => {
    const f = fixture(); await f.session.load(); await f.session.start();
    f.job.status = "succeeded"; f.job.result_identity = f.result; f.result.source.version_id = "wrong-version";
    await f.session.poll(); expect(f.session.getState().result).toBe(null); expect(f.session.getState().phase).toBe("running");
    f.result.source.version_id = "version"; await f.session.poll(); expect(f.session.getState().phase).toBe("completed");
    f.current.version_id = "new-version"; await f.session.load(true); expect(f.session.getState().input?.version_id).toBe("new-version");
    expect(f.session.getState().result).toBe(null); expect(f.calls.length).toBe(1);
  });
  it("failed export requires refresh and explicit new intent", async () => {
    const f = fixture(); await f.session.load(); await f.session.start(); f.job.status = "failed"; await f.session.poll();
    await f.session.start(); expect(f.calls.length).toBe(1); await f.session.load(true); expect(f.calls.length).toBe(1);
    f.job.status = "pending"; await f.session.start(); expect(f.calls.length).toBe(2);
    expect(f.calls[0].attempt.idempotencyKey === f.calls[1].attempt.idempotencyKey).toBe(false);
  });
  it("version conflict requires refreshed source and does not silently retry", async () => {
    const f = fixture(); const start = f.api.start;
    f.api.start = async (...args) => { await start(...args); throw new ApiError(409, "version changed"); };
    await f.session.load(); await f.session.start(); expect(f.session.getState().phase).toBe("blocked"); expect(f.store.read()).toBe(null);
    await f.session.start(); expect(f.calls.length).toBe(1); f.api.start = start; await f.session.load(true); await f.session.start();
    expect(f.calls[0].attempt.idempotencyKey === f.calls[1].attempt.idempotencyKey).toBe(false);
  });
  it("read-only projects retain completed downloads but cannot export anew", async () => {
    const f = fixture(); f.api.list = async () => [f.result];
    const session = createExportSession(f.api, "workspace", f.store, true); await session.load(); await session.start();
    expect(session.getState().result).toEqual(f.result); expect(f.calls.length).toBe(0);
  });
  it("unavailable recovery storage prevents mutation", async () => {
    const f = fixture(); const session = createExportSession(f.api, "workspace", { read: () => null, write: () => { throw new Error("quota"); } });
    await session.load(); await session.start(); expect(f.calls.length).toBe(0); expect(session.getState().phase).toBe("ready");
  });
  it("wire requests bind version, If-Match and key and download only selected output IDs", async () => {
    const original = globalThis.fetch; const calls: { path: string; init: RequestInit | undefined }[] = []; const f = fixture();
    globalThis.fetch = async (path, init) => { calls.push({ path: String(path), init }); return new Response(JSON.stringify(f.job), { status: 202 }); };
    try {
      await exportsApi.start("work/space", f.result.source, { idempotencyKey: "same-intent" });
      expect(calls[0].path).toBe("/api/v2/submission-workspaces/work%2Fspace/exports");
      expect(JSON.parse(String(calls[0].init?.body))).toEqual({ version_id: "version" });
      expect(new Headers(calls[0].init?.headers).get("If-Match")).toBe(f.current.docx_sha256);
      expect(new Headers(calls[0].init?.headers).get("Idempotency-Key")).toBe("same-intent");
      await downloadExport("workspace", "pdf/id"); await downloadExportReport("workspace", "manifest/id");
      expect(calls[1].path).toBe("/api/v2/submission-workspaces/workspace/exports/pdf%2Fid/download");
      expect(calls[2].path).toBe("/api/v2/submission-workspaces/workspace/exports/manifest%2Fid/assessment-report");
      globalThis.fetch = async () => new Response("null", { status: 202 });
      let error: unknown; try { await exportsApi.start("workspace", f.result.source, { idempotencyKey: "same-intent" }); } catch (caught) { error = caught; }
      expect(error instanceof NetworkTransportError).toBe(true);
    } finally { globalThis.fetch = original; }
  });
});
