import { ApiError, NetworkTransportError } from "../../api";
import { docxApi, type DocxCurrent, type DocxRoundInput } from "../api/docx";
import { createDocxRound } from "./docxRoundSession";
import { describe, expect, it } from "./harness";

function fixture() {
  const basis = { document_set_id: "documents", document_set_sha256: "a".repeat(64),
    requirement_set_id: "requirements", requirement_set_sha256: "b".repeat(64) };
  const calls: Array<{ workspace: string; input: DocxRoundInput; file: File; key: string }> = [];
  const receipt = { round_id: "new-round", round_revision: 1, version_id: "new-version", docx_sha256: "c".repeat(64) };
  const api = {
    basis: async () => basis as typeof basis | null,
    current: async (): Promise<DocxCurrent | null> => null,
    publish: async (workspace: string, input: DocxRoundInput, file: File, attempt: { idempotencyKey: string }) => {
      calls.push({ workspace, input: structuredClone(input), file, key: attempt.idempotencyKey });
      return receipt;
    },
  };
  const round = createDocxRound(api, "workspace");
  const file = new File(["sample bytes"], "draft.docx");
  return { basis, api, round, calls, file, receipt };
}

describe("DOCX round publication", () => {
  it("first publication sends an explicit null expected with selected bytes", async () => {
    const f = fixture(); await f.round.load(); f.round.select(f.file); await f.round.publish();
    expect(f.calls[0].input).toEqual({ basis: f.basis, expected: null });
    expect(f.calls[0].file).toBe(f.file);
    expect(f.round.getState().phase).toBe("published");
    await f.round.publish(); expect(f.calls.length).toBe(1);
  });
  it("network ambiguity freezes file, basis, expected and key across retries", async () => {
    const f = fixture(); const send = f.api.publish;
    f.api.publish = async (...args) => { await send(...args); throw new NetworkTransportError(new Error("lost response")); };
    await f.round.load(); f.round.select(f.file); await f.round.publish();
    expect(f.round.unsafe()).toBe(true);
    f.round.select(new File(["other"], "other.docx"));
    f.basis.document_set_id = "changed";
    await f.round.load(); f.api.publish = send; await f.round.publish();
    expect(f.calls[1]).toEqual(f.calls[0]);
    expect(f.calls[1].file).toBe(f.file);
    expect(f.round.unsafe()).toBe(false);
  });
  it("version or input conflicts require fresh basis and explicit confirmation", async () => {
    for (const code of ["DOCX_VERSION_CAS_MISMATCH", "DOCX_ROUND_BASIS_CHANGED", "DOCX_ROUND_REQUIREMENTS_NOT_CURRENT"]) {
      const f = fixture(); const send = f.api.publish;
      f.api.publish = async (...args) => { await send(...args); throw new ApiError(409, "changed", code); };
      await f.round.load(); f.round.select(f.file); await f.round.publish();
      expect(f.round.getState().phase).toBe("conflict");
      await f.round.publish(); expect(f.calls.length).toBe(1);
      f.basis.document_set_id = "changed"; f.api.publish = send;
      await f.round.load(); expect(f.calls.length).toBe(1); await f.round.publish();
      expect(f.calls[1].key === f.calls[0].key).toBe(false);
      expect(f.calls[1].input.basis.document_set_id).toBe("changed");
    }
  });
  it("missing requirements and pending saves prevent publication", async () => {
    const f = fixture(); f.api.basis = async () => null;
    await f.round.load(); f.round.select(f.file); await f.round.publish();
    expect(f.round.getState().phase).toBe("blocked");
    f.api.basis = async () => f.basis;
    f.api.current = async () => ({ editor: { key: "active", pending_save_id: "save" } } as DocxCurrent);
    await f.round.load(); await f.round.publish();
    expect(f.calls.length).toBe(0);
  });
  it("new rounds pin the saved identity and exclude legacy content", async () => {
    const f = fixture(); f.api.current = async () => ({ version_id: "old", docx_sha256: "d".repeat(64),
      editor: { key: "reconnectable-after-clean-close", pending_save_id: null } } as DocxCurrent);
    await f.round.load(); f.round.select(f.file); await f.round.publish();
    expect(f.calls[0].input).toEqual({ basis: f.basis, expected: { version_id: "old", docx_sha256: "d".repeat(64) } });
  });
  it("a late read cannot restore a canceled preparation", async () => {
    const f = fixture(); let finish!: (value: typeof f.basis) => void;
    f.api.basis = () => new Promise(resolve => { finish = resolve; });
    const loading = f.round.load(); f.round.cancelRead(); finish(f.basis); await loading;
    expect(f.round.getState().input).toBe(null);
  });
  it("multipart uses the route contract, browser boundary, and provided retry key", async () => {
    const f = fixture(); const previous = globalThis.fetch;
    let path: unknown; let init: RequestInit | undefined;
    globalThis.fetch = async (url, options) => { path = url; init = options; return new Response(JSON.stringify(f.receipt), { status: 201 }); };
    try {
      await docxApi.publish("workspace/a", { basis: f.basis, expected: null }, f.file, { idempotencyKey: "same-key" });
      expect(path).toBe("/api/v2/submission-workspaces/workspace%2Fa/docx-rounds");
      expect(new Headers(init?.headers).get("Idempotency-Key")).toBe("same-key");
      expect(new Headers(init?.headers).has("Content-Type")).toBe(false);
      const body = init?.body as FormData;
      expect(JSON.parse(body.get("metadata") as string)).toEqual({ basis: f.basis, expected: null });
      expect(await (body.get("file") as File).text()).toBe(await f.file.text());
    } finally { globalThis.fetch = previous; }
  });
  it("an empty success response is uncertain rather than a published round", async () => {
    const f = fixture(); const previous = globalThis.fetch;
    globalThis.fetch = async () => new Response("null", { status: 201 });
    try {
      f.api.publish = docxApi.publish;
      await f.round.load(); f.round.select(f.file); await f.round.publish();
      expect(f.round.getState().phase).toBe("uncertain");
      expect(f.round.getState().receipt).toBe(null);
    } finally { globalThis.fetch = previous; }
  });
});
