import type { BookletPart, Clause, MatchUnit } from "../api";

export const NIL = "00000000-0000-0000-0000-000000000000";

export function fileLabel(s: string): string {
  if (s === "completed") return "已解析";
  if (s === "processing") return "解析中";
  if (s === "pending") return "排队";
  if (s === "failed") return "失败";
  return s;
}

export function suggestionLabel(s?: string): string {
  if (s === "cover") return "覆盖";
  if (s === "pending") return "待勾选";
  if (s === "need_rematch") return "需重配";
  if (s === "unmet" || s === "uncovered") return "未覆盖";
  return s || "—";
}

export function assessmentLabel(s?: string): string {
  if (s === "meet") return "满足";
  if (s === "partial") return "部分";
  if (s === "deviate") return "偏离";
  if (s === "fail") return "不响应";
  return "未评";
}

export function partTitle(key: string, units: MatchUnit[]): string {
  if (key === "1") return "① 扉页";
  if (key === "3") return "③ 偏离表";
  if (key === "4") return "④ 资格材料";
  if (key === "5") return "⑤ 商务缺件";
  if (key === "2:unsectioned") return "② 未归段";
  if (key.startsWith("2:")) {
    const id = key.slice(2);
    const u = units.find((x) => x.id === id);
    return `② ${u?.heading_path || "技术段"}`;
  }
  return key;
}

export function unitIdForView(view: string): string | null {
  if (view === "commercial" || view === "booklet" || view === "files") return null;
  if (view === "unsectioned") return NIL;
  return view;
}

export function bookletKeyFor(view: string, part: string, selected: string | null, clauses: Clause[]): string {
  if (view === "booklet") return part;
  if (view === "commercial") {
    const hit = clauses.find((c) => c.id === selected)?.hit_outcome;
    return hit === "miss" ? "5" : "4";
  }
  if (view === "unsectioned") return "2:unsectioned";
  return `2:${view}`;
}

export function liveClauses(clauses: Clause[], view: string): Clause[] {
  const open = clauses.filter((c) => c.status !== "superseded");
  if (view === "commercial") return open.filter((c) => c.family === "commercial");
  if (view === "booklet" || view === "files") return [];
  if (view === "unsectioned") return open.filter((c) => c.family === "technical" && (!c.unit_id || c.unit_id === NIL));
  return open.filter((c) => c.family === "technical" && c.unit_id === view);
}

export function catalogKeys(units: MatchUnit[], booklet: BookletPart[]): string[] {
  const tech = units.filter((u) => u.kind === "technical");
  const twos = [
    ...tech.map((u) => `2:${u.id}`),
    "2:unsectioned",
  ].filter((key) => booklet.some((p) => p.key === key));
  return ["1", ...twos, "3", "4", "5"];
}
