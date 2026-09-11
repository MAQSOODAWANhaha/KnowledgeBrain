import {
  IconDownload,
  IconFileText,
  IconFolder,
  IconPencil,
} from "@tabler/icons-react";
import { cn } from "../lib/utils";
import type { BidProjectView } from "./api";
import { AUTHORING_STEPS, authoringHref, type AuthoringStep } from "./authoring/routes";

const STEP_ICON = {
  files: IconFileText,
  authoring: IconPencil,
  export: IconDownload,
} as const;

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
            className="flex items-baseline gap-2 px-2.5 py-2 text-[15px] font-semibold text-ink"
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
                      "flex min-h-11 items-center gap-2.5 rounded-[10px] px-3 text-[15px] font-medium",
                      on
                        ? "bg-sky-wash font-semibold text-sky-ink"
                        : "text-quiet hover:bg-[#f6f6f8] hover:text-ink",
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
          className="mx-1 flex min-h-10 items-center gap-2 rounded-[10px] px-3 text-[14.5px] text-ink hover:bg-[#f6f6f8]"
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
