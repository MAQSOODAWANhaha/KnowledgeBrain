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
  tree,
  inspector,
  children,
  className,
  onBeforeLeave,
}: Props) {
  const [mobileTreeOpen, setMobileTreeOpen] = useState(false);
  const side = tree ?? projectNav;
  function leave() {
    if (onBeforeLeave && !onBeforeLeave()) return;
    setToken(null);
    go("/login");
  }
  return (
    <div className={cn("grid h-dvh min-h-dvh grid-rows-[64px_minmax(0,1fr)] grid-cols-[minmax(0,1fr)] bg-canvas md:grid-cols-[240px_minmax(0,1fr)]", className)}>
      <header className="z-20 col-span-full flex h-16 shrink-0 items-stretch gap-6 border-b border-line bg-canvas px-4 md:px-6">
        {side && <button type="button" className="my-auto rounded-md p-1 text-quiet md:hidden"
          aria-label={mobileTreeOpen ? "收起导航" : "展开导航"} aria-expanded={mobileTreeOpen} onClick={() => setMobileTreeOpen((value) => !value)}>
          <IconMenu2 size={19} />
        </button>}
        <a className="flex shrink-0 items-center gap-2.5" href="#/">
          <span className="grid h-7 w-7 place-items-center rounded-[6px] bg-sky text-[11px] font-semibold tracking-wide text-white">KB</span>
          <strong className="hidden text-[13px] font-semibold tracking-tight text-ink sm:block">KnowledgeBrain</strong>
        </a>
        <nav className="flex shrink-0 items-stretch gap-1" aria-label="产品">
          <a className={cn("flex items-center gap-2 rounded-[6px] px-3 text-[14px]", root === "bids" ? "font-semibold text-ink" : "font-medium text-quiet hover:text-ink")} href="#/">
            <IconClipboardList size={18} stroke={1.8} aria-hidden />
            投标项目
          </a>
          <a className={cn("flex items-center gap-2 rounded-[6px] px-3 text-[14px]", root === "assets" ? "font-semibold text-ink" : "font-medium text-quiet hover:text-ink")} href="#/library">
            <IconBooks size={18} stroke={1.8} aria-hidden />
            知识资产
          </a>
        </nav>
        <div className="ml-auto flex items-center gap-2">
          <span className="hidden max-w-40 truncate text-xs text-quiet lg:block" title={email}>{email}</span>
          <button type="button" className="rounded-md p-2 text-quiet hover:bg-canvas" onClick={leave} aria-label="退出">
            <IconLogout size={17} stroke={1.7} />
          </button>
        </div>
      </header>
      {side && mobileTreeOpen && <button type="button" aria-label="关闭导航" className="fixed inset-x-0 bottom-0 top-16 z-20 bg-ink/20 md:hidden" onClick={() => setMobileTreeOpen(false)} />}
      {side && <aside className={cn("side min-h-0 min-w-0 flex-col overflow-hidden border-r border-line bg-canvas md:static md:flex",
        mobileTreeOpen ? "fixed bottom-0 left-0 top-16 z-30 flex w-60 md:w-auto" : "hidden")}
        onClick={(event) => { if (event.target instanceof Element && event.target.closest("a[href]")) setMobileTreeOpen(false); }}>
        <div className="min-h-0 flex-1 overflow-auto p-2">{side}</div>
      </aside>}
      {!side && <div className="hidden md:block" />}
      <div className="flex min-h-0 min-w-0 flex-col overflow-hidden bg-white">
        <div className="flex h-12 shrink-0 items-center gap-4 border-b border-line px-4 md:px-6">
          <div className="min-w-0">
            {title ? <h1 className="m-0 truncate text-[18px] font-semibold tracking-tight text-ink">{title}</h1> : <span className="sr-only">{crumbs}</span>}
          </div>
          {extra ? <div className="ml-auto flex min-w-0 items-center gap-2">{steps}{extra}</div> : steps}
        </div>
        {lead}
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
