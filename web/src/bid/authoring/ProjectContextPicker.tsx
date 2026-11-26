import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Drawer, DrawerHeader } from "@astryxdesign/core/Drawer";
import { HStack } from "@astryxdesign/core/HStack";
import { List, ListItem } from "@astryxdesign/core/List";
import { Pagination } from "@astryxdesign/core/Pagination";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Toolbar } from "@astryxdesign/core/Toolbar";
import { VStack } from "@astryxdesign/core/VStack";
import { useState } from "react";
import type { ProjectContextSources, SourceExcerptRef, TenderTarget } from "../api/workbench";
import { excerptKey } from "./outlineTree";

const PAGE = 40;

export function ProjectContextPicker({
  isOpen,
  onOpenChange,
  target,
  sources,
  outlineFrozenSha,
  selectedExcerpts,
  disabled,
  onChange,
}: {
  isOpen: boolean;
  onOpenChange(open: boolean): void;
  target: TenderTarget | null;
  sources: ProjectContextSources | null | undefined;
  outlineFrozenSha?: string | null;
  selectedExcerpts: SourceExcerptRef[];
  disabled: boolean;
  onChange(excerpts: SourceExcerptRef[]): void;
}) {
  const [filter, setFilter] = useState("");
  const [page, setPage] = useState(1);

  const frozenMismatch = Boolean(sources?.frozen_sha256 && outlineFrozenSha && sources.frozen_sha256 !== outlineFrozenSha);
  const blocked = disabled || frozenMismatch || !sources?.available;
  const selectedKeys = new Set(selectedExcerpts.map(excerptKey));
  const query = filter.trim().toLowerCase();
  const options = (sources?.options ?? []).filter(option => {
    if (!query) return true;
    const pageLabel = typeof option.page === "number" ? `第${option.page}页` : "";
    return [option.heading_path ?? "", option.preview, option.range_id, pageLabel]
      .some(value => value.toLowerCase().includes(query));
  });
  const pages = Math.max(1, Math.ceil(options.length / PAGE));
  const current = Math.min(page, pages);
  const visible = options.slice((current - 1) * PAGE, current * PAGE);

  const toggle = (excerpt: SourceExcerptRef, checked: boolean) => {
    const key = excerptKey(excerpt);
    const remaining = selectedExcerpts.filter(item => excerptKey(item) !== key);
    onChange(checked ? [...remaining, excerpt] : remaining);
  };

  return (
    <Drawer isOpen={isOpen} onOpenChange={onOpenChange} label="选择项目背景" width={640}>
      <DrawerHeader title="选择项目背景" onOpenChange={onOpenChange} />
      {isOpen && target && (
        <VStack gap={3}>
          <Text type="supporting">为「{target.purpose}」绑定当前冻结主件的说明性背景，随「保存大纲草稿」生效；背景仅作写作语境，不替代企业能力依据。</Text>
          {frozenMismatch && <Banner status="warning" title="当前载入大纲的冻结来源与最新主件不一致，请先保存或载入最新大纲后再选择项目背景。" />}
          {!frozenMismatch && !sources?.available && <Banner status="info" title={sources?.reason ?? "当前大纲暂不可选择项目背景。"} />}
          <Toolbar
            label="项目背景筛选"
            dividers={["bottom"]}
            startContent={<Text role="status" hasTabularNumbers>已选 {selectedExcerpts.length} 处</Text>}
            endContent={<HStack gap={2} vAlign="center">
              <Button size="sm" variant="ghost" isDisabled={blocked || !selectedExcerpts.length} onClick={() => onChange([])} label="清空背景" />
              {pages > 1 && <Pagination page={current} onChange={setPage} totalItems={options.length} pageSize={PAGE} />}
            </HStack>}
          />
          {!!sources?.options.length && (
            <TextInput label="筛选原件范围" placeholder="按章节标题或正文关键词筛选" value={filter} onChange={value => { setFilter(value); setPage(1); }} />
          )}
          {sources?.available && !visible.length && <Text type="supporting">没有匹配当前筛选条件的原件范围。</Text>}
          {!!visible.length && (
            <List hasDividers aria-label="主件背景范围">
              {visible.map(option => {
                const location = [
                  typeof option.page === "number" ? `第 ${option.page} 页` : null,
                  option.heading_path || null,
                ].filter(Boolean).join(" · ");
                const label = location ? `[${location}] ${option.preview}` : option.preview;
                return (
                  <ListItem
                    key={option.option_id}
                    label={
                      <CheckboxInput
                        label={label}
                        isDisabled={blocked}
                        value={selectedKeys.has(excerptKey(option.excerpt))}
                        onChange={checked => toggle(option.excerpt, checked)}
                      />
                    }
                  />
                );
              })}
            </List>
          )}
        </VStack>
      )}
    </Drawer>
  );
}
