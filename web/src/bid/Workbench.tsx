import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../api";
import { PageHeader } from "../shell/PageHeader";
import { projectPanel } from "../shell/ProjectNav";
import { guardHashNavigation, parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { authoringHref, type AuthoringStep } from "./authoring/routes";
import { DocxEditor, type DocxEditorHandle } from "./authoring/DocxEditor";
import { OutlinePlanner } from "./authoring/OutlinePlanner";
import type { WordNavigationRequest, WordNavigationResult } from "./authoring/wordNavigation";
import { TENDER_INPUT_ACCEPT } from "./authoring/media";
import type { DocxCurrent, DocxIdentity } from "./api/docx";
import { downloadTenderFile, downloadTenderPdf, executionError, tenderApi, tenderDocxApi, type DocumentStart, type TenderChapter, type TenderDocxCurrent, type TenderDraft, type WorkbenchView } from "./api/workbench";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Center } from "@astryxdesign/core/Center";
import { Drawer, DrawerHeader } from "@astryxdesign/core/Drawer";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Layout, LayoutContent, LayoutPanel } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { MoreMenu } from "@astryxdesign/core/MoreMenu";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Tab, TabList } from "@astryxdesign/core/TabList";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { PanelLeftClose, PanelLeftOpen, PanelRightClose, PanelRightOpen } from "lucide-react";
import { FileInput } from "@astryxdesign/core/FileInput";
import { Table } from "@astryxdesign/core/Table";
import { GuideScreen } from "../guide/GuideScreen";
import { AnalysisPane, ChapterInspector, ChapterTree, flattenChapters } from "./WorkbenchPanels";
import { AnalysisReview } from "./AnalysisReview";
import { AnalysisRecovery } from "./AnalysisRecovery";
import { ChapterRevision } from "./authoring/ChapterRevision";
import { SavedWordReview } from "./authoring/SavedWordReview";
import { KnowledgeSelection, type KnowledgeSelection as KnowledgeChoice } from "./KnowledgeSelection";
import { inProgress, executionStatus } from "./executionStatus";
import { TaskProgress } from "./TaskProgress";
import "./workbench.css";

function ProjectFiles({ documents, download }: { documents: WorkbenchView["documents"]; download(url: string, name: string): Promise<void> }) {
  return <Table data={documents} columns={[
    { key: "file_name", header: "文件" },
    { key: "byte_length", header: "大小", renderCell: (document) => `${Math.ceil(document.byte_length / 1024)} KB` },
    { key: "parse_status", header: "处理状态", renderCell: (document) => <Badge variant={document.parse_status ? "neutral" : "info"} label={document.parse_status ?? "已上传，执行时读取"} /> },
    { key: "id", header: "原件", renderCell: (document) => document.download_url ? <Button size="sm" label="下载原件" onClick={() => void download(document.download_url!, document.file_name)} /> : "—" },
  ]} />;
}

function SavedPdfDownload({ projectId, version }: { projectId: string; version: TenderDraft }) {
  const [error, setError] = useState<string | null>(null);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), []);
  async function download() {
    if (request.current) return;
    const controller = new AbortController();
    request.current = controller;
    setError(null);
    try { await downloadTenderPdf(projectId, version, controller.signal); }
    catch (ex) {
      if (!controller.signal.aborted) setError(ex instanceof Error ? ex.message : "PDF 下载失败，请重试。");
    } finally {
      controller.abort();
      request.current = null;
    }
  }
  return <>
    <Button size="sm" label={`下载 PDF v${version.revision}`} clickAction={download} />
    {error && <Text role="alert" color="secondary">{error}</Text>}
  </>;
}

