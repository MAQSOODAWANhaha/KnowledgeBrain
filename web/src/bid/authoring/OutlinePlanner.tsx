import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import { randomUuid } from "../../uuid";
import { executionError, tenderApi, type DocumentStart, type OutlineRevision, type OutlineSave, type OutlineView, type TenderChapter, type TenderIssue } from "../api/workbench";
import { canEditManualGoal, editChapter, findChapter, manualGoal, moveChapter, moveRelative, outlineNodes, removeChapter, type DropPosition } from "./outlineTree";
import { OutlineRecovery } from "./OutlineRecovery";
import { inProgress as running, executionStatus } from "../executionStatus";
import { TaskProgress } from "../TaskProgress";

type Working = { chapters: TenderChapter[]; issues?: TenderIssue[]; expected: string | null; sourceRevision?: string; sourceExecution?: string; saved: string | null };
type SaveFlight = { body: OutlineSave; attempt: MutationAttempt; working: Working };
const signature = (chapters: TenderChapter[]) => JSON.stringify(outlineNodes(chapters));
const fromDraft = (draft: OutlineRevision): Working => ({ chapters: draft.outline.chapters, issues: draft.outline.issues ?? [], expected: draft.revision_id, sourceRevision: draft.revision_id, saved: signature(draft.outline.chapters) });

