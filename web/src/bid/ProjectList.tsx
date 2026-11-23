import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack } from "@astryxdesign/core/HStack";
import { Link } from "@astryxdesign/core/Link";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Tab, TabList } from "@astryxdesign/core/TabList";
import { pixel, Table } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { VStack } from "@astryxdesign/core/VStack";
import { useEffect, useState } from "react";
import { go } from "../hash";
import { Shell } from "../Shell";
import { PageHeader } from "../shell/PageHeader";
import { OpenProjects } from "../shell/ProjectNav";
import { type BidProjectView } from "./api";
import { tenderApi } from "./api/workbench";
import { authoringHref } from "./authoring/routes";

type Filter = "all" | "open" | "ended";

const dateTime = (value?: string | null) => (value ? value.slice(0, 16).replace("T", " ") : "—");

// List pattern (DESIGN.md §4): filters as header tabs, search above the table, empty state with the next step.
export function ProjectList({ email }: { email: string }) {
  const [rows, setRows] = useState<BidProjectView[] | null>(null);
  const [listError, setListError] = useState(false);
  const [listRetry, setListRetry] = useState(0);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [err, setErr] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  useEffect(() => {
    let canceled = false;
    setListError(false);
    tenderApi
      .listProjects()
      .then((value) => { if (!canceled) setRows(value); })
      .catch(() => { if (!canceled) setListError(true); });
    return () => { canceled = true; };
  }, [listRetry]);
  const ended = (rows ?? []).filter((p) => p.status === "ended").length;
  const shown = (rows ?? []).filter((p) => {
    if (query.trim() && !p.title.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())) return false;
    if (filter === "open") return p.status !== "ended";
    if (filter === "ended") return p.status === "ended";
    return true;
  });
  const create = <Button data-testid="new-bid" label="新建项目" variant="primary" onClick={() => setOpen(true)} />;

  return (
    <Shell
      root="bids"
      email={email}
      header={<PageHeader
        crumbs={[{ label: "投标项目" }]}
        title="投标项目"
        actions={create}
        meta={rows ? `在办 ${rows.length - ended} 个 · 已结束 ${ended} 个` : undefined}
        tabs={<TabList value={filter} onChange={(value) => setFilter(value as Filter)}>
          <Tab value="all" label="全部" />
          <Tab value="open" label="在办" />
          <Tab value="ended" label="已结束" />
        </TabList>}
      />}
      nav={<OpenProjects rows={rows} />}
    >
      <VStack gap={4} padding={6} isScrollable height="100%">
        {listError && <div role="alert"><Banner status="error" title="项目读取失败，已有项目不会因此被清空。" endContent={<Button size="sm" label="重试读取项目" onClick={() => setListRetry((value) => value + 1)} />} /></div>}
        {rows === null ? listError ? null : (
          <Card><VStack gap={3}><Skeleton height={40} /><Skeleton height={40} /><Skeleton height={40} /></VStack></Card>
        ) : rows.length === 0 ? (
          <Card><EmptyState title="还没有项目" description="新建项目后上传招标文件和补遗，系统会读出全部招标事项，再按格式章节生成 Word 初稿。" actions={create} /></Card>
        ) : (
          <Card padding={0}>
            <HStack gap={3} vAlign="center" hAlign="between" padding={3}>
              <TextInput label="过滤项目" isLabelHidden type="search" placeholder="按项目名称搜索" value={query} onChange={setQuery} />
              <Text type="supporting" hasTabularNumbers>共 {shown.length} 个项目</Text>
            </HStack>
            <Table hasHover idKey="id" data={shown} columns={[
              { key: "title", header: "项目名称", renderCell: (p) => <Link href={`#${authoringHref(p.id, "files")}`}>{p.title}</Link> },
              { key: "status", header: "状态", width: pixel(120), renderCell: (p) => <Badge variant={p.status === "ended" ? "neutral" : "info"} label={p.status === "ended" ? "已结束" : "在办"} /> },
              { key: "ends_at", header: "投标截止", width: pixel(180), renderCell: (p) => <Text hasTabularNumbers>{dateTime(p.ends_at)}</Text> },
              { key: "ended_at", header: "结束时间", width: pixel(180), renderCell: (p) => <Text hasTabularNumbers color="secondary">{dateTime(p.ended_at)}</Text> },
            ]} />
            {shown.length === 0 && <EmptyState isCompact title="没有符合条件的项目" description="换一个名称或切换到「全部」。" />}
          </Card>
        )}
      </VStack>
      <Dialog isOpen={open} onOpenChange={setOpen}>
        <DialogHeader title="新建项目" onOpenChange={setOpen} />
        <form
          data-testid="create-bid-form"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!title.trim()) {
              setErr("先写项目名称");
              return;
            }
            try {
              const p = await tenderApi.createProject({ title: title.trim() });
              go(authoringHref(p.id, "files"));
            } catch (ex) {
              setErr(ex instanceof Error ? ex.message : "创建失败");
            }
          }}
        >
          <VStack gap={4}>
            <TextInput id="bid-title" data-testid="bid-title" label="项目名称" isRequired value={title} onChange={setTitle} />
            <TextInput label="负责人" value={email} isReadOnly />
            {err ? <Banner status="error" title={err} /> : null}
            <HStack gap={2} hAlign="end">
              <Button variant="secondary" type="button" label="取消" onClick={() => setOpen(false)} />
              <Button type="submit" variant="primary" label="创建" data-testid="bid-create" />
            </HStack>
          </VStack>
        </form>
      </Dialog>
    </Shell>
  );
}
