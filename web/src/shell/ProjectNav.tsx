import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { FileText, FolderOpen, ListChecks, PackageCheck, PenLine } from "lucide-react";
import { useEffect, useState } from "react";
import type { BidProjectView } from "../bid/api";
import { tenderApi } from "../bid/api/workbench";
import { AUTHORING_STEPS, authoringHref, type AuthoringStep } from "../bid/authoring/routes";

const STEP_ICON = { files: FolderOpen, analysis: ListChecks, authoring: PenLine, export: PackageCheck } as const;

function useProjects() {
  const [rows, setRows] = useState<BidProjectView[] | null>(null);
  useEffect(() => {
    let canceled = false;
    tenderApi.listProjects().then((value) => { if (!canceled) setRows(value); }).catch(() => {});
    return () => { canceled = true; };
  }, []);
  return rows;
}

// Project switcher in SideNav topContent: jump to another project's same step.
export function ProjectSwitcher({ projectId, title, step }: { projectId: string; title: string; step: AuthoringStep }) {
  const rows = useProjects();
  const others = (rows ?? []).filter((row) => row.id !== projectId && row.status !== "ended");
  return (
    <DropdownMenu
      button={{ label: title, variant: "secondary", "aria-label": `当前项目：${title}，切换项目` }}
      menuWidth={280}
      items={[
        ...others.map((row) => ({ label: row.title, href: `#${authoringHref(row.id, step)}` })),
        { type: "divider" as const },
        { label: "查看全部项目", href: "#/" },
      ]}
    />
  );
}

export function ProjectStages({ projectId, step }: { projectId: string; step: AuthoringStep }) {
  return (
    <nav aria-label="项目分区">
      <SideNavSection title="投标流程">
        {AUTHORING_STEPS.map((item) => (
          <SideNavItem
            key={item.key}
            label={item.label}
            icon={STEP_ICON[item.key]}
            href={`#${authoringHref(projectId, item.key)}`}
            isSelected={step === item.key}
            data-testid={`wizard-${item.key}`}
          />
        ))}
      </SideNavSection>
    </nav>
  );
}

// Projects module: open projects as direct entries.
export function OpenProjects({ rows }: { rows: BidProjectView[] | null }) {
  const open = (rows ?? []).filter((row) => row.status !== "ended");
  if (!open.length) return null;
  return (
    <SideNavSection title="在办项目">
      {open.map((row) => (
        <SideNavItem key={row.id} label={row.title} icon={FileText} href={`#${authoringHref(row.id, "files")}`} />
      ))}
    </SideNavSection>
  );
}
