import { useEffect, useMemo, useRef, useState } from "react";
import { Modal } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import {
  ApiError,
  type BidDoc,
  type BookletPart,
  type Candidate,
  type Clause,
  type Derived,
  type ExtractRun,
  type MatchUnit,
  type Pick,
  type Project,
  type Shot,
  api,
  downloadExport,
} from "../api";
import { bidHref, go, parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { BookletPane } from "./BookletPane";
import { ClauseDetail } from "./ClauseDetail";
import { ClauseTable } from "./ClauseTable";
import { FilesPane } from "./FilesPane";
import { Inspector } from "./Inspector";
import { BidSidebar } from "./Sidebar";
import { bookletKeyFor, liveClauses, partTitle, unitIdForView } from "./helpers";

function toast(msg: string, color: "iris" | "red" = "iris") {
  notifications.show({ message: msg, color });
}

function errMsg(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function matchesRetryRunning(status?: string): boolean {
  return status === "pending" || status === "running";
}

type BidStage = "upload" | "parse" | "eval" | "booklet";

function bidStage(view: string, docs: BidDoc[], derived: Derived, clauses: Clause[]): BidStage {
  if (view === "booklet") return "booklet";
  if (docs.length === 0) return "upload";
  if (!derived.files_ready || derived.extract_running || clauses.length === 0) return "parse";
  return "eval";
}

function BidSteps({ stage }: { stage: BidStage }) {
  const items: { key: BidStage; n: string; label: string }[] = [
    { key: "upload", n: "1", label: "上传" },
    { key: "parse", n: "2", label: "解析" },
    { key: "eval", n: "3", label: "评估" },
    { key: "booklet", n: "4", label: "成稿" },
  ];
  const order: BidStage[] = ["upload", "parse", "eval", "booklet"];
  const cur = order.indexOf(stage);
  return (
    <div className="steps" style={{ marginTop: 10 }}>
      {items.map((it, i) => (
        <div key={it.key} className={i === cur ? "on" : i < cur ? "done" : undefined}>
          <i>{it.n}</i>
          <span>{it.label}</span>
        </div>
      ))}
    </div>
  );
}

export function Workbench({ email }: { email: string }) {
  const path = useHash();
  const route = parseBidRoute(path);
  const id = route?.id ?? "";
  const view = route?.view ?? "commercial";
  const part = route?.part ?? "1";
  const pane = route?.pane ?? "table";

  const [project, setProject] = useState<Project | null>(null);
  const [derived, setDerived] = useState<Derived | null>(null);
  const [latestExtract, setLatestExtract] = useState<ExtractRun | null>(null);
  const [docs, setDocs] = useState<BidDoc[]>([]);
  const [clauses, setClauses] = useState<Clause[]>([]);
  const [units, setUnits] = useState<MatchUnit[]>([]);
  const [booklet, setBooklet] = useState<BookletPart[]>([]);
  const [picks, setPicks] = useState<Pick[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [shots, setShots] = useState<Shot[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [addText, setAddText] = useState("");
  const [addMust, setAddMust] = useState(false);
  const [deviateOpen, setDeviateOpen] = useState(false);
  const [deviateNote, setDeviateNote] = useState("");
  const [regenStale, setRegenStale] = useState(false);
  const [mdDraft, setMdDraft] = useState("");
  const [uploading, setUploading] = useState(false);
  const [pendingNames, setPendingNames] = useState<string[]>([]);
  const drafts = useRef<Record<string, string>>({});
  const dirtyKeys = useRef<Set<string>>(new Set());
  const advanceAfterUpload = useRef(false);
  const ended = project?.status === "ended";
  const bookletKey = bookletKeyFor(view, part, selected, clauses);

  useEffect(() => {
    if (!route) return;
    if (path.includes("/picks") || path.includes("/preview") || path.includes("/booklet/")) {
      go(bidHref(route.id, route.view, route.view === "booklet" ? { part: route.part, pane: "draft" } : undefined));
    }
  }, [path, route]);

  async function load() {
    if (!id) return;
    const unit = unitIdForView(view);
    const [b, d, c, un, bk, sh] = await Promise.all([
      api.bid(id),
      api.docs(id).catch(() => ({ documents: [] })),
      api.clauses(id).catch(() => []),
      api.units(id).catch(() => ({ units: [] })),
      api.booklet(id).catch(() => ({ parts: [] })),
      api.shots(id).catch(() => ({ shots: [] })),
    ]);
    setProject(b.project);
    setDerived(b.derived);
    setLatestExtract(b.latest_extract ?? null);
    setDocs(d.documents);
    setClauses(c);
    setUnits(un.units);
    setBooklet(bk.parts);
    setShots(sh.shots);
    if (unit) {
      const pk = await api.picks(id, unit).catch(() => ({ picks: [], candidates: [] }));
      setPicks(pk.picks);
      setCandidates(pk.candidates);
    } else {
      setPicks([]);
      setCandidates([]);
    }
    setSelected((cur) => {
      if (route?.clause && c.some((x) => x.id === route.clause)) return route.clause;
      const pool = liveClauses(c, view);
      if (cur && pool.some((x) => x.id === cur)) return cur;
      return pool.find((x) => x.status === "draft")?.id ?? pool[0]?.id ?? null;
    });
  }

  useEffect(() => {
    drafts.current = {};
    dirtyKeys.current = new Set();
    void load();
    const t = window.setInterval(() => void load(), 4000);
    return () => clearInterval(t);
  }, [id]);

  useEffect(() => {
    void load();
    setSelected(route?.clause ?? null);
  }, [view]);

  useEffect(() => {
    if (!project || !id) return;
    if (view === "commercial" && docs.length === 0 && clauses.length === 0) {
      go(bidHref(id, "files"));
    }
  }, [project, id, view, docs.length, clauses.length]);

  useEffect(() => {
    if (view !== "files" || !derived || !advanceAfterUpload.current) return;
    if (!derived.files_ready || derived.extract_running) return;
    advanceAfterUpload.current = false;
    if (clauses.length > 0) {
      go(bidHref(id, "commercial"));
      toast("文件已解析，条款已抽出，开始评估");
    }
  }, [view, derived, clauses.length, id]);

  useEffect(() => {
    const server = booklet.find((p) => p.key === bookletKey)?.markdown ?? "";
    if (dirtyKeys.current.has(bookletKey)) {
      setMdDraft(drafts.current[bookletKey] ?? server);
      return;
    }
    drafts.current[bookletKey] = server;
    setMdDraft(server);
  }, [booklet, bookletKey]);

  const live = useMemo(() => liveClauses(clauses, view), [clauses, view]);
  const cur = live.find((c) => c.id === selected) ?? live[0] ?? null;
  const techUnits = units.filter((u) => u.kind === "technical");
  const currentUnit = techUnits.find((u) => u.id === view);
  const currentRetryRunning = matchesRetryRunning(currentUnit?.retry_status);
  const currentPart = booklet.find((p) => p.key === bookletKey);
  const anyStale = booklet.some((p) => p.stale);
  const extractNotice = useMemo(() => {
    if (!latestExtract || latestExtract.status === "running" || latestExtract.status === "pending") return "";
    const fallback = latestExtract.diagnostics?.fallback_reasons?.length ?? 0;
    const uncovered = latestExtract.diagnostics?.coverage?.uncovered_spans?.length ?? 0;
    if (latestExtract.status === "failed") return `条款抽取失败：${latestExtract.error_message || "请检查模型和文件后重试"}`;
    if (latestExtract.partial_failure || (latestExtract.failed_documents ?? 0) > 0) {
      return `本次有 ${latestExtract.failed_documents ?? 1} 个文件抽取失败：${latestExtract.error_message || "请检查后重试"}`;
    }
    if (uncovered > 0) return `本次仍有 ${uncovered} 个候选片段未覆盖，请人工复核或重抽。`;
    if (fallback > 0 || latestExtract.extractor_mode === "heuristic") return `本次使用了规则兜底（${latestExtract.extractor_mode}），草稿需重点复核。`;
    return "";
  }, [latestExtract]);

  function setDraft(text: string) {
    drafts.current[bookletKey] = text;
    dirtyKeys.current.add(bookletKey);
    setMdDraft(text);
  }

  async function patch(cid: string, body: Partial<Clause>) {
    try {
      await api.patchClause(id, cid, body);
      await load();
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  async function saveDraft() {
    try {
      await api.saveBooklet(id, bookletKey, drafts.current[bookletKey] ?? mdDraft);
      dirtyKeys.current.delete(bookletKey);
      toast("已保存成稿");
      await load();
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  async function regen() {
    try {
      await api.regenBooklet(id, bookletKey);
      dirtyKeys.current.delete(bookletKey);
      delete drafts.current[bookletKey];
      toast("已按当前数据重生成");
      await load();
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  async function doExport(format: "docx" | "pdf") {
    try {
      await downloadExport(id, format, ended ? false : regenStale);
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  if (!route || !project || !derived) {
    return (
      <Shell root="bids" email={email} crumbs="投标项目" title="投标">
        <div className="wrap">加载中…</div>
      </Shell>
    );
  }

  const job = view === "booklet" ? "成稿" : view === "files" ? "文件" : "评估";
  const sectionLabel =
    view === "commercial"
      ? "商务"
      : view === "unsectioned"
        ? "未归段"
        : view === "files"
          ? ""
          : view === "booklet"
            ? partTitle(part, units)
            : techUnits.find((u) => u.id === view)?.heading_path || "技术段";
  const crumbs = ["投标项目", project.title, job, sectionLabel].filter(Boolean).join(" / ");
  const pageTitle =
    view === "files"
      ? "招标文件"
      : view === "booklet"
        ? partTitle(part, units)
        : pane === "detail" && cur
          ? cur.text.slice(0, 18)
          : view === "commercial"
            ? "商务条款"
            : sectionLabel;

  const matchMsg = view === "commercial" ? "正在按资格条款检索资料" : "正在按本段参数匹配产品";
  const failed = docs.some((d) => d.parse_status === "failed");

  const extra = (
    <>
      {view === "booklet" && (
        <nav className="mode">
          <button type="button" className={pane !== "draft" ? "on" : undefined} onClick={() => go(bidHref(id, "booklet", { part, pane: "table" }))}>
            预览
          </button>
          <button type="button" className={pane === "draft" ? "on" : undefined} onClick={() => go(bidHref(id, "booklet", { part, pane: "draft" }))}>
            编辑
          </button>
        </nav>
      )}
      {pane === "detail" && (
        <button className="btn" type="button" onClick={() => go(bidHref(id, view, { clause: selected }))}>
          返回列表
        </button>
      )}
      {ended ? (
        <span className="chip gray">已结束</span>
      ) : (
        <button
          className="btn"
          type="button"
          onClick={() => {
            void api
              .endBid(id)
              .then(() => {
                toast("本标已结束，文稿只读");
                setRegenStale(false);
                void load();
              })
              .catch((e) => toast(errMsg(e), "red"));
          }}
        >
          结束本标
        </button>
      )}
      {anyStale && !ended && (
        <label className="row" style={{ fontSize: 12.5, color: "var(--muted)" }}>
          <input type="checkbox" checked={regenStale} onChange={(e) => setRegenStale(e.target.checked)} />
          导出时重生成过期稿
        </label>
      )}
      <button className="btn" type="button" onClick={() => void doExport("docx")}>
        Word
      </button>
      <button className="btn pri" type="button" onClick={() => void doExport("pdf")}>
        定稿 PDF
      </button>
    </>
  );

  const tree = (
    <BidSidebar id={id} view={view} part={part} units={units} booklet={booklet} docs={docs} clauses={clauses} />
  );

  const inspector =
    view === "files" || view === "booklet" || pane === "detail" ? undefined : (
      <Inspector
        view={view}
        cur={view === "booklet" ? null : cur}
        ended={ended}
        derivedMatch={derived.match_running}
        candidates={candidates}
        picks={picks}
        shots={shots}
        currentPart={currentPart}
        projectId={id}
        onPatch={patch}
        onConfirm={(c) => void patch(c.id, { status: "confirmed" })}
        onPick={(pid) => {
          const uid = unitIdForView(view);
          if (!uid) return;
          void api
            .pick(id, pid, uid)
            .then(() => load())
            .catch((e) => toast(errMsg(e), "red"));
        }}
        onUnpick={(pid) => {
          const uid = unitIdForView(view);
          if (!uid) return;
          void api
            .unpick(id, pid, uid)
            .then(() => load())
            .catch((e) => toast(errMsg(e), "red"));
        }}
        onRegen={() => void regen()}
        onShots={() => void load()}
        onDeviate={() => {
          setDeviateNote(cur?.deviate_note ?? "");
          setDeviateOpen(true);
        }}
      />
    );

  return (
    <Shell
      root="bids"
      email={email}
      crumbs={crumbs}
      title={pageTitle}
      extra={extra}
      lead={<BidSteps stage={bidStage(view, docs, derived, clauses)} />}
      tree={tree}
      inspector={inspector}
      className={view === "booklet" ? "ed-page" : undefined}
    >
      {(derived.extract_running || derived.match_running || failed) && view !== "booklet" && (
        <div className={`banner ${failed ? "bad" : ""}`} style={{ margin: inspector ? "0 0 16px" : "16px 24px 0" }}>
          {failed ? "有文件解析失败。到「文件」重试或删除后再传。" : derived.extract_running ? "正在抽条款。抽出的草稿会出现在表里。" : matchMsg}
        </div>
      )}
      {extractNotice && !derived.extract_running && view !== "booklet" && (
        <div className={`banner ${latestExtract?.status === "failed" ? "bad" : "warn"}`} style={{ margin: inspector ? "0 0 16px" : "16px 24px 0" }}>
          {extractNotice}
        </div>
      )}
      {anyStale && view === "booklet" && (
        <div className="banner warn" style={{ margin: "12px 24px 0" }}>
          成稿已过期。导出默认保留人句；勾选「重生成」才会覆盖。
        </div>
      )}
      {view === "files" ? (
        <>
        <FilesPane
          docs={docs}
          ended={ended}
          uploading={uploading}
          pendingNames={pendingNames}
          onUpload={(files) => {
            setUploading(true);
            setPendingNames(files.map((f) => f.name));
            advanceAfterUpload.current = true;
            toast(`正在上传 ${files.length} 个文件`);
            void Promise.all(files.map((f) => api.uploadDoc(id, f)))
              .then(() => {
                toast("已上传，正在解析");
                return load();
              })
              .catch((e) => toast(errMsg(e), "red"))
              .finally(() => {
                setUploading(false);
                setPendingNames([]);
              });
          }}
          onRetry={(docId) => {
            void api
              .retryDoc(id, docId)
              .then(() => toast("已重试"))
              .catch((e) => toast(errMsg(e), "red"));
          }}
          onDelete={(docId) => {
            void api
              .deleteDoc(id, docId)
              .then(() => {
                toast("已删除");
                void load();
              })
              .catch((e) => toast(errMsg(e), "red"));
          }}
        />
        {docs.length > 0 && (
          <div className="wrap" style={{ paddingTop: 0 }}>
            {derived.extract_running && <div className="banner">文件已解析。正在抽商务 / 技术条款。</div>}
            {derived.files_ready && !derived.extract_running && clauses.length === 0 && (
              <div className="card">
                <h3 className="h3">抽取没有出条款</h3>
                <p className="note" style={{ margin: "8px 0 16px" }}>
                  可以再抽一次，或先去评估里手补。漏抽不锁死流程。
                </p>
                <div className="row">
                  <button
                    className="btn"
                    type="button"
                    disabled={ended}
                    onClick={() => {
                      void api.reextract(id).then(() => {
                        toast("已重新抽取");
                        return load();
                      }).catch((e) => toast(errMsg(e), "red"));
                    }}
                  >
                    再抽一次
                  </button>
                  <button className="btn pri" type="button" onClick={() => go(bidHref(id, "commercial"))}>
                    去评估手补
                  </button>
                </div>
              </div>
            )}
            {derived.files_ready && !derived.extract_running && clauses.length > 0 && (
              <div className="row" style={{ justifyContent: "flex-end" }}>
                <button className="btn pri" type="button" onClick={() => go(bidHref(id, "commercial"))}>
                  去评估
                </button>
              </div>
            )}
          </div>
        )}
        </>
      ) : view === "booklet" && docs.length === 0 ? (
        <div className="wrap">
          <div className="card">
            <div className="empty">
              <h2>先上传招标文件</h2>
              <p className="note" style={{ margin: "0 0 16px" }}>
                抽出条款并评估后，再编成稿、导出 Word / PDF。
              </p>
              <button className="btn pri" type="button" onClick={() => go(bidHref(id, "files"))}>
                去上传
              </button>
            </div>
          </div>
        </div>
      ) : view === "booklet" ? (
        <BookletPane
          mdDraft={mdDraft}
          ended={ended}
          preview={pane !== "draft"}
          onChange={setDraft}
          onSave={() => void saveDraft()}
          onRegen={() => void regen()}
        />
      ) : pane === "detail" && cur ? (
        <ClauseDetail
          id={id}
          view={view}
          cur={cur}
          ended={ended}
          candidates={candidates}
          picks={picks}
          shots={shots}
          projectId={id}
          onPatch={patch}
          onConfirm={(c) => void patch(c.id, { status: "confirmed" })}
          onPick={(pid) => {
            const uid = unitIdForView(view);
            if (!uid) return;
            void api.pick(id, pid, uid).then(() => load());
          }}
          onUnpick={(pid) => {
            const uid = unitIdForView(view);
            if (!uid) return;
            void api.unpick(id, pid, uid).then(() => load());
          }}
          onDeviate={() => {
            setDeviateNote(cur.deviate_note ?? "");
            setDeviateOpen(true);
          }}
          onShots={() => void load()}
        />
      ) : (
        <ClauseTable
          id={id}
          view={view}
          live={live}
          selected={selected}
          ended={ended}
          addText={addText}
          addMust={addMust}
          hasFiles={docs.length > 0}
          filesReady={derived.files_ready}
          extractRunning={derived.extract_running || currentRetryRunning}
          retryStatus={currentUnit?.retry_status}
          retryError={currentUnit?.error_message}
          onGoFiles={() => go(bidHref(id, "files"))}
          onExtract={() => {
            const retry = view !== "commercial" && view !== "unsectioned"
              ? api.retrySection(id, view)
              : api.reextract(id);
            void retry.then(() => {
              toast(view !== "commercial" && view !== "unsectioned" ? "已排队重抽本段" : "已重新抽取");
              return load();
            }).catch((e) => toast(errMsg(e), "red"));
          }}
          onSelect={setSelected}
          onConfirm={(c) => void patch(c.id, { status: "confirmed" })}
          onReject={(c) => void patch(c.id, { status: "rejected" })}
          onMerge={
            view !== "commercial" && techUnits.find((u) => u.id === view)?.prev_id
              ? () => {
                  const prev = techUnits.find((u) => u.id === view)?.prev_id;
                  if (!prev) return;
                  void api
                    .mergeSection(id, view, prev)
                    .then(() => {
                      toast("已并入上一段");
                      go(bidHref(id, prev));
                    })
                    .catch((e) => toast(errMsg(e), "red"));
                }
              : undefined
          }
          onAddText={setAddText}
          onAddMust={setAddMust}
          onAdd={() => {
            if (!addText.trim()) return;
            const family = view === "commercial" ? "commercial" : "technical";
            const section_id = view === "commercial" || view === "unsectioned" ? null : view;
            void api
              .addClause(id, { text: addText.trim(), family, must: addMust, section_id })
              .then(() => {
                setAddText("");
                setAddMust(false);
                toast("已手补并确认");
                return load();
              })
              .catch((e) => toast(errMsg(e), "red"));
          }}
        />
      )}
      <Modal opened={deviateOpen} onClose={() => setDeviateOpen(false)} title="偏离说明" radius={16}>
        <label className="fld">偏离说明会进 ③ 技术偏离表</label>
        <textarea className="area" value={deviateNote} onChange={(e) => setDeviateNote(e.target.value)} />
        <div className="row" style={{ justifyContent: "flex-end", marginTop: 16 }}>
          <button className="btn" type="button" onClick={() => setDeviateOpen(false)}>
            取消
          </button>
          <button
            className="btn pri"
            type="button"
            onClick={() => {
              if (cur) void patch(cur.id, { deviate: true, deviate_note: deviateNote, assessment: "deviate" });
              setDeviateOpen(false);
            }}
          >
            记下
          </button>
        </div>
      </Modal>
    </Shell>
  );
}
