import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../../api";
import { savedWordReviewApi, type SavedWordReview as Review, type SavedWordReviewBody, type SavedWordVersion } from "../api/savedWordReview";
import { executionError, tenderApi } from "../api/workbench";
import { TaskProgress } from "../TaskProgress";
import { inProgress as running } from "../executionStatus";

type ReviewFlight = { version: SavedWordVersion; body: SavedWordReviewBody; attempt: MutationAttempt };
const sameVersion = (a: SavedWordVersion | null, b: SavedWordVersion | null) => a?.version_id === b?.version_id && a?.docx_sha256 === b?.docx_sha256;

export function SavedWordReview({ projectId, version, disabled, readOnly = false, onUnsafeChange }: {
  projectId: string; version: SavedWordVersion | null; disabled: boolean; readOnly?: boolean; onUnsafeChange?(unsafe: boolean): void;
}) {
  const storageKey = `kb.saved-word-review:${projectId}`;
  const [restored] = useState<ReviewFlight | null>(() => {
    try {
      const value = JSON.parse(sessionStorage.getItem(storageKey) ?? "null") as ReviewFlight | null;
      return value && typeof value.version?.version_id === "string" && /^[a-f0-9]{64}$/.test(value.version.docx_sha256)
        && typeof value.version.revision === "number" && value.body?.docx_sha256 === value.version.docx_sha256
        && typeof value.body.reason === "string" && value.body.reason.trim() && typeof value.attempt?.idempotencyKey === "string" ? value : null;
    } catch { return null; }
  });
  const [basis, setBasis] = useState<SavedWordVersion | null>(restored?.version ?? version);
  const [pending, setPending] = useState<ReviewFlight | null>(restored);
  const [reason, setReason] = useState(restored?.body.reason ?? "");
  const [review, setReview] = useState<Review | null>(null);
  const [readKnown, setReadKnown] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const actionAttempts = useRef(new Map<string, MutationAttempt>());
  const readEpoch = useRef(0);
  const unsafe = !pending && !!reason.trim();
  useEffect(() => { onUnsafeChange?.(unsafe); }, [unsafe, onUnsafeChange]);
  useEffect(() => () => onUnsafeChange?.(false), [onUnsafeChange]);
  useEffect(() => { if (!basis && version) setBasis(version); }, [basis, version]);
  const read = useCallback(async (signal?: AbortSignal) => {
    if (!basis) return;
    const epoch = ++readEpoch.current;
    try {
      const result = await savedWordReviewApi.read(projectId, basis.version_id, signal);
      if (signal?.aborted || epoch !== readEpoch.current) return;
      if (result && (result.operation !== "saved_review" || result.version_id !== basis.version_id || result.docx_sha256 !== basis.docx_sha256)) {
        setReadError("复核结果与所选 Word 版本不一致，已停止展示，请重试读取。"); return;
      }
      setReview(result); setReadKnown(true); setReadError(null);
    } catch (error) { if (!signal?.aborted && epoch === readEpoch.current) setReadError(error instanceof Error ? error.message : "复核状态读取失败，请重试。"); }
  }, [projectId, basis]);
  useEffect(() => {
    const abort = new AbortController(); let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { await read(abort.signal); if (!abort.signal.aborted) timer = setTimeout(() => void poll(), 5000); };
    void poll(); return () => { abort.abort(); clearTimeout(timer); };
  }, [read]);
  async function submit() {
    if (disabled || busy || !basis || !pending && (readOnly || !reason.trim())) return;
    const flight = pending ?? { version: basis, body: { docx_sha256: basis.docx_sha256, reason: reason.trim() }, attempt: createMutationAttempt() };
    try { sessionStorage.setItem(storageKey, JSON.stringify(flight)); }
    catch { setNotice("无法保存复核请求恢复信息，本次尚未发送。请检查浏览器存储后重试。"); return; }
    readEpoch.current++; setPending(flight); setBusy(true); setNotice(null);
    try {
      const result = await savedWordReviewApi.start(projectId, flight.version.version_id, flight.body, flight.attempt);
      if (!result || result.operation !== "saved_review" || result.version_id !== flight.version.version_id || result.docx_sha256 !== flight.version.docx_sha256) {
        setNotice("复核提交结果与请求版本不一致，请确认同一次请求；原版本、理由与请求标识已保留。"); return;
      }
      readEpoch.current++; setReview(result); setReadKnown(true); setReadError(null); setReason("");
      sessionStorage.removeItem(storageKey); setPending(null);
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        sessionStorage.removeItem(storageKey); setPending(null); setNotice(`复核尚未提交：${error.message}`);
      } else setNotice("复核提交结果尚未确认，请确认同一次请求。原 Word 版本与理由已保留。");
    } finally { setBusy(false); }
  }
  async function action(kind: "resume" | "cancel") {
    if (!review || busy || disabled || readOnly && kind === "resume") return;
    const key = `${review.execution_id}:${kind}`;
    const attempt = actionAttempts.current.get(key) ?? createMutationAttempt();
    actionAttempts.current.set(key, attempt); readEpoch.current++; setBusy(true); setNotice(null);
    try {
      await tenderApi.execution(projectId, review.execution_id, kind, attempt);
      actionAttempts.current.delete(key); await read();
    } catch (error) { setNotice(error instanceof Error ? error.message : "操作结果尚未确认，请重试同一操作。"); }
    finally { setBusy(false); }
  }
  const report = review?.report;
  const reportMatches = !!report && report.version_id === basis?.version_id && report.docx_sha256 === basis?.docx_sha256;
  const complete = review?.status === "succeeded" && !!report && report.record_count > 0 && report.reviewed_record_count === report.record_count && !report.gaps.length;
  return <section className="tender-info-card" aria-label="已保存 Word 只读复核">
    <h3>已保存 Word 只读复核</h3>
    <p className="tender-note">读取明确版本的已保存 Word 正文，核对内容本身。该复核不改写 Word，也不自动生成或载入新稿。</p>
    {basis ? <p>复核版本：Word v{basis.revision} <code>{basis.version_id}</code></p> : <p className="tender-note">请先保存 Word，形成明确版本后再复核。</p>}
    {basis && version && !sameVersion(basis, version) && <div className="tender-context-warning">当前 Word 已更新为 v{version.revision}，以下仍对应 v{basis.revision}，未自动复核新版本。<button disabled={busy || !!pending || unsafe} onClick={() => { readEpoch.current++; setBasis(version); setReview(null); setReadKnown(false); setReadError(null); setNotice(null); }}>切换到当前保存版本</button></div>}
    {pending && <p className="tender-note">待确认请求固定于 Word v{pending.version.revision}，确认时继续使用原版本、理由与请求标识。</p>}
    {notice && <p className="tender-alert" role="alert">{notice}</p>}
    {readError && <p className="tender-alert" role="alert">复核状态读取失败：{readError} {review ? "已保留上次读取结果。" : "未读取到可确认的复核结果。"}<button disabled={busy} onClick={() => void read()}>重试读取复核状态</button></p>}
    {basis && !readKnown && !readError && <p role="status">正在读取该版本的复核状态…</p>}
    {basis && readKnown && !review && <p className="tender-note">{pending ? "尚未读取到该版本的任务，请确认原请求，避免重复创建。" : "该版本尚无复核任务，不能据此判断是否存在问题。"}</p>}
    <label>保存稿复核理由<textarea aria-label="保存稿复核理由" rows={3} value={reason} disabled={disabled || readOnly || busy || !!pending || !basis} onChange={event => setReason(event.target.value)} /></label>
    <button disabled={disabled || busy || !basis || !pending && (readOnly || !reason.trim() || running(review?.status))} onClick={() => void submit()}>{pending ? "确认同一次保存稿复核请求" : "复核此保存版本"}</button>
    {!!reason.trim() && !pending && <button disabled={busy} onClick={() => { if (window.confirm("放弃尚未提交的保存稿复核理由？")) setReason(""); }}>放弃保存稿复核理由</button>}
    {review && <div className="tender-gap-list"><p role="status">保存稿复核：{review.status === "published" ? "排队中" : running(review.status) ? "执行中" : ({ succeeded: "已结束", failed: "失败", blocked: "待恢复", cancelled: "已取消" }[review.status] ?? review.status)}</p>
      {review.progress && <TaskProgress label="保存稿复核任务" status={review.status} progress={review.progress} />}
      {review.analysis_identity ? <p className="tender-note">固定复核依据：原分析 <code>{review.analysis_identity.run_id}</code>，来源版本 {review.analysis_identity.source_version}，分析修订 {review.analysis_identity.domain_revision}。</p> : <p className="tender-alert">此历史任务未提供可核对的原始分析身份，不能据此确认已覆盖当前要求。</p>}
      {review.sources_current === false ? <p className="tender-alert" role="alert">项目来源已更新。此任务仍按生成该 Word 时的原始分析依据复核，未覆盖后续上传的补遗或资料；请核对新资料后重新分析和编制。</p> : review.sources_current === true ? <p className="tender-note">最近一次读取时，项目来源与该固定复核依据一致。</p> : <p className="tender-alert">{review.source_status_issue?.detail ?? "尚未确认当前项目来源是否与固定复核依据一致，请等待状态刷新后核对。"}</p>}
      {executionError(review.error) && <p role="alert">{executionError(review.error)}</p>}
      {running(review.status) ? <button disabled={disabled || busy || !!pending} onClick={() => void action("cancel")}>取消保存稿复核</button> : ["failed", "blocked"].includes(review.status) && <button disabled={disabled || readOnly || busy || !!pending} onClick={() => void action("resume")}>继续保存稿复核</button>}
      {review.usage && <p className="tender-note">模型请求 {review.usage.sent_requests} 次，重试 {review.usage.retry_requests} 次，未知用量 {review.usage.unknown_usage_requests} 次。</p>}
      {!report && review.status === "succeeded" && <p className="tender-alert" role="alert">任务已结束，但复核报告尚未读取。请重试状态读取，不能视为无问题。</p>}
      {report && !reportMatches && <p className="tender-alert" role="alert">报告版本与所选 Word 不一致，未展示报告内容。</p>}
      {reportMatches && report && <><h4>Word v{basis?.revision} 的复核报告</h4><p>复核理由：{report.reason}</p><p>已复核 {report.reviewed_record_count} / {report.record_count} 段可读取正文。</p>
        {report.record_count === 0 ? <p className="tender-alert" role="alert">未读到可复核正文，不能据此认定无问题。</p> : report.reviewed_record_count < report.record_count && <p className="tender-alert" role="alert">仍有 {report.record_count - report.reviewed_record_count} 段正文尚未完成复核。</p>}
        {!!report.gaps.length && <div><h4>读取或复核缺口</h4>{report.gaps.map((gap, index) => <p key={`${gap.code}:${index}`}>{gap.detail}</p>)}</div>}
        {report.findings.length ? report.findings.map((finding, index) => <article key={`${finding.code}:${index}`}><p>{finding.detail}</p>{!!finding.task_ids.length && <p className="tender-note">位置：{finding.task_ids.map(id => report.records.find(record => record.task_id === id)?.title ?? "未能关联的正文位置").join("、")}</p>}{finding.suggestion && <p>建议：{finding.suggestion}</p>}</article>) : <p>{complete ? "已复核范围内未报告问题。请继续人工核对适用范围与业务事实。" : "当前已返回结果中未报告问题；仍需处理上方缺口，不能视为完整复核。"}</p>}
      </>}
    </div>}
  </section>;
}
