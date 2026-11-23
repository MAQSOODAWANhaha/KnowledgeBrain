import { pixel, Table } from "@astryxdesign/core/Table";
import { Pagination } from "@astryxdesign/core/Pagination";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Toolbar } from "@astryxdesign/core/Toolbar";
import { Link } from "@astryxdesign/core/Link";
import { Item } from "@astryxdesign/core/Item";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { Card } from "@astryxdesign/core/Card";
import { Blockquote } from "@astryxdesign/core/Blockquote";
import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/HStack";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Banner } from "@astryxdesign/core/Banner";
import { Tab, TabList } from "@astryxdesign/core/TabList";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { TreeList, type TreeListItemData } from "@astryxdesign/core/TreeList";
import { VStack } from "@astryxdesign/core/VStack";
import { useState } from "react";
import { executionError, type TenderChapter, type TenderIssue, type TenderItem, type WorkbenchView } from "./api/workbench";
import { authoringHref } from "./authoring/routes";
import { inProgress } from "./executionStatus";
import { TaskProgress } from "./TaskProgress";

export function flattenChapters(chapters: TenderChapter[]): TenderChapter[] {
  return chapters.flatMap(chapter => [chapter, ...flattenChapters(chapter.children ?? [])]);
}

export function ChapterTree({ chapters, selectedId, gaps, onSelect, onLocate }: {
  chapters: TenderChapter[]; selectedId: string | null; gaps: TenderIssue[]; onSelect(chapter: TenderChapter): void;
  onLocate?(bookmark?: string | null): void;
}) {
  const [query, setQuery] = useState("");
  const needle = query.trim().toLocaleLowerCase();
  const matches = (chapter: TenderChapter): boolean => chapter.title.toLocaleLowerCase().includes(needle) || (chapter.children ?? []).some(matches);
  const items = (nodes: TenderChapter[]): TreeListItemData[] => nodes.filter(matches).map(chapter => {
    const count = gaps.filter(gap => gap.chapter_id === chapter.chapter_id).length;
    const children = chapter.children?.length ? items(chapter.children) : undefined;
    return {
      id: chapter.chapter_id,
      label: `${chapter.is_volume ? "分册 " : ""}${chapter.title}${chapter.manual ? " · 人工新增" : ""}`,
      isSelected: selectedId === chapter.chapter_id,
      isExpanded: true,
      onClick: () => onSelect(chapter),
      endContent: count > 0 ? <Badge variant="warning" label={String(count)} aria-label={`${count} 条待补记录`} /> : undefined,
      children,
    };
  });
  const selected = flattenChapters(chapters).find(chapter => chapter.chapter_id === selectedId);
  const anchors = (selected?.targets ?? []).filter(target => target.kind !== "narrative");
  const tree = items(chapters);
  return <VStack gap={2}>
    <TextInput label="搜索章节" isLabelHidden type="search" placeholder="搜索章节…" value={query} onChange={setQuery} />
    {!chapters.length ? <Text type="supporting">尚无可读取的文件结构。</Text>
      : !tree.length ? <Text type="supporting">没有匹配章节。</Text>
      : <TreeList key={needle} aria-label="章节目录" density="compact" variant="noGuides" items={tree} />}
    {!!anchors.length && <VStack gap={1} role="group" aria-label="本章定位">
      <Text type="supporting">本章表单与附件</Text>
      {anchors.map(target => <Button key={target.target_id} size="sm" variant="ghost" onClick={() => onLocate?.(target.bookmark)} label={`${target.kind === "form" ? "表单：" : "附件："}${target.purpose}`} />)}
    </VStack>}
    <Text type="supporting">目录与待补标记用于定位，章节存在不代表要求已经满足。</Text>
  </VStack>;
}

