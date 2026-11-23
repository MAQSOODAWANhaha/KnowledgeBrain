import { Badge } from "@astryxdesign/core/Badge";
import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { IconButton } from "@astryxdesign/core/IconButton";
import { SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { TextInput } from "@astryxdesign/core/TextInput";
import { VStack } from "@astryxdesign/core/VStack";
import { ChevronsUpDown, Plus } from "lucide-react";
import { type ComponentType, type SVGProps, useEffect, useState } from "react";
import type { BidProjectView } from "../bid/api";
import { tenderApi, type WorkbenchView } from "../bid/api/workbench";
import { AUTHORING_STEPS, authoringHref, type AuthoringStep } from "../bid/authoring/routes";
import { inProgress } from "../bid/executionStatus";
import type { ContextPanel } from "../Shell";

type Tone = "neutral" | "info" | "success" | "warning" | "error";
const TONE_COLOR: Record<Tone, string> = { neutral: "#B3B3B3", info: "#0F6CBD", success: "#107C10", warning: "#BC4B09", error: "#C50F1F" };

// SideNavItem icons are SVG components: a numbered flow node and a status dot.
type Svg = ComponentType<SVGProps<SVGSVGElement>>;
const nodes = new Map<string, Svg>();
function stepNode(index: number, done: boolean): Svg {
  const key = `${index}:${done}`;
  if (!nodes.has(key)) {
    const Node: Svg = (props) => (
      <svg viewBox="0 0 22 22" width={22} height={22} aria-hidden {...props}>
        <circle cx="11" cy="11" r="10" fill={done ? TONE_COLOR.success : "#FFFFFF"} stroke={done ? TONE_COLOR.success : "currentColor"} strokeWidth="1.5" />
        {done
          ? <path d="M6.5 11.2l3 3 6-6.4" fill="none" stroke="#FFFFFF" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
          : <text x="11" y="15.5" textAnchor="middle" fontSize="13" fontWeight="700" fill="currentColor">{index + 1}</text>}
      </svg>
    );
    nodes.set(key, Node);
  }
  return nodes.get(key)!;
}
const dots = new Map<Tone, Svg>();
function dot(tone: Tone): Svg {
  if (!dots.has(tone)) {
    const Dot: Svg = (props) => <svg viewBox="0 0 16 16" width={16} height={16} aria-hidden {...props}><circle cx="8" cy="8" r="4" fill={TONE_COLOR[tone]} /></svg>;
    dots.set(tone, Dot);
  }
  return dots.get(tone)!;
}

const DAY = 24 * 60 * 60 * 1000;
const shortDate = (value?: string | null) => (value ? value.slice(5, 16).replace("T", " ") : null);
function dueText(project: BidProjectView, now = Date.now()) {
  if (project.status === "ended") return { text: "已结束", tone: "neutral" as Tone };
  if (!project.ends_at) return { text: "未设截止时间", tone: "neutral" as Tone };
  const days = Math.ceil((Date.parse(project.ends_at) - now) / DAY);
  if (days < 0) return { text: "已过截止时间", tone: "error" as Tone };
  return { text: days === 0 ? "今天截止" : `${days} 天后截止`, tone: (days <= 7 ? "warning" : "neutral") as Tone };
}

function useProjects() {
  const [rows, setRows] = useState<BidProjectView[] | null>(null);
  useEffect(() => {
    let canceled = false;
    tenderApi.listProjects().then((value) => { if (!canceled) setRows(value); }).catch(() => {});
    return () => { canceled = true; };
  }, []);
  return rows;
}

// Project card: who you are working on, its deadline, and the switcher to another project's same step.
function ProjectCard({ project, step }: { project: BidProjectView; step: AuthoringStep }) {
  const rows = useProjects();
  const others = (rows ?? []).filter((row) => row.id !== project.id && row.status !== "ended");
  const due = dueText(project);
  return (
    <DropdownMenu
      menuWidth={300}
      items={[
        ...others.map((row) => ({ label: row.title, description: shortDate(row.ends_at) ? `截止 ${shortDate(row.ends_at)}` : undefined, href: `#${authoringHref(row.id, step)}` })),
        ...(others.length ? [{ type: "divider" as const }] : []),
        { label: "查看全部项目", href: "#/" },
      ]}
      renderTrigger={(trigger) => (
        <button type="button" className="kb-pcard" aria-label={`当前项目：${project.title}，切换项目`} {...trigger}>
          <span className="kb-pmark" aria-hidden>{project.title.slice(0, 1)}</span>
          <span className="kb-pname">{project.title}<small>{shortDate(project.ends_at) ? `截止 ${shortDate(project.ends_at)}` : "未设截止时间"}</small></span>
          <ChevronsUpDown size={14} aria-hidden />
          <span className="kb-pfoot">
            <Badge variant={project.status === "ended" ? "neutral" : "info"} label={project.status === "ended" ? "已结束" : "在办"} />
            {project.ends_at && <span data-tone={due.tone}>{due.text}</span>}
          </span>
        </button>
      )}
    />
  );
}

type Stage = { pill?: string; tone?: Tone; done: boolean };
function stages(view: WorkbenchView | null): Record<AuthoringStep, Stage> {
  const analysis = view?.analysis;
  const latest = view?.versions.reduce((max, item) => Math.max(max, item.revision), 0) ?? 0;
  const gaps = view?.gaps.length ?? 0;
  return {
    files: { pill: view ? `${view.documents.length} 份` : undefined, done: !!view?.documents.length },
    analysis: !analysis ? { pill: "未分析", tone: "neutral", done: false }
      : inProgress(analysis.status) ? { pill: "进行中", tone: "info", done: false }
      : analysis.confirmed ? { pill: `${analysis.items?.length ?? 0} 条`, done: true }
      : { pill: "待确认", tone: "warning", done: false },
    authoring: inProgress(view?.authoring?.status) ? { pill: "进行中", tone: "info", done: false }
      : !view?.current_docx ? { pill: "未生成", tone: "neutral", done: false }
      : gaps ? { pill: `待补 ${gaps}`, tone: "warning", done: false }
      : { pill: `v${view.current_docx.revision}`, done: true },
    export: latest ? { pill: `v${latest}`, done: true } : { pill: "暂无", tone: "neutral", done: false },
  };
}

type Todo = { key: string; label: string; step: AuthoringStep; tone: Tone };
function todos(view: WorkbenchView | null): Todo[] {
  if (!view) return [];
  const list: Todo[] = [];
  const analysis = view.analysis;
  if (analysis?.sources_current === false) list.push({ key: "sources", label: "招标资料已更新，需重新分析", step: "analysis", tone: "warning" });
  if (analysis && !analysis.confirmed && !inProgress(analysis.status) && analysis.status !== "failed") list.push({ key: "confirm", label: "确认招标解读结果", step: "analysis", tone: "warning" });
  if (analysis?.status === "failed") list.push({ key: "analysis-failed", label: "招标分析失败，需处理", step: "analysis", tone: "error" });
  if (view.authoring?.status === "failed" || view.authoring?.status === "blocked") list.push({ key: "authoring-failed", label: "编制任务未完成，需处理", step: "authoring", tone: "error" });
  view.gaps.slice(0, 5).forEach((gap, index) => list.push({ key: `gap-${index}`, label: gap.detail, step: "authoring", tone: "warning" }));
  return list;
}

// Inside a project: card, the four-step flow, and what needs attention.
export function projectPanel({ projectId, step, view }: { projectId: string; step: AuthoringStep; view: WorkbenchView | null }): ContextPanel {
  const state = stages(view);
  const pending = todos(view);
  const more = (view?.gaps.length ?? 0) - Math.min(view?.gaps.length ?? 0, 5);
  return {
    title: "投标项目",
    label: "项目导航",
    top: view ? <ProjectCard project={view.project} step={step} /> : undefined,
    children: <>
      <nav aria-label="项目分区">
        <SideNavSection title="投标流程">
          {AUTHORING_STEPS.map((item, index) => {
            const stage = state[item.key];
            return (
              <SideNavItem
                key={item.key}
                label={item.label}
                icon={stepNode(index, stage.done && step !== item.key)}
                href={`#${authoringHref(projectId, item.key)}`}
                isSelected={step === item.key}
                endContent={stage.pill ? <span className="kb-pill" data-tone={stage.tone ?? "neutral"}>{stage.pill}</span> : undefined}
                data-testid={`wizard-${item.key}`}
              />
            );
          })}
        </SideNavSection>
      </nav>
      {pending.length > 0 && (
        <SideNavSection title={`需要处理 ${pending.length + more}`}>
          {pending.map((todo) => (
            <SideNavItem key={todo.key} label={todo.label} icon={dot(todo.tone)} href={`#${authoringHref(projectId, todo.step)}`} />
          ))}
          {more > 0 && <SideNavItem label={`另有 ${more} 条待补记录`} icon={dot("neutral")} href={`#${authoringHref(projectId, "authoring")}`} />}
        </SideNavSection>
      )}
    </>,
  };
}

export type ListView = "all" | "open" | "due" | "ended";
export const LIST_VIEWS: { key: ListView; label: string }[] = [
  { key: "all", label: "全部项目" }, { key: "open", label: "在办" }, { key: "due", label: "本周截止" }, { key: "ended", label: "已结束" },
];
export function inView(project: BidProjectView, view: ListView, now = Date.now()) {
  if (view === "open") return project.status !== "ended";
  if (view === "ended") return project.status === "ended";
  if (view === "due") {
    if (project.status === "ended" || !project.ends_at) return false;
    const left = Date.parse(project.ends_at) - now;
    return left >= 0 && left <= 7 * DAY;
  }
  return true;
}

// Project list: saved views with counts; open projects with their deadlines, filtered by the panel search.
export function listPanel({ rows, view, query, onQuery, onCreate }: {
  rows: BidProjectView[] | null; view: ListView; query: string; onQuery(value: string): void; onCreate(): void;
}): ContextPanel {
  const needle = query.trim().toLocaleLowerCase();
  const open = (rows ?? []).filter((row) => row.status !== "ended" && (!needle || row.title.toLocaleLowerCase().includes(needle)));
  return {
    title: "投标项目",
    label: "项目视图",
    actions: <IconButton size="sm" variant="ghost" label="新建项目" icon={<Plus size={16} />} onClick={onCreate} />,
    top: <VStack paddingInline={1}><TextInput label="筛选项目" isLabelHidden type="search" size="sm" placeholder="筛选项目" value={query} onChange={onQuery} /></VStack>,
    children: <>
      <SideNavSection title="视图">
        {LIST_VIEWS.map((item) => {
          const count = (rows ?? []).filter((row) => inView(row, item.key)).length;
          return (
            <SideNavItem key={item.key} label={item.label} href={item.key === "all" ? "#/" : `#/?view=${item.key}`} isSelected={view === item.key}
              endContent={rows ? <span className="kb-pill" data-tone={item.key === "due" && count ? "warning" : "neutral"}>{count}</span> : undefined} />
          );
        })}
      </SideNavSection>
      {(open.length > 0 || needle) && (
        <SideNavSection title="在办项目">
          {!open.length && <SideNavItem label="没有匹配的在办项目" isDisabled />}
          {open.map((row) => {
            const due = dueText(row);
            return <SideNavItem key={row.id} label={row.title} icon={dot(due.tone === "neutral" ? "info" : due.tone)} href={`#${authoringHref(row.id, "files")}`}
              endContent={shortDate(row.ends_at) ? <span className="kb-pill" data-tone={due.tone}>{row.ends_at!.slice(5, 10)}</span> : undefined} />;
          })}
        </SideNavSection>
      )}
    </>,
  };
}
