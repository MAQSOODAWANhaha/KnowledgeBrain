import type { BidDoc, BookletPart, Clause, MatchUnit } from "../api";
import { bidHref } from "../hash";
import { catalogKeys, partTitle } from "./helpers";

function IcoList() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
    </svg>
  );
}
function IcoDoc() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  );
}
function IcoFile() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6M8 13h8M8 17h5" />
    </svg>
  );
}
function IcoBiz() {
  return (
    <svg viewBox="0 0 24 24">
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M8 4V3h8v1M8 10h8M8 14h5" />
    </svg>
  );
}
function IcoTech() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M3 7.5 12 3l9 4.5v9L12 21l-9-4.5z" />
      <path d="M12 12 3 7.5M12 12v9M12 12l9-4.5" />
    </svg>
  );
}

export function BidSidebar({
  id,
  view,
  part,
  units,
  booklet,
  docs,
  clauses,
}: {
  id: string;
  view: string;
  part: string;
  units: MatchUnit[];
  booklet: BookletPart[];
  docs: BidDoc[];
  clauses: Clause[];
}) {
  const techUnits = units.filter((u) => u.kind === "technical");
  const job = view === "booklet" ? "booklet" : view === "files" ? "files" : "eval";
  const open = clauses.filter((c) => c.status !== "superseded");
  const pending = (pred: (c: Clause) => boolean) => open.filter((c) => pred(c) && c.status === "draft").length;
  const commercialPending = pending((c) => c.family === "commercial");
  const unsectionedPending = pending((c) => c.family === "technical" && !c.unit_id);
  const fileFail = docs.some((d) => d.parse_status === "failed");
  const fileBusy = docs.some((d) => d.parse_status === "pending" || d.parse_status === "processing");
  const fileBadge = fileFail ? "失败" : fileBusy ? "解析中" : docs.length || undefined;
  const evalHref = docs.length === 0 ? bidHref(id, "files") : bidHref(id, "commercial");
  return (
    <>
      <div className="side-sec">作业</div>
      <nav className="sidenav">
        <a className={job === "eval" ? "on" : undefined} href={`#${evalHref}`}>
          <IcoList />
          <em>评估</em>
          <span>{open.filter((c) => c.status === "draft").length || undefined}</span>
        </a>
        <a className={job === "booklet" ? "on" : undefined} href={`#${bidHref(id, "booklet", { part: booklet[0]?.key || "1", pane: "draft" })}`}>
          <IcoDoc />
          <em>成稿</em>
        </a>
        <a className={job === "files" ? "on" : undefined} href={`#${bidHref(id, "files")}`}>
          <IcoFile />
          <em>文件</em>
          <span>{fileBadge}</span>
        </a>
      </nav>
      {job === "booklet" ? (
        <>
          <div className="side-sec">分册</div>
          <nav className="sidenav">
            {catalogKeys(units, booklet).map((key) => {
              const stale = booklet.find((p) => p.key === key)?.stale;
              return (
                <a
                  key={key}
                  className={view === "booklet" && part === key ? "flt" : undefined}
                  href={`#${bidHref(id, "booklet", { part: key, pane: "draft" })}`}
                >
                  <IcoDoc />
                  <em>{partTitle(key, units)}</em>
                  {stale && <span>过期</span>}
                </a>
              );
            })}
          </nav>
        </>
      ) : (
        <>
          <div className="side-sec">分段</div>
          <nav className="sidenav">
            <a className={view === "commercial" ? "flt" : undefined} href={`#${bidHref(id, "commercial")}`}>
              <IcoBiz />
              <em>商务</em>
              <span>{commercialPending || undefined}</span>
            </a>
            {techUnits.map((u) => (
              <a
                key={u.id as string}
                className={view === u.id ? "flt" : undefined}
                href={`#${bidHref(id, u.id as string)}`}
              >
                <IcoTech />
                <em>{u.heading_path || "未命名段"}</em>
                <span>{u.technical_count || undefined}</span>
              </a>
            ))}
            <a className={view === "unsectioned" ? "flt" : undefined} href={`#${bidHref(id, "unsectioned")}`}>
              <IcoList />
              <em>未归段</em>
              <span>{unsectionedPending || undefined}</span>
            </a>
          </nav>
        </>
      )}
    </>
  );
}
