import { Button } from "@mantine/core";
import { exportAllowed } from "./assessment";
import type { BidV2Session, BidV2State } from "./session";

export function ExportPane({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const allowed = exportAllowed({
    assessment: state.assessments?.submission ?? null,
    technicalReady: !state.error?.technical,
  });
  const issues = state.assessments?.submission?.issues ?? [];
  return (
    <div className="stack">
      <div className="card">
        <h3 className="h3">导出</h3>
        <p className="note">
          业务提示不阻断导出。缺资产、digest 错误等技术失败才会停。
        </p>
        <div className="row" style={{ marginTop: 16 }}>
          <Button
            data-testid="export-docx"
            disabled={state.ended || !allowed.allowed}
            onClick={() => void session.exportDocument("submission", "docx")}
          >
            导出 DOCX
          </Button>
          <Button
            data-testid="export-pdf"
            variant="default"
            disabled={state.ended || !allowed.allowed}
            onClick={() => void session.exportDocument("submission", "pdf")}
          >
            导出 PDF
          </Button>
          <Button
            variant="default"
            disabled={state.ended}
            onClick={() => void session.exportDocument("review_draft", "pdf")}
          >
            预审稿 PDF
          </Button>
        </div>
      </div>
      <div className="card" data-testid="assessment-report">
        <h3 className="h3">检查报告（提示）</h3>
        <p className="note">
          状态 {state.assessments?.submission?.status ?? "—"} · {issues.length}{" "}
          项。不是 Gate。
        </p>
        {issues.map((issue) => (
          <div key={issue.issue_id} className="note">
            {issue.severity} · {issue.code} · {issue.message}
          </div>
        ))}
        {issues.length === 0 && (
          <p className="note">还没有提交评估，或当前没有提示。</p>
        )}
      </div>
      {state.exports.length > 0 && (
        <div className="card">
          <h3 className="h3">历史导出</h3>
          {state.exports.map((item) => (
            <div key={item.export_id} className="note">
              {item.mode} · {item.format} · {item.status}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
