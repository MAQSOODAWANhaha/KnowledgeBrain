import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../api";
import { Crumbs } from "../Crumbs";
import { guardHashNavigation, parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { ProjectNav } from "./BidTree";
import { type AuthoringStep } from "./authoring/routes";
import { DocxEditor, type DocxEditorHandle } from "./authoring/DocxEditor";
import { OutlinePlanner } from "./authoring/OutlinePlanner";
import type { WordNavigationRequest, WordNavigationResult } from "./authoring/wordNavigation";
import { TENDER_INPUT_ACCEPT } from "./authoring/media";
import type { DocxCurrent, DocxIdentity } from "./api/docx";
import { downloadTenderFile, executionError, tenderApi, tenderDocxApi, type DocumentStart, type TenderChapter, type TenderDocxCurrent, type TenderDraft, type WorkbenchView } from "./api/workbench";
import { AnalysisPane, ChapterInspector, ChapterTree, flattenChapters } from "./WorkbenchPanels";
import { AnalysisReview } from "./AnalysisReview";
import { AnalysisRecovery } from "./AnalysisRecovery";
import { ChapterRevision } from "./authoring/ChapterRevision";
import { SavedWordReview } from "./authoring/SavedWordReview";
import { KnowledgeSelection, type KnowledgeSelection as KnowledgeChoice } from "./KnowledgeSelection";
import { inProgress, executionStatus } from "./executionStatus";
import { TaskProgress } from "./TaskProgress";
import "./workbench.css";

type PendingAction = { kind: "analysis" | "authoring"; attempt: MutationAttempt; body?: DocumentStart | Record<string, never> };
type OpenRequest = { id: string; mode: "view" | "edit"; expected?: DocxIdentity };

export function Workbench({ email }: { email: string }) {
  const route = parseBidRoute(useHash());
  return route && <ProjectWorkbench key={route.projectId} email={email} projectId={route.projectId} step={route.step} />;
}

function ProjectWorkbench({ email, projectId, step }: { email: string; projectId: string; step: AuthoringStep }) {
  const [view, setView] = useState<WorkbenchView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [retry, setRetry] = useState(0);
  const [wordUnsafe, setWordUnsafe] = useState(false);
  const [outlineUnsafe, setOutlineUnsafe] = useState(false);
  const [analysisUnsafe, setAnalysisUnsafe] = useState(false);
  const [recoveryUnsafe, setRecoveryUnsafe] = useState(false);
  const [revisionUnsafe, setRevisionUnsafe] = useState(false);
  const [savedReviewUnsafe, setSavedReviewUnsafe] = useState(false);
  const unsafe = wordUnsafe || outlineUnsafe || analysisUnsafe || recoveryUnsafe || revisionUnsafe || savedReviewUnsafe;
  const knowledgeKey = `kb.tender-knowledge:${projectId}`;
  const [knowledgeSelections, setKnowledgeSelections] = useState<KnowledgeChoice[]>(() => {
    try { const value: unknown = JSON.parse(sessionStorage.getItem(knowledgeKey) ?? "[]"); return Array.isArray(value) ? value.filter(item => item && typeof item.product_id === "string" && typeof item.version_id === "string") : []; }
    catch { return []; }
  });
  const changeKnowledge = (value: KnowledgeChoice[]) => {
    try { sessionStorage.setItem(knowledgeKey, JSON.stringify(value)); setKnowledgeSelections(value); }
    catch { setNotice("无法保留参考资料选择，请检查浏览器存储后重试。"); }
  };
  const [authoringMode, setAuthoringMode] = useState<"outline" | "body" | null>(null);
  const [bodyOpened, setBodyOpened] = useState(false);
  const [pinned, setPinned] = useState<TenderDocxCurrent | null>(null);
  const editorRef = useRef<DocxEditorHandle | null>(null);
  const [navigationRequest, setNavigationRequest] = useState<WordNavigationRequest>();
  const [navigationNotice, setNavigationNotice] = useState<string | null>(null);
  const [openRequest, setOpenRequest] = useState<OpenRequest | null>(null);
  useEffect(() => { if (step === "authoring" && authoringMode === "body" && openRequest) setBodyOpened(true); }, [step, authoringMode, openRequest]);
  const revisionDetails = useRef<HTMLDetailsElement | null>(null);
  const savedReviewDetails = useRef<HTMLDetailsElement | null>(null);
  const footRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const foot = footRef.current;
    if (!foot) return;
    const closeOthers = (current: HTMLDetailsElement) => {
      foot.querySelectorAll("details").forEach(item => { if (item !== current) item.open = false; });
    };
    const onToggle = (event: Event) => {
      const details = event.target;
      if (details instanceof HTMLDetailsElement && details.open) closeOthers(details);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const open = foot.querySelector("details[open]");
      if (!(open instanceof HTMLDetailsElement)) return;
      open.open = false;
      open.querySelector("summary")?.focus({ preventScroll: true });
    };
    foot.addEventListener("toggle", onToggle);
    foot.addEventListener("keydown", onKey);
    return () => { foot.removeEventListener("toggle", onToggle); foot.removeEventListener("keydown", onKey); };
  }, [view]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (step !== "authoring" || authoringMode !== "body") return;
      if (event.key !== "[" && event.key !== "]") return;
      const target = event.target;
      if (target instanceof HTMLElement && target.closest("input, textarea, select, [contenteditable=true]")) return;
      event.preventDefault();
      if (event.key === "[") setLeftClosed(value => !value);
      else setRightClosed(value => !value);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step, authoringMode]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [leftClosed, setLeftClosed] = useState(() => window.matchMedia("(max-width: 700px)").matches);
  const [rightClosed, setRightClosed] = useState(() => window.matchMedia("(max-width: 1200px)").matches);
  useEffect(() => {
    const narrow = window.matchMedia("(max-width: 700px)");
    const closeDirectory = () => { if (narrow.matches) setLeftClosed(true); };
    narrow.addEventListener("change", closeDirectory);
    return () => narrow.removeEventListener("change", closeDirectory);
  }, []);
  const [dismissedDraft, setDismissedDraft] = useState<string | null>(null);
  const [uploading, setUploading] = useState<string[]>([]);
  const executionAttempts = useRef(new Map<string, MutationAttempt>());
  const actionKey = `kb.tender-action:${projectId}`;
  const [pending, setPending] = useState<PendingAction | null>(() => {
    try {
      const item = JSON.parse(sessionStorage.getItem(actionKey) ?? "null") as PendingAction | null;
      return item && ["analysis", "authoring"].includes(item.kind) && typeof item.attempt?.idempotencyKey === "string" ? item : null;
    } catch { return null; }
  });
  const mounted = useRef(true);
  const readRequest = useRef(0);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const reload = useCallback(async (signal?: AbortSignal) => {
    const request = ++readRequest.current;
    try {
      const result = await tenderApi.workbench(projectId, signal);
      if (signal?.aborted || !mounted.current || request !== readRequest.current) return;
      setView(result); setError(null);
      setAuthoringMode(current => current ?? (result.current_docx ? "body" : "outline"));
      if (result.current_docx) {
        const draft = result.current_docx;
        setOpenRequest(current => current ?? { id: draft.id, mode: "view", expected: { version_id: draft.id, docx_sha256: draft.sha256 } });
      }
      setSelectedId(current => current ?? flattenChapters(result.current_docx?.outline?.chapters ?? result.outline?.chapters ?? [])[0]?.chapter_id ?? null);
    } catch (ex) {
      if (!signal?.aborted && mounted.current && request === readRequest.current) setError(ex instanceof Error ? ex.message : "工作台读取失败，请重试。");
    }
  }, [projectId]);
  useEffect(() => {
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const read = async () => { await reload(abort.signal); if (!abort.signal.aborted) timer = setTimeout(() => void read(), 5000); };
    void read();
    return () => { abort.abort(); clearTimeout(timer); };
  }, [reload, retry]);
  useEffect(() => {
    if (!outlineUnsafe && !analysisUnsafe && !recoveryUnsafe && !revisionUnsafe && !savedReviewUnsafe) return;
    const originalHash = window.location.hash;
    const warn = () => setNotice(analysisUnsafe || recoveryUnsafe ? "请先提交或放弃分析修改，再离开项目。" : revisionUnsafe || savedReviewUnsafe ? "请先提交或放弃复核修改，再离开项目。" : "请先保存并确认大纲调整，再离开项目。");
    const unload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    const click = (event: MouseEvent) => {
      const anchor = event.target instanceof Element ? event.target.closest("a[href]") : null;
      if (anchor && parseBidRoute(new URL(anchor.getAttribute("href")!, window.location.href).hash.replace(/^#/, ""))?.projectId !== projectId) {
        event.preventDefault(); event.stopPropagation(); warn();
      }
    };
    const unguard = guardHashNavigation(() => {
      if (parseBidRoute(window.location.hash.replace(/^#/, ""))?.projectId === projectId) return true;
      history.replaceState(null, "", `${window.location.pathname}${window.location.search}${originalHash}`); warn(); return false;
    });
    window.addEventListener("beforeunload", unload); document.addEventListener("click", click, true);
    return () => { window.removeEventListener("beforeunload", unload); document.removeEventListener("click", click, true); unguard(); };
  }, [outlineUnsafe, analysisUnsafe, recoveryUnsafe, revisionUnsafe, savedReviewUnsafe, projectId, step]);
  const onCurrentChange = useCallback((current: DocxCurrent | null) => setPinned(current as TenderDocxCurrent | null), []);
  const onNavigationResult = useCallback((result: WordNavigationResult) => {
    setNavigationNotice(result.ok ? "已定位 Word 中的书签位置。" : result.reason ?? "位置需要重新关联，请在 Word 中核对。");
  }, []);
  const onSaved = useCallback((current: DocxCurrent) => {
    setOpenRequest(request => request && { ...request, expected: { version_id: current.version_id, docx_sha256: current.docx_sha256 } });
    void reload();
  }, [reload]);
  async function start(kind: PendingAction["kind"], body: DocumentStart | Record<string, never> = {}) {
    if (busy || view?.project.status === "ended" && !pending || pending && pending.kind !== kind) return false;
    const requestBody = kind === "authoring" && knowledgeSelections.length ? { ...body, knowledge_selections: knowledgeSelections } : body;
    const action = pending ?? { kind, attempt: createMutationAttempt(), body: requestBody };
    try { sessionStorage.setItem(actionKey, JSON.stringify(action)); }
    catch { setNotice("无法保存任务恢复信息，本次尚未发送。请检查浏览器存储后重试。"); return false; }
    readRequest.current++; setPending(action); setBusy(true); setNotice(null);
    try {
      await tenderApi.start(projectId, kind, action.attempt, action.body ?? {});
      sessionStorage.removeItem(actionKey); setPending(null); await reload();
      return true;
    } catch (ex) {
      if (ex instanceof ApiError && ex.status >= 400 && ex.status < 500 && ex.status !== 408 && ex.status !== 429) {
        sessionStorage.removeItem(actionKey); setPending(null); setNotice(ex.message);
      } else setNotice("任务提交结果尚未确认。请确认同一次请求，避免重复创建付费任务。");
      return false;
    } finally { setBusy(false); }
  }
  async function upload(files: File[]) {
    if (!files.length || busy || view?.project.status === "ended") return;
    readRequest.current++; setBusy(true); setNotice(null); setUploading(files.map(file => file.name));
    try {
      for (const file of files) {
        await tenderApi.upload(projectId, file, createMutationAttempt());
        setUploading(current => current.filter(name => name !== file.name));
      }
    } catch (ex) { setNotice(`上传未全部确认，请核对文件列表。${ex instanceof Error ? ex.message : ""}`); }
    finally { setBusy(false); setUploading([]); await reload(); }
  }
  async function selectDraft(draft: TenderDraft) {
    if (busy || view?.project.status === "ended") return;
    if (unsafe) { setNotice("请先保存并确认当前修改，再查看另一份稿件。"); return; }
    readRequest.current++; setBusy(true); setNotice(null);
    try {
      if (editorRef.current && !await editorRef.current.close()) {
        setNotice("当前文档会话尚未安全关闭，请确认保存状态后重试。");
        return;
      }
      const identity = { version_id: draft.id, docx_sha256: draft.sha256 };
      await tenderApi.selectDraft(projectId, identity, createMutationAttempt());
      setOpenRequest({ id: createMutationAttempt().idempotencyKey, mode: "view", expected: identity });
      setDismissedDraft(null); await reload();
    } catch (ex) { setNotice(ex instanceof ApiError && ex.status === 409 ? "稿件尚未切换，请回到标书编制确认保存并关闭 Word 会话后重试。" : ex instanceof Error ? ex.message : "稿件切换失败，当前稿件保持不变。"); }
    finally { setBusy(false); }
  }
  async function download(url: string, name: string) {
    try { await downloadTenderFile(url, name); }
    catch (ex) { setNotice(ex instanceof Error ? ex.message : "下载失败，请重试。"); }
  }
  async function executionAction(execution: string, action: "resume" | "cancel") {
    if (busy || action === "resume" && view?.project.status === "ended") return;
    const key = `${execution}:${action}`;
    const attempt = executionAttempts.current.get(key) ?? createMutationAttempt();
    executionAttempts.current.set(key, attempt);
    readRequest.current++; setBusy(true); setNotice(null);
    try {
      await tenderApi.execution(projectId, execution, action, attempt);
      executionAttempts.current.delete(key); await reload();
    } catch (ex) { setNotice(ex instanceof Error ? ex.message : "操作结果尚未确认，请重试同一操作。"); }
    finally { setBusy(false); }
  }
  const displayedDraft = pinned ?? view?.current_docx;
  const chapters = displayedDraft ? displayedDraft.outline?.chapters ?? [] : view?.outline?.chapters ?? [];
  const selected = flattenChapters(chapters).find(chapter => chapter.chapter_id === selectedId) ?? chapters[0] ?? null;
  const draftId = pinned?.version_id ?? openRequest?.expected?.version_id;
  const savedReviewVersion = displayedDraft ? { version_id: "version_id" in displayedDraft ? displayedDraft.version_id : displayedDraft.id,
    docx_sha256: "docx_sha256" in displayedDraft ? displayedDraft.docx_sha256 : displayedDraft.sha256,
    revision: displayedDraft.revision } : null;
  const displayedSource = displayedDraft?.source ?? view?.versions.find(version => version.id === savedReviewVersion?.version_id)?.source;
  const contextMatches = !displayedDraft || !!displayedDraft.execution_id && displayedDraft.execution_id === view?.context_execution_id;
  const reviewGeneratedVersion = view?.versions.find(version => version.source === "generated" && version.execution_id === view.context_execution_id);
  const reviewSourceLabel = reviewGeneratedVersion ? `生成稿 v${reviewGeneratedVersion.revision}` : "当前稿对应的生成稿";
  const candidate = view?.versions.find(version => ["generated", "recovered"].includes(version.source) && version.id !== draftId && version.id !== dismissedDraft && version.revision > (pinned?.revision ?? view.current_docx?.revision ?? 0));
  const packageVersion = view?.versions.find(version => version.source === "generated" && version.execution_id === view.artifacts?.execution_id);
  const ended = view?.project.status === "ended";
  const sectionLabel = step === "files" ? "项目资料" : step === "analysis" ? "招标解读" : step === "authoring" ? "标书编制" : "交付文件";
  const locate = (bookmark?: string | null) => {
    if (!draftId) return;
    if (!bookmark) { setNavigationNotice("位置需要重新关联：该位置没有可用的稳定书签，请使用 Word 自带导航核对。"); return; }
    setNavigationNotice("正在定位 Word 中的书签位置…");
    setNavigationRequest({ id: createMutationAttempt().idempotencyKey, versionId: draftId, bookmark });
  };
  const pickChapter = (chapter: TenderChapter) => { setSelectedId(chapter.chapter_id); locate(chapter.bookmark); };
  const showReview = (details: HTMLDetailsElement | null) => {
    if (!details) return;
    footRef.current?.querySelectorAll("details").forEach(item => { if (item !== details) item.open = false; });
    details.open = true;
    details.querySelector("summary")?.focus({ preventScroll: true });
  };
  const moveAuthoringMode = (key: string) => {
    if (key !== "ArrowLeft" && key !== "ArrowRight") return false;
    const order = ["outline", "body"] as const;
    const index = Math.max(0, order.indexOf(authoringMode === "body" ? "body" : "outline"));
    const mode = order[(index + (key === "ArrowRight" ? 1 : -1) + order.length) % order.length];
    setAuthoringMode(mode);
    document.getElementById(mode === "outline" ? "outline-planner-tab" : "word-editor-tab")?.focus();
    return true;
  };
  const reviewShortcuts = savedReviewVersion && <div className="tender-review-shortcuts" role="group" aria-label="稿件复核入口"><div><button onClick={() => showReview(savedReviewDetails.current)}>打开保存稿复核</button><p>当前保存稿：Word v{savedReviewVersion.revision}，读取已保存正文，包含人工修改；进入后确认复核版本。</p></div><div><button disabled={!contextMatches || !view?.context_execution_id || !view?.outline} onClick={() => showReview(revisionDetails.current)}>基于生成稿修订</button><p>生成稿修订基准：{contextMatches && view?.context_execution_id && view?.outline ? reviewSourceLabel : "暂无可用生成基准"}，新稿生成后由你选择载入。</p></div></div>;
  const pageActions = view && <div className="tender-page-actions">
    {step === "analysis" && <button disabled={busy || ended && pending?.kind !== "analysis" || inProgress(view.analysis?.status) && pending?.kind !== "analysis" || !!pending && pending.kind !== "analysis"} onClick={() => void start("analysis")}>{pending?.kind === "analysis" ? "确认同一次分析请求" : "分析招标文件"}</button>}
    {step === "authoring" && <button className="tender-primary" onClick={() => { setAuthoringMode("outline"); window.location.hash = `/bids/${projectId}/authoring`; }}>编排大纲</button>}
  </div>;
  const authoringTabs = <div className="tender-authoring-tabs" role="tablist" aria-label="标书编制视图" onKeyDown={event => { if (moveAuthoringMode(event.key)) event.preventDefault(); }}><button role="tab" tabIndex={authoringMode === "body" ? -1 : 0} aria-selected={authoringMode === "outline"} aria-controls="outline-planner-panel" id="outline-planner-tab" onClick={() => setAuthoringMode("outline")}>大纲编排</button><button role="tab" tabIndex={authoringMode === "body" ? 0 : -1} aria-selected={authoringMode === "body"} aria-controls="word-editor-panel" id="word-editor-tab" onClick={() => setAuthoringMode("body")}>正文编辑</button></div>;
  return <Shell root="bids" layout="workspace" email={email} onBeforeLeave={() => !unsafe}
    crumbs={<Crumbs items={[{ label: "投标项目", href: "/" }, { label: view?.project.title ?? "投标项目" }, { label: sectionLabel }]} />}
    title={sectionLabel} extra={pageActions} tree={<ProjectNav projectId={projectId} currentStep={step} title={view?.project.title ?? "投标项目"} />}>
    <div className={`tender-workspace${step === "authoring" ? " is-authoring" : ""}`}>
      {error && <div className="tender-alert" role="alert">{view ? "暂时无法更新工作台，已读取内容保持可用。" : "工作台读取失败："}{error}<button onClick={() => setRetry(value => value + 1)}>重试</button></div>}
      {notice && <div className="tender-alert" role="alert">{notice}<button aria-label="关闭提示" onClick={() => setNotice(null)}>×</button></div>}
      {!view ? !error && <p className="tender-loading" role="status">正在读取项目、稿件和任务…</p> : <>
        {pending && <p className="tender-note" role="status">存在尚未确认的提交，请确认同一次请求。刷新页面不会自动创建新任务。</p>}
        {candidate && <div className="tender-new-draft" role="status"><div><strong>{candidate.source === "recovered" ? "发现会话恢复稿" : "新稿"} v{candidate.revision} {candidate.source === "recovered" ? "待核对" : "已生成"}</strong><p>当前稿件及人工修改保留，查看新稿前请确认当前修改已经保存。</p></div><button disabled={busy || unsafe || ended} onClick={() => void selectDraft(candidate)}>查看新稿</button><button onClick={() => setDismissedDraft(candidate.id)}>继续当前稿</button></div>}
        {step === "files" && <section className="tender-section" aria-label="项目资料"><p className="tender-note">上传项目文件。分析会自行读取结构和内容，新资料用于后续编制快照，已有稿件保留。</p>
          {!ended && <div className="tender-upload"><label className="btn ghost">选择项目文件<input type="file" hidden multiple accept={TENDER_INPUT_ACCEPT} disabled={busy} onChange={event => { void upload(Array.from(event.target.files ?? [])); event.target.value = ""; }} /></label></div>}
          {uploading.map(name => <p key={name} role="status">正在上传：{name}</p>)}
          <KnowledgeSelection value={knowledgeSelections} onChange={changeKnowledge} disabled={!!ended || busy || !!pending} />
          {!view.documents.length ? <p className="tender-empty">尚未上传项目文件。</p> : <div className="tender-table-scroll"><table className="tender-table"><thead><tr><th>文件</th><th>大小</th><th>处理状态</th><th>原件</th></tr></thead><tbody>{view.documents.map(document => <tr key={document.id}><td>{document.file_name}</td><td>{Math.ceil(document.byte_length / 1024)} KB</td><td>{document.parse_status ?? "已上传，执行时读取"}</td><td>{document.download_url && <button onClick={() => void download(document.download_url!, document.file_name)}>下载原件</button>}</td></tr>)}</tbody></table></div>}
        </section>}
        {step === "authoring" && authoringTabs}
        <div className={`tender-step${view.analysis?.items?.length ? "" : " is-empty"}`} style={{ display: step === "analysis" ? undefined : "none" }}>
          <AnalysisPane analysis={view.analysis} />
          <aside className="tender-analysis-side" aria-label="分析处理">
            <AnalysisReview projectId={projectId} disabled={!!ended} onChanged={reload} onUnsafeChange={setAnalysisUnsafe} />
            <AnalysisRecovery projectId={projectId} readOnly={!!ended} disabled={busy || !!pending} onChanged={reload} onUnsafeChange={setRecoveryUnsafe} />
          </aside>
        </div>
        {step === "authoring" && authoringMode === "outline" && !bodyOpened && view.current_docx && <div className="tender-first-word" role="status"><div><strong>正文已生成</strong><p>可查看已保存的 Word v{view.current_docx.revision}，当前大纲调整继续保留。</p></div><button onClick={() => setAuthoringMode("body")}>正文已生成，查看Word</button></div>}
        {step === "authoring" && view.authoring && <TaskProgress label={view.authoring.operation === "outline" ? "大纲任务" : "正文任务"} status={view.authoring.status} progress={view.authoring.progress} />}
        <div id="outline-planner-panel" role="tabpanel" aria-labelledby="outline-planner-tab" style={{ display: step === "authoring" && authoringMode === "outline" ? undefined : "none" }} className="outline-planner-panel"><OutlinePlanner projectId={projectId} disabled={!!ended} bodyBusy={busy || !!pending && (pending.kind !== "authoring" || !pending.body || !("outline_revision_id" in pending.body)) || inProgress(view.authoring?.status) && pending?.kind !== "authoring"} bodyPending={pending?.kind === "authoring" && !!pending.body && "outline_revision_id" in pending.body} onGenerateBody={body => void start("authoring", body)} onUnsafeChange={setOutlineUnsafe} onExecutionAction={executionAction} /></div>
        <div id="word-editor-panel" role="tabpanel" aria-labelledby="word-editor-tab" style={{ display: step === "authoring" && authoringMode === "body" ? undefined : "none" }} className={`tender-authoring${leftClosed ? " left-closed" : ""}${rightClosed ? " right-closed" : ""}`}>
          <aside className="tender-outline" aria-label="文件与章节目录"><div className="tender-pane-heading"><strong>文件结构</strong><button aria-expanded={!leftClosed} aria-label={leftClosed ? "展开目录" : "收起目录"} onClick={() => setLeftClosed(value => !value)}>{leftClosed ? "›" : "‹"}</button></div>{!leftClosed && <><p className="tender-outline-version">{displayedDraft ? `已保存 Word v${displayedDraft.revision} 的目录` : "编制大纲"}</p><ChapterTree chapters={chapters} selectedId={selected?.chapter_id ?? null} gaps={contextMatches ? view.gaps : []} onSelect={pickChapter} onLocate={locate} />{!!displayedDraft?.outline?.missing_chapter_ids?.length && <p className="tender-context-warning">{displayedDraft.outline.missing_chapter_ids.length} 个原章节未能绑定已保存文件，相关要求与待补记录仍保留。</p>}</>}</aside>
          <main className="tender-word" aria-label="Word 稿件工作区">{rightClosed && reviewShortcuts}{navigationNotice && <p className="tender-navigation-status" role="status">{navigationNotice}</p>}{openRequest && bodyOpened ? <DocxEditor workspaceId={projectId} readOnly={!!ended} ownerIdentity={email} api={tenderDocxApi} controlRef={editorRef} openRequest={openRequest} navigationRequest={navigationRequest} onNavigationResult={onNavigationResult} onUnsafeChange={setWordUnsafe} onCurrentChange={onCurrentChange} onSaved={onSaved} /> : <section className="tender-before-word"><h2>投标稿尚未形成</h2><p>请在大纲编排中生成并保存结构，再按已保存修订生成正文。</p>{inProgress(view.authoring?.status) && <p role="status">编制任务正在执行，当前尚无可打开的 Word 文件。</p>}{executionError(view.authoring?.error) && <p role="alert">{executionError(view.authoring?.error)}</p>}</section>}</main>
          <aside className="tender-inspector" aria-label="要求、依据与待补"><div className="tender-pane-heading"><strong>章节核对</strong><button aria-expanded={!rightClosed} aria-label={rightClosed ? "展开核对栏" : "收起核对栏"} onClick={() => setRightClosed(value => !value)}>{rightClosed ? "‹" : "›"}</button></div>{!rightClosed && <>{reviewShortcuts}<ChapterInspector chapter={selected} view={view} draftId={draftId} contextMatches={contextMatches} onLocate={locate} onDownload={download} /></>}</aside>
        </div>
        {step === "export" && <section className="tender-section"><p className="tender-note">下载明确版本的已保存稿件。资料待补及复核问题随稿保留，文件生成不代表内容全部满足要求。</p>
          {!view.versions.length ? <p className="tender-empty">尚无已保存稿件。</p> : <div className="tender-table-scroll"><table className="tender-table"><thead><tr><th>版本</th><th>来源</th><th>状态</th><th>文件</th></tr></thead><tbody>{view.versions.map(version => <tr key={version.id}><td>v{version.revision}</td><td>{version.source === "manual" ? "人工保存稿" : version.source === "recovered" ? "会话恢复稿（待核对）" : "编制生成稿"}</td><td>{version.id === view.current_docx?.id ? "当前稿件" : "已保留"}</td><td><button onClick={() => void download(version.download_url, `投标文件-v${version.revision}.docx`)}>下载 DOCX v{version.revision}</button>{version.id !== view.current_docx?.id && <button disabled={busy || unsafe || ended} onClick={() => void selectDraft(version)}>选为当前稿 v{version.revision}</button>}</td></tr>)}</tbody></table></div>}
          {view.artifacts?.package_url && <><button onClick={() => void download(view.artifacts!.package_url!, "投标文件及附件.zip")}>下载编制交付包（包含待补清单）</button><p className="tender-note">{packageVersion ? `交付包来源：生成稿 v${packageVersion.revision}。` : "交付包来源：最近一次成功编制。"}</p><p className="tender-note">交付包对应原编制产物，不包含后续人工修改；人工保存稿请按上方版本下载。</p></>}{view.gaps.length > 0 && <div className="tender-gap-list"><h3>编制时的待补记录</h3>{view.gaps.map((gap, index) => <p key={`${gap.code}:${index}`}>{gap.detail}</p>)}</div>}
        </section>}
        <div className="tender-foot" ref={footRef}><div style={{ display: step === "authoring" && authoringMode === "body" ? "contents" : "none" }}><details className="tender-task-details" ref={revisionDetails}><summary>按章节定向复核与修订</summary><ChapterRevision
          source={contextMatches && view.context_execution_id && view.outline ? { execution_id: view.context_execution_id, label: reviewSourceLabel, chapters: view.outline.chapters } : null}
          disabled={!!ended && !(pending?.kind === "authoring" && !!pending.body?.review_request) || wordUnsafe || !!pending && !(pending.kind === "authoring" && !!pending.body?.review_request) || inProgress(view.authoring?.status) && !pending}
          busy={busy} pending={pending?.kind === "authoring" && !!pending.body?.review_request}
          pendingRequest={pending?.kind === "authoring" && pending.body?.previous_execution_id && pending.body.review_request ? { previous_execution_id: pending.body.previous_execution_id, review_request: pending.body.review_request } : null}
          manualWordRevision={displayedSource === "manual" || displayedSource === "recovered" ? displayedDraft?.revision : null}
          onSubmit={body => start("authoring", body)} onUnsafeChange={setRevisionUnsafe} /></details>
          <details className="tender-task-details" ref={savedReviewDetails}><summary>核对已保存的 Word 内容</summary><SavedWordReview projectId={projectId}
            version={savedReviewVersion} readOnly={!!ended} disabled={wordUnsafe || busy} onUnsafeChange={setSavedReviewUnsafe} /></details></div>
        <details className="tender-task-details"><summary>执行详情</summary><p>分析：{executionStatus(view.analysis?.status)}；编制：{executionStatus(view.authoring?.status)}</p>{view.authoring?.usage && <p>已发送 {view.authoring.usage.sent_requests} 次，重试 {view.authoring.usage.retry_requests} 次，Token {view.authoring.usage.total_tokens}；未知用量 {view.authoring.usage.unknown_usage_requests} 次。</p>}{executionError(view.authoring?.error) && <p>{executionError(view.authoring?.error)}</p>}{view.authoring && (inProgress(view.authoring.status) ? <button disabled={busy} onClick={() => void executionAction(view.authoring!.execution_id, "cancel")}>取消本次编制</button> : ["failed", "blocked"].includes(view.authoring.status) && <button disabled={busy || ended} onClick={() => void executionAction(view.authoring!.execution_id, "resume")}>继续本次编制</button>)}</details>
        <details className="tender-task-details tender-legacy-generation"><summary>直接生成整稿</summary><p>此入口重新生成大纲和正文。需要沿用人工编排时，请在大纲编排中按已保存修订生成正文。</p><button disabled={busy || ended && pending?.kind !== "authoring" || inProgress(view.authoring?.status) && pending?.kind !== "authoring" || !!pending && (pending.kind !== "authoring" || !!pending.body && ("outline_revision_id" in pending.body || "review_request" in pending.body))} onClick={() => void start("authoring")}>{pending?.kind === "authoring" && !(pending.body && ("outline_revision_id" in pending.body || "review_request" in pending.body)) ? "确认同一次编制请求" : view.current_docx ? "生成新稿" : "生成投标稿"}</button></details>
        </div>
      </>}
    </div>
  </Shell>;
}
