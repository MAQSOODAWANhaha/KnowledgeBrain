import { useEffect, useState } from "react";
import { Modal } from "@mantine/core";
import { Dropzone } from "@mantine/dropzone";
import { notifications } from "@mantine/notifications";
import {
  ApiError,
  type Product,
  type Project,
  type Workspace,
  api,
  setToken,
  slugify,
  token,
} from "./api";
import { Workbench } from "./bid/Workbench";
import { bidHref, go, parseBidRoute, useHash } from "./hash";
import { Shell } from "./Shell";

function toast(msg: string) {
  notifications.show({ message: msg, color: "iris" });
}

function parseStatus(s: string | object): string {
  return typeof s === "string" ? s : Object.keys(s as object)[0] ?? "";
}

function Login() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <div className="login">
      <header className="login-bar">
        <div className="mark">KB</div>
        <div className="brand">KnowledgeBrain</div>
      </header>
      <div className="login-body">
        <div className="login-copy">
          <h1>投标台</h1>
          <p className="lead">拆条款、勾产品、补图、出 ①–⑤。登录后先做这一标，缺证再进知识资产补。</p>
          <div className="login-ways">
            <div className="login-way">
              <i />
              <div>
                <b>投标项目</b>
                <p>建项、上传招标文件、确认条款、勾选型号、导出过程 Word / 定稿 PDF。</p>
              </div>
            </div>
            <div className="login-way">
              <i />
              <div>
                <b>知识资产</b>
                <p>资质证照、体系认证、业绩案例、服务能力，以及参与排序的型号手册。</p>
              </div>
            </div>
          </div>
        </div>
        <form
          className="login-card"
          onSubmit={async (e) => {
            e.preventDefault();
            setErr("");
            setBusy(true);
            try {
              const r = await api.login(email.trim(), password);
              setToken(r.token);
              go("/");
            } catch (ex) {
              setErr(ex instanceof ApiError ? "登录失败，请再试一次" : "网络错误");
            } finally {
              setBusy(false);
            }
          }}
        >
          <h2>进入</h2>
          <p className="note" style={{ margin: "0 0 28px" }}>
            LDAP 账号。测试环境账号密码可空。
          </p>
          <label className="fld">账号</label>
          <input className="inp" placeholder="账号" value={email} onChange={(e) => setEmail(e.target.value)} />
          <div style={{ height: 16 }} />
          <label className="fld">密码</label>
          <input className="inp" type="password" placeholder="密码" value={password} onChange={(e) => setPassword(e.target.value)} />
          {err && (
            <p className="note" style={{ color: "var(--rose)" }}>
              {err}
            </p>
          )}
          <button className="btn pri lg block" style={{ marginTop: 28, height: 44, fontSize: 15 }} type="submit" disabled={busy}>
            {busy ? "进入中…" : "进入投标台"}
          </button>
        </form>
      </div>
    </div>
  );
}

