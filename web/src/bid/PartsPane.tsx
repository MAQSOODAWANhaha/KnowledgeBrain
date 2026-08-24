import { Button, SegmentedControl } from "@mantine/core";
import { GfmPreview } from "./gfm";
import type { GateIssue } from "../api";
import { partTitle } from "./helpers";
import type { MatchUnit } from "../api";

export function PartsPane({
  partKey,
  markdown,
  stale,
  ended,
  ready,
  preview,
  units,
  gate,
  onChange,
  onSave,
  onRegen,
  onPreview,
}: {
  partKey: string;
  markdown: string;
  stale?: boolean;
  ended: boolean;
  ready: boolean;
  preview: boolean;
  units: MatchUnit[];
  gate?: { status: string; issues: GateIssue[] };
  onChange: (v: string) => void;
  onSave: () => void;
  onRegen: () => void;
  onPreview: (v: boolean) => void;
}) {
  return (
    <div
      className="ed-page"
      data-testid={`part-pane-${partKey}`}
      data-ready={ready ? "true" : "false"}
      style={{ flex: 1, minHeight: 0 }}
    >
      <div className="ed-toolbar">
        <strong>{partTitle(partKey, units)}</strong>
        <SegmentedControl
          data={[
            { value: "preview", label: "预览" },
            { value: "draft", label: "编辑" },
          ]}
          value={preview ? "preview" : "draft"}
          onChange={(v) => onPreview(v === "preview")}
        />
        <Button
          data-testid={`part-regenerate-${partKey}`}
          size="compact-sm"
          variant="default"
          disabled={ended || !ready}
          onClick={onRegen}
        >
          按依赖重生成
        </Button>
        <Button size="compact-sm" disabled={ended || !ready} onClick={onSave}>
          保存
        </Button>
        {stale && <span className="chip amber">stale</span>}
      </div>
      {gate && (
        <div className={`banner ${gate.status === "reject" ? "bad" : "warn"}`} data-testid="gate-issues" style={{ margin: "12px 24px 0" }}>
          Gate {gate.status} · {gate.issues.length} 项
          {gate.issues.slice(0, 6).map((issue) => (
            <div key={`${issue.code}-${issue.part_key}`}>
              {issue.code}
              {issue.part_key ? ` @ ${issue.part_key}` : ""}
            </div>
          ))}
        </div>
      )}
      <div className="ed-stage">
        <div className="ed-doc">
          <div className="ed-sheet">
            {!ready ? (
              <p className="note">正在加载当前分册…</p>
            ) : preview || ended ? (
              markdown.trim() ? (
                <GfmPreview markdown={markdown} />
              ) : (
                <p className="note">这一册还是空的。切到编辑，或按依赖重生成。</p>
              )
            ) : (
              <textarea data-testid="part-editor" value={markdown} onChange={(e) => onChange(e.target.value)} spellCheck={false} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