function ItemContent({ item }: { item: TenderItem }) {
  const facts = [
    item.subject && `主体：${item.subject}`,
    item.scope && `适用范围：${item.scope}`,
    item.conditions?.length && `条件：${item.conditions.join("；")}`,
    item.exceptions?.length && `例外：${item.exceptions.join("；")}`,
  ].filter((fact): fact is string => !!fact);
  const details = [
    ...(item.numbers ?? []).map(number => `量化事实：${number.value} ${number.unit}（${number.quote}）`),
    item.scoring?.method && `评分方法：${item.scoring.method}`,
    item.scoring?.points !== undefined && `分值：${item.scoring.points}`,
    item.scoring?.max_points !== undefined && `最高分：${item.scoring.max_points}`,
    item.consequence?.effect && `后果：${item.consequence.effect}`,
    item.consequence?.trigger && `触发条件：${item.consequence.trigger}`,
    item.submission?.fields?.length && `填写字段：${item.submission.fields.join("；")}`,
    item.submission?.documents?.length && `提交材料：${item.submission.documents.join("；")}`,
  ].filter((line): line is string => !!line);
  return <VStack gap={1}>
    <Text>{item.text}</Text>
    {facts.map(fact => <Text key={fact} type="supporting">{fact}</Text>)}
    {!!details.length && <VStack gap={0.5}>{details.map((line, index) => <Text key={index}>{line}</Text>)}</VStack>}
    {!!item.evidence?.length && <Collapsible trigger="原文引文与位置" defaultIsOpen={false}><VStack gap={2}>
      {item.evidence.map((evidence, index) => <Blockquote key={`${evidence.range_id}:${index}`} cite={`来源位置：${evidence.range_id}`}>{evidence.quote}</Blockquote>)}
    </VStack></Collapsible>}
  </VStack>;
}

const TARGET_KIND = { form: "规定表单", attachment: "证明附件" } as Record<string, string>;

export function ChapterInspector({ chapter, view, draftId, contextMatches = true, onLocate, onDownload }: {
  chapter: TenderChapter | null; view: WorkbenchView; draftId?: string; contextMatches?: boolean;
  onLocate(bookmark?: string | null): void;
  onDownload(url: string, name: string): Promise<void>;
}) {
  const [tab, setTab] = useState<"requirements" | "supports" | "gaps">("requirements");
  const targets = chapter?.targets ?? [];
  const ids = new Set(targets.map(target => target.target_id));
  const boundRules = view.requirements?.rules.filter(rule => rule.target_ids.some(id => ids.has(id))) ?? [];
  const knownChapters = new Set(flattenChapters(view.current_docx?.outline?.chapters ?? []).map(item => item.chapter_id));
  const gaps = view.gaps.filter(gap => !gap.chapter_id || gap.chapter_id === chapter?.chapter_id || draftId && !knownChapters.has(gap.chapter_id));
  const units = (view.units ?? []).filter(unit => unit.chapter_id === chapter?.chapter_id);
  const adoptedIds = new Set(units.flatMap(unit => [...unit.attachments.map(item => item.material_id), ...[...unit.blocks, ...unit.cell_fills].flatMap(item => item.material_ids ?? [])]));
  const materials = (view.materials ?? []).filter(material => adoptedIds.has(material.material_id));
  return <VStack gap={0}>
    <TabList aria-label="章节信息" size="sm" value={tab} onChange={value => setTab(value as typeof tab)}>
      <Tab value="requirements" label="要求" /><Tab value="supports" label="依据" /><Tab value="gaps" label={gaps.length ? `待补 ${gaps.length}` : "待补"} />
    </TabList>
    <VStack gap={2} padding={3}>
      <Text type="supporting">当前选定章节：{chapter?.title ?? "尚未选择"}</Text>
      {!contextMatches ? <Banner role="status" status="warning" title="当前业务记录尚未与这份稿件关联，保留 Word 供阅读，请刷新后核对。" />
        : !chapter ? <Text type="supporting">选择章节查看相关信息。</Text> : <>
        {draftId && <Text type="supporting">以下记录来自本稿的编制快照，后续人工修改仍需核对。</Text>}
        {tab === "requirements" && <>
          {boundRules.length ? boundRules.map((binding, index) => <Card key={index} padding={3}><VStack gap={1}>
            {binding.rule ? <ItemContent item={binding.rule} /> : <Text>关联要求暂不可读取，正式引用仍保留，需核对来源。</Text>}
            {binding.rule_ref.decision_id && <Text type="supporting">显示修订后保留的适用规则。</Text>}
          </VStack></Card>) : <Text type="supporting">当前没有可读取的关联要求；这不表示本章没有招标要求。</Text>}
          {targets.map(target => <Card key={target.target_id} padding={3}>
            <Item density="compact" label={target.purpose} description={`${TARGET_KIND[target.kind] ?? "正文响应"}${target.manual_fill ? " · 人工填写" : ""}`} />
          </Card>)}
        </>}
        {tab === "supports" && <>
          {targets.filter(target => target.source_refs?.length).map(target => <Card key={target.target_id} padding={3}><VStack gap={1}>
            <Text weight="semibold">招标来源 · {target.purpose}</Text>
            {target.source_refs?.map((source, index) => <Text key={`${source.unit_id}:${index}`} type="supporting">来源位置：{source.unit_id}</Text>)}
          </VStack></Card>)}
          {materials.map(material => <Card key={material.material_id} padding={3}><VStack gap={2}>
            <Text weight="semibold">采用资料 · {material.title}</Text>
            {material.text && <Collapsible trigger="查看资料原文" defaultIsOpen={false}><Text>{material.text}</Text></Collapsible>}
            {material.original_url && <HStack><Button size="sm" variant="secondary" onClick={() => void onDownload(material.original_url!, material.title)} label="下载证明原件" /></HStack>}
          </VStack></Card>)}
          {!materials.length && <Text type="supporting">当前尚无可读取的已采用企业资料，不能据此认定资料不存在。</Text>}
          <Text type="supporting">真实证明须沿用原件。来源引用和检索命中均需核对其适用性。</Text>
        </>}
        {tab === "gaps" && <>
          {gaps.length ? gaps.map((gap, index) => {
            const target = targets.find(item => item.target_id === gap.target_id);
            const sameChapter = gap.chapter_id === chapter.chapter_id || !!target;
            return <Banner key={`${gap.code}:${index}`} status="warning" title={gap.detail}
              description={!sameChapter && gap.chapter_id ? "原章节位置需要重新关联。" : undefined}
              endContent={sameChapter ? <Button size="sm" onClick={() => onLocate(target?.bookmark ?? chapter.bookmark)} label={target?.bookmark ? "定位响应位置" : "定位所属章节"} /> : undefined} />;
          }) : <Text type="supporting">当前没有可读取的待补记录，不代表已经完成内容验收。</Text>}
          <Link href={`#${authoringHref(view.project.id, "files")}`}>到项目资料补充文件</Link>
          <Text type="supporting">补充资料后另存新稿。人工写入内容不自动清除原复核意见。</Text>
        </>}
      </>}
    </VStack>
  </VStack>;
}