function Bids({ email }: { email: string }) {
  const [rows, setRows] = useState<Project[] | null>(null);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [owner, setOwner] = useState("");
  const [when, setWhen] = useState("");
  const [err, setErr] = useState("");
  const [filter, setFilter] = useState<"all" | "open" | "ended">("all");
  useEffect(() => {
    api
      .bids()
      .then(setRows)
      .catch(() => setRows([]));
  }, []);
  const shown = (rows ?? []).filter((p) => {
    if (filter === "open") return p.status !== "ended";
    if (filter === "ended") return p.status === "ended";
    return true;
  });

  return (
    <Shell
      root="bids"
      email={email}
      crumbs="投标项目 / 在办的标"
      title="在办的标"
      extra={
        <button className="btn pri" type="button" onClick={() => setOpen(true)}>
          新建标
        </button>
      }
      tree={
        <>
          <div className="side-sec">作业</div>
          <nav className="sidenav">
            <a className="on" href="#/">
              <svg viewBox="0 0 24 24">
                <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
              </svg>
              <em>在办的标</em>
              <span>{rows?.length ?? 0}</span>
            </a>
          </nav>
          {rows && rows.length > 0 && (
            <>
              <div className="side-sec">项目</div>
              <nav className="sidenav">
                {rows.map((p) => (
                  <a key={p.id} href={`#/bids/${p.id}`}>
                    <svg viewBox="0 0 24 24">
                      <rect x="3" y="4" width="18" height="16" rx="2" />
                      <path d="M8 4V3h8v1M8 10h8M8 14h5" />
                    </svg>
                    <em>{p.title}</em>
                    {p.status === "ended" && <span>已结束</span>}
                  </a>
                ))}
              </nav>
            </>
          )}
        </>
      }
    >
      <div className="wrap stack">
        {rows === null ? (
          <div className="card">加载中…</div>
        ) : rows.length === 0 ? (
          <div className="card">
            <div className="empty">
              <h2>还没有标</h2>
              <p className="note" style={{ margin: "0 0 20px" }}>
                先建一项，再把招标文件拖进「文件」。
              </p>
              <button className="btn pri" type="button" onClick={() => setOpen(true)}>
                新建标
              </button>
            </div>
          </div>
        ) : (
          <div className="card pad-0">
            <div className="toolbar">
              <input className="inp" placeholder="按项目名、负责人过滤…" />
              <button className={`chip ${filter === "all" ? "iris" : ""}`} type="button" onClick={() => setFilter("all")}>
                全部
              </button>
              <button className={`chip ${filter === "open" ? "iris" : ""}`} type="button" onClick={() => setFilter("open")}>
                在办
              </button>
              <button className={`chip ${filter === "ended" ? "iris" : ""}`} type="button" onClick={() => setFilter("ended")}>
                已结束
              </button>
            </div>
            <table className="grid">
              <thead>
                <tr>
                  <th>项目</th>
                  <th>负责人</th>
                  <th>招标结束</th>
                  <th>状态</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((p) => (
                  <tr key={p.id} onClick={() => go(`/bids/${p.id}`)} style={{ cursor: "pointer" }}>
                    <td>
                      <div className="name">{p.title}</div>
                    </td>
                    <td className="muted">{p.owner_name || "—"}</td>
                    <td className="muted">{p.expires_at ? p.expires_at.slice(0, 10) : "—"}</td>
                    <td>
                      {p.status === "ended" ? (
                        <span className="chip gray">已结束</span>
                      ) : (
                        <span className="chip iris">
                          <i className="dot" />
                          在办
                        </span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      <Modal opened={open} onClose={() => setOpen(false)} title="新建标" radius={16}>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (!title.trim()) {
              setErr("先写项目名称");
              return;
            }
            try {
              const p = await api.createBid({
                title: title.trim(),
                owner_name: owner.trim(),
                expires_at: when ? new Date(`${when}T16:00:00Z`).toISOString() : null,
              });
              go(bidHref(p.id, "files"));
            } catch (ex) {
              setErr(ex instanceof Error ? ex.message : "创建失败");
            }
          }}
        >
          <label className="fld">项目名称</label>
          <input className="inp" value={title} onChange={(e) => setTitle(e.target.value)} required />
          <label className="fld" style={{ marginTop: 16 }}>
            负责人
          </label>
          <input className="inp" value={owner} onChange={(e) => setOwner(e.target.value)} />
          <label className="fld" style={{ marginTop: 16 }}>
            招标结束日
          </label>
          <input className="inp" type="date" value={when} onChange={(e) => setWhen(e.target.value)} />
          {err && (
            <p className="note" style={{ color: "var(--rose)" }}>
              {err}
            </p>
          )}
          <div className="row" style={{ justifyContent: "flex-end", marginTop: 24 }}>
            <button className="btn" type="button" onClick={() => setOpen(false)}>
              取消
            </button>
            <button className="btn pri" type="submit">
              创建
            </button>
          </div>
        </form>
      </Modal>
    </Shell>
  );
}

function Library({ email, folderId }: { email: string; folderId: string | null }) {
  const [err, setErr] = useState("");
  const [products, setProducts] = useState<Product[]>([]);
  const [lines, setLines] = useState<Workspace[]>([]);
  const [docs, setDocs] = useState<Record<string, { file_name: string; index_ready: boolean; parse_status: string | object }[]>>({});
  async function reload() {
    try {
      const all = await api.workspaces();
      setLines(all.filter((w) => w.kind !== "company"));
      let ws = all.find((w) => w.kind === "company" || w.slug === "company");
      if (!ws) ws = await api.createWorkspace({ name: "公司资料", slug: "company", kind: "company" });
      let ps = await api.products(ws.id);
      if (ps.length === 0) {
        await Promise.all(["资质证照", "体系认证", "业绩案例", "服务能力"].map((name) => api.createProduct(ws.id, { name, slug: slugify(name), kind: "library" })));
        ps = await api.products(ws.id);
      }
      setProducts(ps);
      if (!folderId && ps[0]) {
        const prefer = ps.find((p) => p.name === "资质证照") ?? ps[0];
        go(`/library/${prefer.id}`);
      }
      const entries = await Promise.all(
        ps.map(async (p) => {
          let vid = p.current_version_id;
          if (!vid) {
            try {
              vid = (await api.createVersion(p.id, "current")).id;
            } catch {
              vid = null;
            }
          }
          const list = vid ? await api.documents(p.id, vid).catch(() => []) : [];
          return [p.id, list] as const;
        }),
      );
      setDocs(Object.fromEntries(entries));
    } catch (e) {
      setErr(e instanceof Error ? e.message : "加载失败");
    }
  }
  useEffect(() => {
    void reload();
  }, [folderId]);
  const folder = products.find((p) => p.id === folderId) ?? products[0] ?? null;
  const files = folder ? (docs[folder.id] ?? []) : [];
  return (
    <Shell
      root="assets"
      email={email}
      crumbs={`知识资产 / ${folder?.name ?? "公司资料"}`}
      title={folder?.name ?? "公司资料"}
      extra={
        <button className="btn pri" type="button" onClick={() => document.getElementById("lib-drop")?.click()}>
          上传材料
        </button>
      }
      tree={
        <>
          <div className="side-sec">公司资料</div>
          <nav className="sidenav">
            {products.map((p) => (
              <a key={p.id} className={p.id === folder?.id ? "on" : undefined} href={`#/library/${p.id}`}>
                <svg viewBox="0 0 24 24">
                  <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                </svg>
                <em>{p.name}</em>
                <span>{(docs[p.id] ?? []).length || undefined}</span>
              </a>
            ))}
          </nav>
          <div className="side-sec">产品线</div>
          <nav className="sidenav">
            {lines.map((l) => (
              <a key={l.id} href={`#/products/${l.id}`}>
                <svg viewBox="0 0 24 24">
                  <path d="M3 7.5 12 3l9 4.5v9L12 21l-9-4.5z" />
                  <path d="M12 12 3 7.5M12 12v9M12 12l9-4.5" />
                </svg>
                <em>{l.name}</em>
              </a>
            ))}
          </nav>
        </>
      }
    >
      <div className="wrap stack">
        {err && (
          <div className="banner bad">
            {err}{" "}
            <button className="btn sm" type="button" onClick={() => void reload()}>
              重试
            </button>
          </div>
        )}
        {!folder ? (
          <div className="card">加载中…</div>
        ) : (
          <>
            <Dropzone
              id="lib-drop"
              className="drop"
              multiple
              onDrop={(dropped) => {
                const vid = folder.current_version_id;
                if (!vid) return;
                void Promise.all(dropped.map((f) => api.ingest(folder.id, vid, f))).then(() => {
                  toast("已入库，解析完成后可检索");
                  void reload();
                });
              }}
            >
              <b>把证、案例、服务扫描件拖到这里</b>
              只进公司资料库。可检索之后才会被商务条款打到。这里不是产品。
            </Dropzone>
            <div className="card pad-0">
              {files.length === 0 ? (
                <div className="empty">
                  <h2>这个夹还是空的</h2>
                  <p className="note">拖入扫描件，等可检索后再回评估里确认。</p>
                </div>
              ) : (
                files.map((d) => (
                  <div key={d.file_name} className="item" style={{ gridTemplateColumns: "1fr auto" }}>
                    <div>
                      <div className="name">{d.file_name}</div>
                      <div className="desc">{folder.name}</div>
                    </div>
                    {d.index_ready ? (
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
    </Shell>
  );
}

function Products({ email, lineId }: { email: string; lineId: string | null }) {
  const [lines, setLines] = useState<Workspace[]>([]);
  const [folders, setFolders] = useState<Product[]>([]);
  const [name, setName] = useState("");
  const [lineName, setLineName] = useState("");
  const [catalog, setCatalog] = useState<{ line: Workspace; products: Product[] }[]>([]);
  const [newOpen, setNewOpen] = useState<"line" | "model" | null>(null);
  async function reload() {
    const all = await api.workspaces();
    const ls = all.filter((w) => w.kind !== "company");
    setLines(ls);
    const company = all.find((w) => w.kind === "company");
    if (company) setFolders(await api.products(company.id).catch(() => []));
    if (!lineId && ls[0]) go(`/products/${ls[0].id}`);
    const rows = await Promise.all(ls.map(async (line) => ({ line, products: (await api.products(line.id)).filter((p) => p.kind !== "library") })));
    setCatalog(rows);
  }
  useEffect(() => {
    void reload();
  }, [lineId]);
  const line = lines.find((l) => l.id === lineId) ?? lines[0] ?? null;
  const products = catalog.find((c) => c.line.id === line?.id)?.products ?? [];
  return (
    <Shell
      root="assets"
      email={email}
      crumbs={`知识资产 / ${line?.name ?? "产品线"}`}
      title={line?.name ?? "产品线"}
      extra={
        <>
          <button className="btn" type="button" onClick={() => setNewOpen("line")}>
            建线
          </button>
          <button className="btn pri" type="button" onClick={() => setNewOpen("model")}>
            新型号
          </button>
        </>
      }
      tree={
        <>
          <div className="side-sec">公司资料</div>
          <nav className="sidenav">
            {folders.map((p) => (
              <a key={p.id} href={`#/library/${p.id}`}>
                <svg viewBox="0 0 24 24">
                  <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                </svg>
                <em>{p.name}</em>
              </a>
            ))}
          </nav>
          <div className="side-sec">产品线</div>
          <nav className="sidenav">
            {lines.map((l) => (
              <a key={l.id} className={l.id === line?.id ? "on" : undefined} href={`#/products/${l.id}`}>
                <svg viewBox="0 0 24 24">
                  <path d="M3 7.5 12 3l9 4.5v9L12 21l-9-4.5z" />
                  <path d="M12 12 3 7.5M12 12v9M12 12l9-4.5" />
                </svg>
                <em>{l.name}</em>
                <span>{catalog.find((c) => c.line.id === l.id)?.products.length || undefined}</span>
              </a>
            ))}
          </nav>
        </>
      }
    >
      <div className="wrap stack">
        <div className="card pad-0">
          {products.length === 0 ? (
            <div className="empty">
              <h2>还没有型号</h2>
              <p className="note">建一个型号，再把手册拖上去。招标文件不要放这里。</p>
            </div>
          ) : (
            <table className="grid">
              <thead>
                <tr>
                  <th>型号</th>
                  <th>手册</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {products.map((p) => (
                  <tr key={p.id}>
                    <td>
                      <div className="name">{p.name}</div>
                    </td>
                    <td>
                      <Dropzone
                        className="drop"
                        style={{ padding: 10 }}
                        multiple={false}
                        onDrop={(files) => {
                          const vid = p.current_version_id;
                          const f = files[0];
                          if (!f || !vid) return;
                          void api.ingest(p.id, vid, f).then(() => toast("手册已上传"));
                        }}
                      >
                        上传手册
                      </Dropzone>
                    </td>
                    <td />
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="drop">
          <b>手册进型号，不进资料</b>
          招标文件不要放这里。
        </div>
      </div>
      <Modal opened={newOpen === "line"} onClose={() => setNewOpen(null)} title="建线" radius={16}>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (!lineName.trim()) return;
            const created = await api.createWorkspace({ name: lineName.trim(), slug: slugify(lineName), kind: "product_line" });
            setLineName("");
            setNewOpen(null);
            go(`/products/${created.id}`);
          }}
        >
          <label className="fld">产品线名称</label>
          <input className="inp" value={lineName} onChange={(e) => setLineName(e.target.value)} />
          <div className="row" style={{ justifyContent: "flex-end", marginTop: 20 }}>
            <button className="btn" type="button" onClick={() => setNewOpen(null)}>
              取消
            </button>
            <button className="btn pri" type="submit">
              建立
            </button>
          </div>
        </form>
      </Modal>
      <Modal opened={newOpen === "model"} onClose={() => setNewOpen(null)} title="新型号" radius={16}>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (!name.trim()) return;
            let current = line;
            if (!current) {
              current = await api.createWorkspace({ name: name.trim(), slug: slugify(name), kind: "product_line" });
            }
            await api.createProduct(current.id, { name: name.trim(), slug: slugify(name), kind: "product" });
            setName("");
            setNewOpen(null);
            toast("已建型号");
            void reload();
          }}
        >
          <label className="fld">型号名称</label>
          <input className="inp" value={name} onChange={(e) => setName(e.target.value)} />
          <div className="row" style={{ justifyContent: "flex-end", marginTop: 20 }}>
            <button className="btn" type="button" onClick={() => setNewOpen(null)}>
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

export function App() {
  const path = useHash();
  const [email, setEmail] = useState("");
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (path === "/login") {
      setReady(true);
      return;
    }
    if (!token()) {
      go("/login");
      setReady(true);
      return;
    }
    api
      .me()
      .then((m) => {
        setEmail(m.email);
        setReady(true);
      })
      .catch(() => {
        setToken(null);
        go("/login");
        setReady(true);
      });
  }, [path]);

  if (!ready) {
    return (
      <div className="login">
        <header className="login-bar">
          <div className="mark">KB</div>
          <div className="brand">KnowledgeBrain</div>
        </header>
      </div>
    );
  }
  if (path === "/login") return <Login />;
  const lib = path.match(/^\/library(?:\/([^/]+))?$/);
  if (lib) return <Library email={email} folderId={lib[1] ?? null} />;
  const prod = path.match(/^\/products(?:\/([^/]+))?$/);
  if (prod) return <Products email={email} lineId={prod[1] ?? null} />;
  if (parseBidRoute(path)) return <Workbench email={email} />;
  return <Bids email={email} />;
}
