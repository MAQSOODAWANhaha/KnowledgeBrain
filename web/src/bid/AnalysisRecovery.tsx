import { Banner } from "@astryxdesign/core/Banner";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { Button } from "@astryxdesign/core/Button";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createMutationAttempt, type MutationAttempt } from "../api";
import { analysisRecoveryApi, type AnalysisRecoveryView, type AnalysisRetry, type FailureCategory } from "./api/analysisRecovery";

type Working = { base: AnalysisRecoveryView; reason: string };
type Flight = { body: AnalysisRetry; attempt: MutationAttempt };
const CATEGORY: Record<FailureCategory, string> = {
  uncertain: "模型响应中断，结果与费用未确认", infrastructure: "模型服务连接、超时或限流",
  configuration: "模型配置或鉴权错误，需管理员处理", semantic: "模型返回内容不合规", other: "其他模型调用错误",
};
const REASON: Record<string, string> = {
  model_work_unit_retries_exhausted: "同一工作单元已用尽重试。",
  infrastructure_retries_exhausted: "执行环境（发布、数据库或执行器）故障已用尽重试。",
  execution_config_changed: "执行配置（提示词、模型或预算）已变化，无法以新配置继续原分析修订。",
};
const identity = (value: AnalysisRecoveryView | null) => value && `${value.run_id}:${value.source_version}:${value.domain_revision}:${value.failed_command_id}`;

