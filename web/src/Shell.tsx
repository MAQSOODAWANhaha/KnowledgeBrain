import { AppShell } from "@astryxdesign/core/AppShell";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/HStack";
import { Layout, LayoutHeader } from "@astryxdesign/core/Layout";
import { SideNav, SideNavItem, SideNavSection } from "@astryxdesign/core/SideNav";
import { Text } from "@astryxdesign/core/Text";
import { Theme } from "@astryxdesign/core/theme";
import { TopNav, TopNavHeading } from "@astryxdesign/core/TopNav";
import { VStack } from "@astryxdesign/core/VStack";
import { BookOpen, FolderKanban } from "lucide-react";
import { type ReactNode } from "react";
import { setToken } from "./api";
import { go } from "./hash";
import { brandTheme } from "./theme";

type Props = {
  root: "bids" | "assets";
  email: string;
  header: ReactNode;
  navTop?: ReactNode;
  nav?: ReactNode;
  children: ReactNode;
  onBeforeLeave?: () => boolean;
};

// DESIGN.md §4–5: one frame for every signed-in page. Modules stay in the side nav;
// the page passes its own nav sections (project stages, folders) and its PageHeader.
export function Shell({ root, email, header, navTop, nav, children, onBeforeLeave }: Props) {
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
            heading={<TopNavHeading heading="投标管理平台" headingHref="#/" logoLabel="KnowledgeBrain 投标管理平台" />}
            endContent={<HStack gap={3} vAlign="center"><Text color="secondary">{email}</Text><Button variant="ghost" label="退出" onClick={leave} /></HStack>}
          />
        </Theme>
      }
      sideNav={
        <SideNav aria-label="导航" collapsible={{ buttonLabel: "收起导航" }} topContent={navTop}>
          <SideNavSection title="模块" isHeaderHidden>
            <SideNavItem label="投标项目" href="#/" icon={FolderKanban} isSelected={root === "bids"} />
            <SideNavItem label="知识资产" href="#/library" icon={BookOpen} isSelected={root === "assets"} />
          </SideNavSection>
          {nav}
        </SideNav>
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
