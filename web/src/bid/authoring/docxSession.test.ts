import { ApiError, NetworkTransportError } from "../../api";
import type { DocxApi, DocxCurrent, DocxSave, DocxSavedReceipt } from "../api/docx";
import { describe, expect, it } from "./harness";
import { createDocxSession, docxStatus, type DocxSessionApi, type DocxSessionOptions } from "./docxSession";

function fixture(options: DocxSessionOptions = {}) {
  let head: DocxCurrent = { project_id: "project-a", workspace_id: "workspace-a", round_id: "round-a",
    version_id: "version-a", docx_sha256: "a".repeat(64), revision: 1,
    editor: { key: "editor-a", base_version_id: "version-a", pending_save_id: null, save_error: null } };
  const calls: Array<{ body: DocxSave; key: string }> = [];
  let saved: DocxSavedReceipt | null = null;
  const api: DocxSessionApi = {
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
  const session = createDocxSession(api, "workspace-a", options);
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
  it("view mode requires the server's signed read-only configuration", async () => {
    const f = fixture();
    await f.session.start({ mode: "view" });
    expect(f.session.getState().opened).toBe(null);
    expect(f.session.getState().phase).toBe("failed");
    const open = f.api.open;
    f.api.open = async (...args) => ({ ...(await open(...args)),
      config: { editorConfig: { mode: "view" }, document: { permissions: { edit: false } } } });
    await f.session.start({ mode: "view" }); f.session.ready(); f.edit(); await f.session.save();
    expect(f.session.getState().mode).toBe("view");
    expect(f.session.getState().dirty).toBe(false);
    expect(f.calls.length).toBe(0);
  });
  it("a failed server-side close preserves the current editor and prevents a mode switch", async () => {
    const f = fixture(); await f.start();
    const opened = f.session.getState().opened;
    f.api.close = async () => { throw new Error("offline"); };
    expect(await f.session.start({ mode: "view" })).toBe(false);
    expect(f.session.getState().opened).toBe(opened);
    expect(f.session.getState().mode).toBe("edit");
    expect(await f.session.close()).toBe(false);
    expect(f.session.getState().opened).toBe(opened);
  });
  it("late edits during a mode-switch close preserve the original document for recovery", async () => {
    const f = fixture(); await f.start(); const opened = f.session.getState().opened;
    const open = f.api.open;
    f.api.open = async (...args) => ({ ...(await open(...args)),
      config: { editorConfig: { mode: "view" }, document: { permissions: { edit: false } } } });
    let finish!: () => void;
    f.api.close = () => new Promise(resolve => { finish = resolve; });
    const switching = f.session.start({ mode: "view" });
    await Promise.resolve(); f.edit(); finish();
    expect(await switching).toBe(false);
    expect(f.session.getState().opened).toBe(opened);
    expect(f.session.getState().dirty).toBe(true);
    expect(f.session.unsafe()).toBe(true);
  });
  it("late edits during explicit close cannot destroy the original document", async () => {
    const f = fixture(); await f.start(); const opened = f.session.getState().opened;
    let finish!: () => void;
    f.api.close = () => new Promise(resolve => { finish = resolve; });
    const closing = f.session.close(); await Promise.resolve(); f.edit(); finish();
    expect(await closing).toBe(false);
    expect(f.session.getState().opened).toBe(opened);
    expect(f.session.getState().dirty).toBe(true);
  });
  it("dispose while waiting for close cannot resurrect an editor or create another server session", async () => {
    const f = fixture(); await f.start(); let opens = 0;
    const open = f.api.open; f.api.open = async (...args) => { opens++; return open(...args); };
    let finish!: () => void;
    f.api.close = () => new Promise(resolve => { finish = resolve; });
    const switching = f.session.start({ mode: "view" }); await Promise.resolve();
    f.session.dispose(); finish(); await switching;
    expect(opens).toBe(0);
  });
  it("an unknown editor opening retries its exact request and key after a reload", async () => {
    const data = new Map<string, string>();
    const storage = { getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => { data.set(key, value); }, removeItem: (key: string) => { data.delete(key); } };
    const options = { storage, storageKey: "owned-user:workspace-a" };
    const first = fixture(options); const calls: unknown[] = []; let firstKey: string | null = null;
    const actual = first.api.open;
    first.api.open = async (workspace, expected, attempt, mode) => {
      calls.push({ expected, attempt, mode });
      if (!firstKey) { firstKey = attempt.idempotencyKey; throw new NetworkTransportError(new Error("response lost after commit")); }
      if (attempt.idempotencyKey !== firstKey) throw new ApiError(409, "another editing session is active");
      return actual(workspace, expected, attempt, mode);
    };
    await first.session.start({ mode: "edit" }); first.session.dispose();
    const restored = createDocxSession(first.api, "workspace-a", options);
    await restored.start({ mode: "view" });
    expect(calls.length).toBe(1);
    await restored.confirmOpen();
    expect(calls.length).toBe(2);
    expect(calls[1]).toEqual(calls[0]);
    expect(restored.getState().opened?.session.editor_key).toBe("editor-a");
    expect(data.size).toBe(0);
  });
  it("edits drained by the input-lock preflight keep a live session that can still save", async () => {
    let finish!: () => void;
    const f = fixture({ beforeClose: () => new Promise(resolve => { finish = resolve; }) });
    await f.start(); let closes = 0;
    f.api.close = async () => { closes++; };
    const closing = f.session.close(); f.edit(); finish();
    expect(await closing).toBe(false);
    expect(closes).toBe(0);
    expect(f.session.getState().recoveryRequired).toBe(false);
    await f.session.save();
    expect(f.calls.length).toBe(1);
    f.publish(); await f.session.refresh();
    expect(f.session.getState().confirmed).toBe(true);
  });
  it("a confirmed native recovery export closes only after explicit acknowledgement and never reports a platform save", async () => {
    const f = fixture(); await f.start(); const opened = f.session.getState().opened;
    let finish!: () => void;
    f.api.close = () => new Promise(resolve => { finish = resolve; });
    const closing = f.session.close(); await Promise.resolve(); f.edit(); finish(); await closing;
    expect(f.session.getState().recoveryRequired).toBe(true);
    expect(f.session.getState().opened).toBe(opened);
    await f.session.save(); expect(f.calls.length).toBe(0);
    f.session.changed(true);
    expect(await f.session.confirmRecoveredClose()).toBe(false);
    f.session.changed(false);
    f.api.close = async () => {};
    expect(await f.session.confirmRecoveredClose()).toBe(true);
    expect(f.session.getState().opened).toBe(null);
    expect(f.session.getState().dirty).toBe(false);
    expect(f.session.getState().saved).toBe(null);
    expect(f.session.getState().confirmed).toBe(false);
  });
  it("read-only projects block new edit sessions while an existing edit session can still save and close", async () => {
    const f = fixture({ readOnly: true }); let opens = 0;
    const open = f.api.open; f.api.open = async (...args) => { opens++; return open(...args); };
    expect(await f.session.start({ mode: "edit" })).toBe(false);
    expect(opens).toBe(0);
    f.session.setReadOnly(false); await f.start(); f.edit(); f.session.setReadOnly(true);
    await f.session.save(); f.publish(); await f.session.refresh();
    expect(f.session.getState().confirmed).toBe(true);
    expect(await f.session.close()).toBe(true);
    expect(await f.session.start({ mode: "edit" })).toBe(false);
    expect(opens).toBe(1);
  });
  it("failure to persist opening recovery prevents a new server session", async () => {
    const f = fixture({ storageKey: "owned-user:workspace-a", storage: {
      getItem: () => null, removeItem: () => {}, setItem: () => { throw new Error("storage unavailable"); },
    } });
    let opens = 0; f.api.open = async () => { opens++; throw new Error("must not send"); };
    expect(await f.session.start()).toBe(false);
    expect(opens).toBe(0);
    expect(f.session.getState().error?.includes("本次尚未发送")).toBe(true);
  });
  it("a known rejection from another active session never closes that session or retains a false pending open", async () => {
    const f = fixture(); let closes = 0;
    f.api.close = async () => { closes++; };
    f.api.open = async () => { throw new ApiError(409, "another editing session is active"); };
    expect(await f.session.start()).toBe(false);
    expect(closes).toBe(0);
    expect(f.session.getState().openPending).toBe(false);
  });
  it("a disposed instance cannot close the session that a new instance recovered with its original opening key", async () => {
    const data = new Map<string, string>();
    const options = { storageKey: "owned-user:workspace-a", storage: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => { data.set(key, value); }, removeItem: (key: string) => { data.delete(key); },
    } };
    const first = fixture(options); const open = first.api.open; let closes = 0;
    first.api.close = async () => { closes++; };
    let finish!: (value: Awaited<ReturnType<DocxApi["open"]>>) => void;
    first.api.open = () => new Promise(resolve => { finish = resolve; });
    const old = first.session.start();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    first.session.dispose(); first.api.open = open;
    const restored = createDocxSession(first.api, "workspace-a", options);
    await restored.confirmOpen();
    finish({ config: {}, api_script_url: "https://example.invalid/api.js",
      session: { editor_key: "editor-a", round_id: "round-a", version_id: "version-a" } });
    await old;
    expect(closes).toBe(0);
    expect(restored.getState().opened?.session.editor_key).toBe("editor-a");
  });
  it("an unknown close keeps the iframe locked until the same editor key is explicitly confirmed", async () => {
    const f = fixture(); await f.start(); const opened = f.session.getState().opened;
    const keys: string[] = []; let lose = true;
    f.api.close = async (_workspace, key) => {
      keys.push(key);
      if (lose) { lose = false; throw new NetworkTransportError(new Error("close response lost")); }
    };
    expect(await f.session.close()).toBe(false);
    expect(f.session.getState().closePending).toBe(true);
    expect(f.session.getState().opened).toBe(opened);
    expect(f.session.unsafe()).toBe(true);
    expect(await f.session.start({ mode: "view" })).toBe(false);
    expect(await f.session.confirmClose()).toBe(true);
    expect(keys).toEqual(["editor-a", "editor-a"]);
    expect(f.session.getState().opened).toBe(null);
  });
  it("late edits after an unknown close require exported recovery before retrying that close", async () => {
    const f = fixture(); await f.start(); let lose = true; const keys: string[] = [];
    f.api.close = async (_workspace, key) => {
      keys.push(key);
      if (lose) { lose = false; throw new NetworkTransportError(new Error("close response lost")); }
    };
    await f.session.close(); f.edit();
    expect(f.session.getState().recoveryRequired).toBe(true);
    expect(await f.session.confirmClose()).toBe(false);
    expect(keys.length).toBe(1);
    expect(await f.session.confirmRecoveredClose()).toBe(true);
    expect(keys).toEqual(["editor-a", "editor-a"]);
    expect(f.session.getState().dirty).toBe(false);
    expect(f.session.getState().confirmed).toBe(false);
  });
  it("disposing during the input-lock preflight releases the clean owned session without opening another", async () => {
    let finish!: () => void;
    const f = fixture({ beforeClose: () => new Promise(resolve => { finish = resolve; }) });
    await f.start(); let closes = 0; let opens = 0;
    const open = f.api.open; f.api.open = async (...args) => { opens++; return open(...args); };
    f.api.close = async () => { closes++; };
    const switching = f.session.start({ mode: "view" }); f.session.dispose(); finish(); await switching;
    expect(closes).toBe(1);
    expect(opens).toBe(0);
  });
  it("a historical view resolves the requested immutable version instead of silently opening the head", async () => {
    const f = fixture(); const old = structuredClone(f.head()); f.publish(false);
    f.api.version = async () => old;
    const open = f.api.open;
    f.api.open = async (...args) => ({ ...(await open(...args)),
      config: { editorConfig: { mode: "view" }, document: { permissions: { edit: false } } } });
    await f.session.start({ mode: "view", expected: { version_id: old.version_id, docx_sha256: old.docx_sha256 } });
    f.session.ready(); const opened=f.session.getState().opened;
    await f.session.refresh();
    expect(f.session.getState().current?.version_id).toBe(old.version_id);
    expect(f.session.getState().available?.version_id).toBe("version-2");
    expect(f.session.getState().opened).toBe(opened);
  });
  it("a reloaded read-only view clears completed save metadata without replacing its reading version", async () => {
    const f = fixture(); f.head().editor.pending_save_id = "save-a";
    const original = structuredClone(f.head());
    const open = f.api.open;
    f.api.open = async (...args) => ({ ...(await open(...args)),
      config: { editorConfig: { mode: "view" }, document: { permissions: { edit: false } } } });
    let versionReads = 0;
    f.api.version = async (_workspace, id) => {
      versionReads++;
      expect(id).toBe(original.version_id);
      return { ...original, editor: { ...original.editor, key: null, pending_save_id: null } };
    };
    await f.session.start({ mode: "view" }); f.session.ready();
    const opened = f.session.getState().opened;
    expect(f.session.unsafe()).toBe(true);
    f.publish(); await f.session.refresh();
    expect(versionReads).toBe(1);
    expect(f.session.getState().current?.version_id).toBe(original.version_id);
    expect(f.session.getState().current?.docx_sha256).toBe(original.docx_sha256);
    expect(f.session.getState().current?.round_id).toBe(original.round_id);
    expect(f.session.getState().opened).toBe(opened);
    expect(f.session.getState().available?.version_id).toBe("version-2");
    expect(f.session.getState().current?.editor.pending_save_id).toBe(null);
    expect(f.session.unsafe()).toBe(false);
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.getState().saved).toBe(null);
    expect(docxStatus(f.session.getState())).toBe("当前保存版本已载入");
    expect(await f.session.close()).toBe(true);
  });
  it("read-only polling refreshes same-version editor metadata without asserting a local save receipt", async () => {
    const f = fixture(); f.head().editor.pending_save_id = "save-a";
    const open = f.api.open;
    f.api.open = async (...args) => ({ ...(await open(...args)),
      config: { editorConfig: { mode: "view" }, document: { permissions: { edit: false } } } });
    await f.session.start({ mode: "view" }); f.session.ready();
    f.head().editor.pending_save_id = null;
    await f.session.refresh();
    expect(f.session.unsafe()).toBe(false);
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.getState().saved).toBe(null);
    expect(f.session.getState().current?.version_id).toBe("version-a");
  });
  it("mismatched historical metadata cannot clear the pinned reader's pending save", async () => {
    for (const patch of [{ version_id: "foreign" }, { docx_sha256: "b".repeat(64) }, { round_id: "foreign" }, { workspace_id: "foreign" }, { project_id: "foreign" }]) {
      const f = fixture(); f.head().editor.pending_save_id = "save-a";
      const original = structuredClone(f.head());
      const open = f.api.open;
      f.api.open = async (...args) => ({ ...(await open(...args)),
        config: { editorConfig: { mode: "view" }, document: { permissions: { edit: false } } } });
      f.api.version = async () => ({ ...original, ...patch, editor: { ...original.editor, pending_save_id: null } });
      await f.session.start({ mode: "view" }); f.session.ready();
      f.publish(); await f.session.refresh();
      expect(f.session.getState().current?.version_id).toBe(original.version_id);
      expect(f.session.getState().current?.editor.pending_save_id).toBe("save-a");
      expect(f.session.unsafe()).toBe(true);
      expect(f.session.getState().confirmed).toBe(false);
    }
  });
  it("an explicit reopen cannot discard dirty edits", async () => {
    const f = fixture(); await f.start(); f.edit();
    await f.session.start();
    expect(f.session.getState().dirty).toBe(true);
    expect(f.session.getState().opened?.session.editor_key).toBe("editor-a");
  });
  it("an explicit reopen cannot discard an uncertain save request", async () => {
    const f = fixture();
    f.api.save = async () => { throw new NetworkTransportError(new Error("offline")); };
    await f.start(); f.edit(); await f.session.save();
    await f.session.start();
    expect(f.session.getState().uncertain).toBe(true);
    expect(f.session.getState().dirty).toBe(true);
  });
  it("a newer head does not replace the document being read or downloaded", async () => {
    const f = fixture(); await f.start();
    f.publish(false); await f.session.refresh();
    expect(f.session.getState().current?.version_id).toBe("version-a");
    expect(f.session.getState().opened?.session.version_id).toBe("version-a");
  });
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
    expect(await f.session.close()).toBe(false);
  });
  it("leaving a clean editor releases the server session, while dirty or uncertain edits retain it", async () => {
    for (const mode of ["clean", "dirty", "uncertain"]) {
      const f = fixture(); let closes = 0;
      f.api.close = async () => { closes++; };
      await f.start();
      if (mode !== "clean") f.edit();
      if (mode === "uncertain") {
        f.api.save = async () => { throw new Error("offline"); };
        await f.session.save();
      }
      f.session.dispose();
      expect(closes).toBe(mode === "clean" ? 1 : 0);
    }
  });
  it("only a published version after the correlated command confirms save", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.unsafe()).toBe(true);
    f.publish(); await f.session.refresh();
    expect(docxStatus(f.session.getState())).toBe("已保存");
    expect(await f.session.close()).toBe(true);
  });
  it("an acknowledged pending save can be explicitly retried with the same request", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    await f.session.refresh();
    expect(f.calls.length).toBe(1);
    await f.session.save();
    expect(f.calls[0]).toEqual(f.calls[1]);
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.getState().dirty).toBe(true);
    f.publish(); await f.session.refresh();
    expect(f.session.getState().confirmed).toBe(true);
  });
  it("later edits remain dirty when an earlier forcesave arrives", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    f.edit(); f.publish(); await f.session.refresh();
    expect(f.session.getState().dirty).toBe(true);
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.getState().saved?.version_id).toBe("version-2");
  });
  it("a failed callback retains dirty edits and never confirms persistence", async () => {
    const f = fixture(); await f.start(); f.edit(); await f.session.save();
    f.head().editor.pending_save_id = null;
    f.head().editor.save_error = { kind: "callback", code: 7 };
    await f.session.refresh();
    expect(f.session.getState().dirty).toBe(true);
    expect(f.session.getState().confirmed).toBe(false);
    expect(f.session.getState().saved).toBe(null);
    expect(f.session.getState().saving).toBe(false);
    expect(f.session.unsafe()).toBe(true);
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
    await f.start(); f.edit(); await f.session.save();
    await f.session.refresh(); await f.session.refresh();
    expect(f.calls.length).toBe(1);
    expect(f.session.getState().uncertain).toBe(true);
    f.edit(); await f.session.save();
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
    const closed: string[] = [];
    f.api.close = async (_workspace, key) => { closed.push(key); };
    let finish!: (value: Awaited<ReturnType<DocxApi["open"]>>) => void;
    f.api.open = () => new Promise(resolve => { finish = resolve; });
    const old = f.session.start(); await Promise.resolve(); await Promise.resolve();
    f.session.dispose(); f.api.open = actual; await f.start();
    finish({ config: {}, api_script_url: "https://example.invalid/old.js",
      session: { editor_key: "old-editor", round_id: "round-a", version_id: "old-version" } });
    await old;
    expect(f.session.getState().opened?.session.editor_key).toBe("editor-a");
    expect(f.session.getState().phase).toBe("ready");
    expect(closed).toEqual(["old-editor"]);
  });
});
