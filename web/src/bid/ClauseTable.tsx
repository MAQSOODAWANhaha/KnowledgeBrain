import { useMemo, useState } from "react";
import { Button, Checkbox, SegmentedControl, Select, TextInput } from "@mantine/core";
import { CLAUSE_KINDS, type Clause } from "../api";
import { familyLabel, kindLabel } from "./helpers";

export function ClauseTable({
  live,
  selected,
  ended,
  addText,
  addKind,
  addMust,
  onSelect,
  onConfirm,
  onReject,
  onAddText,
  onAddKind,
  onAddMust,
  onAdd,
}: {
  live: Clause[];
  selected: string | null;
  ended: boolean;
  addText: string;
  addKind: string;
  addMust: boolean;
  onSelect: (id: string) => void;
  onConfirm: (c: Clause) => void;
  onReject: (c: Clause) => void;
  onAddText: (v: string) => void;
  onAddKind: (v: string) => void;
  onAddMust: (v: boolean) => void;
  onAdd: () => void;
}) {
  const [filter, setFilter] = useState<"all" | "draft" | "confirmed">("all");
  const rows = useMemo(() => {
    return live.filter((c) => {
      if (filter === "draft") return c.status === "draft";
      if (filter === "confirmed") return c.status === "confirmed";
      return true;
    });
  }, [live, filter]);

  return (
    <div className="stack">
      <div className="card">
        <h3 className="h3">手补条款</h3>
        <p className="note">只提交 kind。family 由服务端派生。</p>
        <TextInput data-testid="clause-text" mt="sm" value={addText} onChange={(e) => onAddText(e.currentTarget.value)} placeholder="条款原文" />
        <div className="row" style={{ marginTop: 12 }}>
          <Select
            data-testid="clause-kind"
            data={CLAUSE_KINDS.map((k) => ({ value: k, label: kindLabel(k) }))}
            value={addKind}
            onChange={(v) => onAddKind(v || "technical")}
            allowDeselect={false}
            style={{ minWidth: 160 }}
          />
          <Checkbox label="必须" checked={addMust} onChange={(e) => onAddMust(e.currentTarget.checked)} />
          <Button data-testid="clause-add" disabled={ended || !addText.trim()} onClick={onAdd}>
            添加草稿
          </Button>
        </div>
      </div>
      <div className="card pad-0">
        <div className="toolbar">
          <SegmentedControl
            value={filter}
            onChange={(value) => setFilter(value as typeof filter)}
            data={[
              { value: "all", label: `全部 ${live.length}` },
              { value: "draft", label: `待确认 ${live.filter((c) => c.status === "draft").length}` },
              { value: "confirmed", label: `已确认 ${live.filter((c) => c.status === "confirmed").length}` },
            ]}
          />
        </div>
        <table className="grid">
          <thead>
            <tr>
              <th>条款</th>
              <th>kind</th>
              <th>family</th>
              <th>状态</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((c) => (
              <tr
                key={c.id}
                data-testid={`clause-row-${c.id}`}
                className={selected === c.id ? "on" : undefined}
                onClick={() => onSelect(c.id)}
              >
                <td>
                  <div className="name" style={{ maxWidth: 420 }}>
                    {c.text}
                  </div>
                  {c.confirmation_required_reason && <div className="desc">待重新确认</div>}
                </td>
                <td>{kindLabel(c.kind)}</td>
                <td className="muted">{familyLabel(c.family)}</td>
                <td>{c.status}</td>
                <td>
                  {c.status === "draft" && (
                    <Button size="compact-sm" disabled={ended} onClick={() => onConfirm(c)}>
                      确认
                    </Button>
                  )}
                  {c.status !== "rejected" && (
                    <Button size="compact-sm" variant="subtle" color="red" disabled={ended} onClick={() => onReject(c)}>
                      驳回
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
