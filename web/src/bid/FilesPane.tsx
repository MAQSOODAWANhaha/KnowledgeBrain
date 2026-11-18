import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { FileInput } from "@astryxdesign/core/FileInput";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
import { Table } from "@astryxdesign/core/Table";
import { useEffect, useState } from "react";
import { FilePreview } from "../assets/FilePreview";
import type { TenderDocumentView } from "./api/types";
import { TENDER_INPUT_ACCEPT } from "./authoring/media";
import { fileStage } from "./helpers";

export function FilesPane({
  docs,
  ended,
  uploading,
  pendingNames = [],
  focusId,
  onUpload,
  onRetry,
}: {
  docs: TenderDocumentView[];
  ended: boolean;
  uploading?: boolean;
  pendingNames?: string[];
  focusId?: string | null;
  onUpload: (files: File[]) => void;
  onRetry: (doc: TenderDocumentView) => void;
}) {
  const [preview, setPreview] = useState<TenderDocumentView | null>(null);

  useEffect(() => {
    if (!focusId) return;
    document
      .getElementById(`bid-doc-${focusId}`)
      ?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [focusId]);

  function take(files: File[]) {
    if (!files.length || ended || uploading) return;
    onUpload(files);
  }

  const empty = docs.length === 0 && pendingNames.length === 0;
  const pending = pendingNames.filter((n) => !docs.some((d) => d.file_name === n));
  return (
    <div className="stack">
      {!ended && <FileInput data-testid="upload-drop" label={uploading ? "上传中" : "选择文件"} mode="dropzone" isMultiple accept={TENDER_INPUT_ACCEPT} value={null} isDisabled={uploading} isLoading={uploading} onChange={(files) => { const list = Array.isArray(files) ? files : files ? [files] : []; if (list.length) take(list); }} />}
      {!empty && (
        <div className="panel">
          <Table hasHover data={[...pending.map((name) => ({ id: `pending:${name}`, name, progress: 35, busy: true, label: "上传中", retryable: false, docId: "" })), ...docs.map((doc) => { const stage = fileStage(doc); return { id: doc.id, name: doc.file_name, progress: stage.progress, busy: stage.busy, label: stage.label, retryable: stage.retryable, docId: doc.id }; })]} columns={[
            { key: "name", header: "文件", renderCell: (row) => <span id={row.docId ? `bid-doc-${row.docId}` : undefined}>{row.name}</span> },
            { key: "progress", header: "进度", renderCell: (row) => <ProgressBar label="处理进度" isLabelHidden value={row.progress} max={100} isIndeterminate={row.busy && row.progress <= 0} /> },
            { key: "label", header: "状态", renderCell: (row) => <Badge label={row.label} /> },
            { key: "id", header: "操作", renderCell: (row) => row.docId ? <div className="flex justify-end gap-2"><Button variant="secondary" size="sm" label="预览" data-testid={`preview-${row.docId}`} onClick={() => setPreview(docs.find((doc) => doc.id === row.docId) ?? null)} />{row.retryable ? <Button variant="secondary" size="sm" label="重试" isDisabled={ended} onClick={() => { const doc = docs.find((item) => item.id === row.docId); if (doc) onRetry(doc); }} /> : null}</div> : null },
          ]} />
        </div>
      )}
      <Dialog isOpen={preview != null} onOpenChange={(open) => { if (!open) setPreview(null); }}>
        <DialogHeader title={preview?.file_name ?? "预览"} onOpenChange={(open) => { if (!open) setPreview(null); }} />
        {preview ? <div className="tender-preview-scroll"><FilePreview fileName={preview.file_name} objectKey={`objects/${preview.original_sha256}`} /></div> : null}
      </Dialog>
    </div>
  );
}