function SavedVersions({ projectId, versions, currentId, busy, unsafe, ended, download, select }: {
  projectId: string; versions: WorkbenchView["versions"]; currentId?: string; busy: boolean; unsafe: boolean; ended: boolean;
  download(url: string, name: string): Promise<void>; select(version: WorkbenchView["versions"][number]): void;
}) {
  return <Table data={versions} idKey="id" columns={[
    { key: "revision", header: "版本", renderCell: (version) => `v${version.revision}` },
    { key: "source", header: "来源", renderCell: (version) => version.source === "manual" ? "人工保存稿" : version.source === "recovered" ? "会话恢复稿（待核对）" : "编制生成稿" },
    { key: "id", header: "状态", renderCell: (version) => <Badge variant={version.id === currentId ? "success" : "neutral"} label={version.id === currentId ? "当前稿件" : "已保留"} /> },
    { key: "download_url", header: "文件", renderCell: (version) => <HStack gap={2} wrap="wrap" vAlign="center">
      <Button size="sm" label={`下载 DOCX v${version.revision}`} onClick={() => void download(version.download_url, `投标文件-v${version.revision}.docx`)} />
      <SavedPdfDownload projectId={projectId} version={version} />
      {version.id !== currentId && <Button size="sm" label={`选为当前稿 v${version.revision}`} isDisabled={busy || unsafe || ended} onClick={() => select(version)} />}
    </HStack> },
  ]} />;
}

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
  const [authoringMode, setAuthoringMode] = useState<"guide" | "outline" | "body" | null>(null);
  const [bodyOpened, setBodyOpened] = useState(false);
  const [pinned, setPinned] = useState<TenderDocxCurrent | null>(null);
  const editorRef = useRef<DocxEditorHandle | null>(null);
  const [navigationRequest, setNavigationRequest] = useState<WordNavigationRequest>();
  const [navigationNotice, setNavigationNotice] = useState<string | null>(null);
  const [openRequest, setOpenRequest] = useState<OpenRequest | null>(null);
  useEffect(() => { if (step === "authoring" && authoringMode === "body" && openRequest) setBodyOpened(true); }, [step, authoringMode, openRequest]);
  const [tool, setTool] = useState<"revision" | "savedReview" | "execution" | "regenerate" | null>(null);
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
  // Below 900px the analysis side panel stacks under the item list.
  const [stacked, setStacked] = useState(() => window.matchMedia("(max-width: 899px)").matches);
  useEffect(() => {
    const query = window.matchMedia("(max-width: 899px)");
    const update = () => setStacked(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
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
  const coverageIdentity = view?.coverage?.applies_to;
  const coverageVersion = coverageIdentity && view?.versions.find(version => version.source === "generated"
    && version.id === coverageIdentity.version_id && version.sha256 === coverageIdentity.docx_sha256
    && version.execution_id === coverageIdentity.execution_id);
  const coverageMatches = !!coverageVersion && displayedSource === "generated"
    && savedReviewVersion?.version_id === coverageIdentity?.version_id
    && savedReviewVersion?.docx_sha256 === coverageIdentity?.docx_sha256
    && displayedDraft?.execution_id === coverageIdentity?.execution_id;
  const ended = view?.project.status === "ended";
  const sectionLabel = step === "files" ? "项目资料" : step === "analysis" ? "招标解读" : step === "authoring" ? "标书编制" : "交付文件";
  const locate = (bookmark?: string | null) => {
    if (!draftId) return;
    if (!bookmark) { setNavigationNotice("位置需要重新关联：该位置没有可用的稳定书签，请使用 Word 自带导航核对。"); return; }
    setNavigationNotice("正在定位 Word 中的书签位置…");
    setNavigationRequest({ id: createMutationAttempt().idempotencyKey, versionId: draftId, bookmark });
  };
  const pickChapter = (chapter: TenderChapter) => { setSelectedId(chapter.chapter_id); locate(chapter.bookmark); };
  const openReviewTool = (name: "revision" | "savedReview") => { setRightClosed(false); setTool(name); };
  const revisionReady = contextMatches && !!view?.context_execution_id && !!view?.outline;
  const reviewShortcuts = savedReviewVersion && <VStack gap={3} role="group" aria-label="稿件复核入口" padding={3}>
    <VStack gap={1}><Button label="打开保存稿复核" onClick={() => openReviewTool("savedReview")} /><Text type="supporting">当前保存稿：Word v{savedReviewVersion.revision}，读取已保存正文，包含人工修改；进入后确认复核版本。</Text></VStack>
    <VStack gap={1}><Button label="基于生成稿修订" isDisabled={!revisionReady} onClick={() => openReviewTool("revision")} /><Text type="supporting">生成稿修订基准：{revisionReady ? reviewSourceLabel : "暂无可用生成基准"}，新稿生成后由你选择载入。</Text></VStack>
  </VStack>;
  const bodyMode = step === "authoring" && authoringMode === "body";
  const toolItems = [
    ...(bodyMode ? [
      { label: "按章节定向复核与修订", onClick: () => openReviewTool("revision") },
      { label: "核对已保存的 Word 内容", onClick: () => openReviewTool("savedReview") },
      { type: "divider" as const },
    ] : []),
    { label: "执行详情", onClick: () => setTool("execution") },
    { label: "直接生成整稿", onClick: () => setTool("regenerate") },
  ];
  const pageActions = view && <HStack gap={2} vAlign="center">
    <MoreMenu label="更多操作" variant="secondary" alignment="end" items={toolItems} />
    {step === "analysis" && <Button variant="primary" label={pending?.kind === "analysis" ? "确认同一次分析请求" : "分析招标文件"} isDisabled={busy || ended && pending?.kind !== "analysis" || inProgress(view.analysis?.status) && pending?.kind !== "analysis" || !!pending && pending.kind !== "analysis"} onClick={() => void start("analysis")} />}
    {step === "authoring" && <Button variant="primary" label="编排大纲" onClick={() => { setAuthoringMode("outline"); window.location.hash = `/bids/${projectId}/authoring`; }} />}
  </HStack>;
  const authoringTabs = step === "authoring" ? <TabList role="tablist" value={authoringMode === "body" || authoringMode === "guide" ? authoringMode : "outline"} onChange={value => setAuthoringMode(value as "guide" | "outline" | "body")}>
    <Tab value="guide" label="材料引导" panelId="guide-panel" />
    <Tab value="outline" label="大纲编排" panelId="outline-planner-panel" />
    <Tab value="body" label="正文编辑" panelId="word-editor-panel" />
  </TabList> : undefined;
  const closeTool = (open: boolean) => { if (!open) setTool(null); };
  const projectTitle = view?.project.title ?? "投标项目";
  return <Shell root="bids" email={email} onBeforeLeave={() => !unsafe}
    header={<PageHeader crumbs={[{ label: "投标项目", href: "/" }, { label: projectTitle, href: authoringHref(projectId, "files") }, { label: sectionLabel }]}
      title={sectionLabel} actions={pageActions} tabs={authoringTabs} />}
    panel={projectPanel({ projectId, step, view })}>
    <VStack gap={0} height="100%" minHeight={0}>
      {(error || notice || !view || pending || candidate || step === "authoring" && (authoringMode === "outline" && !bodyOpened && view?.current_docx || view?.authoring)) && <VStack gap={2} paddingInline={6} paddingBlockStart={4}>
        {error && <Banner role="alert" status="error" title={`${view ? "暂时无法更新工作台，已读取内容保持可用。" : "工作台读取失败："}${error}`} endContent={<Button size="sm" label="重试" onClick={() => setRetry(value => value + 1)} />} />}
        {notice && <Banner role="alert" status="error" title={notice} endContent={<Button size="sm" label="关闭提示" onClick={() => setNotice(null)} />} />}
        {!view && !error && <Center minHeight={240}><Spinner label="正在读取项目、稿件和任务…" /></Center>}
        {view && pending && <Banner role="status" status="warning" title="存在尚未确认的提交，请确认同一次请求。刷新页面不会自动创建新任务。" />}
        {view && candidate && <Banner role="status" status="info" title={`${candidate.source === "recovered" ? "发现会话恢复稿" : "新稿"} v${candidate.revision} ${candidate.source === "recovered" ? "待核对" : "已生成"}`} description="当前稿件及人工修改保留，查看新稿前请确认当前修改已经保存。"
          endContent={<HStack gap={2}><Button label="查看新稿" variant="primary" isDisabled={busy || unsafe || ended} onClick={() => void selectDraft(candidate)} /><Button label="继续当前稿" variant="secondary" onClick={() => setDismissedDraft(candidate.id)} /></HStack>} />}
        {view && step === "authoring" && authoringMode === "outline" && !bodyOpened && view.current_docx && <Banner role="status" status="success" title="正文已生成" description={`可查看已保存的 Word v${view.current_docx.revision}，当前大纲调整继续保留。`} endContent={<Button label="正文已生成，查看Word" onClick={() => setAuthoringMode("body")} />} />}
        {view && step === "authoring" && view.authoring && <TaskProgress label={view.authoring.operation === "outline" ? "大纲任务" : "正文任务"} status={view.authoring.status} progress={view.authoring.progress} />}
      </VStack>}
      {view && <>
        {step === "files" && <VStack gap={4} padding={6} isScrollable height="100%" role="region" aria-label="项目资料">
          {!ended && <Card><VStack gap={3}>
            <Heading level={2}>上传项目文件</Heading>
            <Text type="supporting">招标文件、附件、补遗和澄清都放在这里。分析会自行读取结构和内容，新资料用于后续编制快照，已有稿件保留。</Text>
            <FileInput label="选择项目文件" mode="dropzone" isMultiple accept={TENDER_INPUT_ACCEPT} value={null} isDisabled={busy} isLoading={uploading.length > 0} onChange={(files) => { const list = Array.isArray(files) ? files : files ? [files] : []; if (list.length) void upload(list); }} />
            {uploading.map(name => <Text key={name} role="status">正在上传：{name}</Text>)}
          </VStack></Card>}
          <Card><KnowledgeSelection value={knowledgeSelections} onChange={changeKnowledge} disabled={!!ended || busy || !!pending} /></Card>
          <Card padding={0}>{!view.documents.length ? <EmptyState title="尚未上传项目文件。" description="上传招标文件后即可分析招标事项。" /> : <ProjectFiles documents={view.documents} download={download} />}</Card>
        </VStack>}
        <Panel isShown={step === "analysis"} fill>
          {stacked
            ? <VStack isScrollable height="100%" gap={4}><AnalysisPane analysis={view.analysis} /><VStack padding={4} role="complementary" aria-label="分析处理"><VStack gap={4}>
              <AnalysisReview projectId={projectId} disabled={!!ended} onChanged={reload} onUnsafeChange={setAnalysisUnsafe} />
              <AnalysisRecovery projectId={projectId} readOnly={!!ended} disabled={busy || !!pending} onChanged={reload} onUnsafeChange={setRecoveryUnsafe} />
            </VStack></VStack></VStack>
            : <Layout height="fill"
              content={<LayoutContent padding={0}><AnalysisPane analysis={view.analysis} /></LayoutContent>}
              end={<LayoutPanel width={380} hasDivider label="分析处理" role="complementary"><VStack gap={4}>
              <AnalysisReview projectId={projectId} disabled={!!ended} onChanged={reload} onUnsafeChange={setAnalysisUnsafe} />
              <AnalysisRecovery projectId={projectId} readOnly={!!ended} disabled={busy || !!pending} onChanged={reload} onUnsafeChange={setRecoveryUnsafe} />
            </VStack></LayoutPanel>} />}
        </Panel>
        <Panel id="guide-panel" role="tabpanel" aria-label="材料引导" isShown={step === "authoring" && authoringMode === "guide"} fill isScrollable><GuideScreen /></Panel>
        <Panel id="outline-planner-panel" role="tabpanel" aria-label="大纲编排" isShown={step === "authoring" && authoringMode === "outline"} fill><OutlinePlanner projectId={projectId} disabled={!!ended} bodyBusy={busy || !!pending && (pending.kind !== "authoring" || !pending.body || !("outline_revision_id" in pending.body)) || inProgress(view.authoring?.status) && pending?.kind !== "authoring"} bodyPending={pending?.kind === "authoring" && !!pending.body && "outline_revision_id" in pending.body} onGenerateBody={body => void start("authoring", body)} onUnsafeChange={setOutlineUnsafe} onExecutionAction={executionAction} /></Panel>
        <Panel id="word-editor-panel" role="tabpanel" aria-label="正文编辑" isShown={bodyMode} fill>
          <Layout height="fill"
            start={leftClosed
              ? <LayoutPanel width={48} hasDivider label="文件与章节目录" role="complementary" padding={1}><IconButton variant="ghost" label="展开目录" icon={<PanelLeftOpen size={18} />} aria-expanded={false} onClick={() => setLeftClosed(false)} /></LayoutPanel>
              : <LayoutPanel width={264} hasDivider label="文件与章节目录" role="complementary" padding={0}>
                <VStack gap={2} padding={3}>
                  <HStack hAlign="between" vAlign="center"><Text weight="semibold">文件结构</Text><IconButton variant="ghost" size="sm" label="收起目录" icon={<PanelLeftClose size={16} />} aria-expanded onClick={() => setLeftClosed(true)} /></HStack>
                  <Text type="supporting">{displayedDraft ? `已保存 Word v${displayedDraft.revision} 的目录` : "编制大纲"}</Text>
                  <ChapterTree chapters={chapters} selectedId={selected?.chapter_id ?? null} gaps={contextMatches ? view.gaps : []} onSelect={pickChapter} onLocate={locate} />
                  {!!displayedDraft?.outline?.missing_chapter_ids?.length && <Banner status="warning" title={`${displayedDraft.outline.missing_chapter_ids.length} 个原章节未能绑定已保存文件，相关要求与待补记录仍保留。`} />}
                </VStack>
              </LayoutPanel>}
            content={<VStack role="region" aria-label="Word 稿件工作区" height="100%" minHeight={0} gap={2} padding={3} isScrollable>
              {rightClosed && reviewShortcuts}
              {navigationNotice && <Text role="status" type="supporting">{navigationNotice}</Text>}
              {openRequest && bodyOpened
                ? <DocxEditor workspaceId={projectId} readOnly={!!ended} ownerIdentity={email} api={tenderDocxApi} controlRef={editorRef} openRequest={openRequest} navigationRequest={navigationRequest} onNavigationResult={onNavigationResult} onUnsafeChange={setWordUnsafe} onCurrentChange={onCurrentChange} onSaved={onSaved} />
                : <Card><EmptyState headingLevel={2} title="投标稿尚未形成" description="请在大纲编排中生成并保存结构，再按已保存修订生成正文。"
                  actions={<>{inProgress(view.authoring?.status) && <Text role="status">编制任务正在执行，当前尚无可打开的 Word 文件。</Text>}{executionError(view.authoring?.error) && <Text role="alert">{executionError(view.authoring?.error)}</Text>}</>} /></Card>}
            </VStack>}
            end={<LayoutPanel width={rightClosed ? 48 : 360} hasDivider label="要求、依据与待补" role="complementary" padding={rightClosed ? 1 : 0}>
                {rightClosed && <IconButton variant="ghost" label="展开核对栏" icon={<PanelRightOpen size={18} />} aria-expanded={false} onClick={() => setRightClosed(false)} />}
                <Panel isShown={!rightClosed}>
                  <HStack hAlign="between" vAlign="center" padding={3}><Text weight="semibold">章节核对</Text><IconButton variant="ghost" size="sm" label="收起核对栏" icon={<PanelRightClose size={16} />} aria-expanded onClick={() => setRightClosed(true)} /></HStack>
                  {reviewShortcuts}
                  <Panel isShown={tool === "revision"} padding={3} gap={2}><HStack hAlign="between" vAlign="center"><Heading level={2}>按章节定向复核与修订</Heading><Button size="sm" variant="ghost" label="返回章节核对" onClick={() => setTool(null)} /></HStack>
                    <ChapterRevision
            source={revisionReady ? { execution_id: view.context_execution_id!, label: reviewSourceLabel, chapters: view.outline!.chapters } : null}
            disabled={!!ended && !(pending?.kind === "authoring" && !!pending.body?.review_request) || wordUnsafe || !!pending && !(pending.kind === "authoring" && !!pending.body?.review_request) || inProgress(view.authoring?.status) && !pending}
            busy={busy} pending={pending?.kind === "authoring" && !!pending.body?.review_request}
            pendingRequest={pending?.kind === "authoring" && pending.body?.previous_execution_id && pending.body.review_request ? { previous_execution_id: pending.body.previous_execution_id, review_request: pending.body.review_request } : null}
            manualWordRevision={displayedSource === "manual" || displayedSource === "recovered" ? displayedDraft?.revision : null}
            onSubmit={body => start("authoring", body)} onUnsafeChange={setRevisionUnsafe} />
                  </Panel>
                  <Panel isShown={tool === "savedReview"} padding={3} gap={2}><HStack hAlign="between" vAlign="center"><Heading level={2}>核对已保存的 Word 内容</Heading><Button size="sm" variant="ghost" label="返回章节核对" onClick={() => setTool(null)} /></HStack>
                    <SavedWordReview projectId={projectId} version={savedReviewVersion} readOnly={!!ended} disabled={wordUnsafe || busy} onUnsafeChange={setSavedReviewUnsafe} />
                  </Panel>
                  <Panel isShown={tool !== "revision" && tool !== "savedReview"}><ChapterInspector chapter={selected} view={view} draftId={draftId} contextMatches={contextMatches} onLocate={locate} onDownload={download} /></Panel>
                </Panel>
              </LayoutPanel>} />
        </Panel>
        {step === "export" && <VStack gap={4} padding={6} isScrollable height="100%" role="region" aria-label="交付文件">
          <Card><VStack gap={2}>
            <Heading level={2}>已保存稿件</Heading>
            <Text type="supporting">下载明确版本的已保存稿件。资料待补及复核问题随稿保留，文件生成不代表内容全部满足要求。</Text>
            {displayedDraft && <Text data-testid="word-coverage">当前查看稿 v{displayedDraft.revision}：{coverageMatches ? view.coverage?.headline : "覆盖待核"}</Text>}
            {coverageVersion && view.coverage?.headline && <Text data-testid="generated-coverage">生成稿 v{coverageVersion.revision}的覆盖统计：{view.coverage.headline}</Text>}
          </VStack></Card>
          <Card padding={0}>{!view.versions.length ? <EmptyState title="尚无已保存稿件。" description="在标书编制中生成正文后，已保存版本会列在这里。" /> : <SavedVersions projectId={projectId} versions={view.versions} currentId={view.current_docx?.id} busy={busy} unsafe={unsafe} ended={!!ended} download={download} select={version => void selectDraft(version)} />}</Card>
          {view.artifacts?.package_url && <Card><VStack gap={2}>
            <Heading level={2}>编制交付包</Heading>
            <Text type="supporting">{packageVersion ? `交付包来源：生成稿 v${packageVersion.revision}。` : "交付包来源：最近一次成功编制。"}</Text>
            <Text type="supporting">交付包对应原编制产物，不包含后续人工修改；人工保存稿请按上方版本下载。</Text>
            <HStack><Button label="下载编制交付包（包含待补清单）" onClick={() => void download(view.artifacts!.package_url!, "投标文件及附件.zip")} /></HStack>
          </VStack></Card>}
          {view.gaps.length > 0 && <Card><VStack gap={2}><Heading level={2}>编制时的待补记录</Heading>{view.gaps.map((gap, index) => <Text key={`${gap.code}:${index}`}>{gap.detail}</Text>)}</VStack></Card>}
        </VStack>}
        <Drawer isOpen={tool === "execution"} onOpenChange={closeTool} label="执行详情" width={440}>
          <DrawerHeader title="执行详情" onOpenChange={closeTool} />
          <VStack gap={3}>
            <MetadataList><MetadataListItem label="分析">{executionStatus(view.analysis?.status)}</MetadataListItem><MetadataListItem label="编制">{executionStatus(view.authoring?.status)}</MetadataListItem></MetadataList>
            {view.authoring?.usage && <Text>已发送 {view.authoring.usage.sent_requests} 次，重试 {view.authoring.usage.retry_requests} 次，Token {view.authoring.usage.total_tokens}；未知用量 {view.authoring.usage.unknown_usage_requests} 次。</Text>}
            {executionError(view.authoring?.error) && <Banner status="error" title={executionError(view.authoring?.error)} />}
            {view.authoring && (inProgress(view.authoring.status)
              ? <HStack><Button label="取消本次编制" isDisabled={busy} onClick={() => void executionAction(view.authoring!.execution_id, "cancel")} /></HStack>
              : ["failed", "blocked"].includes(view.authoring.status) && <HStack><Button label="继续本次编制" isDisabled={busy || ended} onClick={() => void executionAction(view.authoring!.execution_id, "resume")} /></HStack>)}
          </VStack>
        </Drawer>
        <Drawer isOpen={tool === "regenerate"} onOpenChange={closeTool} label="直接生成整稿" width={440}>
          <DrawerHeader title="直接生成整稿" onOpenChange={closeTool} />
          <VStack gap={3}>
            <Text>此入口重新生成大纲和正文。需要沿用人工编排时，请在大纲编排中按已保存修订生成正文。</Text>
            <HStack><Button variant="primary" label={pending?.kind === "authoring" && !(pending.body && ("outline_revision_id" in pending.body || "review_request" in pending.body)) ? "确认同一次编制请求" : view.current_docx ? "生成新稿" : "生成投标稿"} isDisabled={busy || ended && pending?.kind !== "authoring" || inProgress(view.authoring?.status) && pending?.kind !== "authoring" || !!pending && (pending.kind !== "authoring" || !!pending.body && ("outline_revision_id" in pending.body || "review_request" in pending.body))} onClick={() => void start("authoring")} /></HStack>
          </VStack>
        </Drawer>
      </>}
    </VStack>
  </Shell>;
}

// Keeps an inactive view mounted (editor sessions, unsent input) while taking it out of layout and the accessibility tree.
function Panel({ isShown, fill, isScrollable, padding, gap = 0, children, ...rest }: {
  isShown: boolean; fill?: boolean; isScrollable?: boolean; padding?: 3; gap?: 0 | 2; children: ReactNode;
  id?: string; role?: string; "aria-label"?: string;
}) {
  return <VStack {...rest} gap={gap} padding={padding} isScrollable={isScrollable} minHeight={fill ? 0 : undefined}
    style={{ display: isShown ? undefined : "none", flex: fill ? 1 : undefined }}>{children}</VStack>;
}
