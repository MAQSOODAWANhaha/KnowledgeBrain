import { type ReactNode, useEffect, useMemo, useState } from "react";
import { Modal } from "@mantine/core";
import { Dropzone } from "@mantine/dropzone";
import { notifications } from "@mantine/notifications";
import {
  type Doc,
  type Product,
  type Version,
  type Workspace,
  api,
  slugify,
} from "../api";
import { type AssetRoute, go } from "../hash";
import { Shell } from "../Shell";

function toast(msg: string, color: "iris" | "red" = "iris") {
  notifications.show({ message: msg, color });
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

  async function ensureVersions(pid: string): Promise<Version[]> {
    if (versionsByProduct[pid]) return versionsByProduct[pid];
    const list = await api.versions(pid);
    const live = list.filter((v) => v.status !== "archived");
    setVersionsByProduct((cur) => ({ ...cur, [pid]: live }));
    return live;
  }

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
    for (const id of [...open].filter((k) => k.startsWith("folder:") || k.startsWith("product:"))) {
      pids.push(id.split(":")[1]);
    }
    void Promise.all([...new Set(pids)].map((id) => ensureVersions(id).catch(() => [])));
  }, [route, open]);

  const selectedProductId =
    route.kind === "folder" ? route.folderId : route.kind === "product" || route.kind === "version" ? route.productId : null;
  const selectedVersionId =
    route.kind === "folder" ? route.versionId : route.kind === "version" ? route.versionId : undefined;

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
  }, [selectedProductId, selectedVersionId]);

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
        await api.createVersion(p.id, "current");
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
        const v = await api.createVersion(p.id, "current");
        setCreate(null);
        setName("");
        await reloadTree();
        go(`/products/${lineId}/${p.id}/${v.id}`);
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
      toast(e instanceof Error ? e.message : "创建失败", "red");
    }
  }

  const folder = folders.find((p) => p.id === (route.kind === "folder" ? route.folderId : "")) ?? null;
  const line = lines.find((l) => "lineId" in route && l.id === route.lineId) ?? null;
  const product =
    line && (route.kind === "product" || route.kind === "version")
      ? (productsByLine[line.id] ?? []).find((p) => p.id === route.productId) ?? null
      : null;
  const versions = selectedProductId ? (versionsByProduct[selectedProductId] ?? []) : [];
  const version =
    selectedVersionId ? versions.find((v) => v.id === selectedVersionId) ?? null : versions.find((v) => v.current) ?? versions[0] ?? null;

  const crumbs = useMemo(() => {
    if (route.kind === "company") return "知识资产 / 公司资料";
    if (route.kind === "folder") return `知识资产 / 公司资料 / ${folder?.name ?? "分类"}`;
    if (route.kind === "lines") return "知识资产 / 产品线";
    if (route.kind === "line") return `知识资产 / 产品线 / ${line?.name ?? "线"}`;
    if (route.kind === "product") return `知识资产 / ${line?.name ?? "产品线"} / ${product?.name ?? "产品"}`;
    return `知识资产 / ${line?.name ?? "产品线"} / ${product?.name ?? "产品"} / ${version?.label ?? "版本"}`;
  }, [route, folder, line, product, version]);

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
    let vid = version?.id;
    if (!pid) return;
    if (!vid) {
      const created = await api.createVersion(pid, "current");
      vid = created.id;
      setVersionsByProduct((cur) => ({ ...cur, [pid]: [created] }));
    }
    await Promise.all(files.map((f) => api.ingest(pid, vid, f)));
    toast("已入库，解析完成后可检索");
    const list = await api.documents(pid, vid).catch(() => []);
    setDocs(list);
    await reloadTree();
  }

  const selected = keyOf(route);

  return (
    <Shell
      root="assets"
      email={email}
      crumbs={crumbs}
      title={title}
      extra={
        route.kind === "version" || (route.kind === "folder" && version) ? (
          <button className="btn pri" type="button" onClick={() => document.getElementById("asset-drop")?.click()}>
            上传
          </button>
        ) : (
          <button className="btn pri" type="button" onClick={openCreateFromSelection}>
            {createHint}
          </button>
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
          <div className="banner bad">
            {err}{" "}
            <button className="btn sm" type="button" onClick={() => void reloadTree()}>
              重试
            </button>
          </div>
        )}
        {route.kind === "company" && (
          <Pane
            title="公司资料"
            note="证照、体系、业绩、服务能力。分类夹不是产品型号。可检索后才会被商务条款打到。"
            empty={folders.length === 0}
            emptyTitle="还没有分类"
            action="新建分类"
            onAction={() => setCreate({ kind: "folder" })}
          >
            {folders.map((p) => (
              <a key={p.id} className="item" href={`#/library/${p.id}`} style={{ gridTemplateColumns: "1fr auto" }}>
                <div>
                  <div className="name">{p.name}</div>
                  <div className="desc">点开后上传扫描件</div>
                </div>
                <span className="chip gray">分类</span>
              </a>
            ))}
          </Pane>
        )}
        {route.kind === "lines" && (
          <Pane
            title="产品线"
            note="产品线只是分类。型号、手册和版本挂在线下面，不要把招标文件丢进来。"
            empty={lines.length === 0}
            emptyTitle="还没有产品线"
            action="新建产品线"
            onAction={() => setCreate({ kind: "line" })}
          >
            {lines.map((l) => (
              <a key={l.id} className="item" href={`#/products/${l.id}`} style={{ gridTemplateColumns: "1fr auto" }}>
                <div>
                  <div className="name">{l.name}</div>
                  <div className="desc">{(productsByLine[l.id] ?? []).length} 个产品</div>
                </div>
                <span className="chip gray">产品线</span>
              </a>
            ))}
          </Pane>
        )}
        {route.kind === "line" && line && (
          <Pane
            title={line.name}
            note="在这条线下建产品，再给产品建版本、传手册。"
            empty={(productsByLine[line.id] ?? []).length === 0}
            emptyTitle="还没有产品"
            action="新建产品"
            onAction={() => setCreate({ kind: "product", parentId: line.id })}
          >
            {(productsByLine[line.id] ?? []).map((p) => (
              <a key={p.id} className="item" href={`#/products/${line.id}/${p.id}`} style={{ gridTemplateColumns: "1fr auto" }}>
                <div>
                  <div className="name">{p.name}</div>
                  <div className="desc">点开看版本</div>
                </div>
                <span className="chip gray">产品</span>
              </a>
            ))}
          </Pane>
        )}
        {(route.kind === "product" || (route.kind === "folder" && !route.versionId && versions.length !== 1)) && (
          <Pane
            title="版本"
            note={route.kind === "folder" ? "换证可以开新版本。当前版本才进商务检索。" : "发版开新版本。匹配默认打当前版本。"}
            empty={versions.length === 0}
            emptyTitle="还没有版本"
            action="新建版本"
            onAction={() => setCreate({ kind: "version", parentId: selectedProductId ?? undefined })}
          >
            {versions.map((v) => (
              <a
                key={v.id}
                className="item"
                href={`#${route.kind === "folder" ? `/library/${route.folderId}/${v.id}` : `/products/${route.lineId}/${route.productId}/${v.id}`}`}
                style={{ gridTemplateColumns: "1fr auto" }}
              >
                <div>
                  <div className="name">{v.label}</div>
                  <div className="desc">{v.status}</div>
                </div>
                {v.current ? (
                  <span className="chip pine">
                    <i className="dot" />
                    当前
                  </span>
                ) : (
                  <span className="chip gray">版本</span>
                )}
              </a>
            ))}
          </Pane>
        )}
        {(route.kind === "version" || (route.kind === "folder" && (route.versionId || versions.length <= 1))) && (
          <>
            <Dropzone
              id="asset-drop"
              className="drop"
              multiple
              onDrop={(files) => {
                void upload(files).catch((e) => toast(e instanceof Error ? e.message : "上传失败", "red"));
              }}
            >
              <b>{route.kind === "folder" || folder ? "把证、案例、服务扫描件拖到这里" : "把手册或界面图拖到这里"}</b>
              {route.kind === "folder" || folder
                ? "只进公司资料。可检索之后才会被商务条款打到。"
                : "手册进这个版本。招标文件不要放这里。"}
            </Dropzone>
            <div className="card pad-0">
              {docs.length === 0 ? (
                <div className="empty">
                  <h2>这个版本还是空的</h2>
                  <p className="note">拖入文件，等可检索后再回评估里确认或勾选。</p>
                </div>
              ) : (
                docs.map((d) => (
                  <div key={d.id || d.file_name} className="item" style={{ gridTemplateColumns: "1fr auto" }}>
                    <div>
                      <div className="name">{d.file_name || d.title}</div>
                      <div className="desc">{d.error_message || version?.label}</div>
                    </div>
                    {d.error_message && /ocr_error|caption_error|vlm not configured/i.test(d.error_message) ? (
                      <span className="chip rose">
                        <i className="dot" />
                        图像失败
                      </span>
                    ) : d.index_ready ? (
                      <span className="chip pine">
                        <i className="dot" />
                        可检索
                      </span>
                    ) : (
                      <span className="chip amber">
                        <i className="dot" />
                        {parseStatus(d.parse_status) || "解析中"}
                      </span>
                    )}
                  </div>
                ))
              )}
            </div>
          </>
        )}
      </div>
      <Modal
        opened={!!create}
        onClose={() => setCreate(null)}
        title={
          create?.kind === "folder"
            ? "新建分类"
            : create?.kind === "line"
              ? "新建产品线"
              : create?.kind === "product"
                ? "新建产品"
                : "新建版本"
        }
        radius={16}
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submitCreate();
          }}
        >
          <label className="fld">名称</label>
          <input className="inp" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          <div className="row" style={{ justifyContent: "flex-end", marginTop: 20 }}>
            <button className="btn" type="button" onClick={() => setCreate(null)}>
              取消
            </button>
            <button className="btn pri" type="submit">
              建立
            </button>
          </div>
        </form>
      </Modal>
    </Shell>
  );
}

function Pane({
  title,
  note,
  empty,
  emptyTitle,
  action,
  onAction,
  children,
}: {
  title: string;
  note: string;
  empty: boolean;
  emptyTitle: string;
  action: string;
  onAction: () => void;
  children: ReactNode;
}) {
  return (
    <div className="card pad-0">
      <div className="group-h">
        <span>{title}</span>
        <button className="btn sm" type="button" onClick={onAction}>
          {action}
        </button>
      </div>
      <p className="note" style={{ margin: "0 18px 12px" }}>
        {note}
      </p>
      {empty ? (
        <div className="empty">
          <h2>{emptyTitle}</h2>
          <button className="btn pri" type="button" onClick={onAction}>
            {action}
          </button>
        </div>
      ) : (
        children
      )}
    </div>
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
