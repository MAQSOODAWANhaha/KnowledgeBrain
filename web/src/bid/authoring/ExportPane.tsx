import type { BidV2Session, BidV2State } from "./session";

export function ExportPane({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const issues = state.assessments?.submission?.issues ?? [];
  const blocked = state.ended || !state.workspace;
  return (
    <div className="stack">
      <div className="card">
        <h3 className="h3">导出</h3>
        <p className="note">导出当前稿。改完可以再导一份。业务提示不阻断。</p>
        <div className="row" style={{ marginTop: 16 }}>
          <button
            type="button"
            className="btn"
            data-testid="export-docx"
            disabled={blocked}
            onClick={() => void session.exportDocument("submission", "docx")}
          >
            导出 DOCX
          </button>
          <button
            type="button"
            className="btn ghost"
            data-testid="export-pdf"
            disabled={blocked}
            onClick={() => void session.exportDocument("submission", "pdf")}
          >
            导出 PDF
          </button>
          <button
            type="button"
            className="btn ghost"
            disabled={blocked}
            onClick={() => void session.exportDocument("review_draft", "pdf")}
          >
            预审稿 PDF
          </button>
        </div>
        {state.asyncRequests
          .filter((request) => request.kind === "SubmissionExport")
          .map((request) => (
            <p
              key={request.request_artifact_id}
              className="note"
              data-testid="export-status"
            >
              导出 {request.status}
            </p>
          ))}
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
