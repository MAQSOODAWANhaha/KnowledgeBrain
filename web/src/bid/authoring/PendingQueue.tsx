import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { VStack } from "@astryxdesign/core/VStack";
import { useEffect, useState } from "react";
import { tenderApi, type PendingGapStatus, type PendingQueueItem } from "../api/workbench";
import { formatDurationMs, groupPendingByLocation, PENDING_STATUS_LABELS, summarizePendingItems } from "./pendingQueue";
import { authoringHref } from "./routes";

const STATUS_BADGE: Record<PendingGapStatus, "warning" | "success" | "info" | "neutral"> = {
  pending: "warning",
  resolved: "success",
  need_material: "info",
  accepted_gap: "neutral",
};

export function PendingQueue({
  projectId,
  executionId,
  items,
  readOnly,
  draftNotes: controlledDraftNotes,
  onDraftNotesChange,
  onLocate,
  onUpdated,
  onUnsafeChange,
}: {
  projectId: string;
  executionId: string | null;
  items: PendingQueueItem[];
  readOnly: boolean;
  draftNotes?: Record<string, string>;
  onDraftNotesChange?: (updater: (current: Record<string, string>) => Record<string, string>) => void;
  onLocate(bookmark?: string | null): void;
  onUpdated(): Promise<void> | void;
  onUnsafeChange?(unsafe: boolean): void;
}) {
  const [filter, setFilter] = useState<"all" | PendingGapStatus>("all");
  const [internalNotes, setInternalNotes] = useState<Record<string, string>>({});
  const [savingId, setSavingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const draftNotes = controlledDraftNotes ?? internalNotes;
  const updateDraftNotes = onDraftNotesChange ?? setInternalNotes;

  const itemNotes = new Map(items.map(entry => [entry.gap_id, entry.note]));
  const hasUnsavedNote = Object.entries(draftNotes).some(
    ([gapId, draft]) => itemNotes.has(gapId) && draft !== itemNotes.get(gapId),
  );
  useEffect(() => {
    onUnsafeChange?.((controlledDraftNotes ? false : hasUnsavedNote) || savingId !== null);
    return () => onUnsafeChange?.(false);
  }, [controlledDraftNotes, hasUnsavedNote, savingId, onUnsafeChange]);

  const summary = summarizePendingItems(items);
  const groups = groupPendingByLocation(items, filter);

  async function applyStatus(item: PendingQueueItem, status: PendingGapStatus) {
    if (!executionId || readOnly || savingId) return;
    const note = (draftNotes[item.gap_id] ?? item.note).trim();
    setSavingId(item.gap_id);
    setError(null);
    try {
      await tenderApi.updatePendingItem(projectId, executionId, item.gap_id, { status, note });
      updateDraftNotes(current => {
        const next = { ...current };
        delete next[item.gap_id];
        return next;
      });
      await onUpdated();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "待核状态更新失败，请重试。");
    } finally {
      setSavingId(null);
    }
  }

  return <VStack gap={3} role="region" aria-label="编制待核队列">
    <MetadataList label={{ width: 96 }}>
      <MetadataListItem label="队列总数">{summary.total} 项</MetadataListItem>
      <MetadataListItem label="待处理">{summary.pending} 项</MetadataListItem>
      <MetadataListItem label="已处理">{summary.by_status.resolved} 项</MetadataListItem>
      <MetadataListItem label="补资料">{summary.by_status.need_material} 项</MetadataListItem>
      <MetadataListItem label="接受空缺">{summary.by_status.accepted_gap} 项</MetadataListItem>
      <MetadataListItem label="平均处理时长">{formatDurationMs(summary.handling_duration_ms.avg)}</MetadataListItem>
    </MetadataList>
    <SegmentedControl label="筛选待核状态" size="sm" value={filter} onChange={value => setFilter(value as typeof filter)}>
      <SegmentedControlItem value="all" label={`全部 ${summary.total}`} />
      <SegmentedControlItem value="pending" label={`未处理 ${summary.pending}`} />
      <SegmentedControlItem value="resolved" label={`已处理 ${summary.by_status.resolved}`} />
      <SegmentedControlItem value="need_material" label={`补资料 ${summary.by_status.need_material}`} />
      <SegmentedControlItem value="accepted_gap" label={`接受空缺 ${summary.by_status.accepted_gap}`} />
    </SegmentedControl>
    {error && <Banner role="alert" status="error" title={error} />}
    {!executionId && <EmptyState isCompact title="尚无可用编制成品" description="生成正文后，这里会按响应位置列出全部待核事项。" />}
    {executionId && !items.length && <EmptyState isCompact title="当前没有待核事项" description="不代表已完成内容验收，请结合原件核对。" />}
    {executionId && !!items.length && !groups.length && <Text type="supporting">当前筛选状态下没有待核事项。</Text>}
    {groups.map(group => <VStack key={group.key} gap={2}>
      <HStack hAlign="between" vAlign="center" gap={2}>
        <VStack gap={0}>
          <Heading level={3}>{group.chapterTitle}</Heading>
          {group.targetPurpose && <Text type="supporting">响应位置：{group.targetPurpose}</Text>}
        </VStack>
        {group.bookmark && <Button size="sm" variant="ghost" label="定位响应位置" onClick={() => onLocate(group.bookmark)} />}
      </HStack>
      {group.items.map(item => {
        const noteValue = draftNotes[item.gap_id] ?? item.note;
        const disabled = readOnly || savingId === item.gap_id;
        return <Card key={item.gap_id} padding={3}><VStack gap={2}>
          <HStack hAlign="between" vAlign="center" gap={2}>
            <Text type="supporting">{item.code}</Text>
            <Badge variant={STATUS_BADGE[item.status]} label={PENDING_STATUS_LABELS[item.status]} />
          </HStack>
          <Text>{item.detail}</Text>
          {item.status !== "pending" && <Text type="supporting">处理耗时：{formatDurationMs(item.handling_duration_ms)}</Text>}
          <TextInput
            label="处理备注"
            placeholder="补充处理说明或待补资料说明（可选）"
            value={noteValue}
            isDisabled={disabled}
            onChange={value => updateDraftNotes(current => ({ ...current, [item.gap_id]: value }))}
          />
          <HStack gap={2} wrap="wrap">
            <Button size="sm" variant="secondary" label="标记已处理" isDisabled={disabled} onClick={() => void applyStatus(item, "resolved")} />
            <Button size="sm" variant="secondary" label="标记补资料" isDisabled={disabled} onClick={() => void applyStatus(item, "need_material")} />
            <Button size="sm" variant="secondary" label="接受空缺" isDisabled={disabled} onClick={() => void applyStatus(item, "accepted_gap")} />
            {item.status !== "pending" && <Button size="sm" variant="ghost" label="恢复待核" isDisabled={disabled} onClick={() => void applyStatus(item, "pending")} />}
          </HStack>
          {item.status === "need_material" && <Link href={`#${authoringHref(projectId, "files")}`}>到项目资料补充文件</Link>}
        </VStack></Card>;
      })}
    </VStack>)}
  </VStack>;
}
