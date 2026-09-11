import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { IconCloudUpload } from "@tabler/icons-react";
import { toast } from "sonner";
import { Alert } from "../components/ui/alert";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Dialog, DialogContent } from "../components/ui/dialog";
import { Input } from "../components/ui/input";
import { Label } from "../components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/ui/table";
import {
  type Doc,
  type Product,
  type Version,
  type Workspace,
  api,
  slugify,
} from "../api";
import { Crumbs, type Crumb } from "../Crumbs";
import { type AssetRoute, assetDocHref, assetVersionHref, go } from "../hash";
import { Shell } from "../Shell";
import { DocumentDetail } from "./DocumentDetail";

function notify(msg: string, error = false) {
  if (error) toast.error(msg);
  else toast.success(msg);
}

function parseStatus(s: string | object): string {
  return typeof s === "string" ? s : Object.keys(s as object)[0] ?? "";
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

  function toggle(id: string) {
    setOpen((cur) => {
      const next = new Set(cur);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

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
          : route.kind === "product" || route.kind === "folder"
            ? "新建版本"
            : "上传文档";

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

  return (
    <Shell
      root="assets"
      email={email}
      crumbs={<Crumbs items={crumbs} />}
      title={title}
      extra={
        route.kind === "doc" ? undefined : route.kind === "version" || (route.kind === "folder" && version) ? (
          <Button type="button" onClick={() => document.getElementById("asset-file")?.click()}>
            上传
          </Button>
        ) : (
          <Button type="button" onClick={openCreateFromSelection}>
            {createHint}
          </Button>
        )
      }
      tree={
        <nav className="tree">
          <TreeRow
            depth={0}
            label="公司资料"
            href="/library"
            selected={selected === "company"}
            expanded={open.has("company")}
            onToggle={() => toggle("company")}
            onAdd={() => setCreate({ kind: "folder" })}
            addTitle="新建分类"
          />
          {open.has("company") &&
            folders.map((p) => {
              const vs = versionsByProduct[p.id] ?? [];
              const showVersions = vs.length > 1;
              return (
                <div key={p.id}>
                  <TreeRow
                    depth={1}
                    label={p.name}
                    href={`/library/${p.id}`}
                    selected={selected === `folder:${p.id}` || selected.startsWith(`folder:${p.id}:`)}
                    expanded={open.has(`folder:${p.id}`)}
                    onToggle={() => toggle(`folder:${p.id}`)}
                    onAdd={() => setCreate({ kind: "version", parentId: p.id })}
                    addTitle="新建版本"
                  />
                  {open.has(`folder:${p.id}`) &&
                    showVersions &&
                    vs.map((v) => (
                      <TreeRow
                        key={v.id}
                        depth={2}
                        label={v.current ? `${v.label} · 当前` : v.label}
                        href={`/library/${p.id}/${v.id}`}
                        selected={route.kind === "folder" && route.versionId === v.id}
                        leaf
                      />
                    ))}
                </div>
              );
            })}
          <TreeRow
            depth={0}
            label="产品线"
            href="/products"
            selected={selected === "lines"}
            expanded={open.has("lines")}
            onToggle={() => toggle("lines")}
            onAdd={() => setCreate({ kind: "line" })}
            addTitle="新建产品线"
          />
          {open.has("lines") &&
            lines.map((l) => (
              <div key={l.id}>
                <TreeRow
                  depth={1}
                  label={l.name}
                  href={`/products/${l.id}`}
                  selected={selected === `line:${l.id}` || ("lineId" in route && route.lineId === l.id)}
                  expanded={open.has(`line:${l.id}`)}
                  onToggle={() => toggle(`line:${l.id}`)}
                  onAdd={() => setCreate({ kind: "product", parentId: l.id })}
                  addTitle="新建产品"
                  count={(productsByLine[l.id] ?? []).length || undefined}
                />
                {open.has(`line:${l.id}`) &&
                  (productsByLine[l.id] ?? []).map((p) => (
                    <div key={p.id}>
                      <TreeRow
                        depth={2}
                        label={p.name}
                        href={`/products/${l.id}/${p.id}`}
                        selected={selected === `product:${p.id}` || (selected.startsWith("version:") && product?.id === p.id)}
                        expanded={open.has(`product:${p.id}`)}
                        onToggle={() => toggle(`product:${p.id}`)}
                        onAdd={() => setCreate({ kind: "version", parentId: p.id })}
                        addTitle="新建版本"
                      />
                      {open.has(`product:${p.id}`) &&
                        (versionsByProduct[p.id] ?? []).map((v) => (
                          <TreeRow
                            key={v.id}
                            depth={3}
                            label={v.current ? `${v.label} · 当前` : v.label}
                            href={`/products/${l.id}/${p.id}/${v.id}`}
                            selected={route.kind === "version" && route.versionId === v.id}
                            leaf
                          />
                        ))}
                    </div>
                  ))}
              </div>
            ))}
        </nav>
      }
    >
      <div className="wrap stack">
        {err && (
          <Alert>
            {err}
            <Button size="sm" className="mt-2" onClick={() => void reloadTree()}>
              重试
            </Button>
          </Alert>
        )}
        {route.kind === "company" && (
          <Pane empty={folders.length === 0} emptyTitle="还没有分类">
            <NameTable
              nameHeader="分类"
              rows={folders.map((p) => ({
                key: p.id,
                href: `/library/${p.id}`,
                name: p.name,
                badge: "分类",
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
                badge: "产品线",
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
                badge: "产品",
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
                badge: v.current ? "当前" : "版本",
                badgeColor: v.current ? "go" : "gray",
              }))}
            />
          </Pane>
        )}
        {route.kind === "doc" && version?.id ? (
          <DocumentDetail docId={route.docId} backHref={assetVersionHref(route, version.id)} />
        ) : null}
        {(route.kind === "version" || (route.kind === "folder" && Boolean(version))) && (
          <>
            <div
              id="asset-drop"
              className="drop"
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                void upload(Array.from(e.dataTransfer.files)).catch((err) =>
                  notify(err instanceof Error ? err.message : "上传失败", true),
                );
              }}
              onClick={() => document.getElementById("asset-file")?.click()}
            >
              <div className={docs.length === 0 ? "flex items-center justify-center gap-3 py-7" : "flex items-center justify-center gap-3 py-2"}>
                <span className="grid h-10 w-10 place-items-center rounded-md bg-sky-wash text-sky">
                  <IconCloudUpload size={22} />
                </span>
                <div className="font-semibold">把文件拖到这里</div>
              </div>
              <input
                id="asset-file"
                type="file"
                multiple
                hidden
                onChange={(e) => {
                  const list = e.target.files;
                  if (list?.length) void upload(Array.from(list)).catch((err) => notify(err instanceof Error ? err.message : "上传失败", true));
                  e.target.value = "";
                }}
              />
            </div>
            <div className="panel">
              {docs.length === 0 ? (
                <div className="empty">
                  <h2>还没有文件</h2>
                </div>
              ) : (
                <NameTable
                  nameHeader="文件"
                  rows={docs.map((d) => {
                    const failed = d.error_message && /ocr_error|caption_error|vlm not configured/i.test(d.error_message);
                    return {
                      key: d.id || d.file_name,
                      href: version?.id ? assetDocHref(route, version.id, d.id) : undefined,
                      name: d.file_name || d.title,
                      desc: d.error_message || undefined,
                      badge: failed ? "图像失败" : d.index_ready ? "可检索" : parseStatus(d.parse_status) || "解析中",
                      badgeColor: failed ? "stop" : d.index_ready ? "go" : "wait",
                    };
                  })}
                />
              )}
            </div>
          </>
        )}
      </div>
      <Dialog open={!!create} onOpenChange={(open) => { if (!open) setCreate(null); }}>
        <DialogContent
          title={
            create?.kind === "folder"
              ? "新建分类"
              : create?.kind === "line"
                ? "新建产品线"
                : create?.kind === "product"
                  ? "新建产品"
                  : "新建版本"
          }
        >
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submitCreate();
            }}
          >
            <Label htmlFor="asset-name">名称</Label>
            <Input
              id="asset-name"
              value={name}
              onChange={(e) => setName(e.currentTarget.value)}
              autoFocus
            />
            <div className="mt-5 flex justify-end gap-2">
              <Button variant="outline" type="button" onClick={() => setCreate(null)}>
                取消
              </Button>
              <Button type="submit">建立</Button>
            </div>
          </form>
        </DialogContent>
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
  return (
    <div className="panel">
      {empty ? (
        <div className="empty">
          <h2>{emptyTitle}</h2>
        </div>
      ) : (
        children
      )}
    </div>
  );
}

