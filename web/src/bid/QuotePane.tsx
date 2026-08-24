import { Button, Checkbox, Select, TextInput } from "@mantine/core";
import type { Project, QuoteLine, QuoteState } from "../api";

export function QuotePane({
  project,
  quote,
  preview,
  ended,
  noCeiling,
  noCeilingReason,
  onNoCeiling,
  onCreate,
  onPatch,
  onAddLine,
  onUpdateLine,
  onDeleteLine,
  onFinalize,
  onReopen,
}: {
  project: Project;
  quote: QuoteState;
  preview?: { net_total?: string; tax_total?: string; gross_total?: string };
  ended: boolean;
  noCeiling: boolean;
  noCeilingReason: string;
  onNoCeiling: (reviewed: boolean, reason: string) => void;
  onCreate: () => void;
  onPatch: (title: string, taxMode: string, notes: string) => void;
  onAddLine: () => void;
  onUpdateLine: (line: QuoteLine, patch: Partial<QuoteLine>) => void;
  onDeleteLine: (line: QuoteLine) => void;
  onFinalize: () => void;
  onReopen: () => void;
}) {
  if (!quote.exists) {
    return (
      <div className="card">
        <h3 className="h3">还没有报价草稿</h3>
        <p className="note">价格由人录入。系统不算正式价。</p>
        <Button data-testid="quote-create" mt="md" disabled={ended} onClick={onCreate}>
          创建报价草稿
        </Button>
      </div>
    );
  }
  const draft = quote.pointer === "draft";
  return (
    <div className="stack">
      <div className="card">
        <h3 className="h3">{draft ? "报价草稿" : "已定稿"}</h3>
        <p className="note">
          {quote.tax_mode} · {quote.status || quote.pointer} · eligibility {quote.eligibility || "—"}
        </p>
        {draft && (
          <div className="stack" style={{ marginTop: 12 }}>
            <TextInput
              data-testid="quote-title"
              label="标题"
              defaultValue={quote.title ?? ""}
              key={quote.edit_version}
              onBlur={(e) => onPatch(e.currentTarget.value, quote.tax_mode || "tax_exclusive", quote.notes || "")}
            />
            <Select
              label="税模式"
              data={[
                { value: "tax_exclusive", label: "未税计价" },
                { value: "tax_inclusive", label: "含税计价" },
              ]}
              value={quote.tax_mode}
              allowDeselect={false}
              onChange={(v) => onPatch(quote.title || "报价", v || "tax_exclusive", quote.notes || "")}
            />
          </div>
        )}
        {preview && (
          <p className="note">
            净 {preview.net_total} · 税 {preview.tax_total} · 含税 {preview.gross_total}
          </p>
        )}
        {!project.ceiling_price && draft && (
          <div className="inner" style={{ marginTop: 12 }}>
            <Checkbox
              data-testid="no-ceiling-review"
              label="招标未设最高限价，已人工复核"
              checked={noCeiling}
              onChange={(e) => onNoCeiling(e.currentTarget.checked, noCeilingReason)}
            />
            <TextInput mt="sm" value={noCeilingReason} onChange={(e) => onNoCeiling(noCeiling, e.currentTarget.value)} placeholder="复核原因" />
          </div>
        )}
        <div className="row" style={{ marginTop: 16 }}>
          {draft && (
            <Button data-testid="quote-finalize" disabled={ended} onClick={onFinalize}>
              定稿
            </Button>
          )}
          {!draft && quote.snapshot_id && (
            <Button data-testid="quote-reopen" variant="default" disabled={ended} onClick={onReopen}>
              重开
            </Button>
          )}
        </div>
      </div>
      {draft && (
        <div className="card pad-0">
          <div className="toolbar">
            <span>行</span>
            <Button size="compact-sm" disabled={ended} onClick={onAddLine}>
              增行
            </Button>
          </div>
          <table className="grid">
            <thead>
              <tr>
                <th>说明</th>
                <th>方式</th>
                <th>金额</th>
                <th>确认</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {(quote.lines ?? []).map((line) => (
                <tr key={line.id} data-testid={`quote-line-${line.id}`}>
                  <td>
                    <TextInput
                      defaultValue={line.description}
                      onBlur={(e) => onUpdateLine(line, { description: e.currentTarget.value })}
                    />
                  </td>
                  <td>{line.pricing_mode}</td>
                  <td className="muted">{line.gross_amount || line.entered_amount || line.unit_price || "—"}</td>
                  <td>
                    <Checkbox
                      data-testid={`quote-line-confirmed-${line.id}`}
                      checked={line.user_confirmed}
                      onChange={(e) => onUpdateLine(line, { user_confirmed: e.currentTarget.checked })}
                    />
                  </td>
                  <td>
                    <Button size="compact-sm" variant="subtle" color="red" onClick={() => onDeleteLine(line)}>
                      删
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
