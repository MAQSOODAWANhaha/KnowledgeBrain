import { Button } from "@mantine/core";
import type { BidDetail, Clause, MatchUnit, RoutePickSet } from "../api";
import { kindLabel } from "./helpers";

export function MatchingPane({
  view,
  clauses,
  units,
  matching,
  pickSet,
  ended,
  onSchedule,
}: {
  view: string;
  clauses: Clause[];
  units: MatchUnit[];
  matching?: BidDetail["matching"];
  pickSet: RoutePickSet | null;
  ended: boolean;
  onSchedule: () => void;
}) {
  const commercial = matching?.commercial_decisions ?? [];
  const reports = matching?.reports ?? [];
  if (view === "commercial") {
    return (
      <div className="stack">
        <div className="card">
          <div className="row" style={{ justifyContent: "space-between" }}>
            <h3 className="h3">商务证据</h3>
            <Button data-testid="schedule-match" disabled={ended} variant="default" onClick={onSchedule}>
              调度匹配
            </Button>
          </div>
          <p className="note">按条款展示 supported / review / reject，不做产品排名。</p>
          <table className="grid">
            <thead>
              <tr>
                <th>条款</th>
                <th>判定</th>
                <th>证据</th>
              </tr>
            </thead>
            <tbody>
              {commercial.map((d, i) => {
                const clause = clauses.find((c) => c.id === d.clause_id);
                return (
                  <tr key={`${d.clause_id ?? i}`}>
                    <td>{clause?.text || d.clause_id || "—"}</td>
                    <td>
                      {d.system_decision} / {d.final_support}
                    </td>
                    <td className="muted">{d.frozen_document_display_name || d.reason_code || "—"}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {commercial.length === 0 && <p className="note">还没有 current commercial report。</p>}
        </div>
      </div>
    );
  }
  const unit = units.find((u) => (view === "unsectioned" ? u.kind === "unsectioned" : u.id === view));
  return (
    <div className="stack">
      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h3 className="h3">{unit?.heading_path || "技术 route"}</h3>
          <Button disabled={ended} variant="default" onClick={onSchedule}>
            调度匹配
          </Button>
        </div>
        <p className="note">
          reports {reports.length} · 当前 supported {pickSet?.supported_candidates.length ?? 0} · 已选 {pickSet?.items.length ?? 0}
        </p>
        <p className="note">在右侧检查器勾选 1..N 个 supported 候选。</p>
        {(clauses.filter((c) => c.kind === "technical" && c.status === "confirmed") || []).map((c) => (
          <p key={c.id} className="note">
            {kindLabel(c.kind)} · {c.text}
          </p>
        ))}
      </div>
    </div>
  );
}
