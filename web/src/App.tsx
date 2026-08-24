import { useEffect, useState } from "react";
import { Button, Modal, TextInput } from "@mantine/core";
import { ApiError, type Project, api, setToken, token } from "./api";
import { Assets } from "./assets/Assets";
import { Workbench } from "./bid/Workbench";
import { shanghaiEndOfDay } from "./bid/helpers";
import { bidHref, go, parseAssetRoute, parseBidRoute, useHash } from "./hash";
import { Crumbs } from "./Crumbs";
import { Shell } from "./Shell";

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
        <form
          className="login-card"
          data-testid="login-form"
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
          <input
            className="inp"
            data-testid="login-email"
            placeholder="账号"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <div style={{ height: 16 }} />
          <label className="fld">密码</label>
          <input
            className="inp"
            data-testid="login-password"
            type="password"
            placeholder="密码"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          {err && (
            <p className="note" style={{ color: "var(--rose)" }}>
              {err}
            </p>
          )}
          <Button type="submit" fullWidth mt={28} h={44} disabled={busy} data-testid="login-submit">
            {busy ? "进入中…" : "进入"}
          </Button>
        </form>
      </div>
    </div>
  );
}

function Bids({ email }: { email: string }) {
  const [rows, setRows] = useState<Project[] | null>(null);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
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
      crumbs={<Crumbs items={[{ label: "投标项目" }, { label: "在办的标" }]} />}
      title="在办的标"
      extra={
        <Button data-testid="new-bid" onClick={() => setOpen(true)}>
          新建标
        </Button>
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
                  <a key={p.id} href={`#${bidHref(p.id, "files", { step: "files" })}`}>
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
              <Button onClick={() => setOpen(true)}>新建标</Button>
            </div>
          </div>
        ) : (
          <div className="card pad-0">
            <div className="toolbar">
              <input className="inp" placeholder="按项目名过滤…" />
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
                  <th>招标结束</th>
                  <th>状态</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((p) => (
                  <tr key={p.id} onClick={() => go(bidHref(p.id, "files", { step: "files" }))} style={{ cursor: "pointer" }}>
                    <td>
                      <div className="name">{p.title}</div>
                    </td>
                    <td className="muted">{p.ends_at ? p.ends_at.slice(0, 10) : "—"}</td>
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
      <Modal opened={open} onClose={() => setOpen(false)} title="新建标">
        <form
          data-testid="create-bid-form"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!title.trim()) {
              setErr("先写项目名称");
              return;
            }
            if (!when) {
              setErr("先选招标结束日");
              return;
            }
            try {
              const p = await api.createBid({
                title: title.trim(),
                ends_at: shanghaiEndOfDay(when),
              });
              go(bidHref(p.id, "files", { step: "files" }));
            } catch (ex) {
              setErr(ex instanceof Error ? ex.message : "创建失败");
            }
          }}
        >
          <TextInput data-testid="bid-title" label="项目名称" value={title} onChange={(e) => setTitle(e.currentTarget.value)} required />
          <TextInput
            data-testid="bid-ends"
            label="招标结束日"
            type="date"
            mt="md"
            value={when}
            onChange={(e) => setWhen(e.currentTarget.value)}
            required
          />
          {err && (
            <p className="note" style={{ color: "var(--rose)" }}>
              {err}
            </p>
          )}
          <div className="row" style={{ justifyContent: "flex-end", marginTop: 24 }}>
            <Button variant="default" type="button" onClick={() => setOpen(false)}>
              取消
            </Button>
            <Button type="submit" data-testid="bid-create">
              创建
            </Button>
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
  const asset = parseAssetRoute(path);
  if (asset) return <Assets email={email} route={asset} />;
  if (parseBidRoute(path)) return <Workbench email={email} />;
  return <Bids email={email} />;
}