export const kindLabels: Record<string, string> = { obligation: "要求", requirement: "要求", scoring_rule: "评分", consequence_rule: "后果", submission_requirement: "提交要求", fact: "背景事实" };
const kindOf = (item: TenderItem) => kindLabels[item.kind ?? ""] ?? item.kind ?? "未分类";
const phasePrefix = /^(register|freeze|ocr|pack-plan|extract|analysis-review|analysis-locate|analysis-associate|analysis-joint|verify|confirm)\s+/;
function groupedIssues(issues: { detail: string }[]) {
  const counts = new Map<string, number>();
  for (const issue of issues) {
    const text = issue.detail.replace(phasePrefix, "");
    counts.set(text, (counts.get(text) ?? 0) + 1);
  }
  return [...counts.entries()];
}

export type Placement = { chapters: string[]; gap: boolean };
// Where each item lands in the outline: chapters whose targets cite it, and whether any of those targets has a gap.
export function itemPlacements(view: WorkbenchView): Map<string, Placement> {
  const gapTargets = new Set(view.gaps.map(gap => gap.target_id).filter((id): id is string => !!id));
  const result = new Map<string, Placement>();
  for (const chapter of flattenChapters(view.outline?.chapters ?? [])) {
    for (const target of chapter.targets) {
      for (const ref of target.rule_refs ?? []) {
        const entry = result.get(ref.item_ref.item_id) ?? { chapters: [], gap: false };
        if (!entry.chapters.includes(chapter.title)) entry.chapters.push(chapter.title);
        entry.gap ||= gapTargets.has(target.target_id);
        result.set(ref.item_ref.item_id, entry);
      }
    }
  }
  return result;
}
export const itemKey = (item: TenderItem, index: number) => item.item_id ?? `index-${index}`;
function checkBadge(placement?: Placement) {
  if (!placement) return <Badge variant="neutral" label="未落位" />;
  return placement.gap ? <Badge variant="warning" label="待补" /> : <Badge variant="success" label="已落位" />;
}
export function analysisTabs(items: TenderItem[]) {
  const counts = new Map<string, number>();
  for (const item of items) counts.set(kindOf(item), (counts.get(kindOf(item)) ?? 0) + 1);
  return [{ value: "", label: "全部事项", count: items.length }, ...[...counts.entries()].map(([label, count]) => ({ value: label, label, count }))];
}

