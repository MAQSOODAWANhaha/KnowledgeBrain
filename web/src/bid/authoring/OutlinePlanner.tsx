import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Item } from "@astryxdesign/core/Item";
import { Layout, LayoutContent, LayoutPanel } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { TextArea } from "@astryxdesign/core/TextArea";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Toolbar } from "@astryxdesign/core/Toolbar";
import { TreeList, type TreeListItemData } from "@astryxdesign/core/TreeList";
import { VStack } from "@astryxdesign/core/VStack";
import { X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import { randomUuid } from "../../uuid";
import { executionError, tenderApi, type DocumentStart, type OutlineRevision, type OutlineSave, type OutlineView, type TenderChapter, type TenderIssue, type TenderItem } from "../api/workbench";
import { EvaluationReview, pendingEvaluation, submittedDecisions, type Decisions } from "./EvaluationReview";
import { canEditManualGoal, editChapter, findChapter, manualGoal, moveChapter, moveRelative, outlineNodes, removeChapter } from "./outlineTree";
import { OutlineRecovery } from "./OutlineRecovery";
import { inProgress as running } from "../executionStatus";
import { TaskProgress } from "../TaskProgress";

type Working = { chapters: TenderChapter[]; issues?: TenderIssue[]; decisions?: Decisions; expected: string | null; sourceRevision?: string; sourceExecution?: string; saved: string | null };
type SaveFlight = { body: OutlineSave; attempt: MutationAttempt; working: Working };
const signature = (chapters: TenderChapter[]) => JSON.stringify(outlineNodes(chapters));
const fromDraft = (draft: OutlineRevision): Working => ({ chapters: draft.outline.chapters, issues: draft.outline.issues ?? [], expected: draft.revision_id, sourceRevision: draft.revision_id, saved: signature(draft.outline.chapters) });

export function OutlinePlanner({ projectId, items, disabled, bodyBusy, bodyPending, onGenerateBody, onUnsafeChange, onExecutionAction }: {
  projectId: string; items: TenderItem[]; disabled: boolean; bodyBusy: boolean; bodyPending: boolean;
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
  const [reviewOpen, setReviewOpen] = useState(false);
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
  const decided = submittedDecisions(working?.decisions ?? {});
  const dirty = !!working && (working.saved === null ? !!working.chapters.length || !!working.sourceExecution || !!working.sourceRevision : signature(working.chapters) !== working.saved || !!decided.length);
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
      ...(working.sourceExecution ? { source_execution_id: working.sourceExecution } : {}), chapters: outlineNodes(tree),
      ...(decided.length ? { evaluation_decisions: decided } : {}) }, attempt: createMutationAttempt(), working };
    try { sessionStorage.setItem(storageKey, JSON.stringify({ ...flight, working })); }
    catch { setNotice("无法保存浏览器恢复信息，大纲尚未发送。请检查浏览器存储后重试。"); return; }
    readRequest.current++; setPending(flight); setSaving(true); setNotice(null); setConflict(false);
    try {
      const draft = await tenderApi.saveOutline(projectId, flight.body, flight.attempt);
      setConfirmed(draft);
      setWorking(current => current && { ...current, expected: draft.revision_id, sourceRevision: draft.revision_id, sourceExecution: undefined,
        issues: draft.outline.issues ?? [],
        decisions: JSON.stringify(submittedDecisions(current.decisions ?? {})) === JSON.stringify(flight.body.evaluation_decisions ?? []) ? {} : current.decisions,
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
  const treeItems = (nodes: TenderChapter[], prefix = ""): TreeListItemData[] => nodes.map((chapter, index) => {
    const number = prefix ? `${prefix}.${index + 1}` : `${index + 1}`;
    return {
      id: chapter.chapter_id,
      label: `${number} ${chapter.title || "未命名章节"}`,
      isSelected: selected?.chapter_id === chapter.chapter_id,
      isExpanded: true,
      onClick: () => setSelectedId(chapter.chapter_id),
      endContent: chapter.targets.length ? <Badge variant="neutral" label={`${chapter.targets.length} 项响应`} /> : undefined,
      children: chapter.children.length ? treeItems(chapter.children, number) : undefined,
    };
  });
  const bodyDisabled = disabled || bodyBusy || saving || dirty || !!pending || !confirmed || !confirmed.sources_current || remote?.draft?.sources_current === false;
  const pinnedAnalysis = confirmed?.outline.authoring_input;
  const newerAnalysis = remote?.draft?.revision_id === confirmed?.revision_id ? remote?.draft?.newer_analysis_identity : null;
  const candidateAnalysis = remote?.candidate?.outline.authoring_input;
  const saveStatus = saving ? "保存中" : pending ? "保存结果待确认" : dirty ? "有未保存修改" : confirmed ? `已保存修订 ${confirmed.revision}` : "尚无已保存修订";
  const keepCurrent = async () => {
    const request = ++readRequest.current;
    try {
      const latest = await tenderApi.outline(projectId);
      if (request !== readRequest.current) return;
      setRemote(latest); setWorking(current => current && { ...current, expected: latest.draft?.revision_id ?? null }); setConflict(false);
      setNotice("当前编排已保留并更新保存基准。再次保存会按当前全部章节另存修订。");
    } catch (error) { if (request === readRequest.current) setNotice(error instanceof Error ? error.message : "最新修订读取失败，请重试。"); }
  };
  const evaluationRefs = pendingEvaluation(working?.issues ?? []);
  const moves = [["up", "上移"], ["down", "下移"], ["indent", "降为子级"], ["outdent", "提升一级"]] as const;
  return <VStack gap={0} height="100%" minHeight={0} role="region" aria-label="大纲编排工作区">
    <Toolbar label="大纲操作" dividers={["bottom"]}
      startContent={<HStack gap={3} vAlign="center"><Heading level={2}>大纲编排</Heading><Text data-testid="outline-save-status" role="status" type="supporting">{saveStatus}</Text></HStack>}
      endContent={<>
        {dirty && <Button variant="ghost" isDisabled={saving || !!pending} onClick={() => {
          if (window.confirm(remote?.draft ? "放弃当前未保存的大纲调整，载入最新修订？" : "放弃当前未保存的大纲调整？")) {
            setWorking(remote?.draft ? fromDraft(remote.draft) : { chapters: [], expected: null, saved: null }); setConfirmed(remote?.draft ?? null); setSelectedId(null); setConflict(false);
          }
        }} label="放弃当前调整" />}
        <Button variant="secondary" isDisabled={disabled && !generationAttempt || generating || running(remote?.generation?.status) && !generationAttempt} onClick={() => void generate()} label={generationAttempt ? "确认同一次大纲生成请求" : "生成大纲"} />
        <Button variant="secondary" isDisabled={disabled && !pending || saving || !hasSource || !dirty && !pending} onClick={() => void save()} label={pending ? "确认同一次大纲保存" : "保存大纲草稿"} />
        <Button variant="primary" isDisabled={bodyPending ? bodyBusy : bodyDisabled} onClick={() => bodyPending ? onGenerateBody() : confirmed && onGenerateBody({ outline_revision_id: confirmed.revision_id, outline_sha256: confirmed.sha256 })} label={bodyPending ? "确认同一次编制请求" : `按修订 ${confirmed?.revision ?? "—"} 生成正文`} />
      </>} />
    <VStack gap={2} paddingInline={4} paddingBlock={2}>
      {pinnedAnalysis && <Text type="supporting">已保存大纲使用来源版本 {pinnedAnalysis.source_version}、分析修订 {pinnedAnalysis.domain_revision}。</Text>}
      {newerAnalysis && <Banner role="status" status="info" title={`同源分析已更新至来源版本 ${newerAnalysis.source_version}、分析修订 ${newerAnalysis.domain_revision}。请生成大纲，载入候选并保存后采用新版分析。`} />}
      {remote?.generation && <TaskProgress label="大纲任务" status={remote.generation.status} progress={remote.generation.progress} />}
      {notice && <Banner role="alert" status="error" title={notice} endContent={<IconButton size="sm" variant="ghost" label="关闭大纲提示" icon={<X size={14} />} onClick={() => setNotice(null)} />} />}
      {conflict && <Banner status="warning" title="当前编排与最新修订可能不同。保留当前编排另存会用当前全部章节替换最新结构；也可载入最新大纲后重新调整。" endContent={<Button size="sm" onClick={() => void keepCurrent()} label="保留当前编排，按最新修订另存" />} />}
      {remote?.draft && working && remote.draft.revision_id !== working.expected && !pending && <Banner status="info" title={`已有更新的大纲修订 ${remote.draft.revision}。`} endContent={<Button size="sm" onClick={() => { if (!dirty || window.confirm("放弃当前未保存的大纲调整，载入最新修订？")) { setWorking(fromDraft(remote.draft!)); setConfirmed(remote.draft); setSelectedId(null); setConflict(false); } }} label="载入最新大纲" />} />}
      {remote?.candidate && remote.candidate.execution_id !== working?.sourceExecution && remote.candidate.execution_id !== confirmed?.execution_id && <Banner status="info" title="新的候选大纲已生成。"
        description={[candidateAnalysis && `候选依据为来源版本 ${candidateAnalysis.source_version}、分析修订 ${candidateAnalysis.domain_revision}。`, dirty && "请先保存当前调整。"].filter(Boolean).join("")}
        endContent={<Button size="sm" isDisabled={dirty || !!pending || disabled} onClick={adopt} label="载入候选大纲" />} />}
      {remote?.generation && (running(remote.generation.status) || ["failed", "blocked"].includes(remote.generation.status)) && <Banner role="status" status={running(remote.generation.status) ? "info" : "error"} title={executionError(remote.generation.error) || "大纲生成进行中。"}
        endContent={running(remote.generation.status) ? <Button size="sm" isDisabled={bodyBusy} onClick={() => void executionAction("cancel")} label="取消大纲生成" /> : <Button size="sm" isDisabled={disabled || bodyBusy} onClick={() => void executionAction("resume")} label="继续大纲生成" />} />}
      {(confirmed && !confirmed.sources_current || remote?.draft?.sources_current === false) && <Banner role="alert" status="error" title="项目来源已变化，请重新生成大纲并保存新修订后生成正文。" />}
      {!hasSource && <Banner status="info" title="请先生成并载入候选大纲，再编排章节和保存修订。" />}
      {!!evaluationRefs.length && <Banner status="warning" title={`${evaluationRefs.length} 条事项待按原件核定是否按评标程序处理`}
        description={decided.length ? `已核定 ${decided.length} 条，保存大纲草稿后生效。` : undefined}
        endContent={<Button size="sm" onClick={() => setReviewOpen(true)} label="核定评标待核事项" />} />}
      <EvaluationReview isOpen={reviewOpen} onOpenChange={setReviewOpen} refs={evaluationRefs} chapters={tree} items={items}
        decisions={working?.decisions ?? {}} disabled={editingDisabled || !!pending}
        onChange={decisions => setWorking(current => current && { ...current, decisions })} />
      <OutlineRecovery projectId={projectId} tree={tree} sourceRevision={working?.sourceRevision} sourceExecution={working?.sourceExecution}
        sourceTargets={remote?.candidate && remote.candidate.execution_id === working?.sourceExecution ? remote.candidate.outline.chapters.flatMap(function targets(chapter): typeof chapter.targets { return [...chapter.targets, ...chapter.children.flatMap(targets)]; }) : undefined}
        issues={working?.issues ?? []} selectedId={selected?.chapter_id}
        disabled={editingDisabled || !!pending} onChange={change} onRestore={draft => {
          if (dirty && !window.confirm("载入历史大纲会替换当前未保存的编排，继续？")) return;
          setWorking({ chapters: draft.outline.chapters, issues: draft.outline.issues ?? [], expected: remote?.draft?.revision_id ?? null,
            sourceRevision: draft.revision_id, saved: null });
          setConfirmed(null); setSelectedId(null); setNotice("已载入历史大纲，请核对后另存新修订。");
        }} />
    </VStack>
    <Layout height="fill"
      start={<LayoutPanel width={320} hasDivider label="章节结构" role="complementary">
        <VStack gap={2}>
          <HStack hAlign="between" vAlign="center"><Text weight="semibold">章节结构</Text><Button size="sm" variant="secondary" isDisabled={editingDisabled} onClick={() => add("root")} label="新增顶级章节" /></HStack>
          {tree.length ? <TreeList aria-label="大纲章节" density="compact" variant="noGuides" items={treeItems(tree)} /> : <EmptyState isCompact title="当前没有章节" description="已有来源的大纲可保存为空结构。" />}
        </VStack>
      </LayoutPanel>}
      content={<LayoutContent padding={4}>{selected ? <VStack gap={4} maxWidth={760}>
        <Heading level={2}>章节设置</Heading>
        <Toolbar label="章节位置" size="sm" startContent={<>
          {moves.map(([direction, label]) => <Button key={direction} variant="secondary" isDisabled={editingDisabled || moveRelative(tree, selected.chapter_id, direction) === tree} onClick={() => change(moveRelative(tree, selected.chapter_id, direction))} label={label} />)}
        </>} endContent={<>
          <Button variant="secondary" isDisabled={editingDisabled} onClick={() => add("sibling")} label="新增同级" />
          <Button variant="secondary" isDisabled={editingDisabled} onClick={() => add("child")} label="新增子章节" />
          <Button variant="destructive" isDisabled={editingDisabled} onClick={() => { if (window.confirm("删除此章节及其子章节？这些响应位置将从编排移除，原始招标要求仍保留。")) { change(removeChapter(tree, selected.chapter_id)); setSelectedId(null); } }} label="删除章节" />
        </>} />
        <TextInput label="章节名称" value={selected.title} isDisabled={editingDisabled} onChange={title => change(editChapter(tree, selected.chapter_id, { title }))} />
        <Selector label="章节展示方式" value={selected.presentation_role ?? "body"} isDisabled={editingDisabled} onChange={role => change(editChapter(tree, selected.chapter_id, { presentation_role: role }))} options={[{ value: "body", label: "正文章节（参与编号与目录）" }, { value: "cover", label: "封面（不参与编号与目录）" }, { value: "submission_note", label: "提交待办（不进入正文）" }]} />
        <CheckboxInput label="导出为独立分册" value={selected.is_volume ?? false} isDisabled={editingDisabled} onChange={checked => change(editChapter(tree, selected.chapter_id, { is_volume: checked }))} />
        <Text type="supporting">分册会在交付包中额外生成该章节及子章节的 Word 文件。封面归属最近的上级分册；没有上级分册时作为整稿封面。保存这些设置后，需生成新正文才会体现在交付文件中。</Text>
        {canEditManualGoal(selected)
          ? <VStack gap={1}><TextArea label="编写目标" rows={5} value={manualGoal(selected)} isDisabled={disabled || saving} onChange={goal => change(editChapter(tree, selected.chapter_id, { writing_goal: goal }))} /><Text type="supporting">填写目标后为该章节生成正文；留空只保留章节标题。</Text></VStack>
          : <Text type="supporting">已有章节的响应要求随原始引用保留，调整名称或层级不会改写要求。</Text>}
        <VStack gap={2}>
          <Heading level={3}>关联响应（只读）</Heading>
          {selected.targets.length ? selected.targets.map(target => <Card key={target.target_id} padding={3}><Item density="compact" label={target.purpose} description={`${target.rule_refs?.length ?? 0} 条要求引用 · ${target.source_refs?.length ?? 0} 处来源引用`} /></Card>)
            : <Text type="supporting">此章节尚无关联响应。</Text>}
        </VStack>
      </VStack> : <EmptyState title="选择章节进行编排" description="在左侧章节结构中选择一个章节。" />}</LayoutContent>} />
  </VStack>;
}
