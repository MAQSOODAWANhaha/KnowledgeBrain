import type { BidV2State } from "./session";

export function PreviewPane({ state }: { state: BidV2State }) {
  return (
    <div className="ed-page" data-testid="full-preview">
      <div className="ed-toolbar">
        <strong>全文预览</strong>
        <span className="note">
          来自冻结 WorkspaceRevision 的 HTML，不是编辑器内部 JSON。
        </span>
      </div>
      <div className="ed-stage">
        {state.previewHtml ? (
          <iframe
            title="preview"
            className="preview-frame"
            srcDoc={state.previewHtml}
          />
        ) : (
          <p className="note">
            还没有预览。进入本页会向服务端请求 preview HTML。
          </p>
        )}
      </div>
    </div>
  );
}
