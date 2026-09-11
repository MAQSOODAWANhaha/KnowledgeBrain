import { useEffect, useRef, useState } from "react";
import { IconCloudUpload } from "@tabler/icons-react";
import { FilePreview } from "../assets/FilePreview";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Progress } from "../components/ui/progress";
import { Sheet, SheetContent } from "../components/ui/sheet";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/ui/table";
import { cn } from "../lib/utils";
import type { TenderDocumentView } from "./api/types";
import { TENDER_INPUT_ACCEPT } from "./authoring/media";
import { fileStage } from "./helpers";

const TONE = {
  pine: "go",
  amber: "wait",
  rose: "stop",
  gray: "gray",
} as const;

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

  const empty = docs.length === 0 && pendingNames.length === 0;
  const pending = pendingNames.filter((n) => !docs.some((d) => d.file_name === n));
  return (
    <div className="stack">
      {!ended && (
        <div
          data-testid="upload-drop"
          className={cn("drop", dragOn && "on")}
          onDragEnter={(e) => {
            e.preventDefault();
            setDragOn(true);
          }}
          onDragOver={(e) => e.preventDefault()}
          onDragLeave={() => setDragOn(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOn(false);
            take(Array.from(e.dataTransfer.files));
          }}
        >
          <div className={cn("flex items-center justify-center gap-3", empty ? "py-7" : "py-2")}>
            <span className="grid h-10 w-10 place-items-center rounded-md bg-sky-wash text-sky">
              <IconCloudUpload size={22} />
            </span>
            <div className="font-semibold">{uploading ? "上传中" : "把文件拖到这里"}</div>
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
        </div>
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
        <div className="panel">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>文件</TableHead>
                <TableHead className="w-40">进度</TableHead>
                <TableHead className="w-[88px]">状态</TableHead>
                <TableHead className="w-[148px] text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pending.map((n) => (
                <TableRow key={`p-${n}`}>
                  <TableCell>{n}</TableCell>
                  <TableCell>
                    <Progress value={35} animated />
                  </TableCell>
                  <TableCell>
                    <Badge>上传中</Badge>
                  </TableCell>
                  <TableCell />
                </TableRow>
              ))}
              {docs.map((d) => {
                const stage = fileStage(d);
                return (
                  <TableRow
                    key={d.id}
                    id={`bid-doc-${d.id}`}
                    className={focusId === d.id ? "bg-sky-wash" : undefined}
                  >
                    <TableCell>{d.file_name}</TableCell>
                    <TableCell>
                      <Progress
                        value={stage.progress}
                        animated={stage.busy}
                        tone={stage.tone === "rose" ? "stop" : stage.tone === "pine" ? "go" : "sky"}
                      />
                    </TableCell>
                    <TableCell>
                      <Badge tone={TONE[stage.tone]}>{stage.label}</Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex justify-end gap-2">
                        <Button
                          variant="outline"
                          size="sm"
                          data-testid={`preview-${d.id}`}
                          onClick={() => setPreview(d)}
                        >
                          预览
                        </Button>
                        {stage.retryable ? (
                          <Button variant="outline" size="sm" disabled={ended} onClick={() => onRetry(d)}>
                            重试
                          </Button>
                        ) : null}
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </div>
      )}
      <Sheet open={preview != null} onOpenChange={(open) => { if (!open) setPreview(null); }}>
        <SheetContent title={preview?.file_name ?? "预览"}>
          {preview ? (
            <div className="tender-preview-scroll">
              <FilePreview fileName={preview.file_name} objectKey={`objects/${preview.original_sha256}`} />
            </div>
          ) : null}
        </SheetContent>
      </Sheet>
    </div>
  );
}
