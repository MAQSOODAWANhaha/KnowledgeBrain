import { Button, Checkbox, Select, Textarea } from "@mantine/core";
import { CLAUSE_KINDS, type Candidate, type Clause, type RoutePickSet } from "../api";
import { familyLabel, kindLabel } from "./helpers";

export function Inspector({
  step,
  cur,
  ended,
  pickSet,
  selectedCandidateIds,
  pendingCandidateIds,
  onPatch,
  onConfirm,
  onUnconfirm,
  onPickToggle,
}: {
  step: string;
  cur: Clause | null;
  ended: boolean;
  pickSet: RoutePickSet | null;
  selectedCandidateIds?: ReadonlySet<string>;
  pendingCandidateIds: ReadonlySet<string>;
  onPatch: (c: Clause, patch: Record<string, unknown>) => void;
  onConfirm: (c: Clause) => void;
  onUnconfirm: (c: Clause) => void;
  onPickToggle: (candidate: Candidate, include: boolean) => void;
}) {
  if (step === "matching" && pickSet) {
    const picked = selectedCandidateIds ?? new Set(pickSet.items.map((i) => i.candidate_artifact_id));
    return (
      <>
        <p className="lbl">全部 supported</p>
        <p className="note">系统标 recommended，人勾 1..N。不宣称唯一最佳。</p>
        {(pickSet.supported_candidates ?? []).map((c) => (
          <label key={c.candidate_artifact_id} className="row" style={{ marginTop: 10, alignItems: "flex-start" }}>
            <Checkbox
              data-testid={`pick-${c.candidate_artifact_id}`}
              checked={picked.has(c.candidate_artifact_id)}
              disabled={ended || pendingCandidateIds.has(c.candidate_artifact_id)}
              aria-busy={pendingCandidateIds.has(c.candidate_artifact_id)}
              onChange={(e) => onPickToggle(c, e.currentTarget.checked)}
            />
            <span>
              <b>{c.product_id}</b>
              {c.recommended && <span className="chip iris">recommended</span>}
              <div className="desc">{c.product_version_id}</div>
            </span>
          </label>
        ))}
        {(pickSet.supported_candidates ?? []).length === 0 && <p className="note">当前 route 还没有 supported 候选。</p>}
      </>
    );
  }
  if (!cur) {
    return <p className="note">从表里点一条条款。</p>;
  }
  return (
    <>
      <p className="lbl">当前条款</p>
      <h3 className="h3">{cur.text || "（空）"}</h3>
      <p className="note">
        {kindLabel(cur.kind)} · {familyLabel(cur.family)} · {cur.provenance}
        {cur.must ? " · 必须" : ""}
        {cur.confirmation_required_reason ? " · 待重新确认" : ""}
      </p>
      <Select
        label="kind"
        mt="sm"
        data={CLAUSE_KINDS.map((k) => ({ value: k, label: kindLabel(k) }))}
        value={cur.kind}
        disabled={ended || cur.status === "confirmed"}
        allowDeselect={false}
        onChange={(v) => {
          if (v && v !== cur.kind) onPatch(cur, { kind: v });
        }}
      />
      <Checkbox mt="sm" label="必须条款" checked={cur.must} disabled={ended} onChange={(e) => onPatch(cur, { must: e.currentTarget.checked })} />
      <Textarea
        label="正文"
        mt="sm"
        minRows={4}
        defaultValue={cur.text}
        key={cur.id + cur.revision}
        disabled={ended}
        onBlur={(e) => {
          if (e.currentTarget.value !== cur.text) onPatch(cur, { text: e.currentTarget.value });
        }}
      />
      {cur.status === "draft" && (
        <Button fullWidth mt="md" disabled={ended} onClick={() => onConfirm(cur)}>
          确认本条
        </Button>
      )}
      {cur.status === "confirmed" && (
        <Button fullWidth mt="md" variant="default" disabled={ended} onClick={() => onUnconfirm(cur)}>
          取消确认
        </Button>
      )}
    </>
  );
}
