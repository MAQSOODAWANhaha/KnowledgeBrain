import type { AnalysisBusinessFields } from "./api/analysisReview";
import type { TenderItem } from "./api/workbench";
import "./analysisReview.css";

const lines = (value: string) => value === "" ? null : value.split(/\r?\n/);
const meaningful = (value: string) => !!value.trim() && value.length <= 20000;
export function ItemFacts({ item }: { item: TenderItem }) {
  return <>
    {item.evidence?.map((evidence, position) => <blockquote key={position}>{evidence.quote}<small> 来源位置：{evidence.range_id}</small></blockquote>)}
    {item.numbers?.length ? <p>数值：{item.numbers.map(value => `${value.value}${value.unit}`).join("；")}</p> : null}
    {item.conditions?.length ? <p>条件：{item.conditions.join("；")}</p> : null}
    {item.exceptions?.length ? <p>例外：{item.exceptions.join("；")}</p> : null}
    {item.scope && <p>适用范围：{item.scope}</p>}
    {item.subject && <p>主体：{item.subject}</p>}
    {item.domains?.length ? <p>涉及领域：{item.domains.join("；")}</p> : null}
    {item.scoring && <p>评分：{item.scoring.method} {item.scoring.points ?? ""} {item.scoring.max_points !== undefined ? `最高 ${item.scoring.max_points} 分` : ""}</p>}
    {item.consequence && <p>后果：{item.consequence.trigger} {item.consequence.effect}</p>}
    {item.submission && <p>提交材料：{[...(item.submission.documents ?? []), ...(item.submission.fields ?? [])].join("；")}</p>}
  </>;
}

export function validFields(value: AnalysisBusinessFields): boolean {
  if (value.text !== undefined && !meaningful(value.text)) return false;
  if ([value.domains, value.conditions, value.exceptions, value.submission?.fields, value.submission?.documents]
    .some(list => list != null && (!list.length || !list.every(meaningful)))) return false;
  if ([value.subject, value.scope, value.scoring?.method, value.consequence?.effect, value.consequence?.trigger]
    .some(text => text != null && !meaningful(text))) return false;
  if ([value.scoring?.points, value.scoring?.max_points].some(number => number != null && !Number.isFinite(number))) return false;
  if (value.scoring?.max_points != null && value.scoring.max_points < 0) return false;
  return value.numbers == null || (!!value.numbers.length && value.numbers.every(number =>
    meaningful(String(number.value)) && number.unit.length <= 20000 && meaningful(number.quote)));
}

export function ItemFields({ index, item, patch, locked, change }: {
  index: number; item: TenderItem; patch: AnalysisBusinessFields; locked: boolean;
  change(patch: AnalysisBusinessFields): void;
}) {
  const value = { ...item, ...patch }, prefix = `事项 ${index}`;
  const scalar = (field: "subject" | "scope", label: string) => <label key={field}>{label}
    <input aria-label={`${prefix} ${label}`} maxLength={20000} value={value[field] ?? ""}
      onChange={event => change({ [field]: event.target.value || null })} /></label>;
  const list = (field: "domains" | "conditions" | "exceptions", label: string) => <label key={field}>{label}（每行一项）
    <textarea aria-label={`${prefix} ${label}`} maxLength={20000} value={value[field]?.join("\n") ?? ""}
      onChange={event => change({ [field]: lines(event.target.value) })} /></label>;
  function detail(group: "scoring" | "consequence" | "submission", field: string, content: string | number | string[] | null) {
    const fields = { ...value[group], [field]: content };
    const remaining = Object.fromEntries(Object.entries(fields).filter(([, entry]) => entry != null && entry !== ""));
    change({ [group]: Object.keys(remaining).length ? remaining : null });
  }
  return <fieldset className="analysis-business-fields" disabled={locked}><legend>纠正事项 {index}</legend>
    <label>事项类型 <select aria-label={`${prefix} 类型`} value={value.kind ?? "obligation"} onChange={event => {
      const kind = event.target.value;
      change({ kind, ...(kind !== "scoring_rule" ? { scoring: null } : {}),
        ...(kind !== "consequence_rule" ? { consequence: null } : {}),
        ...(kind !== "submission_requirement" ? { submission: null } : {}) });
    }}><option value="obligation">履约要求</option><option value="scoring_rule">评分规则</option>
      <option value="consequence_rule">后果规则</option><option value="submission_requirement">提交要求</option></select></label>
    <label>纠正后的文案 <textarea aria-label={`${prefix} 纠正文案`} maxLength={20000} value={value.text}
      onChange={event => change({ text: event.target.value })} /></label>
    {scalar("subject", "主体")}{scalar("scope", "适用范围")}
    {list("domains", "涉及领域")}{list("conditions", "条件")}{list("exceptions", "例外")}
    <fieldset><legend>数值与原文片段</legend>
      {value.numbers?.map((number, position) => <div key={position}>
        {([ ["value", "值"], ["unit", "单位"], ["quote", "原文片段"] ] as const).map(([field, label]) =>
          <label key={field}>{label}<input aria-label={`${prefix} 数值 ${position + 1} ${label}`} maxLength={20000}
            value={number[field]} onChange={event => change({ numbers: value.numbers!.map((entry, offset) =>
              offset === position ? { ...entry, [field]: event.target.value } : entry) })} /></label>)}
        <button aria-label={`${prefix} 删除数值 ${position + 1}`} onClick={() => {
          const numbers = value.numbers!.filter((_, offset) => offset !== position);
          change({ numbers: numbers.length ? numbers : null });
        }}>删除数值</button>
      </div>)}
      <button onClick={() => change({ numbers: [...(value.numbers ?? []), { value: "", unit: "", quote: "" }] })}>添加数值</button>
    </fieldset>
    {value.kind === "scoring_rule" && <fieldset><legend>评分</legend>
      <label>评分办法 <textarea aria-label={`${prefix} 评分办法`} maxLength={20000} value={value.scoring?.method ?? ""}
        onChange={event => detail("scoring", "method", event.target.value)} /></label>
      {([ ["points", "分值"], ["max_points", "最高分"] ] as const).map(([field, label]) => <label key={field}>{label}
        <input aria-label={`${prefix} ${label}`} type="number" step="any" min={field === "max_points" ? 0 : undefined}
          value={value.scoring?.[field] ?? ""} onChange={event => detail("scoring", field, event.target.value === "" ? null : Number(event.target.value))} /></label>)}
    </fieldset>}
    {value.kind === "consequence_rule" && <fieldset><legend>后果</legend>
      {([ ["trigger", "触发条件"], ["effect", "后果"] ] as const).map(([field, label]) => <label key={field}>{label}
        <textarea aria-label={`${prefix} ${label}`} maxLength={20000} value={value.consequence?.[field] ?? ""}
          onChange={event => detail("consequence", field, event.target.value)} /></label>)}
    </fieldset>}
    {value.kind === "submission_requirement" && <fieldset><legend>提交要求</legend>
      {([ ["fields", "填写字段"], ["documents", "提交材料"] ] as const).map(([field, label]) => <label key={field}>{label}（每行一项）
        <textarea aria-label={`${prefix} ${label}`} maxLength={20000} value={value.submission?.[field]?.join("\n") ?? ""}
          onChange={event => detail("submission", field, lines(event.target.value))} /></label>)}
    </fieldset>}
  </fieldset>;
}