function NameTable({
  nameHeader,
  rows,
}: {
  nameHeader: string;
  rows: { key: string; href?: string; name: string; desc?: string; badge: string; badgeColor?: "sky" | "gray" | "go" | "wait" | "stop" }[];
}) {
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>{nameHeader}</TableHead>
          <TableHead className="w-[88px]">状态</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => (
          <TableRow
            key={row.key}
            className={row.href ? "cursor-pointer" : undefined}
            onClick={row.href ? () => go(row.href as string) : undefined}
          >
            <TableCell>
              <div className="font-semibold">{row.name}</div>
              {row.desc ? <div className="text-sm text-quiet">{row.desc}</div> : null}
            </TableCell>
            <TableCell>
              <Badge tone={row.badgeColor ?? "gray"}>{row.badge}</Badge>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function TreeRow({
  depth,
  label,
  href,
  selected,
  expanded,
  leaf,
  count,
  addTitle,
  onToggle,
  onAdd,
}: {
  depth: number;
  label: string;
  href: string;
  selected: boolean;
  expanded?: boolean;
  leaf?: boolean;
  count?: number;
  addTitle?: string;
  onToggle?: () => void;
  onAdd?: () => void;
}) {
  return (
    <div className={`tree-row ${selected ? "on" : ""}`} style={{ ["--d" as string]: depth }}>
      {leaf ? (
        <span className="tree-leaf" />
      ) : (
        <button
          className={`tree-chev ${expanded ? "open" : ""}`}
          type="button"
          aria-label={expanded ? "收起" : "展开"}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onToggle?.();
          }}
        >
          <svg viewBox="0 0 24 24">
            <path d="M9 6l6 6-6 6" />
          </svg>
        </button>
      )}
      <a href={`#${href}`}>
        <em>{label}</em>
        {count ? <span>{count}</span> : null}
      </a>
      {onAdd && (
        <button
          className="tree-add"
          type="button"
          title={addTitle}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onAdd();
          }}
        >
          +
        </button>
      )}
    </div>
  );
}