const PAGE = 50;
export function AnalysisPane({ analysis, placements, kind, selected, onSelect }: {
  analysis: WorkbenchView["analysis"]; placements: Map<string, Placement>; kind: string;
  selected: string | null; onSelect(key: string): void;
}) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const items = (analysis?.items ?? []).map((item, index) => ({ item, key: itemKey(item, index) }));
  const shown = items.filter(({ item }) => (!kind || kindOf(item) === kind) && (!query.trim() || item.text.includes(query.trim())));
  const pages = Math.max(1, Math.ceil(shown.length / PAGE));
  const current = Math.min(page, pages);
  const rows = shown.slice((current - 1) * PAGE, current * PAGE).map(({ item, key }) => ({ key, item }));
  const taskVisible = !!analysis && (inProgress(analysis.status) || analysis.status === "failed" || analysis.status === "blocked" || !!analysis.progress?.request_states.uncertain);
  const issues = analysis ? groupedIssues(analysis.issues) : [];
  return <VStack gap={3} paddingInline={6} paddingBlock={4} role="region" aria-label="招标事项">
    {!analysis ? <Card><EmptyState title="尚无分析结果" description="上传文件后可开始分析，也可先编制可用稿件。" /></Card> : <>
      {taskVisible && <TaskProgress label="分析任务" status={analysis.status} progress={analysis.progress} />}
      {analysis.sources_current === false && <Banner role="status" status="warning" title="招标资料已有更新，以下是旧资料的分析结果。" description="可重新分析，也可基于最新资料生成新稿。" />}
      {executionError(analysis.error) && <Banner role="alert" status="error" title={executionError(analysis.error)} />}
      <Card padding={0}><VStack gap={0}>
        <Toolbar label="筛选招标事项" dividers={["bottom"]}
          startContent={<TextInput label="搜索招标事项" isLabelHidden type="search" placeholder="搜索事项原文" value={query} onChange={value => { setQuery(value); setPage(1); }} />}
          endContent={<Text type="supporting" hasTabularNumbers>{shown.length ? `第 ${(current - 1) * PAGE + 1}–${Math.min(current * PAGE, shown.length)} 条，共 ${shown.length} 条` : "共 0 条"}</Text>} />
        {!items.length ? <EmptyState isCompact title="当前尚无可读取的事项" description="分析结束后在这里逐条查看。" />
          : !shown.length ? <EmptyState isCompact title="没有匹配事项" description="换一个关键词或事项类型。" />
          : <Table hasHover idKey="key" data={rows} columns={[
            { key: "item", header: "招标事项", renderCell: ({ item, key }) => <Link href={`#item-${key}`} aria-current={key === selected ? "true" : undefined} onClick={event => { event.preventDefault(); onSelect(key); }}>{item.text}</Link> },
            { key: "kind", header: "类型", width: pixel(100), renderCell: ({ item }) => <Badge variant="neutral" label={kindOf(item)} /> },
            { key: "source", header: "原件位置", width: pixel(140), renderCell: ({ item }) => <Text type="supporting">{item.evidence?.[0]?.range_id ?? "—"}</Text> },
            { key: "place", header: "标书位置", width: pixel(180), renderCell: ({ item }) => <Text>{placements.get(item.item_id ?? "")?.chapters.join("、") || "—"}</Text> },
            { key: "check", header: "核对", width: pixel(96), renderCell: ({ item }) => checkBadge(placements.get(item.item_id ?? "")) },
          ]} />}
        {pages > 1 && <Toolbar label="分页" dividers={["top"]} startContent={<Text type="supporting">每页 {PAGE} 条</Text>}
          endContent={<Pagination page={current} onChange={setPage} totalItems={shown.length} pageSize={PAGE} />} />}
      </VStack></Card>
      {!!issues.length && <VStack gap={1}>{issues.map(([text, count]) => <Text key={text} type="supporting">{count > 1 ? `${count} 个阶段同一情况：` : ""}{text}</Text>)}</VStack>}
    </>}
  </VStack>;
}

export function ItemDetail({ item, placement }: { item: TenderItem; placement?: Placement }) {
  return <Card padding={0} role="region" aria-label="事项详情"><VStack gap={0}>
    <Toolbar label="事项详情标题" dividers={["bottom"]} startContent={<Text weight="semibold">事项详情</Text>} />
    <VStack gap={3} padding={4}>
      <MetadataList label={{ width: 72 }}>
        <MetadataListItem label="类型">{kindOf(item)}</MetadataListItem>
        <MetadataListItem label="原件位置">{item.evidence?.[0]?.range_id ?? "—"}</MetadataListItem>
        <MetadataListItem label="标书位置">{placement?.chapters.join("、") || "尚未安排到章节"}</MetadataListItem>
        <MetadataListItem label="核对">{checkBadge(placement)}</MetadataListItem>
      </MetadataList>
      <ItemContent item={item} />
    </VStack>
  </VStack></Card>;
}
