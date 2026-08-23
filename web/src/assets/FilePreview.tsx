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

export function FilePreview({ fileName, objectKey }: { fileName: string; objectKey: string }) {
  const kind = kindOf(fileName);
  const host = useRef<HTMLDivElement>(null);
  const [url, setUrl] = useState<string | null>(null);
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
    setUrl(null);
    if (host.current) host.current.innerHTML = "";
    (async () => {
      if (kind === "unsupported") {
        setBusy(false);
        return;
      }
      try {
        const blob = await fetchBlob(objectKey);
        if (dead) return;
        if (kind === "pdf" || kind === "image") {
          blobUrl = URL.createObjectURL(blob);
          setUrl(blobUrl);
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

  return (
    <div className="file-preview">
      {busy ? <div className="preview-empty">正在打开原件…</div> : null}
      {err ? (
        <div className="preview-empty">
          {err}
          <div style={{ marginTop: 12 }}>
            <DownloadLink objectKey={objectKey} fileName={fileName} />
          </div>
        </div>
      ) : null}
      {!busy && !err && kind === "unsupported" ? (
        <div className="preview-empty">
          {fileName.toLowerCase().endsWith(".ppt") || fileName.toLowerCase().endsWith(".doc")
            ? "旧版 .ppt / .doc 请下载后用 Office 打开。pptx / docx 可在浏览器里预览。"
            : "浏览器里嵌不了这种格式。请下载原件，解析结果看「解析」和「分片」。"}
          <div style={{ marginTop: 12 }}>
            <DownloadLink objectKey={objectKey} fileName={fileName} />
          </div>
        </div>
      ) : null}
      {!busy && !err && kind === "pdf" && url ? <iframe className="preview-frame" title={fileName} src={url} /> : null}
      {!busy && !err && kind === "image" && url ? (
        <div className="preview-image">
          <img src={url} alt={fileName} />
        </div>
      ) : null}
      <div
        ref={host}
        className={kind === "pptx" ? "preview-pptx" : "preview-docx"}
        style={{
          display: kind === "docx" || kind === "pptx" ? "block" : "none",
          visibility: busy || err ? "hidden" : "visible",
        }}
      />
      {!busy && !err && kind === "excel" ? (
        <div className="preview-excel">
          {sheets.map((sheet) => (
            <div key={sheet.name} className="excel-sheet">
              {sheets.length > 1 ? <div className="excel-sheet-name">{sheet.name}</div> : null}
              <table className="grid">
                <tbody>
                  {sheet.rows.map((row, i) => (
                    <tr key={i}>
                      {row.map((cell, j) => (
                        <td key={j}>{cell}</td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))}
        </div>
      ) : null}
      {!busy && !err && kind === "text" ? <pre className="preview-text">{text}</pre> : null}
    </div>
  );
}

function DownloadLink({ objectKey, fileName }: { objectKey: string; fileName: string }) {
  const [href, setHref] = useState<string | null>(null);
  useEffect(() => {
    let u: string | null = null;
    fetchBlob(objectKey)
      .then((b) => {
        u = URL.createObjectURL(b);
        setHref(u);
      })
      .catch(() => undefined);
    return () => {
      if (u) URL.revokeObjectURL(u);
    };
  }, [objectKey]);
  if (!href) return null;
  return (
    <a className="btn pri" href={href} download={fileName}>
      下载原件
    </a>
  );
}
