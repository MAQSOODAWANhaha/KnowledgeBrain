import { useRef, useState } from "react";
import { Dropzone } from "@mantine/dropzone";
import type { BidDoc } from "../api";
import { fileLabel } from "./helpers";

export function FilesPane({
  docs,
  ended,
  uploading,
  pendingNames = [],
  onUpload,
  onRetry,
  onDelete,
}: {
  docs: BidDoc[];
  ended: boolean;
  uploading?: boolean;
  pendingNames?: string[];
  onUpload: (files: File[]) => void;
  onRetry: (docId: string) => void;
  onDelete: (docId: string) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragOn, setDragOn] = useState(false);

  function take(files: File[]) {
    if (!files.length || ended || uploading) return;
    onUpload(files);
  }

  function pick() {
    inputRef.current?.click();
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
    <div className="wrap stack">
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
          style={{ cursor: uploading ? "wait" : "pointer", padding: empty ? "56px 24px" : undefined }}
        >
          <b>{uploading ? "正在上传…" : empty ? "还没有招标文件" : "把招标文件或补遗拖到这里"}</b>
          {uploading
            ? "请稍候，传完会自动解析并抽条款。"
            : empty
              ? "拖进来，或点选。解析完成后会抽商务 / 技术条款。只挂在本标。"
              : "点此也可选文件。只挂在本标，不要丢进知识资产。"}
          <div className="row" style={{ justifyContent: "center", marginTop: 14 }}>
            <button
              className="btn pri"
              type="button"
              disabled={uploading}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                pick();
              }}
            >
              {uploading ? "上传中" : "选择文件"}
            </button>
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
      <div className="card pad-0">
          {pendingNames
            .filter((n) => !docs.some((d) => d.file_name === n))
            .map((n) => (
              <div key={`p-${n}`} className="item" style={{ gridTemplateColumns: "1fr auto" }}>
                <div>
                  <div className="name">{n}</div>
                  <div className="desc">正在上传</div>
                </div>
                <span className="chip amber">
                  <i className="dot" />
                  上传中
                </span>
              </div>
            ))}
          {docs.map((d) => (
            <div key={d.id} className={`item ${d.parse_status === "failed" ? "fail" : ""}`} style={{ gridTemplateColumns: "1fr auto auto auto" }}>
              <div>
                <div className="name">{d.file_name}</div>
                <div className="desc">
                  {d.multimodal_status === "failed"
                    ? `图像处理失败：${d.multimodal_error || "请重试"}`
                    : d.error_message || fileLabel(d.parse_status)}
                </div>
              </div>
              <span
                className={`chip ${
                  d.parse_status === "completed" ? "pine" : d.parse_status === "failed" ? "rose" : d.parse_status === "processing" ? "amber" : "gray"
                }`}
              >
                {(d.parse_status === "completed" || d.parse_status === "failed" || d.parse_status === "processing") && <i className="dot" />}
                {fileLabel(d.parse_status)}
              </span>
              <button className="btn sm" type="button" disabled={ended || d.parse_status !== "failed"} onClick={() => onRetry(d.id)}>
                重试
              </button>
              <button className="btn sm" type="button" disabled={ended} onClick={() => onDelete(d.id)}>
                删除
              </button>
            </div>
          ))}
      </div>
      )}
    </div>
  );
}
