import { Blockquote } from "@astryxdesign/core/Blockquote";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { Button } from "@astryxdesign/core/Button";
import { Selector } from "@astryxdesign/core/Selector";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AnalysisBusinessFields } from "./api/analysisReview";
import type { TenderItem } from "./api/workbench";

const lines = (value: string) => value === "" ? null : value.split(/\r?\n/);
const meaningful = (value: string) => !!value.trim() && value.length <= 20000;
export function ItemFacts({ item }: { item: TenderItem }) {
  return <>
    {item.evidence?.map((evidence, position) => <Blockquote key={position} cite={`来源位置：${evidence.range_id}`}>{evidence.quote}</Blockquote>)}
    {item.numbers?.length ? <Text>数值：{item.numbers.map(value => `${value.value}${value.unit}`).join("；")}</Text> : null}
    {item.conditions?.length ? <Text>条件：{item.conditions.join("；")}</Text> : null}
    {item.exceptions?.length ? <Text>例外：{item.exceptions.join("；")}</Text> : null}
    {item.scope && <Text>适用范围：{item.scope}</Text>}
    {item.subject && <Text>主体：{item.subject}</Text>}
    {item.domains?.length ? <Text>涉及领域：{item.domains.join("；")}</Text> : null}
    {item.scoring && <Text>评分：{item.scoring.method} {item.scoring.points ?? ""} {item.scoring.max_points !== undefined ? `最高 ${item.scoring.max_points} 分` : ""}</Text>}
    {item.consequence && <Text>后果：{item.consequence.trigger} {item.consequence.effect}</Text>}
    {item.submission && <Text>提交材料：{[...(item.submission.documents ?? []), ...(item.submission.fields ?? [])].join("；")}</Text>}
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
  const scalar = (field: "subject" | "scope", label: string) => <TextInput key={field} label={label} aria-label={`${prefix} ${label}`} isDisabled={locked} value={value[field] ?? ""} onChange={next => change({ [field]: next || null })} />;
  const list = (field: "domains" | "conditions" | "exceptions", label: string) => <TextArea key={field} label={`${label}（每行一项）`} aria-label={`${prefix} ${label}`} htmlMaxLength={20000} isDisabled={locked} value={value[field]?.join("\n") ?? ""} onChange={next => change({ [field]: lines(next) })} />;
  function detail(group: "scoring" | "consequence" | "submission", field: string, content: string | number | string[] | null) {
    const fields = { ...value[group], [field]: content };
    const remaining = Object.fromEntries(Object.entries(fields).filter(([, entry]) => entry != null && entry !== ""));
    change({ [group]: Object.keys(remaining).length ? remaining : null });
  }
  return <Section role="group" aria-label={`纠正事项 ${index}`} padding={3}><VStack gap={3}><Heading level={4}>纠正事项 {index}</Heading>
    <Selector label="事项类型" aria-label={`${prefix} 类型`} isDisabled={locked} value={value.kind ?? "obligation"} onChange={kind => change({ kind, ...(kind !== "scoring_rule" ? { scoring: null } : {}), ...(kind !== "consequence_rule" ? { consequence: null } : {}), ...(kind !== "submission_requirement" ? { submission: null } : {}) })} options={[{ value: "obligation", label: "履约要求" }, { value: "scoring_rule", label: "评分规则" }, { value: "consequence_rule", label: "后果规则" }, { value: "submission_requirement", label: "提交要求" }]} />
    <TextArea label="纠正后的文案" aria-label={`${prefix} 纠正文案`} htmlMaxLength={20000} isDisabled={locked} value={value.text ?? ""} onChange={text => change({ text })} />
    {scalar("subject", "主体")}{scalar("scope", "适用范围")}
    {list("domains", "涉及领域")}{list("conditions", "条件")}{list("exceptions", "例外")}
    <Section role="group" aria-label="数值与原文片段" variant="muted" padding={3}><VStack gap={2}><Text weight="semibold">数值与原文片段</Text>
      {value.numbers?.map((number, position) => <HStack key={position} gap={2} vAlign="end" wrap="wrap">
        {([ ["value", "值"], ["unit", "单位"], ["quote", "原文片段"] ] as const).map(([field, label]) =>
          <TextInput key={field} label={label} aria-label={`${prefix} 数值 ${position + 1} ${label}`} isDisabled={locked} value={String(number[field] ?? "")} onChange={next => change({ numbers: value.numbers!.map((entry, offset) => offset === position ? { ...entry, [field]: next } : entry) })} />)}
        <Button aria-label={`${prefix} 删除数值 ${position + 1}`} onClick={() => {
          const numbers = value.numbers!.filter((_, offset) => offset !== position);
          change({ numbers: numbers.length ? numbers : null });
        }} label="删除数值" />
      </HStack>)}
      <HStack><Button variant="secondary" onClick={() => change({ numbers: [...(value.numbers ?? []), { value: "", unit: "", quote: "" }] })} label="添加数值" /></HStack>
    </VStack></Section>
    {value.kind === "scoring_rule" && <Section role="group" aria-label="评分" variant="muted" padding={3}><VStack gap={2}><Text weight="semibold">评分</Text>
      <TextArea label="评分办法" aria-label={`${prefix} 评分办法`} htmlMaxLength={20000} isDisabled={locked} value={value.scoring?.method ?? ""} onChange={next => detail("scoring", "method", next)} />
      {([ ["points", "分值"], ["max_points", "最高分"] ] as const).map(([field, label]) => <TextInput key={field} label={label} aria-label={`${prefix} ${label}`} isDisabled={locked} value={value.scoring?.[field] == null ? "" : String(value.scoring[field])} onChange={next => detail("scoring", field, next.trim() === "" ? null : Number(next))} />)}
    </VStack></Section>}
    {value.kind === "consequence_rule" && <Section role="group" aria-label="后果" variant="muted" padding={3}><VStack gap={2}><Text weight="semibold">后果</Text>
      {([ ["trigger", "触发条件"], ["effect", "后果"] ] as const).map(([field, label]) => <TextArea key={field} label={label} aria-label={`${prefix} ${label}`} htmlMaxLength={20000} isDisabled={locked} value={value.consequence?.[field] ?? ""} onChange={next => detail("consequence", field, next)} />)}
    </VStack></Section>}
    {value.kind === "submission_requirement" && <Section role="group" aria-label="提交要求" variant="muted" padding={3}><VStack gap={2}><Text weight="semibold">提交要求</Text>
      {([ ["fields", "填写字段"], ["documents", "提交材料"] ] as const).map(([field, label]) => <TextArea key={field} label={`${label}（每行一项）`} aria-label={`${prefix} ${label}`} htmlMaxLength={20000} isDisabled={locked} value={value.submission?.[field]?.join("\n") ?? ""} onChange={next => detail("submission", field, lines(next))} />)}
    </VStack></Section>}
  </VStack></Section>;
}
