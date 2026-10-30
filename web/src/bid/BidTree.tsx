import {
  IconDownload,
  IconFileText,
  IconFolder,
  IconPencil,
  IconListDetails,
} from "@tabler/icons-react";
import { cn } from "../lib/utils";
import type { BidProjectView } from "./api";
import { AUTHORING_STEPS, authoringHref, type AuthoringStep } from "./authoring/routes";

const STEP_ICON = {
  files: IconFileText,
  analysis: IconListDetails,
  authoring: IconPencil,
  export: IconDownload,
} as const;

export function ProjectNav({ projectId, currentStep, title }: { projectId: string; currentStep: AuthoringStep; title: string }) {
  return <nav aria-label="项目分区" className="flex min-h-0 flex-col gap-0.5">
    <a className="mb-1 px-2.5 py-2 text-[12px] text-quiet hover:text-ink" href="#/">返回项目列表</a>
    <div className="mb-2 truncate px-2.5 text-[13px] font-semibold text-ink" title={title}>{title}</div>
    {AUTHORING_STEPS.map((step) => {
      const Icon = STEP_ICON[step.key];
      const on = currentStep === step.key;
      return <a key={step.key}
        href={`#${authoringHref(projectId, step.key)}`}
        aria-current={on ? "page" : undefined}
        data-testid={`wizard-${step.key}`}
        className={cn("flex min-h-9 items-center gap-2 border-l-2 px-2.5 text-[14px]",
          on ? "border-sky font-semibold text-ink" : "border-transparent font-medium text-quiet hover:text-ink")}>
        <Icon size={18} stroke={1.7} aria-hidden />
        {step.label}
      </a>;
    })}
  </nav>;
}

export function BidTree({
  rows,
  currentId,
  currentStep,
}: {
  rows: BidProjectView[] | null;
  currentId?: string;
  currentStep?: AuthoringStep;
}) {
  const list = rows ?? [];
  const current = list.find((p) => p.id === currentId) ?? null;
  const rest = list.filter((p) => p.id !== currentId);
  return (
    <nav className="flex min-h-0 flex-col" aria-label="项目">
      {current && (
        <div className="mb-3 border-b border-line px-1 pb-3">
          <a
            className="flex items-baseline gap-2 px-2.5 py-2 text-[13px] font-semibold text-ink"
            href={`#${authoringHref(current.id, "files")}`}
          >
            {current.title}
            {current.status === "ended" && (
              <em className="text-[12px] font-medium not-italic text-quiet">已结束</em>
            )}
          </a>
          {currentStep && (
            <div className="flex flex-col gap-0.5">
              {AUTHORING_STEPS.map((step) => {
                const Icon = STEP_ICON[step.key];
                const on = currentStep === step.key;
                return (
                  <a
                    key={step.key}
                    href={`#${authoringHref(current.id, step.key)}`}
                    className={cn(
                      "flex min-h-8 items-center gap-2 rounded-[6px] px-2.5 text-[13px] font-medium",
                      on
                        ? "bg-sky-wash font-semibold text-sky-ink"
                        : "text-quiet hover:bg-canvas hover:text-ink",
                    )}
                    data-testid={`wizard-${step.key}`}
                  >
                    <Icon size={18} stroke={1.7} aria-hidden />
                    {step.label}
                  </a>
                );
              })}
            </div>
          )}
        </div>
      )}
      <div className="px-3 py-2 text-[11px] font-semibold tracking-wide text-quiet">
        {current ? "其他项目" : "项目"}
      </div>
      {(current ? rest : list).map((p) => (
        <a
          key={p.id}
          className="mx-1 flex min-h-8 items-center gap-2 rounded-[6px] px-2.5 text-[13px] text-ink hover:bg-canvas"
          href={`#${authoringHref(p.id, "files")}`}
        >
          <IconFolder size={16} stroke={1.6} aria-hidden />
          <span className="min-w-0 flex-1 truncate">{p.title}</span>
          {p.status === "ended" && <em className="text-[12px] not-italic text-quiet">已结束</em>}
        </a>
      ))}
    </nav>
  );
}
