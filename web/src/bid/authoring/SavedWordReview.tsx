import { VStack } from "@astryxdesign/core/VStack";
import { Text } from "@astryxdesign/core/Text";
import { HStack } from "@astryxdesign/core/HStack";
import { Heading } from "@astryxdesign/core/Heading";
import { Code } from "@astryxdesign/core/Code";
import { Card } from "@astryxdesign/core/Card";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { TextArea } from "@astryxdesign/core/TextArea";
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
  const statusLabel = (status: string) => status === "published" ? "排队中" : running(status) ? "执行中" : ({ succeeded: "已结束", failed: "失败", blocked: "待恢复", cancelled: "已取消" }[status] ?? status);
  return <VStack role="region" aria-label="已保存 Word 只读复核" gap={3}>
    <Text type="supporting">读取明确版本的已保存 Word 正文，核对内容本身。该复核不改写 Word，也不自动生成或载入新稿。</Text>
    {basis ? <Text>复核版本：Word v{basis.revision} <Code>{basis.version_id}</Code></Text> : <Text type="supporting">请先保存 Word，形成明确版本后再复核。</Text>}
    {basis && version && !sameVersion(basis, version) && <Banner status="warning" title={`当前 Word 已更新为 v${version.revision}，以下仍对应 v${basis.revision}，未自动复核新版本。`}
      endContent={<Button size="sm" isDisabled={busy || !!pending || unsafe} onClick={() => { readEpoch.current++; setBasis(version); setReview(null); setReadKnown(false); setReadError(null); setNotice(null); }} label="切换到当前保存版本" />} />}
    {pending && <Text type="supporting">待确认请求固定于 Word v{pending.version.revision}，确认时继续使用原版本、理由与请求标识。</Text>}
    {notice && <Banner role="alert" status="error" title={notice} />}
    {readError && <Banner role="alert" status="error" title={`复核状态读取失败：${readError} ${review ? "已保留上次读取结果。" : "未读取到可确认的复核结果。"}`} endContent={<Button size="sm" isDisabled={busy} onClick={() => void read()} label="重试读取复核状态" />} />}
    {basis && !readKnown && !readError && <Text role="status" type="supporting">正在读取该版本的复核状态…</Text>}
    {basis && readKnown && !review && <Text type="supporting">{pending ? "尚未读取到该版本的任务，请确认原请求，避免重复创建。" : "该版本尚无复核任务，不能据此判断是否存在问题。"}</Text>}
    <TextArea label="保存稿复核理由" rows={3} value={reason} isDisabled={disabled || readOnly || busy || !!pending || !basis} onChange={setReason} />
    <HStack gap={2} wrap="wrap">
      <Button variant="primary" isDisabled={disabled || busy || !basis || !pending && (readOnly || !reason.trim() || running(review?.status))} onClick={() => void submit()} label={pending ? "确认同一次保存稿复核请求" : "复核此保存版本"} />
      {!!reason.trim() && !pending && <Button variant="ghost" isDisabled={busy} onClick={() => { if (window.confirm("放弃尚未提交的保存稿复核理由？")) setReason(""); }} label="放弃保存稿复核理由" />}
    </HStack>
    {review && <VStack gap={2}>
      <Text role="status" weight="semibold">保存稿复核：{statusLabel(review.status)}</Text>
      {review.progress && <TaskProgress label="保存稿复核任务" status={review.status} progress={review.progress} />}
      {review.analysis_identity
        ? <Text type="supporting">固定复核依据：原分析 <Code>{review.analysis_identity.run_id}</Code>，来源版本 {review.analysis_identity.source_version}，分析修订 {review.analysis_identity.domain_revision}。</Text>
        : <Banner status="warning" title="此历史任务未提供可核对的原始分析身份，不能据此确认已覆盖当前要求。" />}
      {review.sources_current === false ? <Banner role="alert" status="warning" title="项目来源已更新。此任务仍按生成该 Word 时的原始分析依据复核，未覆盖后续上传的补遗或资料；请核对新资料后重新分析和编制。" />
        : review.sources_current === true ? <Text type="supporting">最近一次读取时，项目来源与该固定复核依据一致。</Text>
        : <Banner status="warning" title={review.source_status_issue?.detail ?? "尚未确认当前项目来源是否与固定复核依据一致，请等待状态刷新后核对。"} />}
      {executionError(review.error) && <Banner role="alert" status="error" title={executionError(review.error)} />}
      {running(review.status) ? <HStack><Button variant="secondary" isDisabled={disabled || busy || !!pending} onClick={() => void action("cancel")} label="取消保存稿复核" /></HStack>
        : ["failed", "blocked"].includes(review.status) && <HStack><Button variant="secondary" isDisabled={disabled || readOnly || busy || !!pending} onClick={() => void action("resume")} label="继续保存稿复核" /></HStack>}
      {review.usage && <Text type="supporting">模型请求 {review.usage.sent_requests} 次，重试 {review.usage.retry_requests} 次，未知用量 {review.usage.unknown_usage_requests} 次。</Text>}
      {!report && review.status === "succeeded" && <Banner role="alert" status="error" title="任务已结束，但复核报告尚未读取。请重试状态读取，不能视为无问题。" />}
      {report && !reportMatches && <Banner role="alert" status="error" title="报告版本与所选 Word 不一致，未展示报告内容。" />}
      {reportMatches && report && <VStack gap={2}>
        <Heading level={4}>Word v{basis?.revision} 的复核报告</Heading>
        <Text>复核理由：{report.reason}</Text>
        <Text>已复核 {report.reviewed_record_count} / {report.record_count} 段可读取正文。</Text>
        {report.record_count === 0 ? <Banner role="alert" status="error" title="未读到可复核正文，不能据此认定无问题。" />
          : report.reviewed_record_count < report.record_count && <Banner role="alert" status="warning" title={`仍有 ${report.record_count - report.reviewed_record_count} 段正文尚未完成复核。`} />}
        {!!report.gaps.length && <VStack gap={1}><Heading level={4}>读取或复核缺口</Heading>{report.gaps.map((gap, index) => <Text key={`${gap.code}:${index}`}>{gap.detail}</Text>)}</VStack>}
        {report.findings.length ? report.findings.map((finding, index) => <Card key={`${finding.code}:${index}`} padding={3} role="article"><VStack gap={1}>
          <Text>{finding.detail}</Text>
          {!!finding.task_ids.length && <Text type="supporting">位置：{finding.task_ids.map(id => report.records.find(record => record.task_id === id)?.title ?? "未能关联的正文位置").join("、")}</Text>}
          {finding.suggestion && <Text>建议：{finding.suggestion}</Text>}
        </VStack></Card>) : <Text>{complete ? "已复核范围内未报告问题。请继续人工核对适用范围与业务事实。" : "当前已返回结果中未报告问题；仍需处理上方缺口，不能视为完整复核。"}</Text>}
      </VStack>}
    </VStack>}
  </VStack>;
}
