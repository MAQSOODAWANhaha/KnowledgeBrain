import { Banner } from "@astryxdesign/core/Banner";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { Button } from "@astryxdesign/core/Button";
import { Selector } from "@astryxdesign/core/Selector";
import { TextArea } from "@astryxdesign/core/TextArea";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../api";
import { analysisReviewApi, type AnalysisBusinessFields, type AnalysisConfirmation, type AnalysisCorrection, type AnalysisReviewView, type HumanVerdict } from "./api/analysisReview";
import type { TenderIssue, TenderItem } from "./api/workbench";
import { inProgress } from "./executionStatus";
import { ItemFacts, ItemFields, validFields } from "./AnalysisItemFields";
import {
  applyBatchDecision,
  buildHumanIssueEntries,
  CORRECTION_PAGE_SIZE,
  EMPTY_DECISION,
  filterHumanIssues,
  isDecisionComplete,
  ISSUE_PAGE_SIZE,
  issueCodeLabel,
  paginate,
  summarizeIssueCodes,
  type BatchScope,
  type Decision,
  type HumanIssueEntry,
  type IssueStatusFilter,
} from "./analysisReviewHelpers";

type Working = { base: AnalysisReviewView; decisions: Record<string, Decision>; edits: Record<string, AnalysisBusinessFields | string>; reason: string };
type Flight = { kind: "confirm"; body: AnalysisConfirmation; attempt: MutationAttempt } | { kind: "correct"; body: AnalysisCorrection; attempt: MutationAttempt };
type CorrectionEntry = { pack_id: string; item: TenderItem & { item_id: string } };
const EMPTY_PATCH: AnalysisBusinessFields = {};
const EMPTY_DECISIONS: Record<string, Decision> = {};
const itemKey = (pack: string, item: string) => JSON.stringify([pack, item]);
const identity = (review: AnalysisReviewView | null) => review && `${review.run_id}:${review.source_version}:${review.domain_revision}:${review.checklist_digest}`;
const patchOf = (value: AnalysisBusinessFields | string | undefined): AnalysisBusinessFields => typeof value === "string" ? { text: value } : value ?? EMPTY_PATCH;
const fresh = (base: AnalysisReviewView): Working => ({ base, decisions: {}, edits: {}, reason: "" });
const hasWork = (working: Working | null) => !!working && (!!Object.keys(working.edits).length || !!working.reason || Object.values(working.decisions).some(value => !!value.verdict || !!value.reason));

function serializeSnapshot(baseJson: string, working: Working, pending: Flight | null): string {
  return `{"working":{"base":${baseJson},"decisions":${JSON.stringify(working.decisions)},"edits":${JSON.stringify(working.edits)},"reason":${JSON.stringify(working.reason)}},"pending":${JSON.stringify(pending)}}`;
}

const IssueDecisionRow = memo(function IssueDecisionRow({
  entry, decision, locked, onDecisionChange,
}: {
  entry: HumanIssueEntry;
  decision: Decision;
  locked: boolean;
  onDecisionChange(id: string, patch: Partial<Decision>): void;
}) {
  const number = entry.index + 1;
  return <Section role="group" aria-label={`问题 ${number}`} padding={3}><VStack gap={2}>
    <Text weight="semibold">问题 {number}</Text>
    {entry.issue?.code && <Text type="supporting">类型：{issueCodeLabel(entry.code)}</Text>}
    <Text>{entry.issue?.detail ?? "此问题需要人工核对来源并记录判定。"}</Text>
    <Selector label={`问题 ${number} 人工判定`} isDisabled={locked} value={decision.verdict}
      onChange={verdict => onDecisionChange(entry.id, { verdict: verdict as Decision["verdict"] })}
      options={[{ value: "", label: "请选择判定" }, { value: "acknowledged", label: "保留为已知问题" }, { value: "dismissed", label: "核对后排除此问题" }]} />
    <TextArea label={`问题 ${number} 判定理由`} htmlMaxLength={4000} isDisabled={locked} value={decision.reason}
      onChange={reason => onDecisionChange(entry.id, { reason })} />
  </VStack></Section>;
});

