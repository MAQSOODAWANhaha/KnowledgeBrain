import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { type ReactNode } from "react";

export type Crumb = { label: string; href?: string };

// DESIGN.md §4: every page header has the same rows — crumbs, title + status + actions, meta, tabs.
export function PageHeader({ crumbs, title, status, actions, meta, tabs }: {
  crumbs: Crumb[];
  title: string;
  status?: ReactNode;
  actions?: ReactNode;
  /** One line of facts: label/value pairs, or free text. */
  meta?: ReactNode | Array<[string, ReactNode]>;
  tabs?: ReactNode;
}) {
  return (
    <VStack gap={1} paddingInline={6} paddingBlockStart={3} paddingBlockEnd={tabs ? 0 : 3}>
      <Breadcrumbs variant="supporting" label="当前位置">
        {crumbs.map((crumb, index) => (
          <BreadcrumbItem key={`${index}-${crumb.label}`} href={crumb.href ? (crumb.href.startsWith("#") ? crumb.href : `#${crumb.href}`) : undefined}>
            {crumb.label}
          </BreadcrumbItem>
        ))}
      </Breadcrumbs>
      <HStack gap={3} vAlign="center" hAlign="between" wrap="wrap">
        <HStack gap={3} vAlign="center">
          <Heading level={1}>{title}</Heading>
          {status}
        </HStack>
        {actions ? <HStack gap={2} vAlign="center" wrap="wrap">{actions}</HStack> : null}
      </HStack>
      {Array.isArray(meta)
        ? <HStack gap={5} wrap="wrap">{meta.map(([label, value]) => <Text key={label} type="supporting" hasTabularNumbers>{label} <Text color="primary">{value}</Text></Text>)}</HStack>
        : meta ? <Text type="supporting">{meta}</Text> : null}
      {tabs}
    </VStack>
  );
}
