import { Button } from "@astryxdesign/core/Button";
import { Center } from "@astryxdesign/core/Center";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Table } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { useEffect, useRef, useState } from "react";
import { token } from "../api";

/** WeKnora-style original preview. PPTX uses `pptx-preview` (vanilla), not Vue `@vue-office/pptx`. */
type Kind = "pdf" | "docx" | "pptx" | "image" | "excel" | "text" | "unsupported";

function kindOf(name: string): Kind {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "pdf") return "pdf";
  if (ext === "docx") return "docx";
  if (ext === "pptx") return "pptx";
  if (["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tiff"].includes(ext)) return "image";
  if (["xlsx", "xls", "csv"].includes(ext)) return "excel";
  if (["txt", "md", "markdown", "json", "xml", "html", "css", "js", "ts", "py", "go", "rs", "yml", "yaml", "log"].includes(ext)) {
    return "text";
  }
  return "unsupported";
}

async function fetchBlob(objectKey: string): Promise<Blob> {
  const headers = new Headers();
  const t = token();
  if (t) headers.set("Authorization", `Bearer ${t}`);
  const res = await fetch(`/api/v1/files?key=${encodeURIComponent(objectKey)}`, { headers });
  if (!res.ok) throw new Error("读文件失败");
  return res.blob();
}

async function renderPdf(blob: Blob, el: HTMLElement, cancelled: () => boolean) {
  const { getDocument, GlobalWorkerOptions } = await import("pdfjs-dist");
  GlobalWorkerOptions.workerSrc = new URL(
    "../../node_modules/pdfjs-dist/build/pdf.worker.min.mjs",
    import.meta.url,
  ).toString();
  const data = new Uint8Array(await blob.arrayBuffer());
  const pdf = await getDocument({ data }).promise;
  try {
    if (cancelled()) return;
    el.innerHTML = "";
    const width = Math.max(320, el.clientWidth || el.parentElement?.clientWidth || 800);
    for (let n = 1; n <= pdf.numPages; n++) {
      const page = await pdf.getPage(n);
      if (cancelled()) return;
      const base = page.getViewport({ scale: 1 });
      const scale = Math.max(0.5, (width - 8) / base.width);
      const viewport = page.getViewport({ scale });
      const canvas = document.createElement("canvas");
      canvas.style.display = "block";
      canvas.style.margin = "0 auto 8px";
      canvas.width = Math.floor(viewport.width);
      canvas.height = Math.floor(viewport.height);
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("预览容器未就绪");
      el.appendChild(canvas);
      await page.render({ canvasContext: ctx, viewport }).promise;
    }
  } finally {
    await pdf.destroy();
  }
}

