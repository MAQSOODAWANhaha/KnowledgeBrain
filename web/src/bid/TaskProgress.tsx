import type { TaskProgressView } from "./api/workbench";
import { executionStatus, inProgress } from "./executionStatus";

const phases: Record<string, string> = {
  register: "登记文件", freeze: "冻结来源", ocr: "识别文件", "pack-plan": "规划提取范围",
  extract: "提取招标事项", "analysis-review": "核验事项", "analysis-locate": "定位原文",
  "analysis-associate": "关联要求", "analysis-joint": "联合核验", verify: "核验分析",
  confirm: "确认分析", materials: "准备编制资料", knowledge: "检索证明资料", outline: "编排大纲",
  authoring_basis: "整理写作依据", authoring_write: "编写正文", authoring_review: "复核内容",
  result: "保存产物", other: "处理任务",
};
const phaseName = (phase: string) => phases[phase] ?? "处理任务";

export function TaskProgress({ label, status, progress }: { label: string; status?: string; progress?: TaskProgressView }) {
  if (!status) return null;
  const active = inProgress(status);
  return <section className="tender-progress" aria-label={`${label}进度`}>
    <p role="status"><strong>{label}：{executionStatus(status)}</strong>
      {active && progress?.current_phase && <span> · 当前阶段：{phaseName(progress.current_phase)}</span>}
      {progress?.last_completed_phase && <span> · 最近进展：{phaseName(progress.last_completed_phase)}</span>}
    </p>
    {progress && <>
      <p>已接受 {progress.accepted_units} 个工作单元{active ? "，剩余总量尚未确定。" : "。"}</p>
      {!!progress.request_states.uncertain && <p className="tender-context-warning">{progress.request_states.uncertain} 次请求回执待确认，请沿当前任务恢复入口处理。</p>}
      <details><summary>阶段与请求回执</summary>
        <p>{progress.identity.source_version !== undefined && progress.identity.domain_revision !== undefined ? `来源版本 ${progress.identity.source_version} · 领域修订 ${progress.identity.domain_revision}` : "依据身份尚未就绪，请等待任务状态更新。"}</p>
        {progress.accepted_by_phase.map(item => <p key={item.phase}>{phaseName(item.phase)}：已接受 {item.count} 个工作单元</p>)}
        {progress.active_tasks.map(item => <p key={item.phase}>{phaseName(item.phase)}：准备发送 {item.prepared} 次，等待返回 {item.sent} 次</p>)}
        <p>已收到返回 {progress.request_states.received} 次；失败 {progress.request_states.failed} 次；未通过业务验收 {progress.rejected_responses} 次。</p>
        <p className="tender-note">同一逻辑请求的重试按最新状态统计。请求回执与已接受工作单元分别统计，收到返回仍需业务核验。</p>
      </details>
    </>}
  </section>;
}
