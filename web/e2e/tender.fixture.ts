import { expect, type Page } from "@playwright/test";
import type { DocxCurrent } from "../src/bid/api/docx";
import type { OutlineNode, OutlineSave, OutlineView, TenderChapter, TenderDocxCurrent, TenderDraft, WorkbenchView } from "../src/bid/api/workbench";

const project = "11111111-1111-4111-8111-111111111111";
const path = `/api/v3/tender-projects/${project}`;
const sha = "a".repeat(64);
type WordEvents = { onDocumentReady(): void; onDocumentStateChange(event: { data: boolean }): void };
type WordTestWindow = Window & { wordTest: { modes: string[]; destroyed: number; events: WordEvents | null; navigated: string[] } };
const chapters = [{ chapter_id: "chapter-1", title: "实施方案", bookmark: `kb_c_${"1".repeat(28)}`,
  children: [], targets: [{ target_id: "target-1", kind: "narrative", purpose: "说明实施方法", rule_refs: [{ item_ref: { item_id: "rule-1" } }] }] },
  { chapter_id: "chapter-2", title: "项目人员", bookmark: `kb_c_${"2".repeat(28)}`, children: [], targets: [] }];

function metadata(id = "word-v1", revision = 1): TenderDocxCurrent {
  return { project_id: project, workspace_id: project, version_id: id, docx_sha256: sha,
    execution_id: `execution-${id}`, outline: { chapters },
    round_id: `round-${id}`, revision, editor: { key: null, base_version_id: null, pending_save_id: null, save_error: null } };
}
function draft(id = "word-v1", revision = 1): TenderDraft {
  return { id, sha256: sha, revision, source: "generated", execution_id: `execution-${id}`,
    outline: { chapters },
    download_url: `${path}/docx/versions/${id}/download` };
}