const CorrectionItemRow = memo(function CorrectionItemRow({
  index, packId, item, rawPatch, locked, onEditItem,
}: {
  index: number;
  packId: string;
  item: TenderItem & { item_id: string };
  rawPatch: AnalysisBusinessFields | string | undefined;
  locked: boolean;
  onEditItem(packId: string, item: TenderItem & { item_id: string }, patch: AnalysisBusinessFields): void;
}) {
  const patch = useMemo(() => patchOf(rawPatch), [rawPatch]);
  const change = useCallback((nextPatch: AnalysisBusinessFields) => onEditItem(packId, item, nextPatch), [onEditItem, packId, item]);
  return <Collapsible trigger={`事项 ${index + 1}：${item.text}`} defaultIsOpen={false}><VStack gap={2}>
    <Text><Text weight="semibold">原事项：</Text>{item.text}</Text>
    <ItemFacts item={item} />
    <ItemFields index={index + 1} item={item} patch={patch} locked={locked} change={change} />
  </VStack></Collapsible>;
});

export function AnalysisReview({ projectId, disabled, onChanged, onUnsafeChange }: {
  projectId: string; disabled: boolean; onChanged(): void; onUnsafeChange(unsafe: boolean): void;
}) {
  const storageKey = `kb.analysis-review:${projectId}`;
  const [restored] = useState<{ working: Working; pending: Flight | null } | null>(() => {
    try {
      const value = JSON.parse(sessionStorage.getItem(storageKey) ?? "null");
      return value?.working?.base?.run_id && value.working.decisions && value.working.edits ? value : null;
    } catch { return null; }
  });
  const [working, setWorking] = useState<Working | null>(restored?.working ?? null);
  const [pending, setPending] = useState<Flight | null>(restored?.pending ?? null);
  const [remote, setRemote] = useState<AnalysisReviewView | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);

  const [codeFilter, setCodeFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<IssueStatusFilter>("all");
  const [issuePage, setIssuePage] = useState(1);
  const [correctionPage, setCorrectionPage] = useState(1);
  const [batchScope, setBatchScope] = useState<BatchScope>("undecided");
  const [batchVerdict, setBatchVerdict] = useState<HumanVerdict["verdict"] | "">("");
  const [batchReason, setBatchReason] = useState("");
  const [batchStatus, setBatchStatus] = useState<string | null>(null);

  const active = useRef(true);
  const readRequest = useRef(0);
  const dirty = hasWork(working);

  const read = useCallback(async (signal?: AbortSignal) => {
    const request = ++readRequest.current;
    try {
      const review = await analysisReviewApi.read(projectId, signal);
      if (signal?.aborted || !active.current || request !== readRequest.current) return;
      setRemote(review);
      setWorking(current => hasWork(current) || pending ? current : review ? fresh(review) : null);
    } catch (error) {
      if (!signal?.aborted && active.current && request === readRequest.current) setNotice(error instanceof Error ? error.message : "分析复核读取失败，请重试。");
    } finally { if (!signal?.aborted && active.current && request === readRequest.current) setLoading(false); }
  }, [projectId, pending]);

  useEffect(() => {
    active.current = true;
    const abort = new AbortController(); let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { await read(abort.signal); if (!abort.signal.aborted) timer = setTimeout(() => void poll(), 5000); };
    void poll();
    return () => { active.current = false; abort.abort(); clearTimeout(timer); };
  }, [read]);

  const base = working?.base ?? null;
  const baseKey = identity(base);
  useEffect(() => {
    setCodeFilter("all");
    setStatusFilter("all");
    setIssuePage(1);
    setCorrectionPage(1);
    setBatchStatus(null);
  }, [baseKey]);

  useEffect(() => { onUnsafeChange(dirty || !!pending || saving); }, [dirty, pending, saving, onUnsafeChange]);

  const serializedBase = useMemo(() => (base ? JSON.stringify(base) : null), [base]);
  const snapshotRef = useRef<{ working: Working | null; pending: Flight | null; dirty: boolean; serializedBase: string | null; storageKey: string }>({
    working, pending, dirty, serializedBase, storageKey,
  });
  snapshotRef.current = { working, pending, dirty, serializedBase, storageKey };

  const flushPersist = useCallback(() => {
    const snap = snapshotRef.current;
    if (!snap.working || snap.serializedBase === null || (!snap.dirty && !snap.pending)) return;
    try {
      sessionStorage.setItem(snap.storageKey, serializeSnapshot(snap.serializedBase, snap.working, snap.pending));
    } catch { setNotice("浏览器无法保存恢复信息，请保持页面打开并确认提交结果。"); }
  }, []);

  useEffect(() => {
    if (!working || serializedBase === null || (!dirty && !pending)) {
      try { sessionStorage.removeItem(storageKey); } catch { /* ignore storage removal errors */ }
      return;
    }
    if (pending) {
      try { sessionStorage.setItem(storageKey, serializeSnapshot(serializedBase, working, pending)); }
      catch { setNotice("浏览器无法保存恢复信息，请保持页面打开并确认提交结果。"); }
      return;
    }
    const timer = setTimeout(() => {
      try { sessionStorage.setItem(storageKey, serializeSnapshot(serializedBase, working, null)); }
      catch { setNotice("浏览器无法保存恢复信息，请保持页面打开并确认提交结果。"); }
    }, 150);
    return () => clearTimeout(timer);
  }, [working, dirty, pending, serializedBase, storageKey]);

  useEffect(() => {
    const onBeforeUnload = () => flushPersist();
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => {
      window.removeEventListener("beforeunload", onBeforeUnload);
      flushPersist();
    };
  }, [flushPersist]);

  const newer = !!base && baseKey !== identity(remote);
  const currentState = base && baseKey === identity(remote) ? remote : base;
  const locked = disabled || saving || !!pending;

  const corrections = useMemo(() => base?.correction?.items.flatMap(entry => {
    const patch = patchOf(working?.edits[itemKey(entry.pack_id, entry.item.item_id)]);
    const changed = Object.fromEntries(Object.entries(patch).filter(([field, value]) =>
      JSON.stringify(value ?? null) !== JSON.stringify(entry.item[field as keyof typeof entry.item] ?? null))) as AnalysisBusinessFields;
    return Object.keys(changed).length ? [{ pack_id: entry.pack_id, item_id: entry.item.item_id, ...changed }] : [];
  }) ?? [], [base, working?.edits]);

  const humanEntries = useMemo(
    () => (base ? buildHumanIssueEntries(base.human_issue_ids, base.issues) : []),
    [base],
  );
  const nonHumanIssues = useMemo<TenderIssue[]>(() => {
    if (!base) return [];
    const humanIds = new Set(base.human_issue_ids);
    return base.issues.filter(issue => !humanIds.has(issue.issue_id ?? ""));
  }, [base]);

  const decisions = working?.decisions ?? EMPTY_DECISIONS;
  const decidedCount = useMemo(
    () => humanEntries.reduce((count, entry) => count + (isDecisionComplete(decisions[entry.id]) ? 1 : 0), 0),
    [humanEntries, decisions],
  );
  const codeSummaries = useMemo(() => summarizeIssueCodes(humanEntries, decisions), [humanEntries, decisions]);
  const filteredIssues = useMemo(
    () => filterHumanIssues(humanEntries, decisions, codeFilter, statusFilter),
    [humanEntries, decisions, codeFilter, statusFilter],
  );
  const filteredUndecidedCount = useMemo(
    () => filteredIssues.reduce((count, entry) => count + (isDecisionComplete(decisions[entry.id]) ? 0 : 1), 0),
    [filteredIssues, decisions],
  );
  const pagedIssues = useMemo(() => paginate(filteredIssues, issuePage, ISSUE_PAGE_SIZE), [filteredIssues, issuePage]);

  const indexedCorrections = useMemo<Array<{ index: number; entry: CorrectionEntry }>>(
    () => (base?.correction?.items ?? []).map((entry, index) => ({ index, entry })),
    [base],
  );
  const pagedCorrections = useMemo(
    () => paginate(indexedCorrections, correctionPage, CORRECTION_PAGE_SIZE),
    [indexedCorrections, correctionPage],
  );

  const complete = !!base && decidedCount === humanEntries.length;
  const canConfirm = !!base && currentState?.status === "waiting-confirmation" && !currentState.hard_blocking && !!base.checklist_digest && complete && !corrections.length;

  const onDecisionChange = useCallback((id: string, patch: Partial<Decision>) => {
    setWorking(current => {
      if (!current) return current;
      const prev = current.decisions[id] ?? EMPTY_DECISION;
      return { ...current, decisions: { ...current.decisions, [id]: { ...prev, ...patch } } };
    });
  }, []);

  const onEditItem = useCallback((packId: string, item: TenderItem & { item_id: string }, patch: AnalysisBusinessFields) => {
    const key = itemKey(packId, item.item_id);
    setWorking(current => {
      if (!current) return current;
      const value = { ...patchOf(current.edits[key]), ...patch };
      const changed = Object.fromEntries(Object.entries(value).filter(([field, content]) =>
        JSON.stringify(content ?? null) !== JSON.stringify(item[field as keyof typeof item] ?? null)));
      const edits = { ...current.edits };
      if (Object.keys(changed).length) edits[key] = changed; else delete edits[key];
      return { ...current, edits };
    });
  }, []);

  const batchTargetCount = batchScope === "all" ? filteredIssues.length : filteredUndecidedCount;

  function applyBatch() {
    if (!working || locked || !batchVerdict || !batchReason.trim() || batchTargetCount === 0) return;
    const result = applyBatchDecision(working.decisions, filteredIssues, batchVerdict, batchReason.trim(), batchScope);
    if (result.updated > 0) {
      setWorking(current => (current ? { ...current, decisions: result.next } : current));
    }
    setBatchStatus(result.updated > 0 ? `已批量写入 ${result.updated} 项判定（当前保留已有手动判定不被覆盖的限制按范围执行）。` : "当前筛选范围内没有需要写入的问题。");
  }

  async function submit(kind: "confirm" | "correct") {
    if (!working || saving || serializedBase === null || (disabled && !pending)) return;
    const base = working.base;
    let flight = pending;
    if (!flight) {
      if (newer || conflict) return;
      if (kind === "confirm") {
        if (!canConfirm || !base.checklist_digest) return;
        flight = { kind, attempt: createMutationAttempt(), body: { run_id: base.run_id, source_version: base.source_version,
          domain_revision: base.domain_revision, checklist_digest: base.checklist_digest,
          issue_resolutions: { schema_version: 1, resolutions: base.human_issue_ids.map(id => ({ issue_id: id,
            verdict: working.decisions[id].verdict as HumanVerdict["verdict"], reason: working.decisions[id].reason })) } } };
      } else {
        if (!base.correction || !corrections.length || !working.reason.trim() || corrections.some(value => !validFields(value))) return;
        flight = { kind, attempt: createMutationAttempt(), body: { run_id: base.run_id, source_version: base.source_version,
          expected_revision: base.domain_revision, base_items_digest: base.correction.base_items_digest,
          reason: working.reason, changes: corrections } };
      }
    }
    try { sessionStorage.setItem(storageKey, serializeSnapshot(serializedBase, working, flight)); }
    catch { setNotice("无法保存恢复信息，本次尚未发送。请检查浏览器存储后重试。"); return; }
    readRequest.current++; setPending(flight); setSaving(true); setNotice(null);
    try {
      if (flight.kind === "confirm") await analysisReviewApi.confirm(projectId, flight.body, flight.attempt);
      else await analysisReviewApi.correct(projectId, flight.body, flight.attempt);
      sessionStorage.removeItem(storageKey);
      snapshotRef.current = { ...snapshotRef.current, working: null, pending: null, dirty: false };
      setPending(null); setWorking(null); setConflict(false);
      setNotice(flight.kind === "confirm" ? "人工确认已提交。" : "事项纠正已提交，正在重新审核；请等待新的确认清单。");
      onChanged(); await read();
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        setPending(null); setConflict(error.status === 409);
        setNotice(`${error.message} 当前输入已保留。`);
      } else setNotice("提交结果尚未确认。当前输入已保留，请确认同一次请求，避免重复提交。");
    } finally { if (active.current) setSaving(false); }
  }

  return <VStack role="region" aria-label="招标分析人工复核" gap={3}>
    <Heading level={3}>人工复核与事项纠正</Heading>
    {loading && <Text role="status" type="supporting">正在读取复核清单…</Text>}
    {notice && <Banner role="alert" status="error" title={notice} />}
    <HStack gap={2} wrap="wrap">
      <Button variant="secondary" isDisabled={saving} onClick={() => void read()} label="刷新复核状态" />
      {dirty && !pending && <Button variant="ghost" isDisabled={saving} onClick={() => {
        if (window.confirm("放弃当前尚未提交的人工判定和事项纠正？")) {
          snapshotRef.current = { ...snapshotRef.current, working: null, pending: null, dirty: false };
          setWorking(remote ? fresh(remote) : base ? fresh(base) : null); setConflict(false); setNotice(null); setBatchStatus(null);
        }
      }} label="放弃未提交输入" />}
    </HStack>
    {!loading && !base && <Text type="supporting">完成招标分析后，可在这里处理待确认问题、纠正事项业务字段。</Text>}
    {base && working && <>
      <Text type="supporting">当前核对：来源版本 {base.source_version}，分析修订 {base.domain_revision}。{dirty ? "当前输入尚未提交。" : ""}</Text>
      {!(baseKey === identity(remote) ? remote?.sources_current : base.sources_current) && <Banner status="warning" title="项目来源已变化，以下仍是旧资料的分析。可确认该固定版本，或使用当前上传文件重新分析。" />}
      {(newer || conflict) && <Banner role="alert" status="warning" collapsible={false} title="分析状态或版本已有变化，当前输入仍保留。请刷新核对最新结果后再处理。"
        description={remote ? `最新来源版本 ${remote.source_version}，分析修订 ${remote.domain_revision}；待确认问题 ${remote.human_issue_ids.length} 项。` : undefined}
        endContent={<Button size="sm" isDisabled={saving || !!pending || !remote} onClick={() => {
          if (remote && (!dirty || window.confirm("放弃当前未提交的复核输入，载入最新分析？"))) {
            setWorking(fresh(remote)); setConflict(false); setNotice(null); setBatchStatus(null);
          }
        }} label="载入最新分析重新核对" />}>
        {remote && <Collapsible trigger="查看最新分析内容，对照当前输入" defaultIsOpen={false}><VStack gap={2}>
          {remote.issues.map((issue, index) => <Text key={index}>{issue.detail}</Text>)}
          {remote.correction?.items.map(entry => <VStack key={itemKey(entry.pack_id, entry.item.item_id)} gap={1}><Text>{entry.item.text}</Text><ItemFacts item={entry.item} /></VStack>)}
        </VStack></Collapsible>}
      </Banner>}
      {currentState?.hard_blocking && <Banner status="error" title="存在来源、覆盖或执行问题，人工判定不能解除。请按问题提示补充来源、使用完整纠正流程或联系管理员处理后重新分析。" />}

      {humanEntries.length > 1 && <>
        <Text type="supporting">已完成 {decidedCount} / {humanEntries.length} 项待确认问题</Text>
        <HStack gap={2} wrap="wrap">
          <Selector label="按问题类型筛选" value={codeFilter} onChange={value => { setCodeFilter(value); setIssuePage(1); }}
            options={[
              { value: "all", label: `全部类型（${humanEntries.length}）` },
              ...codeSummaries.map(summary => ({ value: summary.code, label: `${summary.label}（${summary.decided}/${summary.total}）` })),
            ]} />
          <Selector label="按判定状态筛选" value={statusFilter} onChange={value => { setStatusFilter(value as IssueStatusFilter); setIssuePage(1); }}
            options={[
              { value: "all", label: "全部状态" },
              { value: "pending", label: `仅看未判定（${humanEntries.length - decidedCount}）` },
              { value: "decided", label: `仅看已判定（${decidedCount}）` },
            ]} />
        </HStack>
        <Section role="group" aria-label="按筛选结果批量判定" variant="muted" padding={3}><VStack gap={2}>
          <Text weight="semibold">批量判定（当前筛选 {filteredIssues.length} 项，其中未判定 {filteredUndecidedCount} 项）</Text>
          <HStack gap={2} wrap="wrap">
            <Selector label="批量作用范围" isDisabled={locked} value={batchScope} onChange={value => setBatchScope(value as BatchScope)}
              options={[
                { value: "undecided", label: `仅填充未判定问题（${filteredUndecidedCount} 项）` },
                { value: "all", label: `覆盖当前筛选全部问题（${filteredIssues.length} 项）` },
              ]} />
            <Selector label="批量人工判定" isDisabled={locked} value={batchVerdict} onChange={value => setBatchVerdict(value as HumanVerdict["verdict"] | "")}
              options={[
                { value: "", label: "请选择判定" },
                { value: "acknowledged", label: "保留为已知问题" },
                { value: "dismissed", label: "核对后排除此问题" },
              ]} />
          </HStack>
          <TextArea label="批量判定理由" htmlMaxLength={4000} isDisabled={locked} value={batchReason} onChange={setBatchReason} />
          <HStack gap={2} wrap="wrap">
            <Button variant="secondary" isDisabled={locked || !batchVerdict || !batchReason.trim() || batchTargetCount === 0} onClick={applyBatch} label="应用批量判定" />
            {batchStatus && <Text role="status" type="supporting">{batchStatus}</Text>}
          </HStack>
        </VStack></Section>
      </>}

      {pagedIssues.slice.map(entry => (
        <IssueDecisionRow key={entry.id} entry={entry} decision={decisions[entry.id] ?? EMPTY_DECISION} locked={locked} onDecisionChange={onDecisionChange} />
      ))}
      {filteredIssues.length === 0 && humanEntries.length > 0 && <Text type="supporting">当前筛选条件下没有匹配的问题。</Text>}
      {pagedIssues.totalPages > 1 && <HStack gap={2} wrap="wrap" vAlign="center">
        <Button size="sm" variant="secondary" aria-label="问题列表上一页" isDisabled={pagedIssues.page <= 1} onClick={() => setIssuePage(pagedIssues.page - 1)} label="上一页" />
        <Text type="supporting">第 {pagedIssues.page} / {pagedIssues.totalPages} 页（共 {filteredIssues.length} 项）</Text>
        <Button size="sm" variant="secondary" aria-label="问题列表下一页" isDisabled={pagedIssues.page >= pagedIssues.totalPages} onClick={() => setIssuePage(pagedIssues.page + 1)} label="下一页" />
      </HStack>}

      {(inProgress(currentState?.status) || currentState?.status === "failed") ? <Text type="supporting">分析尚未完成，完成后才能人工确认。</Text>
        : !base.human_issue_ids.length && !base.correction?.items.length && <Text type="supporting">当前没有待核对的问题。</Text>}
      {!!base.human_issue_ids.length && <Heading level={4}>待确认问题（{base.human_issue_ids.length}）</Heading>}
      {nonHumanIssues.map((issue, index) => <Text key={index} type="supporting">{issue.detail}</Text>)}
      <HStack><Button variant="primary" isDisabled={disabled && !pending || saving || (pending ? pending.kind !== "confirm" : !canConfirm || newer || conflict)} onClick={() => void submit("confirm")} label={pending?.kind === "confirm" ? "确认同一次人工确认请求" : "提交人工确认"} /></HStack>
      {!!corrections.length && <Text type="supporting">已修改事项字段，请先提交事项纠正，等待重新审核后的确认清单。</Text>}
      {corrections.some(value => !validFields(value)) && <Banner status="warning" title="请补齐已添加的业务字段和数值原文片段；不需要的字段可清空或删除，最高分须大于等于零。" />}
      {!!base.correction?.items.length && <><Heading level={4}>事项业务字段</Heading>
      <Text type="supporting">对照冻结原文纠正事项。来源位置和证据保持冻结；数值原文片段必须能在该事项的冻结来源中找到。</Text>
      {pagedCorrections.slice.map(({ index, entry }) => (
        <CorrectionItemRow key={itemKey(entry.pack_id, entry.item.item_id)} index={index} packId={entry.pack_id} item={entry.item}
          rawPatch={working.edits[itemKey(entry.pack_id, entry.item.item_id)]} locked={locked} onEditItem={onEditItem} />
      ))}
      {pagedCorrections.totalPages > 1 && <HStack gap={2} wrap="wrap" vAlign="center">
        <Button size="sm" variant="secondary" aria-label="事项列表上一页" isDisabled={pagedCorrections.page <= 1} onClick={() => setCorrectionPage(pagedCorrections.page - 1)} label="上一页" />
        <Text type="supporting">第 {pagedCorrections.page} / {pagedCorrections.totalPages} 页（共 {indexedCorrections.length} 项）</Text>
        <Button size="sm" variant="secondary" aria-label="事项列表下一页" isDisabled={pagedCorrections.page >= pagedCorrections.totalPages} onClick={() => setCorrectionPage(pagedCorrections.page + 1)} label="下一页" />
      </HStack>}
      <TextArea label="事项纠正理由" htmlMaxLength={4000} isDisabled={locked} value={working.reason} onChange={reason => setWorking(current => current && { ...current, reason })} />
      <HStack><Button variant="primary" isDisabled={disabled && !pending || saving || (pending ? pending.kind !== "correct" : newer || conflict || !corrections.length || !working.reason.trim() || corrections.some(value => !validFields(value)))} onClick={() => void submit("correct")} label={pending?.kind === "correct" ? "确认同一次事项纠正请求" : "提交事项纠正并重新审核"} /></HStack></>}
    </>}
  </VStack>;
}
