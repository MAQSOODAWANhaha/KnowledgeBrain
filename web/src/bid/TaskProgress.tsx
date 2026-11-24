import { Banner } from "@astryxdesign/core/Banner";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import type { TaskProgressView } from "./api/workbench";
import { executionStatus, inProgress } from "./executionStatus";

const phases: Record<string, string> = {
  register: "登记文件", freeze: "冻结来源", ocr: "识别文件", "pack-plan": "规划提取范围",
  extract: "提取招标事项", "analysis-review": "核验事项", "analysis-locate": "定位原文（仅历史运行）",
  "analysis-associate": "关联要求（仅历史运行）", "analysis-joint": "联合核验（仅历史运行）", verify: "核验分析",
  confirm: "确认分析", materials: "准备编制资料", knowledge: "检索证明资料", outline: "编排大纲",
  authoring_basis: "整理写作依据", authoring_write: "编写正文", authoring_review: "复核内容",
  authoring_structure: "核对提交结构",
  result: "保存产物", other: "处理任务",
};
const phaseName = (phase: string) => phases[phase] ?? "处理任务";

export function TaskProgress({ label, status, progress }: { label: string; status?: string; progress?: TaskProgressView }) {
  if (!status) return null;
  const active = inProgress(status);
  const attention = active || status === "failed" || status === "blocked" || !!progress?.request_states.uncertain;
  if (!attention) return null;
  const identity = progress?.identity;
  const version = identity?.source_version !== undefined && identity.domain_revision !== undefined
    ? `来源版本 ${identity.source_version} · 修订 ${identity.domain_revision}` : null;
  return <Section role="region" aria-label={`${label}进度`} variant="muted" padding={3}>
    <VStack gap={2}>
      <Text role="status"><Text weight="semibold">{label}：{executionStatus(status)}</Text>
        {active && progress?.current_phase && ` · 当前阶段：${phaseName(progress.current_phase)}`}
        {progress?.last_completed_phase && ` · 最近进展：${phaseName(progress.last_completed_phase)}`}
        {version && ` · ${version}`}
      </Text>
      {progress && <>
        <Text>已接受 {progress.accepted_units} 个工作单元{active ? "，总量未知：后续阶段的计划在前序阶段完成后才确定。" : "。"}</Text>
        {!!progress.request_states.uncertain && <Banner status="warning" title={`${progress.request_states.uncertain} 次请求回执待确认，请沿当前任务恢复入口处理。`} />}
        <Collapsible trigger="阶段与请求回执" defaultIsOpen={false}>
          <VStack gap={1}>
            {!version && <Text>依据身份尚未就绪，请等待任务状态更新。</Text>}
            {progress.accepted_by_phase.map(item => <Text key={item.phase}>{phaseName(item.phase)}：已接受 {item.count} 个工作单元，{item.planned === null ? "计划数未知" : `计划 ${item.planned} 个`}</Text>)}
            {progress.active_tasks.map(item => <Text key={item.phase}>{phaseName(item.phase)}：准备发送 {item.prepared} 次，等待返回 {item.sent} 次</Text>)}
            <Text>已收到返回 {progress.request_states.received} 次；失败 {progress.request_states.failed} 次；未通过业务验收 {progress.rejected_responses} 次。</Text>
            <Text type="supporting">同一逻辑请求的重试按最新状态统计。请求回执与已接受工作单元分别统计，收到返回仍需业务核验。</Text>
          </VStack>
        </Collapsible>
      </>}
    </VStack>
  </Section>;
}
