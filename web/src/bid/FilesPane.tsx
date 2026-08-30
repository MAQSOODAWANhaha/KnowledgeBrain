import { useEffect, useRef, useState } from "react";
import { Button, Drawer, Progress } from "@mantine/core";
import { Dropzone } from "@mantine/dropzone";
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
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragOn, setDragOn] = useState(false);
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

  async function filesFromEvent(event: unknown): Promise<File[]> {
    const ev = event as {
      dataTransfer?: DataTransfer;
      target?: EventTarget | null;
    };
    const fromDt = ev.dataTransfer?.files;
    if (fromDt && fromDt.length) return Array.from(fromDt);
    const fromInput = (ev.target as HTMLInputElement | null)?.files;
    if (fromInput && fromInput.length) return Array.from(fromInput);
    return [];
  }

  const empty = docs.length === 0 && pendingNames.length === 0;
  return (
    <div className="stack">
      {!ended && (
        <Dropzone
          multiple
          disabled={uploading}
          activateOnClick={false}
          getFilesFromEvent={filesFromEvent}
          onDrop={take}
          onReject={() => undefined}
          onDragEnter={() => setDragOn(true)}
          onDragLeave={() => setDragOn(false)}
          className={`drop ${dragOn ? "on" : ""}`}
          data-testid="upload-drop"
          style={{
            cursor: uploading ? "wait" : "pointer",
            padding: empty ? "40px 24px" : undefined,
          }}
        >
          <b>{uploading ? "上传中" : "拖入文件"}</b>
          <div
            className="row"
            style={{ justifyContent: "center", marginTop: 14 }}
          >
            <Button
              disabled={uploading}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                inputRef.current?.click();
              }}
            >
              {uploading ? "上传中" : "选择文件"}
            </Button>
          </div>
        </Dropzone>
      )}
      <input
        ref={inputRef}
        type="file"
        multiple
        hidden
        accept={TENDER_INPUT_ACCEPT}
        onChange={(e) => {
          const list = e.target.files;
          if (list?.length) take(Array.from(list));
          e.target.value = "";
        }}
      />
      {!empty && (
        <div className="card pad-0 file-list">
          <div className="file-head">
            <span>文件</span>
            <span>进度</span>
            <span>状态</span>
            <span>操作</span>
          </div>
          {pendingNames
            .filter((n) => !docs.some((d) => d.file_name === n))
            .map((n) => (
              <div key={`p-${n}`} className="file-row item">
                <div className="name">{n}</div>
                <Progress value={35} animated striped size="sm" />
                <span className="chip amber">上传中</span>
                <div className="file-actions">
                  <span className="file-action-slot" />
                  <span className="file-action-slot" />
                </div>
              </div>
            ))}
          {docs.map((d) => {
            const stage = fileStage(d);
            return (
              <div
                key={d.id}
                id={`bid-doc-${d.id}`}
                className={`file-row item${stage.tone === "rose" ? " fail" : ""}${focusId === d.id ? " on" : ""}`}
              >
                <div className="name">{d.file_name}</div>
                <Progress
                  value={stage.progress}
                  animated={stage.busy}
                  striped={stage.busy}
                  color={
                    stage.tone === "rose"
                      ? "red"
                      : stage.tone === "pine"
                        ? "teal"
                        : "blue"
                  }
                  size="sm"
                />
                <span className={`chip ${stage.tone}`}>{stage.label}</span>
                <div className="file-actions">
                  <Button
                    variant="default"
                    size="compact-sm"
                    data-testid={`preview-${d.id}`}
                    onClick={() => setPreview(d)}
                  >
                    预览
                  </Button>
                  {stage.retryable ? (
                    <Button
                      variant="default"
                      size="compact-sm"
                      disabled={ended}
                      onClick={() => onRetry(d)}
                    >
                      重试
                    </Button>
                  ) : (
                    <span className="file-action-slot" />
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
      <Drawer
        opened={preview != null}
        onClose={() => setPreview(null)}
        position="right"
        size="80%"
        title={preview?.file_name ?? "预览"}
        className="tender-preview-drawer"
        styles={{
          content: { display: "flex", flexDirection: "column", height: "100%" },
          header: { flexShrink: 0 },
          body: {
            flex: 1,
            minHeight: 0,
            overflow: "hidden",
            padding: 0,
          },
        }}
      >
        {preview ? (
          <div className="tender-preview-scroll">
            <FilePreview
              fileName={preview.file_name}
              objectKey={`objects/${preview.original_sha256}`}
            />
          </div>
        ) : null}
      </Drawer>
    </div>
  );
}
