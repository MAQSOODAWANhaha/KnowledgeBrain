import { Button } from "@astryxdesign/core/Button";
import { Table } from "@astryxdesign/core/Table";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Selector } from "@astryxdesign/core/Selector";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useEffect, useState } from "react";
import type { QuoteLine, QuoteState } from "../api";

function CommitField({ label, testId, value, disabled, hideLabel = false, onCommit }: {
  label: string; testId?: string; value: string; disabled?: boolean; hideLabel?: boolean; onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return <div onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null) && draft !== value) onCommit(draft); }}>
    <TextInput label={label} isLabelHidden={hideLabel} data-testid={testId} value={draft} isDisabled={disabled} onChange={setDraft} />
  </div>;
}

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
  return <Selector data-testid={testId} label={testId ?? "选项"} isLabelHidden value={value} isDisabled={disabled} onChange={onChange} options={options} />;

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
        <Button data-testid="quote-create" label="创建报价草稿" variant="primary" isDisabled={ended || saving} onClick={onCreate} />
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
            <CommitField key={quote.edit_version} label="标题" testId="quote-title" value={quote.title ?? ""} disabled={saving} onCommit={(value) => onPatch(value, quote.tax_mode || "tax_exclusive", quote.notes || "")} />
            <div>
              <span>税模式</span>
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
            <CheckboxInput data-testid="no-ceiling-review" label="招标未设最高限价，已人工复核" value={noCeiling} isDisabled={saving} onChange={checked => onNoCeiling(checked, noCeilingReason)} />
            <TextInput label="复核原因" isLabelHidden value={noCeilingReason} isDisabled={saving} placeholder="复核原因" onChange={(value) => onNoCeiling(noCeiling, value)} />
          </div>
        )}
        <div className="row mt-4">
          {draft && (
            <Button data-testid="quote-finalize" label="定稿" variant="primary" isDisabled={ended || saving} onClick={onFinalize} />
          )}
          {!draft && quote.snapshot_id && (
            <Button data-testid="quote-reopen" label="重开" variant="secondary" isDisabled={ended || saving} onClick={onReopen} />
          )}
        </div>
      </div>
      {draft && (
        <div className="card pad-0">
          <div className="toolbar">
            <span>行</span>
            <Button data-testid="quote-add-line" label="增行" size="sm" isDisabled={ended || saving} onClick={onAddLine} />
          </div>
          <Table data={quote.lines ?? []} columns={[
            { key: "description", header: "说明", renderCell: (line) => <span data-testid={`quote-line-${line.id}`}><CommitField hideLabel label="说明" testId={`quote-line-description-${line.id}`} value={line.description} disabled={saving} onCommit={(description) => { if (description !== line.description) onUpdateLine(line, { description, user_confirmed: false }); }} /></span> },
            { key: "pricing_mode", header: "方式", renderCell: (line) => <NativeSelect testId={`quote-line-pricing-mode-${line.id}`} value={line.pricing_mode} disabled={saving} options={[{ value: "lump_sum", label: "总价计价" }, { value: "unit_price", label: "单价计价" }]} onChange={(value) => { if (!value || value === line.pricing_mode) return; onUpdateLine(line, value === "lump_sum" ? { pricing_mode: value, quantity: null, unit: null, unit_price: null, user_confirmed: false } : { pricing_mode: value, entered_amount: null, user_confirmed: false }); }} /> },
            { key: "quantity", header: "数量", renderCell: (line) => line.pricing_mode === "unit_price" ? <CommitField hideLabel label="数量" testId={`quote-line-quantity-${line.id}`} value={line.quantity ?? ""} disabled={saving} onCommit={(raw) => { const quantity = nullable(raw); if (quantity !== line.quantity) onUpdateLine(line, { quantity, user_confirmed: false }); }} /> : "—" },
            { key: "unit", header: "单位", renderCell: (line) => line.pricing_mode === "unit_price" ? <CommitField hideLabel label="单位" testId={`quote-line-unit-${line.id}`} value={line.unit ?? ""} disabled={saving} onCommit={(raw) => { const unit = nullable(raw); if (unit !== line.unit) onUpdateLine(line, { unit, user_confirmed: false }); }} /> : "—" },
            { key: "unit_price", header: "单价", renderCell: (line) => line.pricing_mode === "unit_price" ? <CommitField hideLabel label="单价" testId={`quote-line-unit-price-${line.id}`} value={line.unit_price ?? ""} disabled={saving} onCommit={(raw) => { const unitPrice = nullable(raw); if (unitPrice !== line.unit_price) onUpdateLine(line, { unit_price: unitPrice, user_confirmed: false }); }} /> : "—" },
            { key: "entered_amount", header: "总价", renderCell: (line) => line.pricing_mode === "lump_sum" ? <CommitField hideLabel label="总价" testId={`quote-line-entered-amount-${line.id}`} value={line.entered_amount ?? ""} disabled={saving} onCommit={(raw) => { const enteredAmount = nullable(raw); if (enteredAmount !== line.entered_amount) onUpdateLine(line, { entered_amount: enteredAmount, user_confirmed: false }); }} /> : line.gross_amount || "—" },
            { key: "tax_rate", header: "税率", renderCell: (line) => <CommitField hideLabel label="税率" testId={`quote-line-tax-rate-${line.id}`} value={line.tax_rate} disabled={saving} onCommit={(raw) => { const taxRate = raw.trim(); if (taxRate !== line.tax_rate) onUpdateLine(line, { tax_rate: taxRate, user_confirmed: false }); }} /> },
            { key: "user_confirmed", header: "确认", renderCell: (line) => <CheckboxInput data-testid={`quote-line-confirmed-${line.id}`} label="确认" isLabelHidden value={line.user_confirmed} isDisabled={saving} onChange={checked => onUpdateLine(line, { user_confirmed: checked })} /> },
            { key: "id", header: "操作", renderCell: (line) => <Button size="sm" variant="secondary" label="删" isDisabled={saving} onClick={() => onDeleteLine(line)} /> },
          ]} />
        </div>
      )}
    </div>
  );
}
