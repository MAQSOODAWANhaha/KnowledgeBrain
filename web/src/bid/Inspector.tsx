import { useEffect, useState } from "react";
import { Dropzone } from "@mantine/dropzone";
import type { BookletPart, Candidate, Clause, Pick, Shot } from "../api";
import { api, fileBlob } from "../api";
import { bidHref } from "../hash";
import { assessmentLabel, unitIdForView } from "./helpers";

export function Inspector({
  view,
  cur,
  ended,
  derivedMatch,
  candidates,
  picks,
  shots,
  currentPart,
  projectId,
  onPatch,
  onPick,
  onUnpick,
  onRegen,
  onShots,
  onDeviate,
  onConfirm,
}: {
  view: string;
  cur: Clause | null;
  ended: boolean;
  derivedMatch: boolean;
  candidates: Candidate[];
  picks: Pick[];
  shots: Shot[];
  currentPart?: BookletPart;
  projectId: string;
  onPatch: (cid: string, body: Partial<Clause>) => void;
  onPick: (productId: string) => void;
  onUnpick: (productId: string) => void;
  onRegen: () => void;
  onShots: () => void;
  onDeviate: () => void;
  onConfirm: (c: Clause) => void;
}) {
  const uid = unitIdForView(view);
  if (view === "booklet") {
    return (
      <>
        <p className="lbl">本分册</p>
        <p className="note" style={{ margin: 0 }}>
          {currentPart?.stale ? "数据已变，当前是人改过的稿。导出默认保留人句。" : "过程 Word 随时下。定稿 PDF 默认保留人句。"}
        </p>
        {!ended && (
          <button className="btn" type="button" style={{ marginTop: 16 }} onClick={onRegen}>
            按数据重生成
          </button>
        )}
      </>
    );
  }
  if (!cur) {
    return <p className="note">从表里点一条条款。</p>;
  }
  const draft = cur.status === "draft";
  const rejected = cur.status === "rejected";
  return (
    <>
      <p className="lbl">当前条款</p>
      <h3 className="h3">{cur.text || "（空）"}</h3>
      <p className="note" style={{ margin: "0 0 16px" }}>
        {cur.must ? "必须 · " : ""}
        {view === "commercial" ? "商务" : "技术"}
        {draft ? " · 待确认，不进匹配" : rejected ? " · 已驳回，不进匹配" : " · 已确认"}
      </p>
      {draft && (
        <div className="inner" style={{ marginBottom: 14 }}>
          {cur.family_conflict && (
            <p className="note" style={{ margin: "0 0 10px" }}>
              技术与商务抽取器都命中了这段原文。请先选择正确分类，再确认。
            </p>
          )}
          <p className="lbl">条款分类</p>
          <div className="row">
            <button className={`btn sm ${cur.family === "technical" ? "pri" : ""}`} type="button" disabled={ended} onClick={() => onPatch(cur.id, { family: "technical" })}>
              技术条款
            </button>
            <button className={`btn sm ${cur.family === "commercial" ? "pri" : ""}`} type="button" disabled={ended} onClick={() => onPatch(cur.id, { family: "commercial" })}>
              商务条款
            </button>
            <label className="row" style={{ fontSize: 12.5 }}>
              <input type="checkbox" checked={cur.must} disabled={ended} onChange={(event) => onPatch(cur.id, { must: event.target.checked })} />
              必须条款
            </label>
          </div>
        </div>
      )}
      {draft && !ended && (
        <button className="btn pri block lg" type="button" onClick={() => onConfirm(cur)}>
          确认本条
        </button>
      )}
      {rejected && !ended && (
        <button className="btn block" type="button" onClick={() => onPatch(cur.id, { status: "draft" })}>
          恢复为草稿
        </button>
      )}
      {view === "commercial" ? (
        <div style={{ marginTop: 22 }}>
          <p className="lbl">检索结果</p>
          {draft ? (
            <p className="note" style={{ margin: 0 }}>
              确认后才按资料库检索证照。商务只判命中 / 缺件。
            </p>
          ) : cur.hit_outcome === "hit" ? (
            <p className="note" style={{ margin: 0 }}>
              命中 {cur.hit_file || "材料"}
            </p>
          ) : cur.hit_outcome === "miss" ? (
            <>
              <p className="note">资料库未命中。</p>
              <a className="btn pri sm" href="#/library">
                去补证
              </a>
            </>
          ) : (
            <p className="note" style={{ margin: 0 }}>
              尚未检索。
            </p>
          )}
        </div>
      ) : (
        <div style={{ marginTop: 22 }}>
          <p className="lbl">本段候选 · 覆盖率</p>
          {candidates.length === 0 ? (
            <p className="note" style={{ margin: 0 }}>
              {derivedMatch ? "匹配还在跑。" : "确认本段条款后会出现候选。系统排序，人勾选。"}
            </p>
          ) : (
            candidates.map((c) => {
              const on = picks.some((p) => p.product_id === c.product_id);
              return (
                <div key={c.product_id} className="pick">
                  <div className="nm">{c.product_title}</div>
                  <div className="cv">覆盖 {(c.coverage * 100).toFixed(0)}%</div>
                  <button
                    className="btn sm"
                    type="button"
                    disabled={ended || !uid}
                    onClick={() => (on ? onUnpick(c.product_id) : onPick(c.product_id))}
                  >
                    {on ? "去掉" : "勾入"}
                  </button>
                </div>
              );
            })
          )}
          <p className="lbl" style={{ marginTop: 18 }}>
            补图
          </p>
          <Shots clauseId={cur.id} shots={shots} projectId={projectId} picks={picks} candidates={candidates} ended={ended} onChange={onShots} />
        </div>
      )}
      {!ended && (
        <div style={{ marginTop: 22 }}>
          <p className="lbl">人评 {assessmentLabel(cur.assessment)}</p>
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
      )}
      {!draft && view !== "commercial" && (cur.text || "").length >= 36 && (
        <p className="note" style={{ marginTop: 18 }}>
          <a href={`#${bidHref(projectId, view, { pane: "detail", clause: cur.id })}`}>深挖这一条</a>
        </p>
      )}
    </>
  );
}

