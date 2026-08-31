import { Button, FileButton, Select, Tabs } from "@mantine/core";
import { OWNER_MVP_EVIDENCE_MODE_OPTIONS } from "./generation";
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
  const node = nodeId ? session.findNode(nodeId) : null;

  return (
    <Tabs
      value={state.inspectorTab}
      onChange={(value) =>
        value && session.setInspectorTab(value as InspectorTab)
      }
      data-testid="inspector-tab"
    >
      <Tabs.List>
        {TABS.map((tab) => (
          <Tabs.Tab key={tab.value} value={tab.value}>
            {tab.label}
          </Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel value="requirements" pt="md">
        {(state.requirements ?? []).slice(0, 40).map((req) => (
          <div key={req.requirement_revision_id} className="note">
            {req.text}
          </div>
        ))}
        {state.requirements.length === 0 && <p className="note">暂无</p>}
      </Tabs.Panel>
      <Tabs.Panel value="evidence" pt="md">
        <Select
          data={[...OWNER_MVP_EVIDENCE_MODE_OPTIONS]}
          value={state.evidenceMode}
          onChange={(value) =>
            value &&
            session.setEvidenceMode(value as BidV2State["evidenceMode"])
          }
        />
        <Button
          variant="default"
          mt="sm"
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
      </Tabs.Panel>
      <Tabs.Panel value="assets" pt="md">
        <FileButton
          onChange={(file) => {
            if (file) void session.uploadAsset(file);
          }}
        >
          {(props) => (
            <Button {...props} variant="default">
              上传
            </Button>
          )}
        </FileButton>
        {state.assets.map((asset) => (
          <div key={asset.asset_revision_id} className="note">
            {asset.file_name}{" "}
            <Button
              variant="subtle"
              size="compact-sm"
              disabled={state.ended || !nodeId}
              onClick={() =>
                nodeId &&
                void session.insertAssetBlock(
                  nodeId,
                  asset.asset_revision_id,
                  node?.block_lineage_ids.length ?? 0,
                )
              }
            >
              插入本章
            </Button>
          </div>
        ))}
        {state.assets.length === 0 && <p className="note">暂无</p>}
      </Tabs.Panel>
      <Tabs.Panel value="assessment" pt="md" data-testid="assessment-panel">
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
      </Tabs.Panel>
    </Tabs>
  );
}
