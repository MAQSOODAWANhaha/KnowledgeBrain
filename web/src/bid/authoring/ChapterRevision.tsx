import { VStack } from "@astryxdesign/core/VStack";
import { Text } from "@astryxdesign/core/Text";
import { Section } from "@astryxdesign/core/Section";
import { HStack } from "@astryxdesign/core/HStack";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { Code } from "@astryxdesign/core/Code";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useEffect, useState } from "react";
import type { TenderChapter } from "../api/workbench";
import { reviewTargetIds } from "./outlineTree";

export type ChapterRevisionSource = { execution_id: string; label: string; chapters: TenderChapter[] };
export type ChapterRevisionRequest = { previous_execution_id: string; review_request: { target_ids: string[]; reason: string } };
type RevisionDraft = { source: ChapterRevisionSource; targetIds: string[]; reason: string };

export function ChapterRevision({ source, disabled, busy, pending, pendingRequest, manualWordRevision, onSubmit, onUnsafeChange }: {
  source: ChapterRevisionSource | null; disabled: boolean; busy: boolean; pending: boolean;
  pendingRequest?: ChapterRevisionRequest | null;
  manualWordRevision?: number | null;
  onSubmit(body?: ChapterRevisionRequest): Promise<boolean | void>; onUnsafeChange?(unsafe: boolean): void;
}) {
  const [draft, setDraft] = useState<RevisionDraft | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const basis = draft?.source ?? source;
  const frozen = pending ? pendingRequest : null;
  const selected = frozen?.review_request.target_ids ?? draft?.targetIds ?? [];
  const reason = frozen?.review_request.reason ?? draft?.reason ?? "";
  const changedSource = !!draft && draft.source.execution_id !== source?.execution_id;
  const frozenSourceDiffers = !!frozen && frozen.previous_execution_id !== basis?.execution_id;
  const locked = disabled || busy || pending || submitting || changedSource || !basis;
  const unsafe = !pending && (!!selected.length || !!reason.trim());
  useEffect(() => { onUnsafeChange?.(unsafe); }, [unsafe, onUnsafeChange]);
  useEffect(() => () => onUnsafeChange?.(false), [onUnsafeChange]);
  const change = (patch: Partial<Pick<RevisionDraft, "targetIds" | "reason">>) => {
    if (!basis || locked) return;
    setDraft(current => ({ source: current?.source ?? basis, targetIds: current?.targetIds ?? [], reason: current?.reason ?? "", ...patch }));
  };
  const toggle = (ids: string[]) => {
    const allSelected = ids.every(id => selected.includes(id));
    change({ targetIds: allSelected ? selected.filter(id => !ids.includes(id)) : [...new Set([...selected, ...ids])] });
  };
  async function submit() {
    if (submitting) return;
    setSubmitting(true);
    try {
      const body = pending ? undefined : basis && selected.length && reason.trim() ? {
        previous_execution_id: basis.execution_id,
        review_request: { target_ids: reviewTargetIds(basis.chapters).filter(id => selected.includes(id)), reason: reason.trim() },
      } : undefined;
      if (!pending && !body) return;
      if (await onSubmit(body) === true) setDraft(null);
    } finally { setSubmitting(false); }
  }
  const render = (chapters: TenderChapter[], depth = 0) => <VStack gap={1} paddingInlineStart={depth ? 4 : 0}>{chapters.map(chapter => {
    const ids = reviewTargetIds([chapter]);
    const count = ids.filter(id => selected.includes(id)).length;
    return <VStack key={chapter.chapter_id} gap={1}><CheckboxInput label={`复核章节：${chapter.title}`} description={ids.length ? `${count}/${ids.length} 项响应（含子章节）` : "没有可自动复核的响应"} isDisabled={locked || !ids.length} value={!!ids.length && count === ids.length} onChange={() => toggle(ids)} />
      {!!chapter.targets.length && <VStack gap={1} paddingInlineStart={4}>{chapter.targets.map(target => <CheckboxInput key={target.target_id} label={`复核响应：${target.purpose}`} description={target.manual_fill ? "需人工填写" : undefined} isDisabled={locked || !!target.manual_fill} value={selected.includes(target.target_id)} onChange={() => toggle([target.target_id])} />)}</VStack>}
      {!!chapter.children.length && render(chapter.children, depth + 1)}</VStack>;
  })}</VStack>;
  return <Section role="region" aria-label="定向复核与修订" padding={0} variant="transparent"><VStack gap={3}>
    <Text type="supporting">选择生成稿中的章节或响应位置，说明需要核对的问题。系统会基于该次生成结果形成新稿，并在复核需要时修订内容；执行范围可能包含其他章节。</Text>
    {frozen ? <Text>待确认请求的生成基准：{frozen.previous_execution_id === basis?.execution_id ? basis.label : "原生成稿（待确认请求）"}</Text> : basis ? <Text>复核基准：{basis.label}</Text> : <Text type="supporting">当前没有可复核的生成稿，请先选择已有生成稿。</Text>}
    {(frozen || basis) && <Collapsible trigger="查看生成稿标识" defaultIsOpen={false}><Text>{frozen ? "待确认请求的生成基准：" : "生成执行："}<Code>{frozen?.previous_execution_id ?? basis?.execution_id}</Code></Text></Collapsible>}
    {manualWordRevision != null && <Banner status="warning" title={`当前人工 Word v${manualWordRevision} 的编辑内容不作为本次复核输入。`} />}
    <Text type="supporting">现有 Word 稿继续保留，新稿生成后由你选择载入。</Text>
    <Text type="supporting">提交前请先保存并确认 Word 中的修改。</Text>
    {changedSource && <Banner role="alert" status="warning" title="生成稿已切换，原选择与理由仍保留。" endContent={<Button size="sm" isDisabled={disabled || busy || pending || !source} onClick={() => { if (window.confirm("载入当前生成稿会清空尚未提交的复核选择与理由，继续？")) setDraft(null); }} label="载入当前生成稿作为基准" />} />}
    {basis && !frozenSourceDiffers && render(basis.chapters)}
    {frozenSourceDiffers && <Text type="supporting">待确认请求已固定 {selected.length} 项原生成稿响应。当前稿件已变化，确认时继续使用原请求。</Text>}
    <TextArea label="复核与修订理由" rows={3} value={reason} isDisabled={locked} onChange={next => change({ reason: next })} />
    <Text type="supporting">已选择 {selected.length} 项响应。需人工填写的内容请在 Word 中处理。</Text>
    <HStack gap={2} wrap="wrap"><Button variant="primary" isDisabled={pending ? disabled || busy || submitting : locked || !selected.length || !reason.trim()} onClick={() => void submit()} label={pending ? "确认同一次定向复核请求" : "生成复核修订稿"} />
    {draft && <Button variant="ghost" isDisabled={busy || pending || submitting} onClick={() => { if (window.confirm("放弃尚未提交的复核选择与理由？")) setDraft(null); }} label="放弃本次复核选择" />}</HStack>
  </VStack></Section>;
}