async function fixture(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("kb.token", "tender-browser-fixture");
    const target = window as unknown as WordTestWindow;
    target.wordTest = { modes: [], destroyed: 0, events: null, navigated: [] };
    window.addEventListener("message", event => {
      if (event.origin === location.origin && event.data?.type === "result" && event.data?.channel === "kb.word-navigation.v1") target.wordTest.navigated.push(event.data.id);
    });
    Object.assign(window, { DocsAPI: { DocEditor: class {
      private frame: HTMLIFrameElement;
      constructor(element: string, config: { editorConfig: { mode: string }; events: WordEvents }) {
        target.wordTest.modes.push(config.editorConfig.mode);
        target.wordTest.events = config.events;
        this.frame = document.createElement("iframe");
        this.frame.name = "frameEditor";
        this.frame.title = "ONLYOFFICE isolated test document";
        this.frame.src = "/__office/frame";
        const placeholder = document.getElementById(element);
        if (placeholder) placeholder.parentNode?.replaceChild(this.frame, placeholder);
        queueMicrotask(() => config.events.onDocumentReady());
      }
      destroyEditor() { target.wordTest.destroyed++; this.frame.remove(); }
    } } });
  });
  await page.route("**/__office/api.js", route => route.fulfill({ contentType: "text/javascript", body: "/* mock document service; real React editor lifecycle */" }));
  await page.route("**/__office/frame", route => route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><body><h1>合成 Word 测试稿</h1><p>正文只有这一份 DOCX。</p><script>
    const channel = "kb.word-navigation.v1"; let binding;
    const send = data => parent.postMessage({ channel, ...binding, ...data }, location.origin);
    addEventListener("message", event => {
      if (event.origin !== location.origin || event.source !== parent || event.data?.channel !== channel) return;
      if (event.data.type === "bind") { binding = event.data; send({type:"ready"}); }
      if (event.data.type === "unbind") { binding = undefined; send({type:"hello"}); }
      if (event.data.type === "navigate" && binding) send({type:"result",id:event.data.id,ok:true});
    }); send({type:"hello"});
  </script></body></html>` }));
  const view: WorkbenchView = {
    project: { id: project, title: "编制工作台专项", status: "open", ended_at: null, workspace_id: project },
    documents: [{ id: "tender", file_name: "合成招标.pdf", media_type: "application/pdf", byte_length: 4096 }],
    analysis: { run_id: "analysis", status: "waiting-confirmation", confirmed: false,
      source_version: 1, domain_revision: 0, issues: [], items: [{ item_id: "rule-1", kind: "requirement", text: "应提交实施安排。" }] },
    authoring: { execution_id: "execution-word-v1", status: "succeeded" },
    context_execution_id: "execution-word-v1", outline: { version_id: "word-v1", chapters },
    current_docx: draft(), versions: [draft()], gaps: [{ code: "missing_material", detail: "项目人员资料待补。", chapter_id: "chapter-2" }],
  };
  const state = { current: metadata() as DocxCurrent | null, reads: 0, failRead: false, loseStart: false, failClose: false,
    failOutlineSave: false, loseOutlineSave: false, loseOutlineStart: false,
    outlineSaves: [] as { body: OutlineSave; key: string | undefined }[],
    calls: [] as string[], starts: [] as { body: unknown; key: string | undefined; kind: string }[],
    executions: [] as { action: string; key: string | undefined }[], uploads: [] as string[],
    opens: [] as { version_id: string; mode: string }[],
    saves: [] as { body: unknown; key: string | undefined }[],
    downloads: [] as string[], saved: null as object | null };
  let opened = 0;
  const outline: OutlineView = { draft: { revision_id: "outline-r1", revision: 1, parent_id: null,
    execution_id: "outline-execution", sha256: sha, outline: { chapters }, sources_current: true }, candidate: null, generation: null };
  const outlineReceipts = new Map<string, NonNullable<OutlineView["draft"]>>();
  const outlineHistory = new Map([[outline.draft!.revision_id, structuredClone(outline.draft!)]]);
  await page.route("**/api/**", async route => {
    const request = route.request(); const url = new URL(request.url()); const method = request.method();
    const json = (body: unknown, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (url.pathname === "/api/v1/me") return json({ id: "owner", email: "browser@local" });
    if (url.pathname === "/api/v3/tender-projects") return json(method === "POST" ? view.project : [view.project]);
    if (url.pathname === path) return json(view.project);
    if (url.pathname === `${path}/outline` && method === "GET") return json(outline);
    if (url.pathname === `${path}/outline/revisions`) return json({ revisions: [...outlineHistory.values()].reverse().map(draft => ({
      revision_id: draft.revision_id, revision: draft.revision, parent_id: draft.parent_id, created_at: "2026-10-28T00:00:00Z",
    })) });
    if (url.pathname.startsWith(`${path}/outline/revisions/`)) {
      const draft = outlineHistory.get(url.pathname.split("/").at(-1)!);
      const targets = new Map<string, TenderChapter["targets"][number]>();
      const collect = (nodes: TenderChapter[]) => { for (const chapter of nodes) { for (const target of chapter.targets) targets.set(target.target_id, target); collect(chapter.children); } };
      for (const value of outlineHistory.values()) collect(value.outline.chapters);
      return draft ? json({ draft, available_targets: [...targets.values()] }) : json({ error: { message: "历史修订不存在" } }, 404);
    }
    if (url.pathname === `${path}/outline/generate`) {
      state.starts.push({ body: request.postDataJSON(), key: request.headers()["idempotency-key"], kind: "outline" });
      outline.generation = { execution_id: "outline-generation", status: "pending", operation: "outline" };
      if (state.loseOutlineStart) { state.loseOutlineStart = false; return route.abort("failed"); }
      return json({ generation: outline.generation }, 202);
    }
    if (url.pathname === `${path}/outline` && method === "PUT") {
      const body = request.postDataJSON() as OutlineSave;
      const key = request.headers()["idempotency-key"];
      state.outlineSaves.push({ body, key });
      if (state.failOutlineSave) return json({ error: { code: "OUTLINE_REVISION_CONFLICT", message: "其他页面已保存大纲，请核对当前编排。" } }, 409);
      if (key && outlineReceipts.has(key)) return json({ draft: outlineReceipts.get(key) });
      const revision = (outline.draft?.revision ?? 0) + 1;
      const old = new Map<string, TenderChapter>();
      const collect = (nodes: TenderChapter[]) => { for (const node of nodes) { old.set(node.chapter_id, node); collect(node.children); } };
      collect((body.source_revision_id ? outlineHistory.get(body.source_revision_id)?.outline.chapters : null) ?? outline.draft?.outline.chapters ?? outline.candidate?.outline.chapters ?? []);
      const targets = new Map<string, TenderChapter["targets"][number]>();
      const collectTargets = (nodes: TenderChapter[]) => { for (const chapter of nodes) { for (const target of chapter.targets) targets.set(target.target_id, target); collectTargets(chapter.children); } };
      for (const value of outlineHistory.values()) collectTargets(value.outline.chapters);
      const restore = (nodes: OutlineNode[]): TenderChapter[] => nodes.map(node => {
        const { writing_goal, target_ids, ...arrangement } = node;
        const assigned = target_ids ? target_ids.map(id => targets.get(id)!).filter(Boolean) : old.get(node.chapter_id)?.targets ?? [];
        return { ...old.get(node.chapter_id), ...arrangement, is_volume: node.is_volume ?? false, presentation_role: node.presentation_role ?? "body",
          targets: writing_goal !== undefined ? writing_goal ? [{ target_id: `manual-response-${node.chapter_id}`, kind: "narrative", purpose: writing_goal }] : [] : assigned, children: restore(node.children) };
      });
      outline.draft = { revision_id: `outline-r${revision}`, revision, parent_id: body.expected_revision_id,
        execution_id: "outline-execution", sha256: sha, sources_current: true,
        outline: { chapters: restore(body.chapters) } };
      outlineHistory.set(outline.draft.revision_id, structuredClone(outline.draft));
      if (key) outlineReceipts.set(key, outline.draft);
      if (state.loseOutlineSave) { state.loseOutlineSave = false; return route.abort("failed"); }
      return json({ draft: outline.draft });
    }
    if (url.pathname === `${path}/workbench`) {
      state.reads++;
      return state.failRead ? json({ error: { code: "OFFLINE", message: "项目读取暂时不可用" } }, 503) : json(view);
    }
    if (url.pathname === `${path}/authoring` || url.pathname === `${path}/analysis`) {
      state.starts.push({ body: request.postDataJSON(), key: request.headers()["idempotency-key"], kind: url.pathname.split("/").at(-1)! });
      if (state.loseStart) { state.loseStart = false; return route.abort("failed"); }
      return json({ execution_id: "new-execution", status: "pending" }, 202);
    }
    if (url.pathname === `${path}/documents` && method === "POST") {
      state.uploads.push(request.postData() ?? "");
      const document = { id: "company-proof", file_name: "合成企业证明.txt", media_type: "text/plain", byte_length: 30 };
      view.documents.push(document);
      return json(document, 201);
    }
    if (url.pathname.includes("/executions/") && /\/(resume|cancel)$/.test(url.pathname)) {
      state.executions.push({ action: url.pathname.split("/").at(-1)!, key: request.headers()["idempotency-key"] });
      return json({ execution_id: view.authoring?.execution_id, status: "pending" }, 202);
    }
    if (url.pathname === `${path}/docx/current` && method === "GET") return json(state.current);
    if (url.pathname === `${path}/docx/current` && method === "POST") {
      state.calls.push("select");
      const input = request.postDataJSON() as { version_id: string };
      if (!state.calls.some(call => call === "close")) return json({ error: { code: "ACTIVE_EDITOR", message: "请先关闭当前会话" } }, 409);
      state.current = metadata(input.version_id, 2); view.current_docx = draft(input.version_id, 2);
      return json(state.current);
    }
    if (url.pathname === `${path}/docx/editor`) {
      state.calls.push("open"); opened++;
      const input = request.postDataJSON() as { version_id: string; mode: string };
      state.opens.push(input);
      const current = state.current?.version_id === input.version_id ? state.current : metadata(input.version_id, 2);
      if (input.mode === "edit" && state.current) state.current.editor.key = `editor-${opened}`;
      return json({ api_script_url: `${url.origin}/__office/api.js`,
        config: { document: { permissions: { edit: input.mode === "edit" } }, editorConfig: { mode: input.mode } },
        session: { editor_key: `editor-${opened}`, round_id: current.round_id, version_id: current.version_id } });
    }
    if (url.pathname.endsWith("/close")) {
      state.calls.push("close");
      if (state.failClose) return json({ error: { code: "CLOSE_FAILED", message: "会话关闭失败" } }, 503);
      if (state.current) state.current.editor.key = null;
      return json({ closed: true });
    }
    if (url.pathname === `${path}/docx/editor/save`) {
      state.calls.push("save");
      state.saves.push({ body: request.postDataJSON(), key: request.headers()["idempotency-key"] });
      if (state.current) state.current.editor.pending_save_id = "save-1";
      return json({ save_id: "save-1", pending: true, dispatch: true });
    }
    if (url.pathname.includes("/saves/")) return json(state.saved);
    if (url.pathname.endsWith("/download") || url.pathname.endsWith("/package")) {
      expect(request.headers()["authorization"]).toBe("Bearer tender-browser-fixture");
      state.downloads.push(url.pathname);
      return route.fulfill({ contentType: "application/octet-stream", body: "isolated immutable Word artifact" });
    }
    if (url.pathname.includes("/docx/versions/")) return json(metadata(url.pathname.split("/").at(-1), 2));
    return json({ error: { code: "UNEXPECTED_ROUTE", message: url.pathname } }, 404);
  });
  return { view, state, outline, outlineHistory,
    candidate() { view.versions = [draft("word-v2", 2), draft()]; },
    publish() {
      const parent = state.current!;
      state.current = { ...metadata("word-saved-v2", 2), round_id: parent.round_id,
        editor: { ...parent.editor, pending_save_id: null } };
      state.saved = { save_id: "save-1", editor_key: parent.editor.key, round_id: parent.round_id,
        parent_version_id: parent.version_id, version_id: state.current.version_id, docx_sha256: sha, revision: 2 };
      view.current_docx = { ...draft("word-saved-v2", 2), source: "manual", execution_id: "execution-word-v1" };
      view.versions = [view.current_docx, draft()];
    },
    async ready() { await page.goto(`/#/bids/${project}/authoring`); await expect(page.getByTestId("docx-status")).toContainText("当前保存版本已载入"); },
    async edit() {
      await page.getByTestId("docx-mode").click();
      await expect.poll(() => page.evaluate(() => (window as unknown as WordTestWindow).wordTest.modes)).toEqual(["view", "edit"]);
      await expect(page.getByTestId("docx-status")).toHaveText("当前保存版本已载入");
    },
    async dirty() { await page.evaluate(() => { const event = (window as unknown as WordTestWindow).wordTest.events!; event.onDocumentStateChange({ data: true }); event.onDocumentStateChange({ data: false }); }); },
  };
}

export { project, path, metadata, draft, fixture, type WordTestWindow };
