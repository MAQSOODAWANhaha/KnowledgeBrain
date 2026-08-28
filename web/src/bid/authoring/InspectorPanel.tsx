import { Button, SegmentedControl } from "@mantine/core";
import type { BidV2Session, BidV2State, InspectorTab } from "./session";

const TABS: { value: InspectorTab; label: string }[] = [
  { value: "requirements", label: "要求" },
  { value: "evidence", label: "证据" },
  { value: "assets", label: "资产" },
  { value: "assessment", label: "检查" },
];

export function InspectorPanel({
  session,
  state,
}: {
  session: BidV2Session;
  state: BidV2State;
}) {
  const nodeId = state.selectedNodeLineageId;
  return (
    <>
      <SegmentedControl
        fullWidth
        size="xs"
        value={state.inspectorTab}
        data={TABS}
        onChange={(value) => session.setInspectorTab(value as InspectorTab)}
      />
      {state.inspectorTab === "requirements" && (
        <div className="stack" style={{ marginTop: 12 }}>
          <p className="lbl">本章要求</p>
          {(state.requirements ?? []).slice(0, 40).map((req) => (
            <div key={req.requirement_revision_id} className="note">
              {req.text}
            </div>
          ))}
          {state.requirements.length === 0 && (
            <p className="note">还没有要求投影。</p>
          )}
        </div>
      )}
      {state.inspectorTab === "evidence" && (
        <div className="stack" style={{ marginTop: 12 }}>
          <p className="lbl">知识证据</p>
          <SegmentedControl
            size="xs"
            value={state.evidenceMode}
            data={[
              { value: "system_proposed", label: "系统建议" },
              { value: "user_pick_set", label: "人工先选" },
            ]}
            onChange={(value) =>
              session.setEvidenceMode(value as BidV2State["evidenceMode"])
            }
          />
          <Button
            size="compact-sm"
            variant="default"
            disabled={state.ended || !nodeId}
            onClick={() => nodeId && void session.matchEvidence(nodeId)}
          >
            匹配资料
          </Button>
          {(state.evidenceOverview?.bundles ?? []).map((bundle) => (
            <div key={bundle.evidence_bundle_id} className="note">
              {bundle.title}
            </div>
          ))}
          <p className="note">
            证据只在本面板和检查报告中展示，不写入投标正文。
          </p>
        </div>
      )}
      {state.inspectorTab === "assets" && (
        <div className="stack" style={{ marginTop: 12 }}>
          <p className="lbl">本次人工资产</p>
          {state.assets.map((asset) => (
            <div key={asset.asset_revision_id} className="note">
              {asset.file_name}
            </div>
          ))}
          {state.assets.length === 0 && (
            <p className="note">还没有插入证书、案例或图片。</p>
          )}
        </div>
      )}
      {state.inspectorTab === "assessment" && (
        <div
          className="stack"
          style={{ marginTop: 12 }}
          data-testid="assessment-panel"
        >
          <p className="lbl">提示 · 不阻断</p>
          <p className="note">
            大纲 {state.assessments?.outline?.status ?? "—"} · 提交{" "}
            {state.assessments?.submission?.status ?? "—"}
          </p>
          {(state.assessments?.outline?.issues ?? [])
            .concat(state.assessments?.submission?.issues ?? [])
            .map((issue) => (
              <div key={issue.issue_id} className="note">
                {issue.severity} · {issue.code} · {issue.message}
              </div>
            ))}
        </div>
      )}
    </>
  );
}