export function OutlinePlanner({ projectId, disabled, bodyBusy, bodyPending, onGenerateBody, onUnsafeChange, onExecutionAction }: {
  projectId: string; disabled: boolean; bodyBusy: boolean; bodyPending: boolean;
  onGenerateBody(body?: DocumentStart): void; onUnsafeChange(unsafe: boolean): void;
  onExecutionAction(execution: string, action: "resume" | "cancel"): Promise<void>;
}) {
  const storageKey = `kb.outline-save:${projectId}`;
  const [restored] = useState<SaveFlight | null>(() => {
    try {
      const value = JSON.parse(sessionStorage.getItem(storageKey) ?? "null") as SaveFlight | null;
      return value?.attempt?.idempotencyKey && Array.isArray(value.body?.chapters) && Array.isArray(value.working?.chapters) ? value : null;
    } catch { return null; }
  });
  const [working, setWorking] = useState<Working | null>(restored?.working ?? null);
  const initialized = useRef(!!restored);
  const [remote, setRemote] = useState<OutlineView | null>(null);
  const [confirmed, setConfirmed] = useState<OutlineRevision | null>(null);
  const [pending, setPending] = useState<SaveFlight | null>(restored);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [dropPosition, setDropPosition] = useState<DropPosition>("inside");
  const generationKey = `kb.outline-generation:${projectId}`;
  const [generationAttempt, setGenerationAttempt] = useState<MutationAttempt | null>(() => {
    try { const value = JSON.parse(sessionStorage.getItem(generationKey) ?? "null") as MutationAttempt | null; return typeof value?.idempotencyKey === "string" ? value : null; } catch { return null; }
  });
  const [generating, setGenerating] = useState(false);
  const readRequest = useRef(0);
  const read = useCallback(async (signal?: AbortSignal) => {
    const request = ++readRequest.current;
    try {
      const result = await tenderApi.outline(projectId, signal);
      if (signal?.aborted || request !== readRequest.current) return;
      setRemote(result);
      if (!initialized.current) {
        initialized.current = true;
        setWorking(result.draft ? fromDraft(result.draft) : { chapters: [], expected: null, saved: null });
        setConfirmed(result.draft);
      }
    } catch (error) { if (!signal?.aborted && request === readRequest.current) setNotice(error instanceof Error ? error.message : "大纲读取失败，请重试。"); }
  }, [projectId]);
  useEffect(() => {
    const abort = new AbortController(); let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { await read(abort.signal); if (!abort.signal.aborted) timer = setTimeout(() => void poll(), 5000); };
    void poll(); return () => { abort.abort(); clearTimeout(timer); };
  }, [read]);
  const dirty = !!working && (working.saved === null ? !!working.chapters.length || !!working.sourceExecution || !!working.sourceRevision : signature(working.chapters) !== working.saved);
  useEffect(() => { onUnsafeChange(dirty || !!pending || saving); }, [dirty, pending, saving, onUnsafeChange]);
  useEffect(() => {
    if (!pending || !working) return;
    try { sessionStorage.setItem(storageKey, JSON.stringify({ ...pending, working })); }
    catch { setNotice("无法更新浏览器恢复信息，请保持当前页面并确认保存结果。"); }
  }, [pending, working, storageKey]);
  const tree = working?.chapters ?? [];
  const hasSource = !!working?.sourceRevision || !!working?.sourceExecution;
  const editingDisabled = disabled || saving || !hasSource;
  const selected = findChapter(tree, selectedId ?? "") ?? tree[0];
  const change = (chapters: TenderChapter[]) => setWorking(current => current && { ...current, chapters });
  const add = (where: "root" | "sibling" | "child") => {
    const chapter: TenderChapter = { chapter_id: `manual-${randomUuid()}`, title: "新章节", targets: [], children: [], is_volume: false, presentation_role: "body" };
    if (!selected || where === "root") change([...tree, chapter]);
    else if (where === "child") change(editChapter(tree, selected.chapter_id, { children: [...selected.children, chapter] }));
    else change(moveChapter([...tree, chapter], chapter.chapter_id, selected.chapter_id, "after"));
    setSelectedId(chapter.chapter_id);
  };
  async function save() {
    if (!working || saving || disabled && !pending || !hasSource) return;
    const flight = pending ?? { body: { expected_revision_id: working.expected,
      ...(working.sourceRevision ? { source_revision_id: working.sourceRevision } : {}),
      ...(working.sourceExecution ? { source_execution_id: working.sourceExecution } : {}), chapters: outlineNodes(tree) }, attempt: createMutationAttempt(), working };
    try { sessionStorage.setItem(storageKey, JSON.stringify({ ...flight, working })); }
    catch { setNotice("无法保存浏览器恢复信息，大纲尚未发送。请检查浏览器存储后重试。"); return; }
    readRequest.current++; setPending(flight); setSaving(true); setNotice(null); setConflict(false);
    try {
      const draft = await tenderApi.saveOutline(projectId, flight.body, flight.attempt);
      setConfirmed(draft);
      setWorking(current => current && { ...current, expected: draft.revision_id, sourceRevision: draft.revision_id, sourceExecution: undefined,
        issues: draft.outline.issues ?? [],
        saved: signature(draft.outline.chapters),
        chapters: signature(current.chapters) === JSON.stringify(flight.body.chapters) ? draft.outline.chapters : current.chapters });
      sessionStorage.removeItem(storageKey); setPending(null); await read();
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        sessionStorage.removeItem(storageKey); setPending(null); setConflict(error.status === 409);
        setNotice(`大纲保存未完成：${error.message}`);
      } else setNotice("大纲保存结果尚未确认。请确认同一次大纲保存，当前编排已保留。");
    } finally { setSaving(false); }
  }
  async function generate() {
    if (generating || disabled && !generationAttempt) return;
    const attempt = generationAttempt ?? createMutationAttempt();
    try { sessionStorage.setItem(generationKey, JSON.stringify(attempt)); }
    catch { setNotice("无法保存任务恢复信息，本次尚未发送。"); return; }
    readRequest.current++; setGenerationAttempt(attempt); setGenerating(true); setNotice(null);
    try { await tenderApi.generateOutline(projectId, attempt); sessionStorage.removeItem(generationKey); setGenerationAttempt(null); await read(); }
    catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        sessionStorage.removeItem(generationKey); setGenerationAttempt(null); setNotice(error.message);
      } else setNotice("大纲生成请求尚未确认，请确认同一次请求，避免重复创建任务。");
    } finally { setGenerating(false); }
  }
  const adopt = () => {
    if (!remote?.candidate || !working || dirty || pending) return;
    setWorking({ chapters: remote.candidate.outline.chapters, issues: remote.candidate.outline.issues ?? [], expected: remote.draft?.revision_id ?? null, sourceExecution: remote.candidate.execution_id, saved: null });
    setSelectedId(null); setNotice("已载入候选大纲，请核对并保存为草稿修订后生成正文。");
  };
  async function executionAction(action: "resume" | "cancel") {
    if (!remote?.generation) return;
    readRequest.current++;
    await onExecutionAction(remote.generation.execution_id, action);
    await read();
  }
  const renderTree = (nodes: TenderChapter[], prefix = "") => <ol>{nodes.map((chapter, index) => {
    const number = prefix ? `${prefix}.${index + 1}` : `${index + 1}`;
    return <li key={chapter.chapter_id}><button className={selected?.chapter_id === chapter.chapter_id ? "selected" : ""}
      draggable={!disabled && !saving} onDragStart={event => { event.dataTransfer.setData("text/plain", chapter.chapter_id); setDragging(chapter.chapter_id); }} onDragEnd={() => setDragging(null)}
      onDragOver={event => { if (dragging) event.preventDefault(); }} onDrop={event => { event.preventDefault(); change(moveChapter(tree, event.dataTransfer.getData("text/plain"), chapter.chapter_id, dropPosition)); setDragging(null); }}
      onClick={() => setSelectedId(chapter.chapter_id)}><span>{number}</span>{chapter.title || "未命名章节"}<small>{chapter.targets.length ? `${chapter.targets.length} 项响应` : ""}</small></button>{renderTree(chapter.children, number)}</li>;
  })}</ol>;
  const bodyDisabled = disabled || bodyBusy || saving || dirty || !!pending || !confirmed || !confirmed.sources_current || remote?.draft?.sources_current === false;
  return <section className="outline-planner" aria-label="大纲编排工作区">
    <div className="outline-toolbar"><div><strong>大纲编排</strong><p className="tender-note">先调整章节结构，保存独立修订，再按该修订生成正文。</p></div><span data-testid="outline-save-status" role="status">{saving ? "正在保存…" : pending ? "保存结果待确认" : dirty ? "未保存" : confirmed ? `已保存修订 ${confirmed.revision}` : "尚无已保存修订"}</span>
      <button disabled={disabled && !generationAttempt || generating || running(remote?.generation?.status) && !generationAttempt} onClick={() => void generate()}>{generationAttempt ? "确认同一次大纲生成请求" : "生成大纲"}</button>
      <button disabled={disabled && !pending || saving || !hasSource || !dirty && !pending} onClick={() => void save()}>{pending ? "确认同一次大纲保存" : "保存大纲草稿"}</button>
      <button className="tender-primary" disabled={bodyPending ? bodyBusy : bodyDisabled} onClick={() => bodyPending ? onGenerateBody() : confirmed && onGenerateBody({ outline_revision_id: confirmed.revision_id, outline_sha256: confirmed.sha256 })}>{bodyPending ? "确认同一次编制请求" : `按修订 ${confirmed?.revision ?? "—"} 生成正文`}</button>
    </div>
    {remote?.generation && <TaskProgress label="大纲任务" status={remote.generation.status} progress={remote.generation.progress} />}
    {notice && <div className="tender-alert" role="alert">{notice}<button aria-label="关闭大纲提示" onClick={() => setNotice(null)}>×</button></div>}
    {conflict && <div className="outline-notice"><p>当前编排与最新修订可能不同。保留当前编排另存会用当前全部章节替换最新结构；也可载入最新大纲后重新调整。</p><button onClick={async () => { const request = ++readRequest.current; try { const latest = await tenderApi.outline(projectId); if (request !== readRequest.current) return; setRemote(latest); setWorking(current => current && { ...current, expected: latest.draft?.revision_id ?? null }); setConflict(false); setNotice("当前编排已保留并更新保存基准。再次保存会按当前全部章节另存修订。"); } catch (error) { if (request === readRequest.current) setNotice(error instanceof Error ? error.message : "最新修订读取失败，请重试。"); } }}>保留当前编排，按最新修订另存</button></div>}
    {remote?.draft && working && remote.draft.revision_id !== working.expected && !pending && <p className="outline-notice">已有更新的大纲修订 {remote.draft.revision}。<button onClick={() => { if (!dirty || window.confirm("放弃当前未保存的大纲调整，载入最新修订？")) { setWorking(fromDraft(remote.draft!)); setConfirmed(remote.draft); setSelectedId(null); setConflict(false); } }}>载入最新大纲</button></p>}
    {remote?.candidate && remote.candidate.execution_id !== working?.sourceExecution && remote.candidate.execution_id !== confirmed?.execution_id && <div className="outline-notice">新的候选大纲已生成。<button disabled={dirty || !!pending || disabled} onClick={adopt}>载入候选大纲</button>{dirty && <span>请先保存当前调整。</span>}</div>}
    {remote?.generation && <div className="outline-execution" role="status">大纲任务：{executionStatus(remote.generation.status)} {executionError(remote.generation.error)}{running(remote.generation.status) ? <button disabled={bodyBusy} onClick={() => void executionAction("cancel")}>取消大纲生成</button> : ["failed", "blocked"].includes(remote.generation.status) && <button disabled={disabled || bodyBusy} onClick={() => void executionAction("resume")}>继续大纲生成</button>}</div>}
    {(confirmed && !confirmed.sources_current || remote?.draft?.sources_current === false) && <p className="tender-alert" role="alert">项目来源已变化，请重新生成大纲并保存新修订后生成正文。</p>}
    {!hasSource && <p className="outline-notice">请先生成并载入候选大纲，再编排章节和保存修订。</p>}
    {dirty && <button disabled={saving || !!pending} onClick={() => {
      if (window.confirm(remote?.draft ? "放弃当前未保存的大纲调整，载入最新修订？" : "放弃当前未保存的大纲调整？")) {
        setWorking(remote?.draft ? fromDraft(remote.draft) : { chapters: [], expected: null, saved: null }); setConfirmed(remote?.draft ?? null); setSelectedId(null); setConflict(false);
      }
    }}>放弃当前调整</button>}
    <OutlineRecovery projectId={projectId} tree={tree} sourceRevision={working?.sourceRevision} sourceExecution={working?.sourceExecution}
      sourceTargets={remote?.candidate && remote.candidate.execution_id === working?.sourceExecution ? remote.candidate.outline.chapters.flatMap(function targets(chapter): typeof chapter.targets { return [...chapter.targets, ...chapter.children.flatMap(targets)]; }) : undefined}
      issues={working?.issues ?? []} selectedId={selected?.chapter_id}
      disabled={editingDisabled || !!pending} onChange={change} onRestore={draft => {
        if (dirty && !window.confirm("载入历史大纲会替换当前未保存的编排，继续？")) return;
        setWorking({ chapters: draft.outline.chapters, issues: draft.outline.issues ?? [], expected: remote?.draft?.revision_id ?? null,
          sourceRevision: draft.revision_id, saved: null });
        setConfirmed(null); setSelectedId(null); setNotice("已载入历史大纲，请核对后另存新修订。");
      }} />
    <div className="outline-layout"><aside className="outline-arrangement"><div className="outline-tree-actions"><button disabled={editingDisabled} onClick={() => add("root")}>新增顶级章节</button><label>拖放位置 <select aria-label="拖放位置" value={dropPosition} onChange={event => setDropPosition(event.target.value as DropPosition)}><option value="before">放到前面</option><option value="inside">作为子章节</option><option value="after">放到后面</option></select></label></div>{tree.length ? renderTree(tree) : <p className="tender-empty">当前没有章节。已有来源的大纲可保存为空结构。</p>}</aside>
      <main className="outline-chapter-editor">{selected ? <><h2>章节设置</h2><label>章节名称<input aria-label="章节名称" value={selected.title} disabled={editingDisabled} onChange={event => change(editChapter(tree, selected.chapter_id, { title: event.target.value }))} /></label>
        <label>章节展示方式<select aria-label="章节展示方式" value={selected.presentation_role ?? "body"} disabled={editingDisabled} onChange={event => change(editChapter(tree, selected.chapter_id, { presentation_role: event.target.value }))}><option value="body">正文章节（参与编号与目录）</option><option value="cover">封面（不参与编号与目录）</option></select></label>
        <label><span><input type="checkbox" aria-label="导出为独立分册" checked={selected.is_volume ?? false} disabled={editingDisabled} onChange={event => change(editChapter(tree, selected.chapter_id, { is_volume: event.target.checked }))} /> 导出为独立分册</span></label>
        <p className="tender-note">分册会在交付包中额外生成该章节及子章节的 Word 文件。封面归属最近的上级分册；没有上级分册时作为整稿封面。保存这些设置后，需生成新正文才会体现在交付文件中。</p>
        <div className="outline-edit-actions">{([ ["up", "上移"], ["down", "下移"], ["indent", "降为子级"], ["outdent", "提升一级"] ] as const).map(([direction, label]) => <button key={direction} disabled={editingDisabled || moveRelative(tree, selected.chapter_id, direction) === tree} onClick={() => change(moveRelative(tree, selected.chapter_id, direction))}>{label}</button>)}<button disabled={editingDisabled} onClick={() => add("sibling")}>新增同级</button><button disabled={editingDisabled} onClick={() => add("child")}>新增子章节</button><button disabled={editingDisabled} onClick={() => { if (window.confirm("删除此章节及其子章节？这些响应位置将从编排移除，原始招标要求仍保留。")) { change(removeChapter(tree, selected.chapter_id)); setSelectedId(null); } }}>删除章节</button></div>
        {canEditManualGoal(selected) ? <><label>编写目标<textarea aria-label="编写目标" rows={5} value={manualGoal(selected)} disabled={disabled || saving} onChange={event => change(editChapter(tree, selected.chapter_id, { writing_goal: event.target.value }))} /></label><p className="tender-note">填写目标后为该章节生成正文；留空只保留章节标题。</p></> : <p className="tender-note">已有章节的响应要求随原始引用保留，调整名称或层级不会改写要求。</p>}
        <h3>关联响应（只读）</h3>{selected.targets.length ? selected.targets.map(target => <div className="tender-info-card" key={target.target_id}><strong>{target.purpose}</strong><p>{target.rule_refs?.length ?? 0} 条要求引用 · {target.source_refs?.length ?? 0} 处来源引用</p></div>) : <p className="tender-note">此章节尚无关联响应。</p>}
      </> : <p className="tender-empty">选择章节进行编排。</p>}</main></div>
  </section>;
}
