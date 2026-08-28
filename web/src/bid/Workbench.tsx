import { useEffect } from "react";
import { notifications } from "@mantine/notifications";
import { Crumbs } from "../Crumbs";
import { parseBidRoute, useHash } from "../hash";
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
  notifications.show({ message: msg, color });
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

  useEffect(() => {
    if (state.error) toast(state.error.message, "red");
  }, [state.error]);

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
      title={title}
      steps={
        projectId ? <Wizard projectId={projectId} step={step} /> : undefined
      }
      extra={
        step === "authoring" ? (
          <div className="row">
            <select
              className="in"
              value={state.fillPolicy}
              onChange={(event) =>
                session.setFillPolicy(
                  event.currentTarget.value as typeof state.fillPolicy,
                )
              }
            >
              <option value="empty_only">只填空章</option>
              <option value="append_candidate">追加候选</option>
              <option value="missing_requirements_only">只补缺项</option>
            </select>
            <button
              type="button"
              className="btn ghost"
              data-testid="generate-outline"
              disabled={ended}
              onClick={() => void session.generateOutline()}
            >
              生成大纲
            </button>
            <button
              type="button"
              className="btn ghost"
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
            </button>
            <button
              type="button"
              className="btn ghost"
              disabled={ended || !state.selectedNodeLineageId}
              onClick={() =>
                void session.generateContent(
                  "subtree",
                  state.selectedNodeLineageId ?? undefined,
                )
              }
            >
              生成子树
            </button>
            <button
              type="button"
              className="btn ghost"
              data-testid="generate-workspace"
              disabled={ended}
              onClick={() => void session.generateContent("workspace")}
            >
              填充全部空章
            </button>
            <button
              type="button"
              className="btn"
              disabled={ended}
              onClick={() => void session.save()}
            >
              保存
            </button>
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
        <AuthoringShell session={session} state={state} />
      )}
      {step === "export" && (
        <div className="wrap">
          <ExportPane session={session} state={state} />
        </div>
      )}
    </Shell>
  );
}
