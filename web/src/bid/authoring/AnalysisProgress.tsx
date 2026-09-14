import { useEffect, useState, type ReactNode } from "react";
import { createMutationAttempt } from "../../api";
import { Button } from "../../components/ui/button";
import { createBidV2Client } from "../api/client";
import type { RequirementSetCompileRequestView, TenderDocumentView } from "../api/types";
import { fileStage } from "../helpers";

type Progress = NonNullable<RequirementSetCompileRequestView["progress"]> & {
  checkpoint_sequence?: number;
  boundary?: string;
};

function parsed(docs: TenderDocumentView[]) {
  return docs.length > 0 && docs.every((doc) => doc.parse_status === "ready" || doc.parse_status === "completed");
}

function fileSummary(docs: TenderDocumentView[]) {
  if (!docs.length) return "未上传";
  const failed = docs.filter((doc) => fileStage(doc).retryable).length;
  const done = docs.filter((doc) => parsed([doc])).length;
  if (failed) return `${failed} 个解析失败`;
  if (done < docs.length) return `解析中 ${done}/${docs.length}`;
  return `已完成 ${docs.length} 个`;
}

function analysisSummary(job: RequirementSetCompileRequestView | null) {
  if (!job) return "未开始";
  if (job.status === "failed") return job.error_code ?? "未完成";
  if (job.status === "succeeded") return "已完成";
  const progress = (job.progress ?? {}) as Progress;
  const step = typeof progress.turn === "number" ? progress.turn : progress.checkpoint_sequence;
  const parts: string[] = [];
  if (progress.boundary === "prepared") parts.push("正在等待模型");
  else if (progress.phase === "reviewer" || progress.phase === "verifying") parts.push("正在复核");
  else parts.push("正在分析");
  if (typeof step === "number") parts.push(`第 ${step} 步`);
  if (typeof progress.records === "number") parts.push(`已提取 ${progress.records} 条`);
  if (typeof progress.review_rounds === "number" && progress.review_rounds > 0) {
    parts.push(`已复核 ${progress.review_rounds} 轮`);
  }
  return parts.join(" · ");
}

export function AnalysisProgress({
  projectId,
  allowStart = true,
  children,
}: {
  projectId: string;
  allowStart?: boolean;
  children?: ReactNode;
}) {
  const [job, setJob] = useState<RequirementSetCompileRequestView | null>(null);
  const [docs, setDocs] = useState<TenderDocumentView[]>([]);
  const [ended, setEnded] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    const client = createBidV2Client();
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function read() {
      let complete = false;
      try {
        const [value, documents, project] = await Promise.all([
          client.latestRequirementSetCompilation(projectId, abort.signal),
          client.listTenderDocuments(projectId, abort.signal),
          client.getProject(projectId, abort.signal),
        ]);
        if (abort.signal.aborted) return;
        setJob(value); setDocs(documents); setEnded(project.status === "ended");
        setLoaded(true); setError(false);
        complete = value?.status === "succeeded";
      } catch { if (!abort.signal.aborted) setError(true); }
      if (!abort.signal.aborted && !complete) timer = setTimeout(() => void read(), 3000);
    }
    void read();
    return () => { abort.abort(); clearTimeout(timer); };
  }, [projectId]);
  async function start() {
    if (busy || ended || !parsed(docs)) return;
    setBusy(true); setStartError(null);
    const client = createBidV2Client();
    try {
      const sets = await client.listDocumentSets(projectId);
      await client.freezeDocumentSet(projectId, docs.map((doc) => doc.id), sets[0] ?? null, createMutationAttempt());
      setJob(await client.latestRequirementSetCompilation(projectId));
    } catch { setStartError("分析未能开始，请确认文件已解析完成。"); }
    finally { setBusy(false); }
  }
  async function resume() {
    if (busy || ended || job?.status !== "pending") return;
    setBusy(true); setStartError(null);
    try {
      setJob(await createBidV2Client().continueRequirementSetCompilation(projectId, createMutationAttempt()));
    } catch { setStartError("未能继续分析，请稍后重试。"); }
    finally { setBusy(false); }
  }
  const ready = loaded && job?.status === "succeeded";
  const showStart = allowStart && loaded && !ended && (!job || job.status === "failed");
  const showResume = allowStart && loaded && !ended && job?.status === "pending";
  return <>
    <section className="card stack" data-testid="analysis-progress">
      <h2 className="h3">当前进度</h2>
      <div role="status" aria-live="polite" className="stack text-sm">
        {!loaded && <p>正在读取进度…</p>}
        {loaded && <>
          <p>文件 {fileSummary(docs)}</p>
          <p>分析 {analysisSummary(job)}</p>
        </>}
      </div>
      {error && <p role="alert">暂时无法更新进度，正在重试。</p>}
      {job?.status === "failed" && <p role="alert">{job.error_code ?? "分析失败"}</p>}
      {startError && <p role="alert">{startError}</p>}
      {showResume && <Button disabled={busy} onClick={() => void resume()}>继续分析</Button>}
      {showStart && <Button disabled={busy || !parsed(docs)} onClick={() => void start()}>开始分析</Button>}
    </section>
    {ready ? children : null}
  </>;
}
