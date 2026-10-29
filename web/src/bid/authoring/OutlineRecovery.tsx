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
  return <section className="outline-recovery" aria-label="大纲历史与响应安排">
    {catalogError?.source === source && <p role="alert">{catalogError.message}<button onClick={() => setRetry(value => value + 1)}>重新读取响应位置</button></p>}
    {historyError && <p role="alert">{historyError} 请使用历史修订列表或预览按钮重试。</p>}
    <details><summary>历史大纲</summary><button disabled={loading} onClick={() => void readHistory()}>读取历史修订</button>
      {!!history.length && <ul>{history.map(entry => <li key={entry.revision_id}>修订 {entry.revision} <time>{entry.created_at}</time> <button disabled={loading} onClick={() => void readPreview(entry.revision_id)}>预览修订 {entry.revision}</button></li>)}</ul>}
      {preview && <div aria-label="历史大纲预览"><h3>修订 {preview.revision} 与当前编排对照</h3>
        <table className="tender-table"><thead><tr><th>历史章节</th><th>当前同标识章节</th></tr></thead><tbody>{flatten(preview.outline.chapters).map(chapter => <tr key={chapter.chapter_id}><td>{chapter.title}</td><td>{nodes.find(node => node.chapter_id === chapter.chapter_id)?.title ?? "已移出"}</td></tr>)}</tbody></table>
        {!preview.sources_current && <p className="tender-note">此修订来源已过期，可读取或恢复；生成正文前需要更新来源大纲。</p>}
        <p className="tender-note">载入后请保存为新修订，现有历史和 Word 稿件保留。</p><button disabled={disabled || loading} onClick={() => onRestore(preview)}>载入此历史大纲另存</button>
      </div>}
    </details>
    <details><summary>响应安排（{catalogReady ? `${unassigned.length} 项未安排` : "响应目录待读取"}）</summary><p className="tender-note">将原响应安排到当前选定章节，原要求、来源和表单随响应保留。保存后生效。</p>
      {visibleIssues.map((issue, index) => <p key={`${issue.code}:${index}`}>{issue.detail}</p>)}
      {[...targets.values()].map(target => <div className="tender-info-card" key={target.target_id}><strong>{target.purpose}</strong><p>{current.get(target.target_id)?.title ?? "尚未安排到章节"}</p><button disabled={disabled || !selectedId || current.get(target.target_id)?.chapter_id === selectedId} onClick={() => selectedId && onChange(assignTarget(tree, target, selectedId))}>安排到当前章节</button></div>)}
    </details>
  </section>;
}
