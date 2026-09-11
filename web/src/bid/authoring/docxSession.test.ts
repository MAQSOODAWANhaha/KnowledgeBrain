import { NetworkTransportError } from "../../api";
import type { DocxApi, DocxCurrent, DocxSave, DocxSavedReceipt } from "../api/docx";
import { describe, expect, it } from "./harness";
import { createDocxSession, docxStatus } from "./docxSession";

function fixture() {
  let head: DocxCurrent = { project_id: "project-a", workspace_id: "workspace-a", round_id: "round-a",
    version_id: "version-a", docx_sha256: "a".repeat(64), revision: 1,
    editor: { key: "editor-a", base_version_id: "version-a", pending_save_id: null, save_error: null } };
  const calls: Array<{ body: DocxSave; key: string }> = [];
  let saved: DocxSavedReceipt | null = null;
  const api: DocxApi = {
    current: async () => structuredClone(head),
    open: async () => ({ config: {}, api_script_url: "https://example.invalid/api.js",
      session: { editor_key: "editor-a", round_id: "round-a", version_id: "version-a" } }),
    save: async (_workspace, body, attempt) => {
      calls.push({ body: structuredClone(body), key: attempt.idempotencyKey });
      head.editor.pending_save_id = "save-a";
      return { save_id: "save-a", pending: true, dispatch: true };
    },
    download: async () => new Blob(),
    saved: async () => saved && structuredClone(saved),
  };
  const session = createDocxSession(api, "workspace-a");
  return { api, session, calls, head: () => head,
    publish: (correlated = true) => {
      const parent = head.version_id;
      head = { ...head, revision: head.revision + 1, version_id: `version-${head.revision + 1}`,
        editor: { ...head.editor, pending_save_id: null } };
      if (correlated) saved = { save_id: "save-a", editor_key: "editor-a", round_id: head.round_id,
        parent_version_id: parent, revision: head.revision, version_id: head.version_id, docx_sha256: head.docx_sha256 };
    },
    edit: () => { session.changed(true); session.changed(false); },
    start: async () => { await session.start(); session.ready(); },
  };
}

describe("DOCX save confirmation", () => {
  it("another save cannot confirm this request merely because current advanced and pending cleared", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    f.publish(false); await f.session.refresh();
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.getState().dirty).toBe(true);
    expect(f.session.unsafe()).toBe(true);
  });
  it("a successful old save does not certify a newer current version", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    f.publish(); f.publish(false); await f.session.refresh();
    expect(f.session.getState().phase).toBe("stale");
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.unsafe()).toBe(true);
  });
  it("receipt lookup failure or mismatched scope cannot clear unsaved changes", async () => {
    for (const patch of [null, { save_id: "foreign" }, { editor_key: "other" }, { round_id: "other" },
      { parent_version_id: "other" }, { docx_sha256: "b".repeat(64) }]) {
      const f = fixture(); await f.start(); f.edit(); await f.session.save(); f.publish();
      const read = f.api.saved;
      f.api.saved = async (...args) => {
        if (!patch) throw new Error("offline");
        return { ...(await read(...args))!, ...patch };
      };
      await f.session.refresh();
      expect(f.session.getState().confirmed).toBe(false);
      expect(f.session.unsafe()).toBe(true);
    }
  });
  it("a callback committing during polling is confirmed by a later poll without false staleness", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    const saved = f.api.saved;
    f.api.saved = async (...args) => { f.publish(); f.api.saved = saved; return saved(...args); };
    await f.session.refresh();
    expect(f.session.getState().phase).toBe("ready");
    expect(f.session.getState().confirmed).toBe(false);
    await f.session.refresh();
    expect(f.session.getState().confirmed).toBe(true);
  });
  it("an absent current never opens an editor and a disappearing current invalidates it", async () => {
    const f = fixture();
    f.api.current = async () => null;
    await f.session.start();
    expect(f.session.getState().opened).toBe(null);
    expect(f.session.getState().error).toBe("尚未创建 DOCX 稿件。");
    f.api.current = async () => f.head();
    await f.start(); f.edit();
    f.api.current = async () => null;
    await f.session.refresh();
    expect(f.session.getState().phase).toBe("stale");
    expect(f.session.unsafe()).toBe(true);
  });
  it("editor synchronization never acknowledges persistence or permits leaving", async () => {
    const f = fixture(); await f.start(); f.edit();
    expect(f.session.getState().dirty).toBe(true);
    expect(docxStatus(f.session.getState())).toBe("有修改尚未保存");
    expect(f.session.close()).toBe(false);
  });
  it("only a published version after the correlated command confirms save", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.unsafe()).toBe(true);
    f.publish(); await f.session.refresh();
    expect(docxStatus(f.session.getState())).toBe("已保存");
    expect(f.session.close()).toBe(true);
  });
  it("later edits remain dirty when an earlier forcesave arrives", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    f.edit(); f.publish(); await f.session.refresh();
    expect(f.session.getState().dirty).toBe(true);
    expect(f.session.getState().confirmed).toBe(false);
  });
  it("uncertain delivery retains the exact payload and idempotency key", async () => {
    const f = fixture(); const send = f.api.save;
    let first = true;
    f.api.save = async (workspace, body, attempt) => {
      if (first) {
        first = false; f.calls.push({ body: structuredClone(body), key: attempt.idempotencyKey });
        throw new NetworkTransportError(new Error("connection reset"));
      }
      return send(workspace, body, attempt);
    };
    await f.start(); f.edit(); await f.session.save(); f.edit(); await f.session.save();
    expect(f.calls[0]).toEqual(f.calls[1]);
    f.publish(); await f.session.refresh();
    expect(f.session.getState().dirty).toBe(true);
  });
  it("a new round suspends saving without silently replacing the open editor", async () => {
    const f = fixture(); await f.start(); f.edit();
    f.head().round_id = "round-b"; await f.session.refresh(); await f.session.save();
    expect(f.session.getState().phase).toBe("stale");
    expect(f.session.getState().opened?.session.round_id).toBe("round-a");
    expect(f.calls.length).toBe(0);
  });
  it("a failed status read cannot preserve an all-saved indicator", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save(); f.publish(); await f.session.refresh();
    const read = f.api.current;
    f.api.current = async () => { throw new Error("offline"); };
    await f.session.refresh();
    expect(f.session.getState().confirmed).toBe(false);
    expect(docxStatus(f.session.getState())).toBe("保存状态待确认");
    f.api.current = read; await f.session.refresh();
    expect(f.session.getState().error).toBe(null);
    expect(docxStatus(f.session.getState())).toBe("当前保存版本已载入");
  });
  it("a StrictMode cleanup/restart ignores the previous open response", async () => {
    const f = fixture(); const actual = f.api.open;
    let finish!: (value: Awaited<ReturnType<DocxApi["open"]>>) => void;
    f.api.open = () => new Promise(resolve => { finish = resolve; });
    const old = f.session.start(); await Promise.resolve(); await Promise.resolve();
    f.session.dispose(); f.api.open = actual; await f.start();
    finish({ config: {}, api_script_url: "https://example.invalid/old.js",
      session: { editor_key: "old-editor", round_id: "round-a", version_id: "old-version" } });
    await old;
    expect(f.session.getState().opened?.session.editor_key).toBe("editor-a");
    expect(f.session.getState().phase).toBe("ready");
  });
});
