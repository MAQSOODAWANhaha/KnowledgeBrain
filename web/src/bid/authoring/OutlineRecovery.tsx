import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Item } from "@astryxdesign/core/Item";
import { List, ListItem } from "@astryxdesign/core/List";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Timestamp } from "@astryxdesign/core/Timestamp";
import { VStack } from "@astryxdesign/core/VStack";
import { Button } from "@astryxdesign/core/Button";
import { Table } from "@astryxdesign/core/Table";
import { useEffect, useState } from "react";
import { tenderApi, type OutlineHistoryEntry, type OutlineRevision, type TenderChapter, type TenderIssue, type TenderTarget } from "../api/workbench";
import { assignTarget } from "./outlineTree";

const flatten = (tree: TenderChapter[]): TenderChapter[] => tree.flatMap(chapter => [chapter, ...flatten(chapter.children)]);

export function OutlineRecovery({ projectId, tree, sourceRevision, sourceExecution, sourceTargets, issues, selectedId, disabled, onChange, onRestore }: {
  projectId: string; tree: TenderChapter[]; sourceRevision?: string; sourceExecution?: string; sourceTargets?: TenderTarget[]; issues: TenderIssue[];
  selectedId?: string; disabled: boolean; onChange(tree: TenderChapter[]): void; onRestore(draft: OutlineRevision): void;
}) {
  const [history, setHistory] = useState<OutlineHistoryEntry[]>([]);
  const [preview, setPreview] = useState<OutlineRevision | null>(null);
  const [catalog, setCatalog] = useState<{ source: string; targets: TenderTarget[] } | null>(null);
  const [catalogError, setCatalogError] = useState<{ source: string; message: string } | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  const source = sourceRevision ? `revision:${sourceRevision}` : sourceExecution ? `execution:${sourceExecution}` : null;
  useEffect(() => {
    if (!source) return;
    if (!sourceRevision) {
      if (sourceTargets !== undefined) setCatalog({ source, targets: sourceTargets });
      return;
    }
    const abort = new AbortController();
    void tenderApi.outlineRevision(projectId, sourceRevision, abort.signal).then(value => {
      if (!abort.signal.aborted) { setCatalog({ source, targets: value.available_targets }); setCatalogError(null); }
    }).catch(error => { if (!abort.signal.aborted) setCatalogError({ source, message: error instanceof Error ? error.message : "响应位置读取失败，请重试。" }); });
    return () => abort.abort();
  }, [projectId, source, sourceRevision, sourceTargets, retry]);
  const nodes = flatten(tree);
  const current = new Map(nodes.flatMap(chapter => chapter.targets.map(target => [target.target_id, chapter] as const)));
  const catalogReady = !!source && !!catalog && catalog.source === source;
  const targets = new Map([...(catalogReady ? catalog.targets : []), ...nodes.flatMap(chapter => chapter.targets)].map(target => [target.target_id, target]));
  const unassigned = [...targets.values()].filter(target => !current.has(target.target_id));
  const issueKeys = new Set<string>();
  const visibleIssues = issues.filter(issue => {
    if (issue.code === "outline_edit_target_removed" && current.has(issue.target_id ?? "")) return false;
    if (issue.code === "outline_edit_reference_unbound" && current.has(issue.target_id ?? "")) {
      const related = targets.get(issue.target_id!)?.related_target_ids;
      if (related && related.every(id => current.has(id))) return false;
    }
    const key = JSON.stringify([issue.code, issue.target_id, issue.detail]);
    if (issueKeys.has(key)) return false;
    issueKeys.add(key); return true;
  });
  async function readHistory() {
    setLoading(true); setHistoryError(null);
    try { setHistory(await tenderApi.outlineHistory(projectId)); }
    catch (error) { setHistoryError(error instanceof Error ? error.message : "历史大纲读取失败。"); }
    finally { setLoading(false); }
  }
  async function readPreview(id: string) {
    setLoading(true); setHistoryError(null);
    try { setPreview((await tenderApi.outlineRevision(projectId, id)).draft); }
    catch (error) { setHistoryError(error instanceof Error ? error.message : "历史修订读取失败。"); }
    finally { setLoading(false); }
  }
  return <Section role="region" aria-label="大纲历史与响应安排" padding={0} variant="transparent"><VStack gap={2}>
    {catalogError?.source === source && <Banner role="alert" status="error" title={catalogError.message} endContent={<Button size="sm" onClick={() => setRetry(value => value + 1)} label="重新读取响应位置" />} />}
    {historyError && <Banner role="alert" status="error" title={`${historyError} 请使用历史修订列表或预览按钮重试。`} />}
    <Collapsible trigger="历史大纲" defaultIsOpen={false}><VStack gap={2}>
      <HStack><Button variant="secondary" isDisabled={loading} onClick={() => void readHistory()} label="读取历史修订" /></HStack>
      {!!history.length && <List density="compact" hasDividers>{history.map(entry => <ListItem key={entry.revision_id} label={`修订 ${entry.revision}`} description={<Timestamp value={entry.created_at} />} endContent={<Button size="sm" variant="secondary" isDisabled={loading} onClick={() => void readPreview(entry.revision_id)} label={`预览修订 ${entry.revision}`} />} />)}</List>}
      {preview && <Section role="region" aria-label="历史大纲预览" variant="muted" padding={3}><VStack gap={2}><Heading level={3}>修订 {preview.revision} 与当前编排对照</Heading>
        <Table data={flatten(preview.outline.chapters)} columns={[{ key: "title", header: "历史章节" }, { key: "chapter_id", header: "当前同标识章节", renderCell: (chapter) => nodes.find(node => node.chapter_id === chapter.chapter_id)?.title ?? "已移出" }]} />
        {!preview.sources_current && <Text type="supporting">此修订来源已过期，可读取或恢复；生成正文前需要更新来源大纲。</Text>}
        <Text type="supporting">载入后请保存为新修订，现有历史和 Word 稿件保留。</Text>
        <HStack><Button variant="primary" isDisabled={disabled || loading} onClick={() => onRestore(preview)} label="载入此历史大纲另存" /></HStack>
      </VStack></Section>}
    </VStack></Collapsible>
    <Collapsible trigger={`响应安排（${catalogReady ? `${unassigned.length} 项未安排` : "响应目录待读取"}）`} defaultIsOpen={false}><VStack gap={2}>
      <Text type="supporting">将原响应安排到当前选定章节，原要求、来源和表单随响应保留。保存后生效。</Text>
      {visibleIssues.map((issue, index) => <Text key={`${issue.code}:${index}`}>{issue.detail}</Text>)}
      {[...targets.values()].map(target => <Card key={target.target_id} padding={3} role="group" aria-label={target.purpose}>
        <Item density="compact" label={target.purpose} description={current.get(target.target_id)?.title ?? "尚未安排到章节"}
          endContent={<Button size="sm" variant="secondary" isDisabled={disabled || !selectedId || current.get(target.target_id)?.chapter_id === selectedId} onClick={() => selectedId && onChange(assignTarget(tree, target, selectedId))} label="安排到当前章节" />} />
      </Card>)}
    </VStack></Collapsible>
  </VStack></Section>;
}
