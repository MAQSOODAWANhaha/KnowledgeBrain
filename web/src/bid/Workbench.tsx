import { useEffect, useState } from "react";
import { Button, Select } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { ApiError, type QuoteLine, type QuoteState, api } from "../api";
import { Crumbs } from "../Crumbs";
import { parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { FilesPane } from "./FilesPane";
import { QuotePane } from "./QuotePane";
import { ExportPane } from "./authoring/ExportPane";
import { InspectorPanel } from "./authoring/InspectorPanel";
import { OutlineTree } from "./authoring/OutlineTree";
import { PreviewPane } from "./authoring/PreviewPane";
import { RequirementsPane } from "./authoring/RequirementsPane";
import { SectionEditor } from "./authoring/SectionEditor";
import {
  AUTHORING_STEPS,
  authoringHref,
  type AuthoringStep,
} from "./authoring/routes";
import { useBidV2Session } from "./authoring/useBidV2Session";

function toast(msg: string, color: "blue" | "red" = "blue") {
  notifications.show({ message: msg, color });
}

function errMsg(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function Wizard({
  projectId,
  step,
}: {
  projectId: string;
  step: AuthoringStep;
}) {
  const cur = AUTHORING_STEPS.findIndex((item) => item.key === step);
  return (
    <nav className="wizard">
      {AUTHORING_STEPS.map((item, index) => (
        <a
          key={item.key}
          data-testid={`wizard-${item.key}`}
          className={
            item.key === step ? "on" : index < cur ? "done" : undefined
          }
          href={`#${authoringHref(projectId, item.key)}`}
        >
          <i>{item.n}</i>
          <span>{item.label}</span>
        </a>
      ))}
    </nav>
  );
}

export function Workbench({ email }: { email: string }) {
  const path = useHash();
  const route = parseBidRoute(path);
  const { session, state } = useBidV2Session(route);
  const projectId = route?.projectId ?? "";
  const step: AuthoringStep = route?.step ?? "files";
  const ended = state.ended;
  const [quote, setQuote] = useState<QuoteState>({ exists: false });
  const [quotePreview, setQuotePreview] = useState<{
    net_total?: string;
    tax_total?: string;
    gross_total?: string;
  }>();
  const [quoteSaving, setQuoteSaving] = useState(false);
  const [noCeiling, setNoCeiling] = useState(false);
  const [noCeilingReason, setNoCeilingReason] = useState(
    "招标文件未设置最高限价，已人工复核",
  );

  useEffect(() => {
    if (state.error) toast(state.error.message, "red");
  }, [state.error]);

  useEffect(() => {
    if (step !== "quote" || !projectId) return;
    let cancelled = false;
    void Promise.all([
      api.quote(projectId).catch(() => ({ exists: false }) as QuoteState),
      api.previewQuote(projectId).catch(() => undefined),
    ]).then(([nextQuote, preview]) => {
      if (cancelled) return;
      setQuote(nextQuote);
      setQuotePreview(preview);
    });
    return () => {
      cancelled = true;
    };
  }, [step, projectId]);

  function runQuote(request: () => Promise<unknown>, success?: string) {
    if (quoteSaving) return;
    setQuoteSaving(true);
    void request()
      .then(async () => {
        if (success) toast(success);
        setQuote(await api.quote(projectId));
        setQuotePreview(
          await api.previewQuote(projectId).catch(() => undefined),
        );
      })
      .catch((error) => toast(errMsg(error), "red"))
      .finally(() => setQuoteSaving(false));
  }

  const title =
    step === "files"
      ? "招标文件"
      : step === "requirements"
        ? "要求台账"
        : step === "quote"
          ? "报价"
          : step === "authoring"
            ? (session.findNode(state.selectedNodeLineageId ?? "")?.title ??
              "编制")
            : step === "preview"
              ? "全文预览"
              : "导出";

  return (
    <Shell
      root="bids"
      email={email}
      crumbs={
        <Crumbs
          items={[
            { label: "投标项目", href: "/" },
            {
              label: state.project?.title ?? "本标",
              href: authoringHref(projectId, "files"),
            },
            { label: title },
          ]}
        />
      }
      title={title}
      steps={
        projectId ? <Wizard projectId={projectId} step={step} /> : undefined
      }
      extra={
        step === "authoring" ? (
          <div className="row">
            <Select
              size="xs"
              w={160}
              value={state.fillPolicy}
              allowDeselect={false}
              data={[
                { value: "empty_only", label: "只填空章" },
                { value: "append_candidate", label: "追加候选" },
                { value: "missing_requirements_only", label: "只补缺项" },
              ]}
              onChange={(value) =>
                value && session.setFillPolicy(value as typeof state.fillPolicy)
              }
            />
            <Button
              size="compact-sm"
              variant="default"
              disabled={ended}
              onClick={() => void session.generateOutline()}
            >
              生成大纲
            </Button>
            <Button
              size="compact-sm"
              variant="default"
              disabled={ended || !state.selectedNodeLineageId}
              onClick={() =>
                void session.generateContent(
                  "node",
                  state.selectedNodeLineageId ?? undefined,
                )
              }
            >
              生成本章
            </Button>
            <Button
              size="compact-sm"
              variant="default"
              disabled={ended || !state.selectedNodeLineageId}
              onClick={() =>
                void session.generateContent(
                  "subtree",
                  state.selectedNodeLineageId ?? undefined,
                )
              }
            >
              生成子树
            </Button>
            <Button
              size="compact-sm"
              variant="default"
              disabled={ended}
              onClick={() => void session.generateContent("workspace")}
            >
              生成全部空章节
            </Button>
            <Button
              size="compact-sm"
              disabled={ended}
              onClick={() => void session.save()}
            >
              保存
            </Button>
          </div>
        ) : undefined
      }
      tree={
        step === "authoring" ? (
          <OutlineTree session={session} state={state} />
        ) : (
          <>
            <div className="side-sec">本标</div>
            <nav className="sidenav">
              {step === "files" &&
                state.documents.map((doc) => (
                  <a
                    key={doc.id}
                    href={`#${authoringHref(projectId, "files")}`}
                  >
                    <em>{doc.file_name}</em>
                  </a>
                ))}
              {step !== "files" && (
                <a className="on" href={`#${authoringHref(projectId, step)}`}>
                  <em>{title}</em>
                </a>
              )}
            </nav>
          </>
        )
      }
      inspector={
        step === "authoring" ? (
          <InspectorPanel session={session} state={state} />
        ) : undefined
      }
    >
      {step === "files" && (
        <div className="wrap">
          <FilesPane
            docs={state.documents}
            ended={ended}
            uploading={state.busy && state.pendingUploads.length > 0}
            pendingNames={state.pendingUploads}
            onUpload={(files) => void session.uploadTenderDocuments(files)}
            onRetry={(doc) =>
              void session.retryTenderDocument(
                doc.id,
                doc.conversion_generation,
              )
            }
          />
        </div>
      )}
      {step === "requirements" && (
        <div className="wrap">
          <RequirementsPane session={session} state={state} />
        </div>
      )}
      {step === "quote" && (
        <div className="wrap">
          <QuotePane
            project={{ ceiling_price: null }}
            quote={quote}
            preview={quotePreview}
            ended={ended}
            saving={quoteSaving}
            noCeiling={noCeiling}
            noCeilingReason={noCeilingReason}
            onNoCeiling={(reviewed, reason) => {
              setNoCeiling(reviewed);
              setNoCeilingReason(reason);
            }}
            onCreate={() =>
              runQuote(() =>
                api.createQuoteDraft(projectId, {
                  tax_mode: "tax_exclusive",
                  title: `${state.project?.title ?? ""} 报价`,
                }),
              )
            }
            onPatch={(titleText, taxMode, notes) => {
              if (quote.edit_version == null) return;
              runQuote(() =>
                api.patchQuote(projectId, {
                  expected_edit_version: quote.edit_version ?? 0,
                  tax_mode: taxMode,
                  title: titleText,
                  notes,
                }),
              );
            }}
            onAddLine={() =>
              runQuote(() =>
                api.upsertQuoteLine(projectId, crypto.randomUUID(), {
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
              )
            }
            onUpdateLine={(line: QuoteLine, patch) => {
              const next = { ...line, ...patch };
              runQuote(() =>
                api.upsertQuoteLine(projectId, line.id, {
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
            onDeleteLine={(line) =>
              runQuote(() =>
                api.deleteQuoteLine(
                  projectId,
                  line.id,
                  quote.edit_version ?? 0,
                ),
              )
            }
            onFinalize={() =>
              runQuote(
                () =>
                  api.finalizeQuote(projectId, {
                    expected_edit_version: quote.edit_version ?? 0,
                    expected_fact_revision: 0,
                    expected_ceiling_revision: 0,
                    expected_ceiling_identity_sha256: "0".repeat(64),
                    expected_pricing_revision: 0,
                    expected_pricing_set_sha256: "0".repeat(64),
                    no_ceiling_reviewed: noCeiling,
                    no_ceiling_reason: noCeilingReason,
                  }),
                "报价已定稿",
              )
            }
            onReopen={() => {
              if (!quote.snapshot_id) return;
              runQuote(() =>
                api.reopenQuote(projectId, {
                  expected_snapshot_id: quote.snapshot_id as string,
                  expected_fact_revision: 0,
                  expected_pricing_revision: 0,
                }),
              );
            }}
          />
        </div>
      )}
      {step === "authoring" && (
        <SectionEditor session={session} state={state} />
      )}
      {step === "preview" && <PreviewPane state={state} />}
      {step === "export" && (
        <div className="wrap">
          <ExportPane session={session} state={state} />
        </div>
      )}
    </Shell>
  );
}
