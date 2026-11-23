import { Badge } from "@astryxdesign/core/Badge";
import { FileInput } from "@astryxdesign/core/FileInput";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack } from "@astryxdesign/core/HStack";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Link } from "@astryxdesign/core/Link";
import { pixel, Table } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TreeList, type TreeListItemData } from "@astryxdesign/core/TreeList";
import { VStack } from "@astryxdesign/core/VStack";
import { Plus } from "lucide-react";
import { TextInput } from "@astryxdesign/core/TextInput";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  type Doc,
  type Product,
  type Version,
  type Workspace,
  api,
  slugify,
} from "../api";
import { PageHeader, type Crumb } from "../shell/PageHeader";
import { SideNavSection } from "@astryxdesign/core/SideNav";
import { type AssetRoute, assetDocHref, assetVersionHref, go } from "../hash";
import { Shell } from "../Shell";
import { DocumentDetail } from "./DocumentDetail";

function notify(msg: string, error = false) {
  if (error) toast.error(msg);
  else toast.success(msg);
}

const parseLabels: Record<string, string> = {
  pending: "排队中", processing: "解析中", finalizing: "建立索引", completed: "已解析",
  failed: "解析失败", cancelled: "已取消", deleting: "删除中", deleted: "已删除",
};

function parseStatus(s: string | object): string {
  const value = typeof s === "string" ? s : Object.keys(s as object)[0] ?? "";
  return parseLabels[value] ?? value;
}

type CreateKind = "folder" | "line" | "product" | "version";

function keyOf(route: AssetRoute): string {
  switch (route.kind) {
    case "company":
      return "company";
    case "folder":
      return route.versionId ? `folder:${route.folderId}:${route.versionId}` : `folder:${route.folderId}`;
    case "lines":
      return "lines";
    case "line":
      return `line:${route.lineId}`;
    case "product":
      return `product:${route.productId}`;
    case "version":
      return `version:${route.versionId}`;
    case "doc":
      return `doc:${route.docId}`;
  }
}

function ancestors(route: AssetRoute): string[] {
  switch (route.kind) {
    case "company":
      return ["company"];
    case "folder":
      return ["company", `folder:${route.folderId}`];
    case "lines":
      return ["lines"];
    case "line":
      return ["lines", `line:${route.lineId}`];
    case "product":
      return ["lines", `line:${route.lineId}`, `product:${route.productId}`];
    case "version":
      return ["lines", `line:${route.lineId}`, `product:${route.productId}`];
    case "doc":
      if ("folderId" in route) return ["company", `folder:${route.folderId}`];
      return ["lines", `line:${route.lineId}`, `product:${route.productId}`];
  }
}

