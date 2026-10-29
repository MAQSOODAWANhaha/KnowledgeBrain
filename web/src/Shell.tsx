import { type ReactNode, useState } from "react";
import { IconBooks, IconClipboardList, IconLogout, IconMenu2 } from "@tabler/icons-react";
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
  projectNav?: ReactNode;
  layout?: "standard" | "workspace";
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
  projectNav,
  layout = "standard",
  tree,
  inspector,
  children,
  className,
  onBeforeLeave,
}: Props) {
  const [mobileTreeOpen, setMobileTreeOpen] = useState(false);
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
        "grid h-dvh min-h-dvh grid-rows-[62px_minmax(0,1fr)] bg-white",
        layout === "workspace" ? "grid-cols-[minmax(0,1fr)]" : "grid-cols-[minmax(0,1fr)] md:grid-cols-[240px_minmax(0,1fr)]",
        className,
      )}
    >
      <header className={cn("z-20 flex h-[62px] items-stretch justify-between gap-4 border-b border-line bg-white/80 px-4 backdrop-blur-2xl md:px-6", layout !== "workspace" && "md:col-span-2")}>
        {layout !== "workspace" && tree && <button type="button" className="my-auto rounded-md p-1 text-quiet md:hidden"
          aria-label={mobileTreeOpen ? "收起导航" : "展开导航"} aria-expanded={mobileTreeOpen} onClick={() => setMobileTreeOpen((value) => !value)}>
          <IconMenu2 size={19} />
        </button>}
        <a className="flex min-w-0 items-center gap-2.5" href="#/">
          <span className="grid h-8 w-8 place-items-center rounded-[10px] bg-[linear-gradient(140deg,#60a5fa,#2563eb_55%,#1d4ed8)] text-[13px] font-extrabold text-white shadow-[0_2px_8px_rgba(37,99,235,.35)]">
            KB
          </span>
          <strong className="hidden text-[16px] font-semibold tracking-tight text-ink sm:block">KnowledgeBrain</strong>
        </a>
        <nav className="flex items-stretch gap-4 md:gap-6" aria-label="产品">
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
        <div className="flex items-center gap-2">
          <span className="hidden max-w-40 truncate text-xs text-quiet lg:block" title={email}>{email}</span>
          <button type="button" className="rounded-md p-2 text-quiet hover:bg-canvas" onClick={leave} aria-label="退出">
            <IconLogout size={17} stroke={1.7} />
          </button>
        </div>
      </header>
      {layout !== "workspace" && mobileTreeOpen && <button type="button" aria-label="关闭导航" className="fixed inset-x-0 bottom-0 top-[62px] z-20 bg-black/20 md:hidden" onClick={() => setMobileTreeOpen(false)} />}
      {layout !== "workspace" && <aside className={cn("side min-h-0 min-w-0 flex-col overflow-hidden border-r border-line bg-canvas md:static md:flex",
        mobileTreeOpen ? "fixed bottom-0 left-0 top-[62px] z-30 flex w-60 md:w-auto" : "hidden")}
        onClick={(event) => { if (event.target instanceof Element && event.target.closest("a[href]")) setMobileTreeOpen(false); }}>
        <div className="min-h-0 flex-1 overflow-auto p-2">{tree}</div>
        <div className="flex shrink-0 items-center gap-2 border-t border-line px-2 py-3">
          <span className="grid h-8 w-8 place-items-center rounded-full bg-[linear-gradient(135deg,#f59e0b,#ef4444)] text-[12px] font-bold text-white">
            {initial}
          </span>
          <span className="min-w-0 flex-1 truncate text-[12.5px] text-quiet" title={email || "dev@local"}>
            {email || "dev@local"}
          </span>
        </div>
      </aside>}
      <div className="flex min-h-0 min-w-0 flex-col overflow-hidden bg-white">
        <div className="flex shrink-0 flex-col gap-3 px-4 py-3 md:px-6">
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
          {projectNav}
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