function Shots({
  clauseId,
  shots,
  projectId,
  picks,
  candidates,
  ended,
  onChange,
}: {
  clauseId: string;
  shots: Shot[];
  projectId: string;
  picks: Pick[];
  candidates: Candidate[];
  ended: boolean;
  onChange: () => void;
}) {
  const mine = shots.filter((s) => s.clause_id === clauseId);
  const [productId, setProductId] = useState(picks[0]?.product_id ?? "");
  useEffect(() => {
    if (!picks.some((p) => p.product_id === productId)) {
      setProductId(picks[0]?.product_id ?? "");
    }
  }, [picks, productId]);
  const pick = picks.find((p) => p.product_id === productId);
  return (
    <div>
      <div className="shots">
        {mine.map((s) => (
          <ShotImg key={s.id} objectKey={s.object_ref} onDel={ended ? undefined : () => api.deleteShot(projectId, s.id).then(onChange)} />
        ))}
        {!ended && pick && (
          <Dropzone
            className="shot"
            style={{ display: "grid", placeItems: "center", color: "var(--faint)", fontSize: 12, fontWeight: 650 }}
            accept={{ "image/*": [] }}
            multiple={false}
            onDrop={(files) => {
              const f = files[0];
              if (!f) return;
              void api
                .uploadShot(projectId, { clause_id: clauseId, product_id: pick.product_id, version_id: pick.version_id, file: f })
                .then(onChange);
            }}
          >
            再拖一张
          </Dropzone>
        )}
      </div>
      {!ended && picks.length > 1 && (
        <div className="row" style={{ marginTop: 8, flexWrap: "wrap" }}>
          {picks.map((p) => (
            <button
              key={p.product_id}
              type="button"
              className={`chip ${p.product_id === productId ? "iris" : ""}`}
              onClick={() => setProductId(p.product_id)}
            >
              {candidates.find((c) => c.product_id === p.product_id)?.product_title || p.product_id.slice(0, 6)}
            </button>
          ))}
        </div>
      )}
      {!pick && (
        <p className="note">{ended ? "已结束，不能补图。" : "先勾入本段产品，才能补图。"}</p>
      )}
    </div>
  );
}

function ShotImg({ objectKey, onDel }: { objectKey: string; onDel?: () => void }) {
  const [src, setSrc] = useState("");
  useEffect(() => {
    void fileBlob(objectKey).then(setSrc);
  }, [objectKey]);
  return (
    <div className="shot">
      {src && <img src={src} alt="本条条款配图" />}
      {onDel && (
        <button className="btn sm" type="button" style={{ position: "absolute", right: 6, bottom: 6 }} onClick={onDel}>
          去掉
        </button>
      )}
    </div>
  );
}
