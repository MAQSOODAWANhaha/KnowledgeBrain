import { useEffect, useRef, useState } from "react";
import { Button } from "@mantine/core";
import { Dropzone } from "@mantine/dropzone";
import type { BidDoc } from "../api";
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
  docs: BidDoc[];
  ended: boolean;
  uploading?: boolean;
  pendingNames?: string[];
  focusId?: string | null;
  onUpload: (files: File[]) => void;
  onRetry: (doc: BidDoc) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragOn, setDragOn] = useState(false);

  useEffect(() => {
    if (!focusId) return;
    document.getElementById(`bid-doc-${focusId}`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [focusId]);

  function take(files: File[]) {
    if (!files.length || ended || uploading) return;
    onUpload(files);
  }

  async function filesFromEvent(event: unknown): Promise<File[]> {
    const ev = event as { dataTransfer?: DataTransfer; target?: EventTarget | null };
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
          style={{ cursor: uploading ? "wait" : "pointer", padding: empty ? "56px 24px" : undefined }}
        >
          <b>{uploading ? "正在上传…" : empty ? "还没有招标文件" : "把招标文件或补遗拖到这里"}</b>
          {uploading ? "请稍候，传完会自动解析并抽条款。" : "点此也可选文件。只挂在本标，不要丢进知识资产。"}
          <div className="row" style={{ justifyContent: "center", marginTop: 14 }}>
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
        onChange={(e) => {
          const list = e.target.files;
          if (list?.length) take(Array.from(list));
          e.target.value = "";
        }}
      />
      {!empty && (
        <div className="card pad-0 file-list">
          <div className="group-h">
            <span>招标文件</span>
            <span>{docs.length} 个文件</span>
          </div>
          {pendingNames
            .filter((n) => !docs.some((d) => d.file_name === n))
            .map((n) => (
              <div key={`p-${n}`} className="item" style={{ gridTemplateColumns: "1fr auto" }}>
                <div>
                  <div className="name">{n}</div>
                  <div className="desc">正在上传</div>
                </div>
                <span className="chip amber">上传中</span>
              </div>
            ))}
          {docs.map((d) => {
            const stage = fileStage(d);
            return (
              <div key={d.id} id={`bid-doc-${d.id}`} className={`item file-row${stage.tone === "rose" ? " fail" : ""}${focusId === d.id ? " on" : ""}`}>
                <div>
                  <div className="name">{d.file_name}</div>
                  <div className="desc">{stage.desc}</div>
                </div>
                <span className={`chip ${stage.tone}`}>{stage.label}</span>
                <Button variant="default" size="compact-sm" disabled={ended || !stage.retryable} onClick={() => onRetry(d)}>
                  重试
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
