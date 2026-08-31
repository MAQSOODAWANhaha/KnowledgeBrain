import { useEffect, useRef } from "react";
import { Button, Select, Stepper } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { Crumbs } from "../Crumbs";
import { go, parseBidRoute, useHash } from "../hash";
import { Shell } from "../Shell";
import { FilesPane } from "./FilesPane";
import { AuthoringShell } from "./authoring/AuthoringShell";
import { ExportPane } from "./authoring/ExportPane";
import { InspectorPanel } from "./authoring/InspectorPanel";
import { OutlineTree } from "./authoring/OutlineTree";
import {
  AUTHORING_STEPS,
  authoringHref,
  type AuthoringStep,
} from "./authoring/routes";
import { useBidV2Session } from "./authoring/useBidV2Session";

function toast(msg: string, color: "blue" | "red" = "blue") {
  notifications.show({
    title: color === "red" ? "失败" : "提示",
    message: msg,
    color,
    autoClose: 5000,
    withCloseButton: true,
  });
}

function outlineErrorMessage(code?: string | null): string {
  switch (code) {
    case "AGENT_MAP_FAILED":
      return "招标结构分析失败，请再试一次";
    case "AGENT_DEADLINE_EXCEEDED":
      return "生成超时，请再试一次";
    case "AGENT_TURN_TIMEOUT":
    case "AGENT_PROVIDER_ERROR":
    case "AGENT_PROVIDER_UNAVAILABLE":
      return "模型服务异常或超时，请再试一次";
    case "STRUCTURE_EVIDENCE_INSUFFICIENT":
      return "招标文件缺少可验证的目录或章节结构证据";
    case "AGENT_GROUPING_FAILED":
      return "招标要求分组失败，请再试一次";
    case "AGENT_SEMANTIC_VALIDATION_FAILED":
    case "AGENT_REQUIREMENT_CLOSURE_FAILED":
    case "AGENT_OBLIGATION_COVERAGE_FAILED":
    case "AGENT_OUTPUT_INVALID":
      return "大纲结构校验失败，请再试一次";
    default:
      return "大纲生成失败";
  }
}

function Wizard({
  projectId,
  step,
}: {
  projectId: string;
  step: AuthoringStep;
}) {
  const cur = Math.max(
    0,
    AUTHORING_STEPS.findIndex((item) => item.key === step),
  );
  return (
    <div className="bid-stepper-wrap">
      <Stepper
        active={cur}
        size="lg"
        allowNextStepsSelect
        className="bid-stepper"
        styles={{
          content: { display: "none" },
          stepDescription: { display: "none" },
        }}
        onStepClick={(index) => {
          const next = AUTHORING_STEPS[index];
          if (next) go(authoringHref(projectId, next.key));
        }}
      >
        {AUTHORING_STEPS.map((item) => (
          <Stepper.Step
            key={item.key}
            label={item.label}
            data-testid={`wizard-${item.key}`}
          />
        ))}
      </Stepper>
    </div>
  );
}

export function Workbench({ email }: { email: string }) {
  const path = useHash();
  const route = parseBidRoute(path);
  const { session, state } = useBidV2Session(route);
  const projectId = route?.projectId ?? "";
  const step: AuthoringStep = route?.step ?? "files";
  const ended = state.ended;
  const outlineToastKey = useRef("");

  useEffect(() => {
    if (state.error) toast(state.error.message, "red");
  }, [state.error]);

  useEffect(() => {
    const outline = state.asyncRequests.find(
      (request) => request.kind === "OutlineGenerate",
    );
    if (!outline || state.preparingOutline) return;
    if (outline.status !== "failed") return;
    const key = `${outline.request_artifact_id}:${outline.status}:${outline.error_code ?? ""}`;
    if (outlineToastKey.current === key) return;
    outlineToastKey.current = key;
    toast(outlineErrorMessage(outline.error_code), "red");
  }, [state.asyncRequests, state.preparingOutline]);

  useEffect(() => {
    if (
      state.candidate?.kind !== "outline" ||
      state.candidate.status !== "proposed"
    )
      return;
    const key = `ok:${state.candidate.candidate_id}`;
    if (outlineToastKey.current === key) return;
    outlineToastKey.current = key;
    toast("大纲候选已生成，请核对后接受");
  }, [state.candidate]);

  const title =
    step === "files"
      ? "招标文件"
      : step === "authoring"
        ? (session.findNode(state.selectedNodeLineageId ?? "")?.title ?? "编制")
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
      steps={
        projectId ? <Wizard projectId={projectId} step={step} /> : undefined
      }
      extra={
        step === "authoring" ? (
          <div className="row">
            <Select
              w={140}
              allowDeselect={false}
              value={state.fillPolicy}
              onChange={(value) =>
                value &&
                session.setFillPolicy(value as typeof state.fillPolicy)
              }
              data={[
                { value: "empty_only", label: "只填空章" },
                { value: "append_candidate", label: "追加候选" },
                {
                  value: "missing_requirements_only",
                  label: "只补缺项",
                },
              ]}
            />
            <Button
              variant="default"
              data-testid="generate-outline"
              disabled={ended}
              onClick={() => {
                const pending = state.asyncRequests.some(
                  (request) =>
                    request.kind === "OutlineGenerate" &&
                    request.status === "pending",
                );
                if (!pending) toast("正在生成大纲…");
                void session.generateOutline();
              }}
            >
              生成大纲
            </Button>
            <Button
              variant="default"
              data-testid="generate-node"
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
              variant="default"
              data-testid="generate-workspace"
              disabled={ended}
              onClick={() => void session.generateContent("workspace")}
            >
              填充全部空章
            </Button>
            <Button disabled={ended} onClick={() => void session.save()}>
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
      {step === "authoring" && (
        <div className="wrap">
          <AuthoringShell session={session} state={state} />
        </div>
      )}
      {step === "export" && (
        <div className="wrap">
          <ExportPane session={session} state={state} />
        </div>
      )}
    </Shell>
  );
}
