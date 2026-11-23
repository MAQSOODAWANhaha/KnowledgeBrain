import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/HStack";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Banner } from "@astryxdesign/core/Banner";
import { Selector } from "@astryxdesign/core/Selector";
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
  return <><p>{item.text}</p>{item.subject && <p className="tender-note">主体：{item.subject}</p>}{item.scope && <p className="tender-note">适用范围：{item.scope}</p>}
    {!!item.conditions?.length && <p className="tender-note">条件：{item.conditions.join("；")}</p>}
    {!!item.exceptions?.length && <p className="tender-note">例外：{item.exceptions.join("；")}</p>}
    {!!item.numbers?.length && <div className="tender-rule-details">{item.numbers.map((number, index) => <p key={index}>量化事实：{number.value} {number.unit}（{number.quote}）</p>)}</div>}
    {item.scoring && <div className="tender-rule-details">{item.scoring.method && <p>评分方法：{item.scoring.method}</p>}{item.scoring.points !== undefined && <p>分值：{item.scoring.points}</p>}{item.scoring.max_points !== undefined && <p>最高分：{item.scoring.max_points}</p>}</div>}
    {item.consequence && <div className="tender-rule-details">{item.consequence.effect && <p>后果：{item.consequence.effect}</p>}{item.consequence.trigger && <p>触发条件：{item.consequence.trigger}</p>}</div>}
    {item.submission && <div className="tender-rule-details">{!!item.submission.fields?.length && <p>填写字段：{item.submission.fields.join("；")}</p>}{!!item.submission.documents?.length && <p>提交材料：{item.submission.documents.join("；")}</p>}</div>}
    {!!item.evidence?.length && <details><summary>原文引文与位置</summary>{item.evidence.map((evidence, index) => <blockquote key={`${evidence.range_id}:${index}`}><p>{evidence.quote}</p><small>来源位置：{evidence.range_id}</small></blockquote>)}</details>}
  </>;
}

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
  return <div className="tender-context">
    <TabList aria-label="章节信息" size="sm" value={tab} onChange={value => setTab(value as typeof tab)}>
      <Tab value="requirements" label="要求" /><Tab value="supports" label="依据" /><Tab value="gaps" label={gaps.length ? `待补 ${gaps.length}` : "待补"} />
    </TabList>
    <div className="tender-context-body"><p className="tender-note">当前选定章节：{chapter?.title ?? "尚未选择"}</p>
      {!contextMatches ? <p className="tender-context-warning" role="status">当前业务记录尚未与这份稿件关联，保留 Word 供阅读，请刷新后核对。</p> : !chapter ? <p className="tender-note">选择章节查看相关信息。</p> : <>
        {draftId && <p className="tender-note">以下记录来自本稿的编制快照，后续人工修改仍需核对。</p>}
        {tab === "requirements" && <>{boundRules.length ? boundRules.map((binding, index) => <article className="tender-info-card" key={index}>{binding.rule ? <ItemContent item={binding.rule} /> : <p>关联要求暂不可读取，正式引用仍保留，需核对来源。</p>}{binding.rule_ref.decision_id && <p className="tender-note">显示修订后保留的适用规则。</p>}</article>) : <p className="tender-note">当前没有可读取的关联要求；这不表示本章没有招标要求。</p>}
          {targets.map(target => <article className="tender-info-card" key={target.target_id}><small>{target.kind === "form" ? "规定表单" : target.kind === "attachment" ? "证明附件" : "正文响应"}{target.manual_fill ? " · 人工填写" : ""}</small><p>{target.purpose}</p></article>)}</>}
        {tab === "supports" && <>{targets.filter(target => target.source_refs?.length).map(target => <article className="tender-info-card" key={target.target_id}><strong>招标来源 · {target.purpose}</strong>{target.source_refs?.map((source, index) => <p key={`${source.unit_id}:${index}`} className="tender-note">来源位置：{source.unit_id}</p>)}</article>)}{materials.map(material => <article className="tender-info-card" key={material.material_id}><strong>采用资料 · {material.title}</strong>{material.text && <details><summary>查看资料原文</summary><p className="tender-material-text">{material.text}</p></details>}{material.original_url && <Button onClick={() => void onDownload(material.original_url!, material.title)} label="下载证明原件" />}</article>)}{!materials.length && <p className="tender-note">当前尚无可读取的已采用企业资料，不能据此认定资料不存在。</p>}<p className="tender-note">真实证明须沿用原件。来源引用和检索命中均需核对其适用性。</p></>}
        {tab === "gaps" && <>{gaps.length ? gaps.map((gap, index) => {
          const target = targets.find(item => item.target_id === gap.target_id);
          const sameChapter = gap.chapter_id === chapter.chapter_id || !!target;
          return <article className="tender-info-card tender-info-gap" key={`${gap.code}:${index}`}><p>{gap.detail}</p>{sameChapter ? <Button onClick={() => onLocate(target?.bookmark ?? chapter.bookmark)} label={target?.bookmark ? "定位响应位置" : "定位所属章节"} /> : gap.chapter_id && <small>原章节位置需要重新关联。</small>}</article>;
        }) : <p className="tender-note">当前没有可读取的待补记录，不代表已经完成内容验收。</p>}<p><a href={`#${authoringHref(view.project.id, "files")}`}>到项目资料补充文件</a></p><p className="tender-note">补充资料后另存新稿。人工写入内容不自动清除原复核意见。</p></>}
      </>}
    </div>
  </div>;
}