export function FilePreview({ fileName, objectKey }: { fileName: string; objectKey: string }) {
  const kind = kindOf(fileName);
  const host = useRef<HTMLElement>(null);
  const [text, setText] = useState("");
  const [sheets, setSheets] = useState<{ name: string; rows: string[][] }[]>([]);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    let dead = false;
    let blobUrl: string | null = null;
    let pptx: { preview: (file: ArrayBuffer) => Promise<unknown>; destroy: () => void } | null = null;
    setBusy(true);
    setErr("");
    setText("");
    setSheets([]);
    if (host.current) host.current.innerHTML = "";
    (async () => {
      if (kind === "unsupported") {
        setBusy(false);
        return;
      }
      try {
        const blob = await fetchBlob(objectKey);
        if (dead) return;
        if (kind === "pdf") {
          const el = host.current;
          if (!el) throw new Error("预览容器未就绪");
          await renderPdf(blob, el, () => dead);
        } else if (kind === "image") {
          // Rendered into the host like the PDF pages: full size, scaled to the preview width.
          const el = host.current;
          if (!el) throw new Error("预览容器未就绪");
          blobUrl = URL.createObjectURL(blob);
          const image = document.createElement("img");
          image.src = blobUrl; image.alt = fileName;
          image.style.maxWidth = "100%"; image.style.display = "block"; image.style.margin = "0 auto";
          el.appendChild(image);
        } else if (kind === "docx") {
          const { renderAsync } = await import("docx-preview");
          if (dead) return;
          const el = host.current;
          if (!el) throw new Error("预览容器未就绪");
          await renderAsync(blob, el, undefined, {
            className: "docx-preview-wrapper",
            inWrapper: true,
            breakPages: true,
            useBase64URL: true,
          });
        } else if (kind === "pptx") {
          const { init } = await import("pptx-preview");
          if (dead) return;
          const el = host.current;
          if (!el) throw new Error("预览容器未就绪");
          const width = Math.max(640, Math.min(el.parentElement?.clientWidth || el.clientWidth || 960, 1100));
          const height = Math.round(width * 0.5625);
          pptx = init(el, { width, height, mode: "list" });
          await pptx.preview(await blob.arrayBuffer());
        } else if (kind === "excel") {
          const XLSX = await import("xlsx");
          const buf = await blob.arrayBuffer();
          const wb = fileName.toLowerCase().endsWith(".csv")
            ? XLSX.read(await blob.text(), { type: "string" })
            : XLSX.read(buf, { type: "array" });
          setSheets(
            wb.SheetNames.map((name) => ({
              name,
              rows: (XLSX.utils.sheet_to_json(wb.Sheets[name], { header: 1, defval: "" }) as unknown[][]).map((row) =>
                row.map((cell) => (cell == null ? "" : String(cell))),
              ),
            })),
          );
        } else {
          setText(await blob.text());
        }
      } catch (e) {
        if (!dead) setErr(e instanceof Error ? e.message : "预览失败");
      } finally {
        if (!dead) setBusy(false);
      }
    })();
    return () => {
      dead = true;
      if (blobUrl) URL.revokeObjectURL(blobUrl);
      pptx?.destroy();
    };
  }, [fileName, objectKey, kind]);

  const download = <DownloadButton objectKey={objectKey} fileName={fileName} />;
  return (
    <VStack gap={3} padding={4}>
      {busy ? <Center minHeight={240}><Spinner label="正在打开原件…" /></Center> : null}
      {err ? <EmptyState title="原件预览失败" description={err} actions={download} /> : null}
      {!busy && !err && kind === "unsupported" ? (
        <EmptyState title="无法在浏览器中预览"
          description={fileName.toLowerCase().endsWith(".ppt") || fileName.toLowerCase().endsWith(".doc")
            ? "旧版 .ppt / .doc 请下载后用 Office 打开。pptx / docx 可在浏览器里预览。"
            : "浏览器里嵌不了这种格式。请下载原件，解析结果看「解析」和「正文」。"}
          actions={download} />
      ) : null}
      <VStack ref={host} gap={0} isScrollable
        style={{ display: kind === "docx" || kind === "pptx" || kind === "pdf" || kind === "image" ? "flex" : "none", visibility: busy || err ? "hidden" : "visible" }} />
      {!busy && !err && kind === "excel" ? (
        <VStack gap={4}>
          {sheets.map((sheet) => (
            <VStack key={sheet.name} gap={2}>
              {sheets.length > 1 ? <Text weight="semibold">{sheet.name}</Text> : null}
              <Table data={sheet.rows.map((row, index) => ({ id: String(index), cells: row }))} columns={Array.from({ length: Math.max(1, ...sheet.rows.map(row => row.length)) }, (_, column) => ({ key: String(column), header: String(column + 1), renderCell: (row: { cells: string[] }) => row.cells[column] ?? "" }))} />
            </VStack>
          ))}
        </VStack>
      ) : null}
      {!busy && !err && kind === "text" ? <CodeBlock code={text} title={fileName} hasLineNumbers /> : null}
    </VStack>
  );
}

function DownloadButton({ objectKey, fileName }: { objectKey: string; fileName: string }) {
  const [busy, setBusy] = useState(false);
  return <Button variant="primary" label="下载原件" isLoading={busy} onClick={async () => {
    setBusy(true);
    try {
      const url = URL.createObjectURL(await fetchBlob(objectKey));
      const link = Object.assign(document.createElement("a"), { href: url, download: fileName });
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 0);
    } finally { setBusy(false); }
  }} />;
}
