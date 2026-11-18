import { AppShell } from "@astryxdesign/core/AppShell";
import { Button } from "@astryxdesign/core/Button";
import { Heading } from "@astryxdesign/core/Heading";
import { SideNav } from "@astryxdesign/core/SideNav";
import { TopNav, TopNavHeading, TopNavItem } from "@astryxdesign/core/TopNav";
import { type ReactNode } from "react";
import { setToken } from "./api";
import { go } from "./hash";

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
  onBeforeLeave,
}: Props) {
  const side = tree ?? projectNav;
  function leave() {
    if (onBeforeLeave && !onBeforeLeave()) return;
    setToken(null);
    go("/login");
  }
  return (
    <AppShell
      variant="section"
      height="fill"
      contentPadding={0}
      topNav={
        <TopNav
          className="kb-top"
          label="产品"
          heading={<TopNavHeading heading="KnowledgeBrain" headingHref="#/" logoLabel="KnowledgeBrain" />}
          endContent={<><span className="kb-account">{email}</span><Button variant="secondary" label="退出" onClick={leave} /></>}
        >
          <TopNavItem size="lg" href="#/" label="投标项目" isSelected={root === "bids"} />
          <TopNavItem size="lg" href="#/library" label="知识资产" isSelected={root === "assets"} />
        </TopNav>
      }
      sideNav={side ? <SideNav className="kb-side" aria-label="导航">{side}</SideNav> : undefined}
    >
      <div className="kb-page">
        <header className="kb-page-header">
          <div className="kb-page-heading">
            <nav className="crumbs kb-crumbs" aria-label="当前位置">{crumbs}</nav>
            {title ? <Heading level={1}>{title}</Heading> : null}
          </div>
          {(extra || steps) ? <div className="kb-page-actions">{steps}{extra}</div> : null}
        </header>
        {lead}
        {inspector ? (
          <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_320px]">
            <div className="min-h-0 overflow-auto">{children}</div>
            <aside className="overflow-auto border-l border-line">{inspector}</aside>
          </div>
        ) : <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div>}
      </div>
    </AppShell>
  );
}
