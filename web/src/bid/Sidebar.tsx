import type { ReactNode } from "react";
import type { BidDoc, BookletPart, Clause, MatchUnit } from "../api";
import { type BidStep, bidHref } from "../hash";
import { fileStage, partTitle } from "./helpers";

function IcoList() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
    </svg>
  );
}
function IcoFile() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
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
function IcoDoc() {
  return (
    <svg viewBox="0 0 24 24">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6M8 13h8M8 17h6" />
    </svg>
  );
}

function Item({
  href,
  on,
  sub,
  icon,
  label,
  badge,
  tone,
}: {
  href: string;
  on?: boolean;
  sub?: boolean;
  icon: ReactNode;
  label: string;
  badge?: string | number;
  tone?: "pine" | "amber" | "rose";
}) {
  return (
    <a className={[on ? "flt" : "", sub ? "sub" : ""].filter(Boolean).join(" ") || undefined} href={`#${href}`}>
      {icon}
      <em>{label}</em>
      {badge !== undefined && badge !== "" && <span className={tone}>{badge}</span>}
    </a>
  );
}

function fileBadge(doc: BidDoc): { badge?: string | number; tone?: "pine" | "amber" | "rose" } {
  const st = fileStage(doc);
  if (st.tone === "rose") return { badge: "失败", tone: "rose" };
  if (st.tone === "amber") return { badge: "处理中", tone: "amber" };
  if (st.tone === "pine") return { badge: doc.clause_count || "完成", tone: "pine" };
  if (doc.parse_status === "pending") return { badge: "排队" };
  return {};
}

export function BidSidebar({
  id,
  step,
  view,
  part,
  doc,
  units,
  booklet,
  clauses,
  docs,
}: {
  id: string;
  step: BidStep;
  view: string;
  part: string;
  doc: string | null;
  units: MatchUnit[];
  booklet: BookletPart[];
  clauses: Clause[];
  docs: BidDoc[];
}) {
  const techUnits = units.filter((u) => u.kind === "technical");
  const open = clauses.filter((c) => c.status !== "superseded");
  const pending = (pred: (c: Clause) => boolean) => open.filter((c) => pred(c) && c.status === "draft").length;
  const commercialPending = pending((c) => c.family === "commercial");
  const unsectionedPending = pending((c) => c.family === "technical" && !c.unit_id);
  const bookletKeys = ["1", ...techUnits.map((u) => `2:${u.id}`), "2:unsectioned", "3", "4", "5"];
  const heading = step === "files" ? "招标文件" : step === "eval" ? "评估" : "成稿";

  return (
    <>
      <div className="side-sec">{heading}</div>
      {step === "files" ? (
        <nav className="sidenav tree">
          <Item
            href={bidHref(id, "files", { step: "files" })}
            on={!doc}
            icon={<IcoFile />}
            label={docs.length === 0 ? "还没有文件" : "本标文件"}
            badge={docs.length || undefined}
          />
          {docs.length > 0 && (
            <div className="tree-kids">
              {docs.map((d) => {
                const b = fileBadge(d);
                return (
                  <Item
                    key={d.id}
                    href={bidHref(id, "files", { step: "files", doc: d.id })}
                    on={doc === d.id}
                    sub
                    icon={<IcoFile />}
                    label={d.file_name.replace(/\.[^.]+$/, "")}
                    badge={b.badge}
                    tone={b.tone}
                  />
                );
              })}
            </div>
          )}
        </nav>
      ) : null}

      {step === "eval" ? (
        <nav className="sidenav tree">
          <Item
            href={bidHref(id, "commercial", { step: "eval" })}
            on={view === "commercial"}
            icon={<IcoBiz />}
            label="商务"
            badge={commercialPending || undefined}
          />
          <div className="tree-kids">
            {techUnits.map((u) => (
              <Item
                key={u.id as string}
                href={bidHref(id, u.id as string, { step: "eval" })}
                on={view === u.id}
                sub
                icon={<IcoTech />}
                label={u.heading_path || "未命名段"}
                badge={u.technical_count || undefined}
              />
            ))}
            <Item
              href={bidHref(id, "unsectioned", { step: "eval" })}
              on={view === "unsectioned"}
              sub
              icon={<IcoList />}
              label="未归段"
              badge={unsectionedPending || undefined}
            />
          </div>
        </nav>
      ) : null}

      {step === "booklet" ? (
        <nav className="sidenav tree">
          <Item
            href={bidHref(id, "booklet", { step: "booklet", part: "1", pane: "draft" })}
            on={false}
            icon={<IcoDoc />}
            label="应答卷"
          />
          <div className="tree-kids">
            {bookletKeys.map((key) => {
              const stale = booklet.find((p) => p.key === key)?.stale;
              return (
                <Item
                  key={key}
                  href={bidHref(id, "booklet", { step: "booklet", part: key, pane: "draft" })}
                  on={part === key}
                  sub
                  icon={<IcoDoc />}
                  label={partTitle(key, units)}
                  badge={stale ? "过期" : undefined}
                  tone={stale ? "amber" : undefined}
                />
              );
            })}
          </div>
        </nav>
      ) : null}
    </>
  );
}
