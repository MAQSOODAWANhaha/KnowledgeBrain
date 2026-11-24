import { Banner } from "@astryxdesign/core/Banner";
import { Heading } from "@astryxdesign/core/Heading";
import { pixel, Table } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { useEffect, useState } from "react";
import { tenderApi, type ExecutionReportView } from "./api/workbench";

const kinds: Record<string, string> = { send: "发送失败", contract: "未通过业务验收" };
const tokens = (value: number | undefined) => value === undefined ? "未知" : String(value);

/** Read-only stage report for one authoring execution; reading it sends no model request. */
export function ExecutionReport({ projectId, executionId, status }: { projectId: string; executionId: string; status: string }) {
  const [report, setReport] = useState<ExecutionReportView | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    setError(null);
    tenderApi.executionReport(projectId, executionId, abort.signal)
      .then(value => { if (!abort.signal.aborted) setReport(value); })
      .catch(reason => { if (!abort.signal.aborted) setError(reason instanceof Error ? reason.message : "执行报表读取失败，请重试。"); });
    return () => abort.abort();
  }, [projectId, executionId, status]);
  if (error) return <Banner status="error" title={`执行报表读取失败：${error}`} />;
  if (report === undefined) return <Text type="supporting">正在读取执行报表…</Text>;
  if (report === null) return <Text type="supporting">此任务尚未开始执行，暂无执行报表。</Text>;
  const stages = Object.entries(report.stages).map(([task, stage]) => ({ key: task, ...stage }));
  return <VStack gap={3} role="region" aria-label="执行报表">
    <VStack gap={1}>
      <Heading level={3}>各阶段请求</Heading>
      {!stages.length ? <Text type="supporting">本次执行尚未发送模型请求。</Text>
        : <Table idKey="key" data={stages} columns={[
          { key: "stage", header: "阶段", renderCell: item => <Text>{item.stage}</Text> },
          { key: "logical", header: "逻辑请求", width: pixel(96), renderCell: item => <Text>{item.logical_requests}</Text> },
          { key: "requests", header: "实际发送", width: pixel(96), renderCell: item => <Text>{item.requests}</Text> },
          { key: "tokens", header: "Token", width: pixel(72), renderCell: item => <Text>{tokens(item.reported_tokens.total_tokens)}</Text> },
        ]} />}
      <Text type="supporting">实际发送含重试；Token 只计供应商已回报的用量。</Text>
    </VStack>
    <VStack gap={1}>
      <Heading level={3}>失败分类</Heading>
      {!report.failures.length ? <Text type="supporting">本次执行没有发送失败或未通过业务验收的回包。</Text>
        : <Table idKey="key" data={report.failures.map(row => ({ ...row, key: `${row.task}:${row.kind}:${row.code}` }))} columns={[
          { key: "stage", header: "阶段", renderCell: item => <VStack gap={0}><Text>{item.stage}</Text><Text type="supporting">{kinds[item.kind] ?? item.kind}：{item.code}</Text></VStack> },
          { key: "count", header: "次数", width: pixel(56), renderCell: item => <Text>{item.count}</Text> },
          { key: "tokens", header: "Token", width: pixel(96), renderCell: item => <Text>{item.reported_tokens}{item.unknown_usage ? `（另 ${item.unknown_usage} 次未知）` : ""}</Text> },
        ]} />}
      <Text type="supporting">按失败消耗的 Token 排序。</Text>
    </VStack>
  </VStack>;
}
