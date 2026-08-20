import { useEffect, useState } from "react";
import type { Candidate, Clause, Pick, Shot } from "../api";
import { bidHref, go } from "../hash";
import { assessmentLabel } from "./helpers";

function sentences(text: string): string[] {
  return text
    .split(/[。；;]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

export function ClauseDetail({
  id,
  view,
  cur,
  ended,
  candidates,
  picks,
  shots: _shots,
  onPatch,
  onPick,
  onUnpick,
  onConfirm,
  onDeviate,
  onShots: _onShots,
  projectId: _projectId,
}: {
  id: string;
  view: string;
  cur: Clause;
  ended: boolean;
  candidates: Candidate[];
  picks: Pick[];
  shots: Shot[];
  projectId: string;
  onPatch: (cid: string, body: Partial<Clause>) => void;
  onPick: (productId: string) => void;
  onUnpick: (productId: string) => void;
  onConfirm: (c: Clause) => void;
  onDeviate: () => void;
  onShots: () => void;
}) {
  const [editedText, setEditedText] = useState(cur.text);
  useEffect(() => setEditedText(cur.text), [cur.id, cur.text]);
  const bits = sentences(cur.text || cur.raw_text || "");
  const draft = cur.status === "draft";
  const rejected = cur.status === "rejected";
  return (
    <div className="wrap stack">
      <div className="card between">
        <div style={{ flex: 1, minWidth: 0 }}>
          <p className="sub" style={{ margin: 0 }}>
            招标原文。一条条款，按句拆开只为方便读，保存仍写回这一条。
          </p>
          <p className="sub" style={{ margin: "10px 0 0", color: "var(--ink)" }}>
            {cur.text}
          </p>
          <div className="row" style={{ marginTop: 16, flexWrap: "wrap" }}>
            {draft ? (
              <span className="chip gray">待确认</span>
            ) : rejected ? (
              <span className="chip rose">已驳回</span>
            ) : (
              <span className="chip pine">
                <i className="dot" />
                已确认
              </span>
            )}
            {cur.family_conflict && <span className="chip rose">分类冲突，请核对</span>}
            <span className="chip amber">
              <i className="dot" />
              人评 {assessmentLabel(cur.assessment)}
            </span>
          </div>
        </div>
        <div className="inner" style={{ width: 260, flexShrink: 0 }}>
          <div className="lbl" style={{ margin: 0 }}>
            建议
          </div>
          <div style={{ fontSize: 18, fontWeight: 700, letterSpacing: "-0.03em", margin: "6px 0 8px" }}>
            {cur.suggestion === "cover" ? "覆盖" : cur.suggestion === "pending" ? "待勾选" : cur.suggestion === "need_rematch" ? "需重配" : "—"}
          </div>
          <p className="note" style={{ margin: 0 }}>
            覆盖按本段现算。系统排序，人不宣布唯一最佳。
          </p>
        </div>
      </div>

      <div className="split-2">
        <div className="card">
          <div className="row">
            <span className="step-dot" />
            <span className="step-id">STEP 01</span>
          </div>
          <h3 className="h3" style={{ margin: "6px 0 4px" }}>
            这一条
          </h3>
          <p className="note" style={{ margin: "0 0 14px" }}>
            确认、人评、偏离说明都挂在这一条上。
          </p>
          <div className="inner iris">
            <label className="fld">条款文本</label>
            <textarea
              className="area"
              readOnly={ended}
              value={editedText}
              onChange={(event) => setEditedText(event.target.value)}
            />
            {!ended && editedText.trim() && editedText !== cur.text && (
              <button
                className="btn block"
                type="button"
                style={{ marginTop: 10 }}
                onClick={() => onPatch(cur.id, { text: editedText.trim() })}
              >
                保存修改
              </button>
            )}
            <div style={{ marginTop: 14 }}>
              <label className="fld">人评</label>
              <div className="rate">
                {[
                  ["unset", "未评"],
                  ["meet", "满足"],
                  ["partial", "部分"],
                  ["deviate", "偏离"],
                  ["fail", "不响应"],
                ].map(([v, lab]) => (
                  <button
                    key={v}
                    type="button"
                    className={(!cur.assessment && v === "unset") || cur.assessment === v ? "on" : undefined}
                    disabled={ended}
                    onClick={() => {
                      if (v === "deviate") {
                        onDeviate();
                        return;
                      }
                      onPatch(cur.id, { assessment: v, deviate: false });
                    }}
                  >
                    {lab}
                  </button>
                ))}
              </div>
            </div>
            {cur.deviate_note && (
              <div style={{ marginTop: 14 }}>
                <label className="fld">偏离说明</label>
                <textarea className="area" readOnly value={cur.deviate_note} />
              </div>
            )}
            {draft && !ended && (
              <div className="row" style={{ marginTop: 14 }}>
                <button className="btn block lg" type="button" onClick={() => onPatch(cur.id, { status: "rejected" })}>
                  驳回草稿
                </button>
                <button className="btn pri block lg" type="button" onClick={() => onConfirm(cur)}>
                  确认本条
                </button>
              </div>
            )}
            {rejected && !ended && (
              <button className="btn block" type="button" style={{ marginTop: 14 }} onClick={() => onPatch(cur.id, { status: "draft" })}>
                恢复为草稿
              </button>
            )}
          </div>
        </div>
        <div className="card">
          <div className="row">
            <span className="step-dot" />
            <span className="step-id">STEP 02</span>
          </div>
          <h3 className="h3" style={{ margin: "6px 0 4px" }}>
            按句对照
          </h3>
          <p className="note" style={{ margin: "0 0 14px" }}>
            展示层。点一句只是帮你读，不会另存要求点。
          </p>
          {(bits.length ? bits : [cur.text]).map((s, i) => (
            <div key={i} className={`inner ${i === 0 ? "iris" : ""}`} style={{ marginTop: i ? 8 : 0, padding: "12px 14px" }}>
              <div className="note" style={{ margin: 0 }}>
                句 {i + 1}
              </div>
              <div style={{ fontWeight: 650, marginTop: 2 }}>{s}</div>
            </div>
          ))}
        </div>
      </div>

      <div className="card">
        <div className="row">
          <span className="step-dot" />
          <span className="step-id">STEP 03</span>
        </div>
        <h3 className="h3" style={{ margin: "6px 0 4px" }}>
          本段勾选与补图
        </h3>
        <p className="note" style={{ margin: "0 0 16px" }}>
          勾选挂在当前段，不是挂在某一句上。先勾型号，再补功能界面图。
        </p>
        <div className="split-eq">
          <div>
            <p className="lbl">系统排序 · 覆盖率</p>
            {candidates.length === 0 ? (
              <p className="note">确认后才会出现候选。</p>
            ) : (
              candidates.map((c) => {
                const on = picks.some((p) => p.product_id === c.product_id);
                return (
                  <div key={c.product_id} className="pick">
                    <div className="nm">{c.product_title}</div>
                    <div className="cv">覆盖 {(c.coverage * 100).toFixed(0)}%</div>
                    <button className="btn sm" type="button" disabled={ended} onClick={() => (on ? onUnpick(c.product_id) : onPick(c.product_id))}>
                      {on ? "已勾入" : "勾入"}
                    </button>
                  </div>
                );
              })
            )}
          </div>
          <div>
            <p className="lbl">补图</p>
            <p className="note">图只挂本条，不写回产品库。回到列表检查器里拖图。</p>
            <button className="btn" type="button" style={{ marginTop: 12 }} onClick={() => go(bidHref(id, view, { clause: cur.id }))}>
              返回列表补图
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
