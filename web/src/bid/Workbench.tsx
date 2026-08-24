import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import {
  ApiError,
  type BidDoc,
  type Candidate,
  type Clause,
  type CompanyProfile,
  type Derived,
  type FactSuggestion,
  type GateIssue,
  type MatchUnit,
  type MutationAttempt,
  type PartStatus,
  type ProceduralAttachment,
  type ProceduralClassification,
  type Project,
  type QuoteLine,
  type QuoteState,
  type RoutePickSet,
  type SubmissionProfile,
  api,
  createMutationAttempt,
  createSubmissionExportAttempt,
  exportSubmission,
} from "../api";
import { Crumbs } from "../Crumbs";
import { BID_STEPS, type BidStep, bidHref, parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { ClauseTable } from "./ClauseTable";
import { FactsPane } from "./FactsPane";
import { FilesPane } from "./FilesPane";
import { Inspector } from "./Inspector";
import { MatchingPane } from "./MatchingPane";
import { MaterialsPane } from "./MaterialsPane";
import { PartsPane } from "./PartsPane";
import { QuotePane } from "./QuotePane";
import { BidSidebar } from "./Sidebar";
import { liveClauses, partTitle } from "./helpers";

function toast(msg: string, color: "blue" | "red" = "blue") {
  notifications.show({ message: msg, color });
}

function errMsg(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

async function uploadContentSha256(file: File): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", await file.arrayBuffer());
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function Wizard({ id, step }: { id: string; step: BidStep }) {
  const cur = BID_STEPS.findIndex((s) => s.key === step);
  return (
    <nav className="wizard">
      {BID_STEPS.map((it, i) => (
        <a
          key={it.key}
          data-testid={`wizard-${it.key}`}
          className={it.key === step ? "on" : i < cur ? "done" : undefined}
          href={`#${bidHref(id, it.key, { step: it.key })}`}
        >
          <i>{it.n}</i>
          <span>{it.label}</span>
        </a>
      ))}
    </nav>
  );
}

export function Workbench({ email }: { email: string }) {
  const path = useHash();
  const route = parseBidRoute(path);
  const id = route?.id ?? "";
  const step: BidStep = route?.step ?? "files";
  const view = route?.view ?? "files";
  const part = route?.part ?? "1";
  const pane = route?.pane ?? "table";
  const doc = route?.doc ?? null;

  const [project, setProject] = useState<Project | null>(null);
  const [derived, setDerived] = useState<Derived | null>(null);
  const [docs, setDocs] = useState<BidDoc[]>([]);
  const [clauses, setClauses] = useState<Clause[]>([]);
  const [units, setUnits] = useState<MatchUnit[]>([]);
  const [suggestions, setSuggestions] = useState<FactSuggestion[]>([]);
  const [quote, setQuote] = useState<QuoteState>({ exists: false });
  const [preview, setPreview] = useState<{ net_total?: string; tax_total?: string; gross_total?: string }>();
  const [matching, setMatching] = useState<Awaited<ReturnType<typeof api.matching>>>();
  const [pickSet, setPickSet] = useState<RoutePickSet | null>(null);
  const [company, setCompany] = useState<CompanyProfile>({});
  const [submission, setSubmission] = useState<SubmissionProfile>({});
  const [classifications, setClassifications] = useState<ProceduralClassification[]>([]);
  const [attachments, setAttachments] = useState<ProceduralAttachment[]>([]);
  const [parts, setParts] = useState<PartStatus[]>([]);
  const [requiredKeys, setRequiredKeys] = useState<string[]>([]);
  const [clauseSets, setClauseSets] = useState<Array<{ set_kind: string; revision: number; content_sha256: string }>>([]);

  const [partMarkdown, setPartMarkdown] = useState("");
  const [partRevision, setPartRevision] = useState(0);
  const [partDependencySha, setPartDependencySha] = useState<string | null>(null);
  const [partStale, setPartStale] = useState(false);
  const [loadedPartIdentity, setLoadedPartIdentity] = useState<string | null>(null);
  const [gate, setGate] = useState<{ status: string; issues: GateIssue[] }>();
  const [selected, setSelected] = useState<string | null>(null);
  const [addText, setAddText] = useState("");
  const [addKind, setAddKind] = useState("technical");
  const [addMust, setAddMust] = useState(false);
  const [factDrafts, setFactDrafts] = useState<Record<string, string>>({});
  const [noCeiling, setNoCeiling] = useState(false);
  const [noCeilingReason, setNoCeilingReason] = useState("招标文件未设置最高限价，已人工复核");
  const [quoteSaving, setQuoteSaving] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [pendingNames, setPendingNames] = useState<string[]>([]);
  const [partPreview, setPartPreview] = useState(pane !== "draft");
  const dirtyPart = useRef(false);
  const activePartIdentity = useRef<string | null>(null);
  const partLoadSequence = useRef(0);
  const uploadRetryAttempts = useRef(new Map<string, MutationAttempt[]>());
  const submissionExportAttempts = useRef(new Map<string, ReturnType<typeof createSubmissionExportAttempt>>());
  const companyDirty = useRef(false);
  const submissionDirty = useRef(false);
  const pickMutationTail = useRef<Promise<void>>(Promise.resolve());
  const ended = project?.status === "ended";
  const partIdentity = step === "parts" ? `${id}:${part}` : null;
  const partReady = partIdentity !== null && loadedPartIdentity === partIdentity;
  activePartIdentity.current = partIdentity;

  async function uploadWithAttempt<T>(
    operation: string,
    file: File,
    request: (attempt: MutationAttempt) => Promise<T>,
  ): Promise<T> {
    const contentSha256 = await uploadContentSha256(file);
    const attemptKey = `${id}:${operation}:${file.name}:${file.type}:${file.size}:${contentSha256}`;
    const retryQueue = uploadRetryAttempts.current.get(attemptKey);
    const attempt = retryQueue?.shift() ?? createMutationAttempt();
    if (retryQueue?.length === 0) uploadRetryAttempts.current.delete(attemptKey);
    try {
      return await request(attempt);
    } catch (error) {
      if (!(error instanceof ApiError)) {
        const pending = uploadRetryAttempts.current.get(attemptKey) ?? [];
        pending.push(attempt);
        uploadRetryAttempts.current.set(attemptKey, pending);
      }
      throw error;
    }
  }

  const load = useCallback(async () => {
    if (!id) return;
    try {
      const [detail, clausePage, unitPage, factPage, partPage] = await Promise.all([
        api.bid(id),
        api.clauses(id).catch(() => ({ clauses: [] })),
        api.units(id).catch(() => ({ units: [] })),
        api.facts(id).catch(() => ({ suggestions: [] as FactSuggestion[] })),
        api.parts(id).catch(() => ({ required_part_keys: [] as string[], parts: [] as PartStatus[] })),
      ]);
      setProject(detail.project);
      setDerived(detail.derived);
      setDocs(detail.documents);
      setPendingNames((names) => names.filter((n) => !detail.documents.some((d) => d.file_name === n)));
      setQuote(detail.quote ?? { exists: false });
      setMatching(detail.matching);
      setClauses(clausePage.clauses);
      setUnits(unitPage.units);
      setSuggestions(factPage.suggestions ?? detail.facts?.suggestions ?? []);
      setRequiredKeys(partPage.required_part_keys ?? []);
      setParts(partPage.parts ?? []);
      setClauseSets(detail.clause_sets ?? []);
      const pool = liveClauses(clausePage.clauses, view);
      setSelected((cur) => (cur && pool.some((x) => x.id === cur) ? cur : pool[0]?.id ?? null));
      if (step === "matching") {
        const routeId =
          view === "commercial"
            ? unitPage.units.find((u) => u.kind === "commercial")?.route_id
            : view === "unsectioned"
              ? unitPage.units.find((u) => u.kind === "unsectioned")?.route_id
              : unitPage.units.find((u) => u.id === view)?.route_id;
        if (routeId) {
          const set = await api.routePickSet(id, routeId).catch(() => null);
          setPickSet(set);
        } else {
          setPickSet(null);
        }
      }
      if (step === "quote") {
        const [cp, sp, pr, at, pv] = await Promise.all([
          api.companyProfile(id).catch(() => null),
          api.submissionProfile(id).catch(() => null),
          api.procedural(id).catch(() => ({ classifications: [] })),
          api.attachments(id).catch(() => ({ attachments: [] })),
          api.previewQuote(id).catch(() => undefined),
        ]);
        if (cp && !companyDirty.current) setCompany(cp);
        if (sp && !submissionDirty.current) setSubmission(sp);
        setClassifications(pr.classifications ?? []);
        setAttachments(at.attachments ?? []);
        setPreview(pv);
      }
      if (step === "parts") {
        const requestedPartIdentity = `${id}:${part}`;
        const loadSequence = ++partLoadSequence.current;
        const current = await api.part(id, part).catch(() => null);
        if (
          activePartIdentity.current === requestedPartIdentity &&
          partLoadSequence.current === loadSequence &&
          !dirtyPart.current
        ) {
          setPartMarkdown(current?.markdown ?? "");
          setPartRevision(current?.content_revision ?? 0);
          setPartDependencySha(current?.dependency_sha256 ?? null);
          setPartStale(!!current?.stale);
          setLoadedPartIdentity(requestedPartIdentity);
        }
        const g = await api.gateIssues(id, "pdf").catch(() => undefined);
        if (g) setGate({ status: g.status, issues: g.issues });
      }
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }, [id, part, step, view]);

  useEffect(() => {
    dirtyPart.current = false;
    void load();
    const t = window.setInterval(() => void load(), 5000);
    return () => clearInterval(t);
  }, [load]);

  useEffect(() => {
    uploadRetryAttempts.current.clear();
    submissionExportAttempts.current.clear();
    companyDirty.current = false;
    submissionDirty.current = false;
    pickMutationTail.current = Promise.resolve();
  }, [id]);

  useEffect(() => {
    if (project?.ceiling_price) setNoCeiling(false);
  }, [project?.ceiling_price]);

  const live = useMemo(() => liveClauses(clauses, view), [clauses, view]);
  const cur = live.find((c) => c.id === selected) ?? live[0] ?? null;

  async function mutateClause(c: Clause, action: "patch" | "confirm" | "unconfirm" | "reject", patch?: Record<string, unknown>) {
    try {
      await api.mutateClause(id, c.id, { action, expected_revision: c.revision, patch });
      await load();
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  async function setFact(field: string, raw: string, basis = project?.ceiling_basis ?? "unspecified") {
    if (!project) return;
    let typed: unknown = raw;
    if (field === "bid_valid_days") typed = Number(raw);
    if (field === "budget_amount") typed = { amount: raw, currency_code: "CNY" };
    if (field === "ceiling_price" || field === "ceiling_basis") {
      const amount = field === "ceiling_price" ? raw : project.ceiling_price || raw;
      if (!amount) {
        toast("先写入最高限价金额", "red");
        return;
      }
      typed = { amount, currency_code: "CNY", basis: field === "ceiling_basis" ? raw : basis };
      field = "ceiling_price";
    }
    try {
      await api.mutateFact(id, {
        action: "set",
        expected_fact_revision: project.fact_revision,
        field,
        typed_value: typed,
      });
      toast("已写入事实");
      await load();
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  async function clearFact(field: string) {
    if (!project) return;
    try {
      await api.mutateFact(id, {
        action: "clear",
        expected_fact_revision: project.fact_revision,
        field,
      });
      toast("已清除事实");
      await load();
    } catch (e) {
      toast(errMsg(e), "red");
    }
  }

  function runQuoteMutation(request: () => Promise<unknown>, successMessage?: string) {
    if (quoteSaving) return;
    setQuoteSaving(true);
    void request()
      .then(async () => {
        if (successMessage) toast(successMessage);
        await load();
      })
      .catch((e) => toast(errMsg(e), "red"))
      .finally(() => setQuoteSaving(false));
  }

  function queuePickMutation(candidate: Candidate, include: boolean) {
    const routeId = pickSet?.route_id;
    if (!routeId) return;
    pickMutationTail.current = pickMutationTail.current
      .then(async () => {
        const current = await api.routePickSet(id, routeId);
        if (!current.source_report_artifact_id || !current.report_sha256) return;
        const items = current.items
          .filter((item) => include || item.candidate_artifact_id !== candidate.candidate_artifact_id)
          .map((item) => ({
            requirement_artifact_id: item.requirement_artifact_id,
            candidate_artifact_id: item.candidate_artifact_id,
          }));
        if (include && !items.some((item) => item.candidate_artifact_id === candidate.candidate_artifact_id)) {
          items.push({
            requirement_artifact_id: candidate.requirement_artifact_id,
            candidate_artifact_id: candidate.candidate_artifact_id,
          });
        }
        await api.replaceRoutePickSet(id, routeId, {
          source_report_artifact_id: current.source_report_artifact_id,
          report_sha256: current.report_sha256,
          expected_revision: current.revision,
          items,
        });
        const refreshed = await api.routePickSet(id, routeId);
        setPickSet((active) => (active?.route_id === routeId ? refreshed : active));
      })
      .catch(async (error) => {
        toast(errMsg(error), "red");
        const refreshed = await api.routePickSet(id, routeId).catch(() => null);
        if (refreshed) setPickSet((active) => (active?.route_id === routeId ? refreshed : active));
      });
  }

  async function doExport(format: "docx" | "pdf") {
    const attemptKey = `${id}:${format}`;
    const attempt = submissionExportAttempts.current.get(attemptKey) ?? createSubmissionExportAttempt();
    submissionExportAttempts.current.set(attemptKey, attempt);
    try {
      await exportSubmission(id, format, attempt);
      submissionExportAttempts.current.delete(attemptKey);
      toast(format === "pdf" ? "正式 PDF 已下载" : "过程 Word 已下载");
    } catch (e) {
      if (e instanceof ApiError && e.code !== "SUBMISSION_RENDER_TIMEOUT") {
        submissionExportAttempts.current.delete(attemptKey);
      }
      toast(errMsg(e), "red");
    }
  }

  if (!route || !project || !derived) {
    return (
      <Shell root="bids" email={email} crumbs={<Crumbs items={[{ label: "投标项目" }]} />} title="投标">
        <div className="wrap">加载中…</div>
      </Shell>
    );
  }

  const extra = (
    <>
      {ended ? (
        <span className="chip gray">已结束</span>
      ) : (
        <Button
          variant="default"
          onClick={() => {
            void api
              .endBid(id, project.fact_revision)
              .then(() => {
                toast("本标已结束");
                void load();
              })
              .catch((e) => toast(errMsg(e), "red"));
          }}
        >
          结束本标
        </Button>
      )}
      <Button data-testid="export-docx" variant="default" onClick={() => void doExport("docx")}>
        过程 Word
      </Button>
      <Button data-testid="export-pdf" onClick={() => void doExport("pdf")}>
        正式 PDF
      </Button>
    </>
  );

  const inspector =
    step === "facts" || step === "matching" ? (
      <Inspector
        step={step}
        cur={cur}
        ended={ended}
        pickSet={pickSet}
        onPatch={(c, patch) => void mutateClause(c, "patch", patch)}
        onConfirm={(c) => void mutateClause(c, "confirm")}
        onUnconfirm={(c) => void mutateClause(c, "unconfirm")}
        onPickToggle={(candidate: Candidate, include: boolean) => {
          queuePickMutation(candidate, include);
        }}
      />
    ) : undefined;

  return (
    <Shell
      root="bids"
      email={email}
      crumbs={
        <Crumbs
          items={[
            { label: "投标项目", href: "/" },
            { label: project.title, href: bidHref(id, "files", { step: "files" }) },
            { label: BID_STEPS.find((s) => s.key === step)?.label ?? "" },
          ]}
        />
      }
      title={
        step === "parts"
          ? partTitle(part, units)
          : step === "files"
            ? "招标文件"
            : BID_STEPS.find((s) => s.key === step)?.label ?? "投标"
      }
      extra={extra}
      find={false}
      steps={
        <div className="work-nav">
          <Wizard id={id} step={step} />
        </div>
      }
      tree={
        <BidSidebar
          id={id}
          step={step}
          view={view}
          part={part}
          doc={doc}
          units={units}
          parts={parts}
          requiredKeys={requiredKeys}
          clauses={clauses}
          docs={docs}
        />
      }
      inspector={inspector}
      className={step === "parts" ? "ed-page" : undefined}
    >
      {step === "files" && (
        <div className="wrap stack">
          <FilesPane
            docs={docs}
            ended={ended}
            uploading={uploading}
            pendingNames={pendingNames}
            focusId={doc}
            onUpload={(files) => {
              setUploading(true);
              setPendingNames(files.map((f) => f.name));
              void Promise.all(
                files.map((file) =>
                  uploadWithAttempt("document", file, (attempt) => api.uploadDoc(id, file, attempt)),
                ),
              )
                .then(() => {
                  toast("已上传，正在解析");
                  return load();
                })
                .catch((e) => toast(errMsg(e), "red"))
                .finally(() => setUploading(false));
            }}
            onRetry={(d) => {
              void api
                .retryDoc(id, d.id, d.conversion_generation)
                .then(() => load())
                .catch((e) => toast(errMsg(e), "red"));
            }}
          />
        </div>
      )}
      {step === "facts" && view === "facts" && (
        <div className="wrap">
          <FactsPane
            project={project}
            suggestions={suggestions}
            drafts={factDrafts}
            ended={ended}
            onAccept={(s) => {
              void api
                .mutateFact(id, {
                  action: "accept",
                  expected_fact_revision: project.fact_revision,
                  candidate_id: s.id,
                })
                .then(() => load())
                .catch((e) => toast(errMsg(e), "red"));
            }}
            onSet={(field, value) => void setFact(field, value)}
            onClear={(field) => void clearFact(field)}
            onChangeDraft={(field, value) => setFactDrafts((cur) => ({ ...cur, [field]: value }))}
            onChangeCeilingBasis={(basis) => void setFact("ceiling_basis", basis)}
          />
        </div>
      )}
      {step === "facts" && view !== "facts" && (
        <div className="wrap">
          <ClauseTable
            live={live}
            selected={selected}
            ended={ended}
            addText={addText}
            addKind={addKind}
            addMust={addMust}
            onSelect={setSelected}
            onConfirm={(c) => void mutateClause(c, "confirm")}
            onReject={(c) => void mutateClause(c, "reject")}
            onAddText={setAddText}
            onAddKind={setAddKind}
            onAddMust={setAddMust}
            onAdd={() => {
              void api
                .addClause(id, { text: addText.trim(), kind: addKind, must: addMust })
                .then(() => {
                  setAddText("");
                  toast("已添加草稿");
                  return load();
                })
                .catch((e) => toast(errMsg(e), "red"));
            }}
          />
        </div>
      )}
      {step === "matching" && (
        <div className="wrap">
          <MatchingPane
            view={view}
            clauses={clauses}
            units={units}
            matching={matching}
            pickSet={pickSet}
            ended={ended}
            onSchedule={() => {
              void api
                .rematch(id)
                .then(() => {
                  toast("已调度匹配");
                  return load();
                })
                .catch((e) => toast(errMsg(e), "red"));
            }}
          />
        </div>
      )}
      {step === "quote" && view === "quote" && (
        <div className="wrap">
          <QuotePane
            project={project}
            quote={quote}
            preview={preview}
            ended={ended}
            saving={quoteSaving}
            noCeiling={noCeiling}
            noCeilingReason={noCeilingReason}
            onNoCeiling={(reviewed, reason) => {
              setNoCeiling(reviewed);
              setNoCeilingReason(reason);
            }}
            onCreate={() => {
              runQuoteMutation(() =>
                api.createQuoteDraft(id, { tax_mode: "tax_exclusive", title: `${project.title} 报价` }),
              );
            }}
            onPatch={(title, taxMode, notes) => {
              if (!quote.edit_version && quote.edit_version !== 0) return;
              runQuoteMutation(() =>
                api.patchQuote(id, {
                  expected_edit_version: quote.edit_version ?? 0,
                  tax_mode: taxMode,
                  title,
                  notes,
                }),
              );
            }}
            onAddLine={() => {
              const lineId = crypto.randomUUID();
              runQuoteMutation(() =>
                api.upsertQuoteLine(id, lineId, {
                  expected_edit_version: quote.edit_version ?? 0,
                  ordinal: (quote.lines?.length ?? 0) + 1,
                  description: "新报价行",
                  pricing_mode: "lump_sum",
                  quantity: null,
                  unit: null,
                  unit_price: null,
                  entered_amount: null,
                  tax_rate: "0.130000",
                  user_confirmed: false,
                }),
              );
            }}
            onUpdateLine={(line: QuoteLine, patch) => {
              const next = { ...line, ...patch };
              runQuoteMutation(() =>
                api.upsertQuoteLine(id, line.id, {
                  expected_edit_version: quote.edit_version ?? 0,
                  ordinal: next.ordinal,
                  description: next.description,
                  pricing_mode: next.pricing_mode,
                  quantity: next.quantity,
                  unit: next.unit,
                  unit_price: next.unit_price,
                  entered_amount: next.entered_amount,
                  tax_rate: next.tax_rate,
                  user_confirmed: next.user_confirmed,
                }),
              );
            }}
            onDeleteLine={(line) => {
              runQuoteMutation(() => api.deleteQuoteLine(id, line.id, quote.edit_version ?? 0));
            }}
            onFinalize={() => {
              const pricingSet = clauseSets.find((s) => s.set_kind === "pricing");
              if (!pricingSet) {
                toast("缺少价格条款集，无法定稿报价", "red");
                return;
              }
              runQuoteMutation(
                () =>
                  api.finalizeQuote(id, {
                  expected_edit_version: quote.edit_version ?? 0,
                  expected_fact_revision: project.fact_revision,
                  expected_ceiling_revision: project.ceiling_revision,
                  expected_ceiling_identity_sha256: project.ceiling_identity_sha256,
                  expected_pricing_revision: pricingSet.revision,
                  expected_pricing_set_sha256: pricingSet.content_sha256,
                  no_ceiling_reviewed: !project.ceiling_price && noCeiling,
                  no_ceiling_reason: noCeilingReason,
                  }),
                "报价已定稿",
              );
            }}
            onReopen={() => {
              const snapshotId = quote.snapshot_id;
              if (!snapshotId) return;
              runQuoteMutation(() =>
                api.reopenQuote(id, {
                  expected_snapshot_id: snapshotId,
                  expected_fact_revision: project.fact_revision,
                  expected_pricing_revision: clauseSets.find((s) => s.set_kind === "pricing")?.revision ?? 0,
                }),
              );
            }}
          />
        </div>
      )}
      {step === "quote" && view !== "quote" && (
        <div className="wrap">
          <MaterialsPane
            view={view}
            company={company}
            submission={submission}
            classifications={classifications}
            attachments={attachments}
            ended={ended}
            onSaveCompany={(body) => {
              const save = body === company;
              if (!save) companyDirty.current = true;
              setCompany(body);
              if (save) {
                void api
                  .updateCompanyProfile(id, {
                    expected_revision: Number(company.revision ?? 0),
                    legal_name: company.legal_name || "",
                    unified_social_credit_code: company.unified_social_credit_code || "",
                    registered_address: company.registered_address || "",
                    legal_representative: company.legal_representative || "",
                    contact_name: company.contact_name || "",
                    contact_phone: company.contact_phone || "",
                    contact_email: company.contact_email || "",
                  })
                  .then(() => {
                    companyDirty.current = false;
                    return load();
                  })
                  .catch((e) => toast(errMsg(e), "red"));
              }
            }}
            onSaveSubmission={(body) => {
              const save = body === submission;
              if (!save) submissionDirty.current = true;
              setSubmission(body);
              if (save) {
                if (!submission.submission_date?.trim()) {
                  toast("请填写投标日期", "red");
                  return;
                }
                void api
                  .updateSubmissionProfile(id, {
                    expected_revision: Number(submission.revision ?? 0),
                    buyer_name: submission.buyer_name || "",
                    project_code: submission.project_code || "",
                    authorized_representative: submission.authorized_representative || "",
                    submission_date: submission.submission_date,
                    submission_place: submission.submission_place || "",
                    seal_confirmed: !!submission.seal_confirmed,
                    signature_confirmed: !!submission.signature_confirmed,
                  })
                  .then(() => {
                    submissionDirty.current = false;
                    return load();
                  })
                  .catch((e) => toast(errMsg(e), "red"));
              }
            }}
            onOverride={(cid, kind, reason) => {
              void api.overrideClassification(id, cid, { effective_kind: kind, reason }).then(() => load()).catch((e) => toast(errMsg(e), "red"));
            }}
            onResolve={(cid, resolution, attachmentId, reason) => {
              void api
                .resolveRequirement(id, cid, { resolution, attachment_id: attachmentId, reason })
                .then(() => load())
                .catch((e) => toast(errMsg(e), "red"));
            }}
            onUpload={(kind, file) => {
              void uploadWithAttempt(`attachment:${kind}`, file, (attempt) =>
                api.uploadAttachment(id, kind, file, attempt),
              )
                .then(() => load())
                .catch((e) => toast(errMsg(e), "red"));
            }}
            onAttachAction={(aid, action, revision) => {
              void api.mutateAttachment(id, aid, action, revision).then(() => load()).catch((e) => toast(errMsg(e), "red"));
            }}
          />
        </div>
      )}
      {step === "parts" && (
        <PartsPane
          partKey={part}
          markdown={partMarkdown}
          stale={partStale}
          ended={ended}
          ready={partReady}
          preview={partPreview}
          units={units}
          gate={gate}
          onChange={(text) => {
            dirtyPart.current = true;
            setPartMarkdown(text);
          }}
          onSave={() => {
            if (!partReady) return;
            void api
              .updatePart(id, part, partRevision, partMarkdown)
              .then(() => {
                dirtyPart.current = false;
                toast("已保存");
                return load();
              })
              .catch((e) => toast(errMsg(e), "red"));
          }}
          onRegen={() => {
            if (!partReady) return;
            void api
              .regeneratePart(id, part, {
                expected_content_revision: partRevision,
                expected_dependency_sha256: partDependencySha,
              })
              .then(() => {
                dirtyPart.current = false;
                return load();
              })
              .catch((e) => toast(errMsg(e), "red"));
          }}
          onPreview={setPartPreview}
        />
      )}
    </Shell>
  );
}
