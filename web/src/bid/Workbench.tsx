import { useEffect, useState, type ReactNode } from "react";
import { Alert } from "../components/ui/alert";
import { Button } from "../components/ui/button";
import { Crumbs } from "../Crumbs";
import { go, parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { createMutationAttempt } from "../api";
import { BidTree } from "./BidTree";
import { FilesPane } from "./FilesPane";
import { authoringHref, type AuthoringStep } from "./authoring/routes";
import { createBidV2Client } from "./api/client";
import { docxApi, type DocxCurrent } from "./api/docx";
import type { BidProjectView, TenderDocumentView } from "./api/types";
import { DocxEditor } from "./authoring/DocxEditor";
import { DocxStart } from "./authoring/DocxStart";

export function Workbench({ email }: { email: string }) {
  const route = parseBidRoute(useHash());
  const [rows, setRows] = useState<BidProjectView[] | null>(null);
  useEffect(() => {
    createBidV2Client()
      .listProjects()
      .then(setRows)
      .catch(() => setRows([]));
  }, []);
  if (!route) return null;
  const tree = (
    <BidTree rows={rows} currentId={route.projectId} currentStep={route.step} />
  );
  return route.step === "files"
    ? <ProjectFiles key={route.projectId} email={email} projectId={route.projectId} tree={tree} />
    : <DocxGate key={`${route.projectId}:${route.step}`} email={email} projectId={route.projectId} step={route.step} tree={tree} />;
}

function DocxGate({ email, projectId, step, tree }: { email: string; projectId: string; step: AuthoringStep; tree: ReactNode }) {
  const [result, setResult] = useState<{ project: BidProjectView; current: DocxCurrent | null } | null>(null);
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  const [unsafe, setUnsafe] = useState(false);
  const [creating, setCreating] = useState(false);
  useEffect(() => {
    let canceled = false;
    setError(false);
    void (async () => {
      const project = await createBidV2Client().getProject(projectId);
      const current = await docxApi.current(project.workspace_id);
      if (!canceled) setResult({ project, current });
    })().catch(() => { if (!canceled) setError(true); });
    return () => { canceled = true; };
  }, [projectId, retry]);
  const stepLabel = step === "export" ? "导出" : "编制";
  return <Shell root="bids" email={email} onBeforeLeave={() => !unsafe}
    crumbs={<Crumbs items={[{ label: "投标项目", href: "/" }, { label: result?.project.title ?? "投标稿" }, { label: stepLabel }]} />}
    tree={tree}>
    <div className="wrap stack">
      {error ? <Alert>
        无法读取当前稿件
        <Button size="sm" className="mt-2" onClick={() => setRetry((value) => value + 1)}>重试</Button>
      </Alert>
        : !result ? <p role="status">正在读取当前稿件…</p>
        : (creating || (!result.current && step === "authoring" && result.project.status !== "ended")) ? <DocxStart key={result.project.workspace_id} workspaceId={result.project.workspace_id} onUnsafeChange={setUnsafe}
          onCancel={() => { setCreating(false); setUnsafe(false); if (!result.current) go(authoringHref(projectId, "files")); }}
          onPublished={() => { setCreating(false); setUnsafe(false); setResult(null); setRetry(value => value + 1); }} />
        : step === "authoring" && result.project.status !== "ended"
          ? <DocxEditor key={result.project.workspace_id} workspaceId={result.project.workspace_id}
            onUnsafeChange={setUnsafe} onCreateRound={() => setCreating(true)} />
          : result.current ? <SavedDocx workspaceId={result.project.workspace_id} current={result.current} /> : <p>尚未创建 DOCX 投标稿。</p>}
    </div>
  </Shell>;
}

function SavedDocx({ workspaceId, current }: { workspaceId: string; current: DocxCurrent }) {
  const [error, setError] = useState(false);
  async function download() {
    try {
      const url = URL.createObjectURL(await docxApi.download(workspaceId, current.version_id));
      const link = document.createElement("a"); link.href = url; link.download = `投标稿-${current.revision}.docx`;
      link.click(); URL.revokeObjectURL(url);
    } catch { setError(true); }
  }
  return <section><h2>已保存的投标稿</h2><p>保存版本 {current.revision}</p>
    {current.editor.pending_save_id && <p role="status">还有保存请求待确认，下载文件为当前已保存版本。</p>}
    <Button onClick={() => void download()}>下载 DOCX</Button>
    {error && <Alert className="mt-2">下载失败，请重试。</Alert>}
  </section>;
}

function ProjectFiles({ email, projectId, tree }: { email: string; projectId: string; tree: ReactNode }) {
  const [client] = useState(createBidV2Client);
  const [project, setProject] = useState<BidProjectView | null>(null);
  const [docs, setDocs] = useState<TenderDocumentView[]>([]);
  const [pending, setPending] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    const read = async () => {
      try {
        const [p, d] = await Promise.all([client.getProject(projectId, abort.signal), client.listTenderDocuments(projectId, abort.signal)]);
        if (!abort.signal.aborted) { setProject(p); setDocs(d); }
      } catch { if (!abort.signal.aborted) setError("文件读取失败，请刷新重试。"); }
    };
    void read();
    const timer = window.setInterval(() => void read(), 2000);
    return () => { abort.abort(); window.clearInterval(timer); };
  }, [client, projectId]);
  async function upload(files: File[]) {
    if (busy) return;
    setBusy(true); setError(null); setPending(files.map(file => file.name));
    try { for (const file of files) await client.uploadTenderDocument(projectId, file, createMutationAttempt()); }
    catch { setError("上传结果未全部确认，请核对文件列表后重试。"); }
    finally { setBusy(false); setPending([]); }
  }
  async function freeze() {
    if (busy) return;
    setBusy(true); setError(null);
    try {
      const sets = await client.listDocumentSets(projectId);
      const expected = sets[0] ?? null;
      await client.freezeDocumentSet(projectId, docs.map(doc => doc.id), expected, createMutationAttempt());
      setNotice("文件集已冻结，要求整理完成后可在编制页生成模板。");
    } catch { setError("文件冻结未成功，请核对文件处理状态后重试。"); }
    finally { setBusy(false); }
  }
  return <Shell root="bids" email={email}
    crumbs={<Crumbs items={[{ label: "投标项目", href: "/" }, { label: project?.title ?? "招标文件" }, { label: "文件" }]} />}
    tree={tree}>
    <div className="wrap stack">
      {error && <Alert className="mb-2">{error}</Alert>}
      {notice && <Alert tone="go" className="mb-2">{notice}</Alert>}
      <FilesPane docs={docs} ended={project?.status === "ended"} uploading={busy && pending.length > 0}
        pendingNames={pending} onUpload={files => void upload(files)}
        onRetry={doc => void client.retryTenderDocument(projectId, doc.id, doc.conversion_generation, createMutationAttempt())
          .catch(() => setError("重试处理失败，请刷新后重试。"))} />
      <div className="panel-foot">
        <Button disabled={busy || !project || project.status === "ended" || !docs.length}
          onClick={() => void freeze()}>冻结文件并整理要求</Button>
      </div>
    </div>
  </Shell>;
}
