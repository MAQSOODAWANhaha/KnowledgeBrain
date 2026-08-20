import { GfmPreview } from "./gfm";

export function BookletPane({
  mdDraft,
  ended,
  preview,
  onChange,
  onSave,
  onRegen,
}: {
  mdDraft: string;
  ended: boolean;
  preview: boolean;
  onChange: (v: string) => void;
  onSave: () => void;
  onRegen: () => void;
}) {
  return (
    <div className="ed-page" style={{ flex: 1, minHeight: 0 }}>
      {!preview && (
        <div className="ed-toolbar">
          <button className="btn sm" type="button" disabled={ended} onClick={onRegen}>
            按数据重生成
          </button>
          <button className="btn sm" type="button" disabled={ended} onClick={onSave}>
            保存
          </button>
          <span className="note" style={{ margin: 0 }}>
            原文即源。导出默认保留人句。
          </span>
        </div>
      )}
      <div className="ed-stage">
        <div className="ed-doc">
          <div className="ed-sheet">
            {preview || ended ? (
              mdDraft.trim() ? (
                <GfmPreview markdown={mdDraft} />
              ) : (
                <p className="note">这一册还是空的。切到编辑，或按数据重生成。</p>
              )
            ) : (
              <textarea value={mdDraft} onChange={(e) => onChange(e.target.value)} spellCheck={false} />
            )}
          </div>
        </div>
        <aside className="ed-aside">
          <p className="lbl">导出</p>
          <p className="note" style={{ margin: 0 }}>
            过程 Word 随时下。定稿 PDF 默认保留人句。导出时可勾选「重生成过期稿」。
          </p>
        </aside>
      </div>
    </div>
  );
}