export function AnalysisRecovery({ projectId, disabled, readOnly = false, onChanged, onUnsafeChange }: {
  projectId: string; disabled: boolean; readOnly?: boolean; onChanged(): void; onUnsafeChange(unsafe: boolean): void;
}) {
  const storageKey = `kb.analysis-recovery:${projectId}`;
  const [restored] = useState<{ working: Working; pending: Flight | null } | null>(() => {
    try {
      const value = JSON.parse(sessionStorage.getItem(storageKey) ?? "null");
      return value?.working?.base?.run_id && typeof value.working.reason === "string" ? value : null;
    } catch { return null; }
  });
  const [working, setWorking] = useState<Working | null>(restored?.working ?? null);
  const [pending, setPending] = useState<Flight | null>(restored?.pending ?? null);
  const [remote, setRemote] = useState<AnalysisRecoveryView | null>(null);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [succeeded, setSucceeded] = useState(false);
  const [conflict, setConflict] = useState(false);
  const request = useRef(0);
  const read = useCallback(async (signal?: AbortSignal) => {
    const turn = ++request.current;
    try {
      const latest = await analysisRecoveryApi.read(projectId, signal);
      if (signal?.aborted || turn !== request.current) return;
      setRemote(latest);
      setWorking(current => current?.reason || pending ? current : latest ? { base: latest, reason: "" } : null);
    } catch (error) {
      if (!signal?.aborted && turn === request.current) setNotice(error instanceof Error ? error.message : "恢复状态读取失败，请重试。");
    }
  }, [projectId, pending]);
  useEffect(() => {
    const abort = new AbortController(); let timer: ReturnType<typeof setTimeout>;
    const poll = async () => { await read(abort.signal); if (!abort.signal.aborted) timer = setTimeout(() => void poll(), 5000); };
    void poll(); return () => { abort.abort(); clearTimeout(timer); };
  }, [read]);
  const dirty = !!working?.reason;
  useEffect(() => { onUnsafeChange(dirty || !!pending || saving); }, [dirty, pending, saving, onUnsafeChange]);
  useEffect(() => {
    try {
      if (working && (dirty || pending)) sessionStorage.setItem(storageKey, JSON.stringify({ working, pending }));
      else sessionStorage.removeItem(storageKey);
    } catch { setNotice("浏览器恢复信息未能保存，请保持页面打开并确认提交结果。"); }
  }, [working, dirty, pending, storageKey]);
  const base = working?.base;
  const changed = !!base && identity(base) !== identity(remote);
  const currentState = base && identity(base) === identity(remote) ? remote : base;
  async function retry() {
    if (disabled || readOnly && !pending || saving || !working) return;
    const base = working.base;
    if (!pending && (!currentState?.can_retry || !base.failed_command_id || !working.reason.trim() || changed || conflict)) return;
    const flight = pending ?? { attempt: createMutationAttempt(), body: { run_id: base.run_id,
      source_version: base.source_version, domain_revision: base.domain_revision,
      failed_command_id: base.failed_command_id!, reason: working.reason } };
    try { sessionStorage.setItem(storageKey, JSON.stringify({ working, pending: flight })); }
    catch { setNotice("无法保存恢复信息，本次请求尚未发送。请检查浏览器存储后重试。"); return; }
    request.current++; setPending(flight); setSaving(true); setNotice(null); setSucceeded(false);
    try {
      await analysisRecoveryApi.retry(projectId, flight.body, flight.attempt);
      sessionStorage.removeItem(storageKey); setPending(null); setWorking(null); setConflict(false);
      setNotice("恢复请求已提交，继续原分析任务。请等待任务状态更新。"); setSucceeded(true);
      onChanged(); await read();
    } catch (error) {
      if (error instanceof ApiError && error.status >= 400 && error.status < 500 && error.status !== 408 && error.status !== 429) {
        setPending(null); setConflict(error.status === 409);
        setNotice(`${error.message} 恢复理由已保留。`);
      } else setNotice("恢复请求结果尚未确认，理由已保留。请确认同一次恢复请求。");
    } finally { setSaving(false); }
  }
  if (!notice && !dirty && !pending && base?.status !== "failed") return null;
  return <VStack role="region" aria-label="分析任务恢复" gap={3}>
    <Heading level={3}>继续失败的分析</Heading>
    <Text type="supporting">继续原任务和原资料。未完成的模型调用仍可能产生费用。</Text>
    {notice && (succeeded ? <Banner role="status" status="success" title={notice} /> : <Banner role="alert" status="error" title={notice} />)}
    <HStack><Button variant="secondary" isDisabled={saving} onClick={() => void read()} label="刷新分析恢复状态" /></HStack>
    {base && working && <>
      <Text>来源版本 {base.source_version}，分析修订 {base.domain_revision}；已保存成功回包 {base.saved_responses} 份。</Text>
      {base.status === "failed" && <VStack role="group" aria-label="故障原因" gap={1}>
        <Text weight="semibold">故障原因</Text>
        {base.failure_reason && REASON[base.failure_reason] && <Text>{REASON[base.failure_reason]}</Text>}
        {base.failure_causes?.length
          ? base.failure_causes.map(cause => <Text key={cause.error_type}>{CATEGORY[cause.category] ?? CATEGORY.other}（{cause.error_type}）：{cause.count} 次</Text>)
          : <Text type="supporting">未记录模型调用错误；请在「阶段与请求回执」查看未通过业务验收的回包。</Text>}
      </VStack>}
      {currentState?.notice && currentState.blocker !== "not_failed" && (currentState.can_retry ? <Text type="supporting">{currentState.notice}</Text> : <Banner status="warning" title={currentState.notice} />)}
      {(changed || conflict) && <Banner role="alert" status="warning" title="失败记录或任务状态已有变化，当前恢复理由仍保留。请刷新并核对原任务状态。"
        description={remote ? `最新来源版本 ${remote.source_version}，分析修订 ${remote.domain_revision}。${remote.notice ?? ""}` : undefined}
        endContent={<Button size="sm" isDisabled={saving || !!pending || !remote?.can_retry} onClick={() => {
          if (remote) { setWorking({ base: remote, reason: working.reason }); setConflict(false); setNotice("已载入最新失败记录，恢复理由保留；请核对后提交。"); }
        }} label="按最新失败记录核对，保留理由" />} />}
      <TextArea label="分析恢复理由" isRequired description="填写核对过的故障原因和处理情况后才能继续。" htmlMaxLength={4000} isDisabled={disabled || readOnly || saving || !!pending} value={working.reason} onChange={reason => setWorking(current => current && { ...current, reason })} />
      <HStack gap={2} wrap="wrap">
        <Button variant="primary" isDisabled={disabled || saving || (!pending && (readOnly || !currentState?.can_retry || !working.reason.trim() || changed || conflict))} onClick={() => void retry()} label={pending ? "确认同一次恢复请求" : "继续原分析任务"} />
        {dirty && !pending && <Button variant="ghost" isDisabled={saving} onClick={() => {
          if (window.confirm("放弃当前未提交的恢复理由？")) { setWorking(remote ? { base: remote, reason: "" } : null); setConflict(false); setNotice(null); }
        }} label="放弃恢复理由" />}
      </HStack>
    </>}
  </VStack>;
}
