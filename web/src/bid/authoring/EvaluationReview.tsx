import { Drawer, DrawerHeader } from "@astryxdesign/core/Drawer";
import { List, ListItem } from "@astryxdesign/core/List";
import { Pagination } from "@astryxdesign/core/Pagination";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { Toolbar } from "@astryxdesign/core/Toolbar";
import { VStack } from "@astryxdesign/core/VStack";
import { useState } from "react";
import type { EvaluationDecision, RuleRef, TenderChapter, TenderIssue, TenderItem } from "../api/workbench";

// Same text as EVALUATION_PENDING_DETAIL in services/docreader/tender/item_placement.py.
const PENDING_DETAIL = "模型建议按评标程序处理，请按原件核定是否另设响应。";
const PAGE = 50;
const resolutions = [
  { value: "pending", label: "继续待核" },
  { value: "evaluation", label: "纯评审流程，不需响应" },
  { value: "reference", label: "原文索引，不另设响应" },
  { value: "document_wide", label: "整份投标文件约束" },
] as const;

export type Decisions = Record<string, EvaluationDecision>;
export const ruleKey = (ref: RuleRef) =>
  JSON.stringify([ref.item_ref.item_id, ref.item_ref.artifact_ref?.sha256 ?? null, ref.decision_id ?? null, ref.rule_index ?? null]);

export function pendingEvaluation(issues: TenderIssue[]): RuleRef[] {
  return issues.filter(issue => issue.code === "outline_rule_unassigned" && issue.detail === PENDING_DETAIL)
    .flatMap(issue => issue.rule_refs ?? []);
}

/** Only changed rows are sent; "pending" keeps the item where it is. */
export const submittedDecisions = (decisions: Decisions) => Object.values(decisions).filter(choice => choice.resolution !== "pending");

function targetOptions(chapters: TenderChapter[], path: string[] = []): Array<{ value: string; label: string }> {
  return chapters.flatMap(chapter => {
    const title = [...path, chapter.title || "未命名章节"];
    return [...chapter.targets.map(target => ({ value: `target:${target.target_id}`, label: `归入：${title.join(" / ")} · ${target.purpose}` })),
      ...targetOptions(chapter.children, title)];
  });
}

export function EvaluationReview({ isOpen, onOpenChange, refs, chapters, items, decisions, disabled, onChange }: {
  isOpen: boolean; onOpenChange(open: boolean): void; refs: RuleRef[]; chapters: TenderChapter[]; items: TenderItem[];
  decisions: Decisions; disabled: boolean; onChange(decisions: Decisions): void;
}) {
  const [page, setPage] = useState(1);
  const text = new Map(items.map(item => [item.item_id, item.text]));
  const options = [...resolutions, ...targetOptions(chapters)];
  const pages = Math.max(1, Math.ceil(refs.length / PAGE));
  const current = Math.min(page, pages);
  const rows = refs.slice((current - 1) * PAGE, current * PAGE).map(ref => ({ key: ruleKey(ref), ref }));
  const decided = submittedDecisions(decisions).length;
  const choose = (ref: RuleRef, value: string) => {
    const key = ruleKey(ref);
    const { [key]: _previous, ...rest } = decisions;
    if (value === "pending") return onChange(rest);
    onChange({ ...rest, [key]: value.startsWith("target:")
      ? { rule_ref: ref, resolution: "target", target_id: value.slice("target:".length) }
      : { rule_ref: ref, resolution: value as EvaluationDecision["resolution"] } });
  };
  const valueOf = (key: string) => {
    const choice = decisions[key];
    return !choice ? "pending" : choice.resolution === "target" ? `target:${choice.target_id}` : choice.resolution;
  };
  return <Drawer isOpen={isOpen} onOpenChange={onOpenChange} label="核定评标待核事项" width={720}>
    <DrawerHeader title="核定评标待核事项" onOpenChange={onOpenChange} />
    <VStack gap={3}>
      <Text type="supporting">模型建议这些事项按评标程序处理。请对照原件逐条核定去向，核定随「保存大纲草稿」一起保存。</Text>
      <Toolbar label="核定进度" dividers={["bottom"]}
        startContent={<Text role="status" hasTabularNumbers>已核定 {decided} 条，共 {refs.length} 条</Text>}
        endContent={pages > 1 ? <Pagination page={current} onChange={setPage} totalItems={refs.length} pageSize={PAGE} /> : undefined} />
      <List hasDividers aria-label="评标待核事项">{rows.map(({ key, ref }) => {
        const label = text.get(ref.item_ref.item_id) ?? `事项 ${ref.item_ref.item_id}`;
        return <ListItem key={key} label={<VStack gap={2} paddingBlock={1}>
          <Text>{label}</Text>
          <Selector label={`去向：${label}`} isLabelHidden isDisabled={disabled} value={valueOf(key)} options={options} onChange={value => choose(ref, value)} />
        </VStack>} />;
      })}</List>
    </VStack>
  </Drawer>;
}
