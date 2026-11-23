import { AppShell } from "@astryxdesign/core/AppShell";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/HStack";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Layout, LayoutHeader } from "@astryxdesign/core/Layout";
import { SideNav } from "@astryxdesign/core/SideNav";
import { Text } from "@astryxdesign/core/Text";
import { Theme } from "@astryxdesign/core/theme";
import { TopNav, TopNavHeading } from "@astryxdesign/core/TopNav";
import { VStack } from "@astryxdesign/core/VStack";
import { BookOpen, ChevronsLeft, ChevronsRight, FolderKanban } from "lucide-react";
import { type ReactNode, useState } from "react";
import { setToken } from "./api";
import { go } from "./hash";
import "./shell/shell.css";
import { brandTheme } from "./theme";

export type ContextPanel = {
  /** Module name shown at the top of the panel. */
  title: string;
  /** Accessible name of the panel's navigation landmark. */
  label: string;
  /** Header controls next to the title (e.g. a create button). */
  actions?: ReactNode;
  /** Content above the sections: search, project card. */
  top?: ReactNode;
  children?: ReactNode;
};

type Props = {
  root: "bids" | "assets";
  email: string;
  header: ReactNode;
  panel: ContextPanel;
  children: ReactNode;
  onBeforeLeave?: () => boolean;
};

const PANEL_KEY = "kb.context-panel";
function readPanelOpen() {
  try { return localStorage.getItem(PANEL_KEY) !== "closed"; } catch { return true; }
}

// DESIGN.md §4–5: app bar, icon rail for modules, context panel for the place inside the module.
export function Shell({ root, email, header, panel, children, onBeforeLeave }: Props) {
  const [open, setOpen] = useState(readPanelOpen);
  function setPanel(value: boolean) {
    setOpen(value);
    try { localStorage.setItem(PANEL_KEY, value ? "open" : "closed"); } catch { /* per-viewer convenience only */ }
  }
  function leave() {
    if (onBeforeLeave && !onBeforeLeave()) return;
    setToken(null);
    go("/login");
  }
  return (
    <AppShell
      variant="surface"
      height="fill"
      contentPadding={0}
      topNav={
        <Theme theme={brandTheme} mode="dark">
          <TopNav
            label="产品"
            heading={<TopNavHeading heading="投标管理平台" headingHref="#/" logo={<span className="kb-logo">KB</span>} logoLabel="KnowledgeBrain 投标管理平台" />}
            endContent={<HStack gap={3} vAlign="center"><Text color="secondary">{email}</Text><Button variant="ghost" label="退出" onClick={leave} /></HStack>}
          />
        </Theme>
      }
      sideNav={
        <div className="kb-nav">
          <nav className="kb-rail" aria-label="导航">
            <a href="#/" aria-current={root === "bids" ? "page" : undefined}><FolderKanban size={20} aria-hidden />投标项目</a>
            <a href="#/library" aria-current={root === "assets" ? "page" : undefined}><BookOpen size={20} aria-hidden />知识资产</a>
            {!open && <button type="button" className="kb-rail-end" aria-label={`展开${panel.title}面板`} onClick={() => setPanel(true)}><ChevronsRight size={18} aria-hidden />展开</button>}
          </nav>
          {open && (
            <div className="kb-ctx">
              <SideNav
                aria-label={panel.label}
                header={
                  <HStack gap={1} vAlign="center" hAlign="between" paddingInlineStart={2} minHeight={48}>
                    <Text weight="semibold">{panel.title}</Text>
                    <HStack gap={1} vAlign="center">{panel.actions}<IconButton size="sm" variant="ghost" label="收起面板" icon={<ChevronsLeft size={16} />} onClick={() => setPanel(false)} /></HStack>
                  </HStack>
                }
                topContent={panel.top}
              >
                {panel.children}
              </SideNav>
            </div>
          )}
        </div>
      }
    >
      <Layout
        height="fill"
        header={<LayoutHeader hasDivider padding={0}>{header}</LayoutHeader>}
        content={<VStack height="100%" minHeight={0}>{children}</VStack>}
      />
    </AppShell>
  );
}
