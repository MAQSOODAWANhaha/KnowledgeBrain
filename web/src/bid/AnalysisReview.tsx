import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../api";
import { analysisReviewApi, type AnalysisBusinessFields, type AnalysisConfirmation, type AnalysisCorrection, type AnalysisReviewView, type HumanVerdict } from "./api/analysisReview";
import { ItemFacts, ItemFields, validFields } from "./AnalysisItemFields";

type Decision = { verdict: HumanVerdict["verdict"] | ""; reason: string };
type Working = { base: AnalysisReviewView; decisions: Record<string, Decision>; edits: Record<string, AnalysisBusinessFields | string>; reason: string };
type Flight = { kind: "confirm"; body: AnalysisConfirmation; attempt: MutationAttempt } | { kind: "correct"; body: AnalysisCorrection; attempt: MutationAttempt };
const itemKey = (pack: string, item: string) => JSON.stringify([pack, item]);
const identity = (review: AnalysisReviewView | null) => review && `${review.run_id}:${review.source_version}:${review.domain_revision}:${review.checklist_digest}`;
const patchOf = (value: AnalysisBusinessFields | string | undefined): AnalysisBusinessFields => typeof value === "string" ? { text: value } : value ?? {};
const fresh = (base: AnalysisReviewView): Working => ({ base, decisions: {}, edits: {}, reason: "" });
const hasWork = (working: Working | null) => !!working && (!!Object.keys(working.edits).length || !!working.reason || Object.values(working.decisions).some(value => !!value.verdict || !!value.reason));

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
  useEffect(() => { onUnsafeChange(dirty || !!pending || saving); }, [dirty, pending, saving, onUnsafeChange]);
  useEffect(() => {
    try {
      if (working && (dirty || pending)) sessionStorage.setItem(storageKey, JSON.stringify({ working, pending }));
      else sessionStorage.removeItem(storageKey);
    } catch { setNotice("浏览器无法保存恢复信息，请保持页面打开并确认提交结果。"); }
  }, [working, dirty, pending, storageKey]);

  const base = working?.base;
  const newer = !!base && identity(base) !== identity(remote);
  const currentState = base && identity(base) === identity(remote) ? remote : base;
  const locked = disabled || saving || !!pending;
  const corrections = base?.correction?.items.flatMap(entry => {
    const patch = patchOf(working?.edits[itemKey(entry.pack_id, entry.item.item_id)]);
    const changed = Object.fromEntries(Object.entries(patch).filter(([field, value]) =>
      JSON.stringify(value ?? null) !== JSON.stringify(entry.item[field as keyof typeof entry.item] ?? null))) as AnalysisBusinessFields;
    return Object.keys(changed).length ? [{ pack_id: entry.pack_id, item_id: entry.item.item_id, ...changed }] : [];
  }) ?? [];
  const complete = !!base && base.human_issue_ids.every(id => working?.decisions[id]?.verdict && working.decisions[id].reason.trim());
  const canConfirm = !!base && currentState?.status === "waiting-confirmation" && !currentState.hard_blocking && !!base.checklist_digest && complete && !corrections.length;

  async function submit(kind: "confirm" | "correct") {
    if (!working || saving || disabled && !pending) return;
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
    try { sessionStorage.setItem(storageKey, JSON.stringify({ working, pending: flight })); }
    catch { setNotice("无法保存恢复信息，本次尚未发送。请检查浏览器存储后重试。"); return; }
    readRequest.current++; setPending(flight); setSaving(true); setNotice(null);
    try {
      if (flight.kind === "confirm") await analysisReviewApi.confirm(projectId, flight.body, flight.attempt);
      else await analysisReviewApi.correct(projectId, flight.body, flight.attempt);
      sessionStorage.removeItem(storageKey); setPending(null); setWorking(null); setConflict(false);
      setNotice(flight.kind === "confirm" ? "人工确认已提交。" : "事项纠正已提交，正在重新审核；请等待新的确认清单。");
      onChanged(); await read();
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        setPending(null); setConflict(error.status === 409);
        setNotice(`${error.message} 当前输入已保留。`);
      } else setNotice("提交结果尚未确认。当前输入已保留，请确认同一次请求，避免重复提交。");
    } finally { if (active.current) setSaving(false); }
  }

  return <section className="tender-info-card" aria-label="招标分析人工复核">
    <h3>人工复核与事项纠正</h3>
    {loading && <p role="status">正在读取复核清单…</p>}
    {notice && <p className="tender-alert" role="alert">{notice}</p>}
    <button disabled={saving} onClick={() => void read()}>刷新复核状态</button>
    {dirty && !pending && <button disabled={saving} onClick={() => {
      if (window.confirm("放弃当前尚未提交的人工判定和事项纠正？")) {
        setWorking(remote ? fresh(remote) : base ? fresh(base) : null); setConflict(false); setNotice(null);
      }
    }}>放弃未提交输入</button>}
    {!loading && !base && <p className="tender-note">完成招标分析后，可在这里处理待确认问题、纠正事项业务字段。</p>}
    {base && working && <>
      <p className="tender-note">当前核对：来源版本 {base.source_version}，分析修订 {base.domain_revision}。{dirty ? "当前输入尚未提交。" : ""}</p>
      {!(identity(base) === identity(remote) ? remote?.sources_current : base.sources_current) && <p className="tender-alert">项目来源已变化，以下仍是旧资料的分析。可确认该固定版本，或使用当前上传文件重新分析。</p>}
      {(newer || conflict) && <div className="tender-alert" role="alert">
        <p>分析状态或版本已有变化，当前输入仍保留。请刷新核对最新结果后再处理。</p>
        {remote && <p>最新来源版本 {remote.source_version}，分析修订 {remote.domain_revision}；待确认问题 {remote.human_issue_ids.length} 项。</p>}
        {remote && <details><summary>查看最新分析内容，对照当前输入</summary>
          {remote.issues.map((issue, index) => <p key={index}>{issue.detail}</p>)}
          {remote.correction?.items.map(entry => <div key={itemKey(entry.pack_id, entry.item.item_id)}><p>{entry.item.text}</p><ItemFacts item={entry.item} /></div>)}
        </details>}
        <button disabled={saving || !!pending || !remote} onClick={() => {
          if (remote && (!dirty || window.confirm("放弃当前未提交的复核输入，载入最新分析？"))) {
            setWorking(fresh(remote)); setConflict(false); setNotice(null);
          }
        }}>载入最新分析重新核对</button>
      </div>}
      {currentState?.hard_blocking && <p className="tender-alert">存在来源、覆盖或执行问题，人工判定不能解除。请按问题提示补充来源、使用完整纠正流程或联系管理员处理后重新分析。</p>}
      {base.human_issue_ids.map((id, index) => {
        const issue = base.issues.find(value => value.issue_id === id);
        const value = working.decisions[id] ?? { verdict: "", reason: "" };
        const change = (patch: Partial<Decision>) => setWorking(current => current && { ...current,
          decisions: { ...current.decisions, [id]: { ...value, ...patch } } });
        return <fieldset key={id} disabled={locked}><legend>问题 {index + 1}</legend>
          <p>{issue?.detail ?? "此问题需要人工核对来源并记录判定。"}</p>
          <label>人工判定 <select aria-label={`问题 ${index + 1} 人工判定`} value={value.verdict} onChange={event => change({ verdict: event.target.value as Decision["verdict"] })}>
            <option value="">请选择判定</option><option value="acknowledged">保留为已知问题</option><option value="dismissed">核对后排除此问题</option>
          </select></label>
          <label>判定理由 <textarea aria-label={`问题 ${index + 1} 判定理由`} value={value.reason} maxLength={4000} onChange={event => change({ reason: event.target.value })} /></label>
        </fieldset>;
      })}
      {!base.human_issue_ids.length && !base.correction?.items.length && <p className="tender-note">当前没有待核对的问题。</p>}
      {!!base.human_issue_ids.length && <h4>待确认问题（{base.human_issue_ids.length}）</h4>}
      {base.issues.filter(issue => !base.human_issue_ids.includes(issue.issue_id ?? "")).map((issue, index) => <p key={index} className="tender-note">{issue.detail}</p>)}
      <button className="tender-primary" disabled={disabled && !pending || saving || (pending ? pending.kind !== "confirm" : !canConfirm || newer || conflict)} onClick={() => void submit("confirm")}>{pending?.kind === "confirm" ? "确认同一次人工确认请求" : "提交人工确认"}</button>
      {!!corrections.length && <p className="tender-note">已修改事项字段，请先提交事项纠正，等待重新审核后的确认清单。</p>}
      {corrections.some(value => !validFields(value)) && <p className="tender-alert">请补齐已添加的业务字段和数值原文片段；不需要的字段可清空或删除，最高分须大于等于零。</p>}
      {!!base.correction?.items.length && <><h4>事项业务字段</h4>
      <p className="tender-note">对照冻结原文纠正事项。来源位置和证据保持冻结；数值原文片段必须能在该事项的冻结来源中找到。</p>
      {base.correction.items.map((entry, index) => {
        const key = itemKey(entry.pack_id, entry.item.item_id), item = entry.item;
        return <details key={key}><summary>事项 {index + 1}：{item.text}</summary>
          <p><strong>原事项：</strong>{item.text}</p>
          <ItemFacts item={item} />
          <ItemFields index={index + 1} item={item} patch={patchOf(working.edits[key])} locked={locked} change={patch => {
            setWorking(current => {
              if (!current) return current;
              const value = { ...patchOf(current.edits[key]), ...patch };
              const changed = Object.fromEntries(Object.entries(value).filter(([field, content]) =>
                JSON.stringify(content ?? null) !== JSON.stringify(item[field as keyof typeof item] ?? null)));
              const edits = { ...current.edits };
              if (Object.keys(changed).length) edits[key] = changed; else delete edits[key];
              return { ...current, edits };
            });
          }} />
        </details>;
      })}
      <label>纠正理由 <textarea aria-label="事项纠正理由" disabled={locked} maxLength={4000} value={working.reason} onChange={event => setWorking(current => current && { ...current, reason: event.target.value })} /></label>
      <button disabled={disabled && !pending || saving || (pending ? pending.kind !== "correct" : newer || conflict || !corrections.length || !working.reason.trim() || corrections.some(value => !validFields(value)))} onClick={() => void submit("correct")}>{pending?.kind === "correct" ? "确认同一次事项纠正请求" : "提交事项纠正并重新审核"}</button></>}
    </>}
  </section>;
}
