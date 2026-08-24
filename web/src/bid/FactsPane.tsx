import { Button, TextInput } from "@mantine/core";
import type { FactSuggestion, Project } from "../api";

const FIELDS: { key: string; label: string; kind: "amount" | "time" | "days" }[] = [
  { key: "budget_amount", label: "预算", kind: "amount" },
  { key: "ceiling_price", label: "最高限价", kind: "amount" },
  { key: "expires_at", label: "投标截止", kind: "time" },
  { key: "bid_open_at", label: "开标时间", kind: "time" },
  { key: "bid_valid_until", label: "有效期截止", kind: "time" },
  { key: "bid_valid_days", label: "有效期天数", kind: "days" },
];

export function FactsPane({
  project,
  suggestions,
  drafts,
  ended,
  onAccept,
  onSet,
  onClear,
  onChangeDraft,
  onChangeCeilingBasis,
}: {
  project: Project;
  suggestions: FactSuggestion[];
  drafts: Record<string, string>;
  ended: boolean;
  onAccept: (s: FactSuggestion) => void;
  onSet: (field: string, value: string) => void;
  onClear: (field: string) => void;
  onChangeDraft: (field: string, value: string) => void;
  onChangeCeilingBasis: (basis: string) => void;
}) {
  const conflict = project.bid_valid_days != null && project.bid_valid_until != null;
  return (
    <div className="stack">
      {conflict && (
        <div className="banner warn" data-testid="validity-conflict">
          有效期天数与截止日期同时存在，正式 PDF 会被拒绝。请只保留一项。
        </div>
      )}
      {project.ceiling_price && project.ceiling_basis === "unspecified" && (
        <div className="banner warn" data-testid="ceiling-unspecified">
          已有最高限价但口径未标明含税/未税，不能 finalize 报价。
        </div>
      )}
      <div className="card">
        <h3 className="h3">项目事实</h3>
        <p className="note">接受建议或人工写入。限价口径必须明确。revision {project.fact_revision}</p>
        <div className="stack" style={{ marginTop: 16 }}>
          {FIELDS.map((f) => {
            const current = (project as unknown as Record<string, unknown>)[f.key];
            const related = suggestions.filter((s) => s.field === f.key);
            return (
              <div key={f.key} className="inner">
                <p className="lbl">{f.label}</p>
                <div className="row" style={{ marginBottom: 8 }}>
                  <p className="note">
                    当前：{current == null || current === "" ? "未设置" : String(current)}
                  </p>
                  {current != null && current !== "" && (
                    <Button
                      size="compact-sm"
                      variant="subtle"
                      color="red"
                      disabled={ended}
                      data-testid={`fact-clear-${f.key}`}
                      onClick={() => onClear(f.key)}
                    >
                      清除
                    </Button>
                  )}
                </div>
                {related.map((s) => (
                  <div key={s.id} className="row" style={{ marginBottom: 8 }}>
                    <span className="note">{JSON.stringify(s.typed_value)}</span>
                    <Button size="compact-sm" disabled={ended} onClick={() => onAccept(s)}>
                      接受建议
                    </Button>
                  </div>
                ))}
                <div className="row">
                  <TextInput
                    data-testid={`fact-${f.key}`}
                    value={drafts[f.key] ?? ""}
                    onChange={(e) => onChangeDraft(f.key, e.currentTarget.value)}
                    placeholder={f.kind === "amount" ? "100000.00" : f.kind === "days" ? "90" : "2026-12-31T16:00:00+08:00"}
                    style={{ flex: 1 }}
                  />
                  <Button variant="default" disabled={ended} onClick={() => onSet(f.key, drafts[f.key] ?? "")}>
                    写入
                  </Button>
                </div>
              </div>
            );
          })}
          <div className="inner">
            <p className="lbl">最高限价口径</p>
            <div className="row">
              {["tax_inclusive", "tax_exclusive", "unspecified"].map((basis) => (
                <Button
                  key={basis}
                  size="compact-sm"
                  variant={project.ceiling_basis === basis ? "filled" : "default"}
                  disabled={ended}
                  onClick={() => onChangeCeilingBasis(basis)}
                >
                  {basis === "tax_inclusive" ? "含税" : basis === "tax_exclusive" ? "未税" : "未标明"}
                </Button>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
