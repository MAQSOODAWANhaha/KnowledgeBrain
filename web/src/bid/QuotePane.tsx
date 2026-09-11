import type { QuoteLine, QuoteState } from "../api";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Label } from "../components/ui/label";

function NativeSelect({
  value,
  onChange,
  disabled,
  options,
  testId,
}: {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  options: { value: string; label: string }[];
  testId?: string;
}) {
  return (
    <select
      data-testid={testId}
      className="flex h-9 w-full rounded-[6px] border border-input-line bg-white px-2 text-sm text-ink disabled:opacity-50"
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.currentTarget.value)}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
  );
}

export function QuotePane({
  project,
  quote,
  preview,
  ended,
  saving,
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
  project: { ceiling_price: string | null };
  quote: QuoteState;
  preview?: { net_total?: string; tax_total?: string; gross_total?: string };
  ended: boolean;
  saving: boolean;
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
        <Button data-testid="quote-create" className="mt-4" disabled={ended || saving} onClick={onCreate}>
          创建报价草稿
        </Button>
      </div>
    );
  }
  const draft = quote.pointer === "draft";
  const nullable = (value: string) => value.trim() || null;
  return (
    <div className="stack">
      <div className="card">
        <h3 className="h3">{draft ? "报价草稿" : "已定稿"}</h3>
        <p className="note">
          {quote.tax_mode} · {quote.status || quote.pointer} · eligibility {quote.eligibility || "—"}
        </p>
        {draft && (
          <div className="stack mt-3">
            <div>
              <Label>标题</Label>
              <Input
                data-testid="quote-title"
                defaultValue={quote.title ?? ""}
                key={quote.edit_version}
                disabled={saving}
                onBlur={(e) =>
                  onPatch(e.currentTarget.value, quote.tax_mode || "tax_exclusive", quote.notes || "")
                }
              />
            </div>
            <div>
              <Label>税模式</Label>
              <NativeSelect
                value={quote.tax_mode || "tax_exclusive"}
                disabled={saving}
                options={[
                  { value: "tax_exclusive", label: "未税计价" },
                  { value: "tax_inclusive", label: "含税计价" },
                ]}
                onChange={(v) => onPatch(quote.title || "报价", v || "tax_exclusive", quote.notes || "")}
              />
            </div>
          </div>
        )}
        {preview && (
          <p className="note">
            净 {preview.net_total} · 税 {preview.tax_total} · 含税 {preview.gross_total}
          </p>
        )}
        {!project.ceiling_price && draft && (
          <div className="inner mt-3">
            <label className="flex items-center gap-2 text-sm">
              <input
                data-testid="no-ceiling-review"
                type="checkbox"
                checked={noCeiling}
                disabled={saving}
                onChange={(e) => onNoCeiling(e.currentTarget.checked, noCeilingReason)}
              />
              招标未设最高限价，已人工复核
            </label>
            <Input
              className="mt-2"
              value={noCeilingReason}
              disabled={saving}
              onChange={(e) => onNoCeiling(noCeiling, e.currentTarget.value)}
              placeholder="复核原因"
            />
          </div>
        )}
        <div className="row mt-4">
          {draft && (
            <Button data-testid="quote-finalize" disabled={ended || saving} onClick={onFinalize}>
              定稿
            </Button>
          )}
          {!draft && quote.snapshot_id && (
            <Button data-testid="quote-reopen" variant="outline" disabled={ended || saving} onClick={onReopen}>
              重开
            </Button>
          )}
        </div>
      </div>
      {draft && (
        <div className="card pad-0">
          <div className="toolbar">
            <span>行</span>
            <Button data-testid="quote-add-line" size="sm" disabled={ended || saving} onClick={onAddLine}>
              增行
            </Button>
          </div>
          <table className="grid" style={{ minWidth: 1120 }}>
            <thead>
              <tr>
                <th>说明</th>
                <th>方式</th>
                <th>数量</th>
                <th>单位</th>
                <th>单价</th>
                <th>总价</th>
                <th>税率</th>
                <th>确认</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {(quote.lines ?? []).map((line) => (
                <tr key={line.id} data-testid={`quote-line-${line.id}`}>
                  <td>
                    <Input
                      data-testid={`quote-line-description-${line.id}`}
                      key={`description-${quote.edit_version}-${line.description}`}
                      defaultValue={line.description}
                      disabled={saving}
                      onBlur={(e) => {
                        const description = e.currentTarget.value;
                        if (description !== line.description)
                          onUpdateLine(line, { description, user_confirmed: false });
                      }}
                    />
                  </td>
                  <td>
                    <NativeSelect
                      testId={`quote-line-pricing-mode-${line.id}`}
                      value={line.pricing_mode}
                      disabled={saving}
                      options={[
                        { value: "lump_sum", label: "总价计价" },
                        { value: "unit_price", label: "单价计价" },
                      ]}
                      onChange={(value) => {
                        if (!value || value === line.pricing_mode) return;
                        onUpdateLine(
                          line,
                          value === "lump_sum"
                            ? {
                                pricing_mode: value,
                                quantity: null,
                                unit: null,
                                unit_price: null,
                                user_confirmed: false,
                              }
                            : { pricing_mode: value, entered_amount: null, user_confirmed: false },
                        );
                      }}
                    />
                  </td>
                  <td>
                    {line.pricing_mode === "unit_price" ? (
                      <Input
                        data-testid={`quote-line-quantity-${line.id}`}
                        key={`quantity-${quote.edit_version}-${line.quantity ?? ""}`}
                        defaultValue={line.quantity ?? ""}
                        disabled={saving}
                        onBlur={(e) => {
                          const quantity = nullable(e.currentTarget.value);
                          if (quantity !== line.quantity)
                            onUpdateLine(line, { quantity, user_confirmed: false });
                        }}
                      />
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td>
                    {line.pricing_mode === "unit_price" ? (
                      <Input
                        data-testid={`quote-line-unit-${line.id}`}
                        key={`unit-${quote.edit_version}-${line.unit ?? ""}`}
                        defaultValue={line.unit ?? ""}
                        disabled={saving}
                        onBlur={(e) => {
                          const unit = nullable(e.currentTarget.value);
                          if (unit !== line.unit) onUpdateLine(line, { unit, user_confirmed: false });
                        }}
                      />
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td>
                    {line.pricing_mode === "unit_price" ? (
                      <Input
                        data-testid={`quote-line-unit-price-${line.id}`}
                        key={`unit-price-${quote.edit_version}-${line.unit_price ?? ""}`}
                        defaultValue={line.unit_price ?? ""}
                        disabled={saving}
                        onBlur={(e) => {
                          const unitPrice = nullable(e.currentTarget.value);
                          if (unitPrice !== line.unit_price)
                            onUpdateLine(line, { unit_price: unitPrice, user_confirmed: false });
                        }}
                      />
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td>
                    {line.pricing_mode === "lump_sum" ? (
                      <Input
                        data-testid={`quote-line-entered-amount-${line.id}`}
                        key={`entered-amount-${quote.edit_version}-${line.entered_amount ?? ""}`}
                        defaultValue={line.entered_amount ?? ""}
                        disabled={saving}
                        onBlur={(e) => {
                          const enteredAmount = nullable(e.currentTarget.value);
                          if (enteredAmount !== line.entered_amount) {
                            onUpdateLine(line, { entered_amount: enteredAmount, user_confirmed: false });
                          }
                        }}
                      />
                    ) : (
                      <span className="muted">{line.gross_amount || "—"}</span>
                    )}
                  </td>
                  <td>
                    <Input
                      data-testid={`quote-line-tax-rate-${line.id}`}
                      key={`tax-rate-${quote.edit_version}-${line.tax_rate}`}
                      defaultValue={line.tax_rate}
                      disabled={saving}
                      onBlur={(e) => {
                        const taxRate = e.currentTarget.value.trim();
                        if (taxRate !== line.tax_rate)
                          onUpdateLine(line, { tax_rate: taxRate, user_confirmed: false });
                      }}
                    />
                  </td>
                  <td>
                    <input
                      data-testid={`quote-line-confirmed-${line.id}`}
                      type="checkbox"
                      checked={line.user_confirmed}
                      disabled={saving}
                      onChange={(e) => onUpdateLine(line, { user_confirmed: e.currentTarget.checked })}
                    />
                  </td>
                  <td>
                    <Button size="sm" variant="destructive" disabled={saving} onClick={() => onDeleteLine(line)}>
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