const kindLabels: Record<string, string> = { obligation: "要求", requirement: "要求", scoring_rule: "评分规则", consequence_rule: "后果规则", submission_requirement: "提交要求", fact: "背景事实" };
const phasePrefix = /^(register|freeze|ocr|pack-plan|extract|analysis-review|analysis-locate|analysis-associate|analysis-joint|verify|confirm)\s+/;
function groupedIssues(issues: { detail: string }[]) {
  const counts = new Map<string, number>();
  for (const issue of issues) {
    const text = issue.detail.replace(phasePrefix, "");
    counts.set(text, (counts.get(text) ?? 0) + 1);
  }
  return [...counts.entries()];
}
export function AnalysisPane({ analysis }: { analysis: WorkbenchView["analysis"] }) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("");
  const items = analysis?.items ?? [];
  const shown = items.filter(item => (!kind || item.kind === kind) && (!query.trim() || item.text.includes(query.trim())));
  const taskVisible = !!analysis && (inProgress(analysis.status) || analysis.status === "failed" || analysis.status === "blocked" || !!analysis.progress?.request_states.uncertain);
  const issues = analysis ? groupedIssues(analysis.issues) : [];
  return <section className="tender-section tender-analysis-main">
    {!analysis ? <EmptyState title="尚无分析结果" description="上传文件后可开始分析，也可先编制可用稿件。" /> : <>
      {!taskVisible && <HStack gap={2} vAlign="center"><Badge variant={analysis.confirmed ? "success" : "warning"} label={analysis.confirmed ? "已确认" : "未确认"} /><Text type="supporting" hasTabularNumbers>来源版本 {analysis.source_version}，修订 {analysis.domain_revision}</Text></HStack>}
      <TaskProgress label="分析任务" status={analysis.status} progress={analysis.progress} />
      {analysis.sources_current === false && <Banner role="status" status="warning" title="招标资料已有更新，以下是旧资料的分析结果。" description="可重新分析，也可基于最新资料生成新稿。" />}
      {executionError(analysis.error) && <Banner role="alert" status="error" title={executionError(analysis.error)} />}
      {!!items.length && <div className="tender-filter"><TextInput label="搜索招标事项" type="search" placeholder="搜索事项原文…" value={query} onChange={setQuery} />
        <Selector label="事项类型" value={kind} onChange={setKind} options={[{ value: "", label: "全部类型" }, ...Array.from(new Set(items.map(item => item.kind).filter((value): value is string => !!value))).map(value => ({ value, label: kindLabels[value] ?? value }))]} /></div>}
      {!items.length ? <EmptyState isCompact title="当前尚无可读取的事项" description="分析结束后在这里逐条查看。" /> : !shown.length ? <EmptyState isCompact title="没有匹配事项" description="换一个关键词或事项类型。" /> : shown.map((item, index) => <article className="tender-analysis-item" key={item.item_id ?? index}>{item.kind && <Badge variant="neutral" label={kindLabels[item.kind] ?? item.kind} />}<ItemContent item={item} /></article>)}
      {!!issues.length && <VStack gap={1}>{issues.map(([text, count]) => <Text key={text} type="supporting">{count > 1 ? `${count} 个阶段同一情况：` : ""}{text}</Text>)}</VStack>}
    </>}
  </section>;
}