export function Assets({ email, route }: { email: string; route: AssetRoute }) {
  const [err, setErr] = useState("");
  const [company, setCompany] = useState<Workspace | null>(null);
  const [folders, setFolders] = useState<Product[]>([]);
  const [lines, setLines] = useState<Workspace[]>([]);
  const [productsByLine, setProductsByLine] = useState<Record<string, Product[]>>({});
  const [versionsByProduct, setVersionsByProduct] = useState<Record<string, Version[]>>({});
  const [docs, setDocs] = useState<Doc[]>([]);
  const [open, setOpen] = useState<Set<string>>(() => new Set(["company", "lines"]));
  const [create, setCreate] = useState<{ kind: CreateKind; parentId?: string } | null>(null);
  const [name, setName] = useState("");

  const ensureVersions = useCallback(
    async (pid: string): Promise<Version[]> => {
      if (versionsByProduct[pid]) return versionsByProduct[pid];
      const list = await api.versions(pid);
      const live = list.filter((v) => v.status !== "archived");
      setVersionsByProduct((cur) => ({ ...cur, [pid]: live }));
      return live;
    },
    [versionsByProduct],
  );

  async function reloadTree() {
    const all = await api.workspaces();
    let ws = all.find((w) => w.kind === "company" || w.slug === "company") ?? null;
    if (!ws) ws = await api.createWorkspace({ name: "公司资料", slug: "company", kind: "company" });
    setCompany(ws);
    let ps = await api.products(ws.id);
    if (ps.length === 0) {
      await Promise.all(
        ["资质证照", "体系认证", "业绩案例", "服务能力"].map((folder) =>
          api.createProduct(ws.id, { name: folder, slug: slugify(folder), kind: "library" }),
        ),
      );
      ps = await api.products(ws.id);
    }
    setFolders(ps);
    const ls = all.filter((w) => w.kind === "product_line" || (w.kind !== "company" && w.slug !== "company"));
    setLines(ls);
    const rows = await Promise.all(
      ls.map(async (line) => [line.id, (await api.products(line.id)).filter((p) => p.kind !== "library")] as const),
    );
    setProductsByLine(Object.fromEntries(rows));
    const ids = [...ps.map((p) => p.id), ...rows.flatMap(([, products]) => products.map((p) => p.id))];
    const versions = await Promise.all(ids.map(async (id) => [id, (await api.versions(id)).filter((v) => v.status !== "archived")] as const));
    setVersionsByProduct((cur) => ({ ...cur, ...Object.fromEntries(versions) }));
  }

  useEffect(() => {
    setOpen((cur) => {
      const next = new Set(cur);
      for (const k of ancestors(route)) next.add(k);
      return next;
    });
  }, [route]);

  useEffect(() => {
    void reloadTree().catch((e) => setErr(e instanceof Error ? e.message : "加载失败"));
  }, []);

  useEffect(() => {
    const pids: string[] = [];
    if (route.kind === "folder") pids.push(route.folderId);
    if (route.kind === "product" || route.kind === "version") pids.push(route.productId);
    if (route.kind === "doc") pids.push("folderId" in route ? route.folderId : route.productId);
    for (const id of [...open].filter((k) => k.startsWith("folder:") || k.startsWith("product:"))) {
      pids.push(id.split(":")[1]);
    }
    void Promise.all([...new Set(pids)].map((id) => ensureVersions(id).catch(() => [])));
  }, [route, open, ensureVersions]);

  const selectedProductId =
    route.kind === "folder"
      ? route.folderId
      : route.kind === "product" || route.kind === "version"
        ? route.productId
        : route.kind === "doc"
          ? "folderId" in route
            ? route.folderId
            : route.productId
          : null;
  const selectedVersionId =
    route.kind === "folder"
      ? route.versionId
      : route.kind === "version" || route.kind === "doc"
        ? route.versionId
        : undefined;

  useEffect(() => {
    let cancelled = false;
    async function loadDocs() {
      if (!selectedProductId) {
        setDocs([]);
        return;
      }
      const versions = await ensureVersions(selectedProductId);
      const vid = selectedVersionId ?? versions.find((v) => v.current)?.id ?? versions[0]?.id;
      if (!vid) {
        setDocs([]);
        return;
      }
      const list = await api.documents(selectedProductId, vid).catch(() => []);
      if (!cancelled) setDocs(list);
    }
    void loadDocs();
    return () => {
      cancelled = true;
    };
  }, [selectedProductId, selectedVersionId, ensureVersions]);


  async function submitCreate() {
    if (!create || !name.trim()) return;
    try {
      if (create.kind === "folder") {
        if (!company) return;
        const p = await api.createProduct(company.id, { name: name.trim(), slug: slugify(name), kind: "library" });
        setCreate(null);
        setName("");
        await reloadTree();
        go(`/library/${p.id}`);
        return;
      }
      if (create.kind === "line") {
        const line = await api.createWorkspace({ name: name.trim(), slug: slugify(name), kind: "product_line" });
        setCreate(null);
        setName("");
        await reloadTree();
        go(`/products/${line.id}`);
        return;
      }
      if (create.kind === "product") {
        const lineId = create.parentId;
        if (!lineId) return;
        const p = await api.createProduct(lineId, { name: name.trim(), slug: slugify(name), kind: "product" });
        setCreate(null);
        setName("");
        await reloadTree();
        go(`/products/${lineId}/${p.id}`);
        return;
      }
      const pid = create.parentId;
      if (!pid) return;
      const v = await api.createVersion(pid, name.trim());
      setCreate(null);
      setName("");
      setVersionsByProduct((cur) => {
        const next = { ...cur };
        delete next[pid];
        return next;
      });
      const lineId = lines.find((l) => (productsByLine[l.id] ?? []).some((p) => p.id === pid))?.id;
      if (folders.some((f) => f.id === pid)) go(`/library/${pid}/${v.id}`);
      else if (lineId) go(`/products/${lineId}/${pid}/${v.id}`);
    } catch (e) {
      notify(e instanceof Error ? e.message : "创建失败", true);
    }
  }

  const folder =
    folders.find(
      (p) =>
        p.id ===
        (route.kind === "folder" ? route.folderId : route.kind === "doc" && "folderId" in route ? route.folderId : ""),
    ) ?? null;
  const line = lines.find((l) => "lineId" in route && l.id === route.lineId) ?? null;
  const product =
    line && (route.kind === "product" || route.kind === "version" || (route.kind === "doc" && "productId" in route))
      ? (productsByLine[line.id] ?? []).find((p) => p.id === ("productId" in route ? route.productId : "")) ?? null
      : null;
  const versions = selectedProductId ? (versionsByProduct[selectedProductId] ?? []) : [];
  const version =
    selectedVersionId ? versions.find((v) => v.id === selectedVersionId) ?? null : versions.find((v) => v.current) ?? versions[0] ?? null;

  const crumbs = useMemo(() => {
    const items: Crumb[] = [{ label: "知识资产" }];
    const fileName =
      route.kind === "doc" ? (docs.find((d) => d.id === route.docId)?.file_name ?? "文件") : "";
    const inLibrary =
      route.kind === "company" || route.kind === "folder" || (route.kind === "doc" && "folderId" in route);
    if (inLibrary) {
      items.push({ label: "公司资料", href: route.kind === "company" ? undefined : "/library" });
      if (route.kind === "company") return items;
      const folderId = "folderId" in route ? route.folderId : "";
      const folderName = folder?.name ?? "分类";
      if (route.kind === "folder" && !route.versionId) {
        items.push({ label: folderName });
        return items;
      }
      items.push({ label: folderName, href: `/library/${folderId}` });
      if (route.kind === "folder") {
        items.push({ label: version?.label ?? "版本" });
        return items;
      }
      items.push({
        label: version?.label ?? "版本",
        href: `/library/${folderId}/${route.kind === "doc" && "versionId" in route ? route.versionId : ""}`,
      });
      items.push({ label: fileName });
      return items;
    }
    items.push({ label: "产品线", href: route.kind === "lines" ? undefined : "/products" });
    if (route.kind === "lines") return items;
    const lineId = "lineId" in route ? route.lineId : "";
    const lineName = line?.name ?? "产品线";
    if (route.kind === "line") {
      items.push({ label: lineName });
      return items;
    }
    items.push({ label: lineName, href: `/products/${lineId}` });
    const productId = "productId" in route ? route.productId : "";
    const productName = product?.name ?? "产品";
    if (route.kind === "product") {
      items.push({ label: productName });
      return items;
    }
    items.push({ label: productName, href: `/products/${lineId}/${productId}` });
    const verLabel = version?.label ?? "版本";
    if (route.kind === "version") {
      items.push({ label: verLabel });
      return items;
    }
    const versionId = route.kind === "doc" && "versionId" in route ? route.versionId : version?.id ?? "";
    items.push({ label: verLabel, href: `/products/${lineId}/${productId}/${versionId}` });
    items.push({ label: fileName });
    return items;
  }, [route, folder, line, product, version, docs]);

  const title =
    route.kind === "company"
      ? "公司资料"
      : route.kind === "folder"
        ? (folder?.name ?? "分类")
        : route.kind === "lines"
          ? "产品线"
          : route.kind === "line"
            ? (line?.name ?? "产品线")
            : route.kind === "product"
              ? (product?.name ?? "产品")
              : route.kind === "doc"
                ? (docs.find((d) => d.id === route.docId)?.file_name ?? "文件")
                : (version?.label ?? "版本");

  const createHint =
    route.kind === "company"
      ? "新建分类"
      : route.kind === "lines"
        ? "新建产品线"
        : route.kind === "line"
          ? "新建产品"
          : "新建版本";

  function openCreateFromSelection() {
    if (route.kind === "company") setCreate({ kind: "folder" });
    else if (route.kind === "lines") setCreate({ kind: "line" });
    else if (route.kind === "line") setCreate({ kind: "product", parentId: route.lineId });
    else if (route.kind === "folder") setCreate({ kind: "version", parentId: route.folderId });
    else if (route.kind === "product") setCreate({ kind: "version", parentId: route.productId });
  }

  async function upload(files: File[]) {
    const pid = selectedProductId;
    const vid = version?.id;
    if (!pid) return;
    if (!vid) {
      notify("请先新建版本，再上传文件", true);
      return;
    }
    await Promise.all(files.map((f) => api.ingest(pid, vid, f)));
    notify("已入库");
    const list = await api.documents(pid, vid).catch(() => []);
    setDocs(list);
    await reloadTree();
  }

  const selected = keyOf(route);
  const add = (label: string, kind: CreateKind, parentId?: string) => (
    <IconButton size="sm" variant="ghost" label={label} icon={<Plus size={14} />} onClick={(e) => { e.preventDefault(); e.stopPropagation(); setCreate({ kind, parentId }); }} />
  );
  const versionItems = (vs: Version[], href: (v: Version) => string, isSelected: (v: Version) => boolean): TreeListItemData[] =>
    vs.map((v) => ({ id: `version:${v.id}`, label: v.current ? `${v.label} · 当前` : v.label, href: `#${href(v)}`, isSelected: isSelected(v) }));
  const catalog: TreeListItemData[] = [
    {
      id: "company", label: "公司资料", href: "#/library", isSelected: selected === "company", isExpanded: open.has("company"),
      endContent: add("新建分类", "folder"),
      children: folders.map((p) => {
        const vs = versionsByProduct[p.id] ?? [];
        return {
          id: `folder:${p.id}`, label: p.name, href: `#/library/${p.id}`,
          isSelected: selected === `folder:${p.id}`, isExpanded: open.has(`folder:${p.id}`),
          endContent: add("新建版本", "version", p.id),
          children: vs.length > 1 ? versionItems(vs, (v) => `/library/${p.id}/${v.id}`, (v) => route.kind === "folder" && route.versionId === v.id) : undefined,
        };
      }),
    },
    {
      id: "lines", label: "产品线", href: "#/products", isSelected: selected === "lines", isExpanded: open.has("lines"),
      endContent: add("新建产品线", "line"),
      children: lines.map((l) => ({
        id: `line:${l.id}`, label: l.name, href: `#/products/${l.id}`,
        isSelected: selected === `line:${l.id}`, isExpanded: open.has(`line:${l.id}`),
        endContent: <HStack gap={1} vAlign="center">{(productsByLine[l.id] ?? []).length > 0 && <Badge variant="neutral" label={String((productsByLine[l.id] ?? []).length)} />}{add("新建产品", "product", l.id)}</HStack>,
        children: (productsByLine[l.id] ?? []).map((p) => {
          const vs = versionsByProduct[p.id] ?? [];
          return {
            id: `product:${p.id}`, label: p.name, href: `#/products/${l.id}/${p.id}`,
            isSelected: selected === `product:${p.id}`, isExpanded: open.has(`product:${p.id}`),
            endContent: add("新建版本", "version", p.id),
            children: vs.length ? versionItems(vs, (v) => `/products/${l.id}/${p.id}/${v.id}`, (v) => route.kind === "version" && route.versionId === v.id) : undefined,
          };
        }),
      })),
    },
  ];

  return (
    <Shell
      root="assets"
      email={email}
      header={<PageHeader crumbs={crumbs} title={title} actions={
        route.kind === "doc" || route.kind === "version" || (route.kind === "folder" && version) ? undefined : (
          <Button type="button" label={createHint} variant="primary" onClick={openCreateFromSelection} />
        )
      } />}
      panel={{
        title: "知识资产",
        label: "资料导航",
        children: <SideNavSection title="资料目录">
          <TreeList key={[...open].sort().join("|")} aria-label="资料目录" density="compact" variant="noGuides" items={catalog} />
        </SideNavSection>,
      }}
    >
      <VStack gap={4} padding={6} isScrollable height="100%">
        {err && (
          <Banner role="alert" status="error" title="资料目录读取失败，已有资料不会因此被清空。" description={err} endContent={<Button size="sm" label="重试" onClick={() => { setErr(""); void reloadTree().catch((e) => setErr(e instanceof Error ? e.message : "加载失败")); }} />} />
        )}
        {route.kind === "company" && (
          <Pane empty={folders.length === 0} emptyTitle="还没有分类">
            <NameTable
              nameHeader="分类"
              rows={folders.map((p) => ({
                key: p.id,
                href: `/library/${p.id}`,
                name: p.name,
              }))}
            />
          </Pane>
        )}
        {route.kind === "lines" && (
          <Pane empty={lines.length === 0} emptyTitle="还没有产品线">
            <NameTable
              nameHeader="产品线"
              rows={lines.map((l) => ({
                key: l.id,
                href: `/products/${l.id}`,
                name: l.name,
              }))}
            />
          </Pane>
        )}
        {route.kind === "line" && line && (
          <Pane empty={(productsByLine[line.id] ?? []).length === 0} emptyTitle="还没有产品">
            <NameTable
              nameHeader="产品"
              rows={(productsByLine[line.id] ?? []).map((p) => ({
                key: p.id,
                href: `/products/${line.id}/${p.id}`,
                name: p.name,
              }))}
            />
          </Pane>
        )}
        {(route.kind === "product" || (route.kind === "folder" && !route.versionId && versions.length !== 1)) && (
          <Pane empty={versions.length === 0} emptyTitle="还没有版本">
            <NameTable
              nameHeader="版本"
              rows={versions.map((v) => ({
                key: v.id,
                href: route.kind === "folder" ? `/library/${route.folderId}/${v.id}` : `/products/${route.lineId}/${route.productId}/${v.id}`,
                name: v.label,
                badge: v.current ? "当前" : undefined,
                badgeColor: "go",
              }))}
            />
          </Pane>
        )}
        {route.kind === "doc" && version?.id ? (
          <DocumentDetail docId={route.docId} backHref={assetVersionHref(route, version.id)} />
        ) : null}
        {(route.kind === "version" || (route.kind === "folder" && Boolean(version))) && (
          <>
            <Card>
              <FileInput label="上传资料文件" mode="dropzone" isMultiple value={null} onChange={(files) => { const list = Array.isArray(files) ? files : files ? [files] : []; if (list.length) void upload(list).catch((error) => notify(error instanceof Error ? error.message : "上传失败", true)); }} />
            </Card>
            <Card padding={0}>
              {docs.length === 0 ? (
                <EmptyState title="还没有文件" description="上传后系统会解析并建立检索，状态显示为「可检索」后即可在编制中引用。" />
              ) : (
                <NameTable
                  nameHeader="文件"
                  rows={docs.map((d) => {
                    const failed = d.error_message && /ocr_error|caption_error|vlm not configured/i.test(d.error_message);
                    return {
                      key: d.id || d.file_name,
                      href: version?.id ? assetDocHref(route, version.id, d.id) : undefined,
                      name: d.file_name || d.title,
                      desc: d.error_message ? `${failed ? "图片内容未能识别" : "处理失败"}：${d.error_message}` : undefined,
                      badge: failed ? "图像失败" : d.index_ready ? "可检索" : parseStatus(d.parse_status) || "解析中",
                      badgeColor: failed || /^failed$/.test(String(d.parse_status)) ? "stop" : d.index_ready ? "go" : "wait",
                    };
                  })}
                />
              )}
            </Card>
          </>
        )}
      </VStack>
      <Dialog isOpen={!!create} onOpenChange={(open) => { if (!open) setCreate(null); }}>
        <DialogHeader title={create?.kind === "folder" ? "新建分类" : create?.kind === "line" ? "新建产品线" : create?.kind === "product" ? "新建产品" : "新建版本"} onOpenChange={(open) => { if (!open) setCreate(null); }} />
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submitCreate();
            }}
          >
            <VStack gap={4}>
              <TextInput id="asset-name" label="名称" value={name} onChange={setName} hasAutoFocus />
              <HStack gap={2} hAlign="end">
                <Button variant="secondary" type="button" label="取消" onClick={() => setCreate(null)} />
                <Button type="submit" variant="primary" label="建立" />
              </HStack>
            </VStack>
          </form>
      </Dialog>
    </Shell>
  );
}

