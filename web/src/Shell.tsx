import { type ReactNode } from "react";
import { IconBooks, IconClipboardList, IconLogout } from "@tabler/icons-react";
import { setToken } from "./api";
import { go } from "./hash";
import { cn } from "./lib/utils";

type Props = {
  root: "bids" | "assets";
  email: string;
  crumbs: ReactNode;
  title?: string;
  extra?: ReactNode;
  lead?: ReactNode;
  steps?: ReactNode;
  tree?: ReactNode;
  inspector?: ReactNode;
  children: ReactNode;
  className?: string;
  onBeforeLeave?: () => boolean;
};

export function Shell({
  root,
  email,
  crumbs,
  title,
  extra,
  lead,
  steps,
  tree,
  inspector,
  children,
  className,
  onBeforeLeave,
}: Props) {
  const initial =
    !email || email.startsWith("dev@") ? "张" : email.slice(0, 1).toUpperCase();
  function leave() {
    if (onBeforeLeave && !onBeforeLeave()) return;
    setToken(null);
    go("/login");
  }
  return (
    <div
      className={cn(
        "grid h-dvh min-h-dvh grid-cols-[280px_minmax(0,1fr)] grid-rows-[62px_minmax(0,1fr)] bg-white",
        className,
      )}
    >
      <header className="col-span-2 z-20 grid h-[62px] grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-stretch border-b border-line bg-white/80 px-7 backdrop-blur-2xl">
        <a className="flex min-w-0 items-center gap-2.5" href="#/">
          <span className="grid h-8 w-8 place-items-center rounded-[10px] bg-[linear-gradient(140deg,#60a5fa,#2563eb_55%,#1d4ed8)] text-[13px] font-extrabold text-white shadow-[0_2px_8px_rgba(37,99,235,.35)]">
            KB
          </span>
          <strong className="text-[16px] font-semibold tracking-tight text-ink">KnowledgeBrain</strong>
        </a>
        <nav className="flex items-stretch gap-6" aria-label="产品">
          <a
            className={cn(
              "flex items-center gap-2 border-b-[2.5px] text-[16px] font-medium",
              root === "bids"
                ? "border-sky font-semibold text-ink"
                : "border-transparent text-quiet hover:text-ink",
            )}
            href="#/"
          >
            <IconClipboardList size={18} stroke={1.7} aria-hidden />
            投标项目
          </a>
          <a
            className={cn(
              "flex items-center gap-2 border-b-[2.5px] text-[16px] font-medium",
              root === "assets"
                ? "border-sky font-semibold text-ink"
                : "border-transparent text-quiet hover:text-ink",
            )}
            href="#/library"
          >
            <IconBooks size={18} stroke={1.7} aria-hidden />
            知识资产
          </a>
        </nav>
        <div />
      </header>
      <aside className="side flex min-h-0 min-w-0 flex-col overflow-hidden border-r border-line bg-canvas">
        <div className="min-h-0 flex-1 overflow-auto p-2">{tree}</div>
        <div className="flex shrink-0 items-center gap-2 border-t border-line px-2 py-3">
          <span className="grid h-8 w-8 place-items-center rounded-full bg-[linear-gradient(135deg,#f59e0b,#ef4444)] text-[12px] font-bold text-white">
            {initial}
          </span>
          <span className="min-w-0 flex-1 truncate text-[12.5px] text-quiet" title={email || "dev@local"}>
            {email || "dev@local"}
          </span>
          <button
            type="button"
            className="inline-flex items-center gap-1 rounded-[8px] px-2 py-1 text-[12.5px] font-medium text-quiet hover:bg-[#f6f6f8] hover:text-ink"
            onClick={leave}
            aria-label="退出"
          >
            <IconLogout size={16} stroke={1.7} />
            退出
          </button>
        </div>
      </aside>
      <div className="flex min-h-0 min-w-0 flex-col overflow-hidden bg-white">
        <div className="flex shrink-0 flex-col gap-3 px-7 py-3">
          <nav className="crumbs" aria-label="面包屑">
            {crumbs}
          </nav>
          {steps}
          {title || extra ? (
            <div className="flex items-center justify-between gap-4">
              {title ? <h1 className="m-0 text-[24px] font-semibold tracking-tight text-ink">{title}</h1> : <span />}
              {extra ? <div className="flex items-center gap-2">{extra}</div> : null}
            </div>
          ) : null}
          {lead}
        </div>
        {inspector ? (
          <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_320px]">
            <div className="min-h-0 overflow-auto">{children}</div>
            <aside className="overflow-auto border-l border-line bg-white">{inspector}</aside>
          </div>
        ) : (
          children
        )}
      </div>
    </div>
  );
}
