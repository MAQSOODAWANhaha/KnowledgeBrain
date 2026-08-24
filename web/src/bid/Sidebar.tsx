import type { ReactNode } from "react";
import type { BidDoc, Clause, MatchUnit, PartStatus } from "../api";
import { type BidStep, bidHref } from "../hash";
import { catalogKeys, fileStage, partTitle } from "./helpers";

function Item({
  href,
  on,
  sub,
  label,
  badge,
  tone,
  testId,
}: {
  href: string;
  on?: boolean;
  sub?: boolean;
  label: string;
  badge?: string | number;
  tone?: "pine" | "amber" | "rose";
  testId?: string;
}) {
  return (
    <a className={[on ? "flt" : "", sub ? "sub" : ""].filter(Boolean).join(" ") || undefined} href={`#${href}`} data-testid={testId}>
      <em>{label}</em>
      {badge !== undefined && badge !== "" && <span className={tone}>{badge}</span>}
    </a>
  );
}

export function BidSidebar({
  id,
  step,
  view,
  part,
  doc,
  units,
  parts,
  requiredKeys,
  clauses,
  docs,
}: {
  id: string;
  step: BidStep;
  view: string;
  part: string;
  doc: string | null;
  units: MatchUnit[];
  parts: PartStatus[];
  requiredKeys: string[];
  clauses: Clause[];
  docs: BidDoc[];
}) {
  const pending = clauses.filter((c) => c.status === "draft").length;
  const confirmed = clauses.filter((c) => c.status === "confirmed").length;
  const tech = units.filter((u) => u.kind === "technical" || u.kind === "unsectioned");
  const keys = catalogKeys(requiredKeys, units);

  let tree: ReactNode = null;
  if (step === "files") {
    tree = (
      <nav className="sidenav">
        {docs.map((d) => {
          const st = fileStage(d);
          return (
            <Item
              key={d.id}
              href={bidHref(id, "files", { step: "files", doc: d.id })}
              on={doc === d.id}
              label={d.file_name}
              badge={st.label}
              tone={st.tone === "gray" ? undefined : st.tone}
            />
          );
        })}
      </nav>
    );
  } else if (step === "facts") {
    tree = (
      <nav className="sidenav">
        <Item href={bidHref(id, "facts", { step: "facts" })} on={view === "facts"} label="项目事实" />
        <Item href={bidHref(id, "pending", { step: "facts" })} on={view === "pending"} label="待确认" badge={pending} tone="amber" testId="nav-pending" />
        <Item href={bidHref(id, "confirmed", { step: "facts" })} on={view === "confirmed"} label="已确认" badge={confirmed} tone="pine" />
      </nav>
    );
  } else if (step === "matching") {
    tree = (
      <nav className="sidenav">
        <Item href={bidHref(id, "commercial", { step: "matching" })} on={view === "commercial"} label="商务" />
        {tech.map((u) => {
          const v = u.kind === "unsectioned" || !u.id ? "unsectioned" : u.id;
          return <Item key={u.route_id || v} href={bidHref(id, v, { step: "matching" })} on={view === v} label={u.heading_path} />;
        })}
      </nav>
    );
  } else if (step === "quote") {
    tree = (
      <nav className="sidenav">
        <Item href={bidHref(id, "quote", { step: "quote" })} on={view === "quote"} label="人工报价" testId="nav-quote" />
        <Item href={bidHref(id, "company", { step: "quote" })} on={view === "company"} label="公司资料" />
        <Item href={bidHref(id, "submission", { step: "quote" })} on={view === "submission"} label="投标资料" />
        <Item href={bidHref(id, "procedural", { step: "quote" })} on={view === "procedural"} label="程序附件" testId="nav-procedural" />
      </nav>
    );
  } else {
    tree = (
      <nav className="sidenav">
        {keys.map((key) => {
          const stale = parts.find((p) => p.part_key === key)?.stale;
          return (
            <Item
              key={key}
              href={bidHref(id, "parts", { step: "parts", part: key })}
              on={part === key}
              label={partTitle(key, units)}
              badge={stale ? "stale" : undefined}
              tone={stale ? "amber" : undefined}
            />
          );
        })}
      </nav>
    );
  }

  return (
    <>
      <div className="side-sec">本标</div>
      {tree}
    </>
  );
}