function Pane({
  empty,
  emptyTitle,
  children,
}: {
  empty: boolean;
  emptyTitle: string;
  children: ReactNode;
}) {
  return <Card padding={0}>{empty ? <EmptyState title={emptyTitle} description="用页头右上角的按钮新建。" /> : children}</Card>;
}

const badgeVariant = { sky: "info", gray: "neutral", go: "success", wait: "warning", stop: "error" } as const;

function NameTable({
  nameHeader,
  rows,
}: {
  nameHeader: string;
  rows: { key: string; href?: string; name: string; desc?: string; badge?: string; badgeColor?: keyof typeof badgeVariant }[];
}) {
  const name = { key: "name" as const, header: nameHeader, renderCell: (row: (typeof rows)[number]) => <VStack gap={0}>{row.href ? <Link href={`#${row.href}`}>{row.name}</Link> : <Text weight="semibold">{row.name}</Text>}{row.desc ? <Text type="supporting">{row.desc}</Text> : null}</VStack> };
  const status = { key: "badge" as const, header: "状态", width: pixel(140), renderCell: (row: (typeof rows)[number]) => row.badge ? <Badge variant={badgeVariant[row.badgeColor ?? "sky"]} label={row.badge} /> : null };
  return <Table hasHover idKey="key" data={rows} columns={rows.some((row) => row.badge) ? [name, status] : [name]} />;
}
