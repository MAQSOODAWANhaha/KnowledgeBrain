import { useEffect, useState } from "react";
import {
  Button,
  Modal,
  PasswordInput,
  SegmentedControl,
  Skeleton,
  TextInput,
} from "@mantine/core";
import { ApiError, api, setToken, token } from "./api";
import { Assets } from "./assets/Assets";
import { createBidV2Client, type BidProjectView } from "./bid/api";
import { authoringHref } from "./bid/authoring/routes";
import { Workbench } from "./bid/Workbench";
import { shanghaiEndOfDay } from "./bid/helpers";
import { go, parseAssetRoute, parseBidRoute, useHash } from "./hash";

const bidApi = createBidV2Client();
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
              const r = await api.login(email.trim() || "dev@local", password);
              setToken(r.token);
              go("/");
            } catch (ex) {
              setErr(
                ex instanceof ApiError
                  ? "登录失败，请再试一次"
                  : ex instanceof Error
                    ? ex.message
                    : "网络错误",
              );
            } finally {
              setBusy(false);
            }
          }}
        >
          <h2>进入</h2>
          <p className="note" style={{ margin: "0 0 28px" }}>
            LDAP 账号。测试环境账号密码可空。
          </p>
          <TextInput
            data-testid="login-email"
            label="账号"
            placeholder="账号"
            value={email}
            onChange={(e) => setEmail(e.currentTarget.value)}
          />
          <PasswordInput
            data-testid="login-password"
            label="密码"
            mt="md"
            placeholder="密码"
            value={password}
            onChange={(e) => setPassword(e.currentTarget.value)}
          />
          {err && (
            <p className="note" style={{ color: "var(--rose)" }}>
              {err}
            </p>
          )}
          <Button
            type="submit"
            fullWidth
            mt={28}
            h={44}
            disabled={busy}
            data-testid="login-submit"
          >
            {busy ? "进入中…" : "进入"}
          </Button>
        </form>
      </div>
    </div>
  );
}

function Bids({ email }: { email: string }) {
  const [rows, setRows] = useState<BidProjectView[] | null>(null);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [when, setWhen] = useState("");
  const [err, setErr] = useState("");
  const [filter, setFilter] = useState<"all" | "open" | "ended">("all");
  const [query, setQuery] = useState("");
  useEffect(() => {
    bidApi
      .listProjects()
      .then(setRows)
      .catch(() => setRows([]));
  }, []);
  const shown = (rows ?? []).filter((p) => {
    if (
      query.trim() &&
      !p.title.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
    )
      return false;
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
                  <a key={p.id} href={`#${authoringHref(p.id, "files")}`}>
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
          <div className="card stack">
            <Skeleton height={48} radius="md" />
            <Skeleton height={48} radius="md" />
            <Skeleton height={48} radius="md" />
          </div>
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
              <TextInput
                placeholder="按项目名过滤…"
                value={query}
                onChange={(event) => setQuery(event.currentTarget.value)}
                style={{ flex: 1 }}
              />
              <SegmentedControl
                value={filter}
                onChange={(value) => setFilter(value as typeof filter)}
                data={[
                  { value: "all", label: "全部" },
                  { value: "open", label: "在办" },
                  { value: "ended", label: "已结束" },
                ]}
              />
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
                  <tr
                    key={p.id}
                    onClick={() => go(authoringHref(p.id, "files"))}
                    style={{ cursor: "pointer" }}
                  >
                    <td>
                      <div className="name">{p.title}</div>
                    </td>
                    <td className="muted">
                      {p.ends_at ? p.ends_at.slice(0, 10) : "—"}
                    </td>
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
              const p = await bidApi.createProject({
                title: title.trim(),
                ends_at: shanghaiEndOfDay(when),
              });
              go(authoringHref(p.id, "files"));
            } catch (ex) {
              setErr(ex instanceof Error ? ex.message : "创建失败");
            }
          }}
        >
          <TextInput
            data-testid="bid-title"
            label="项目名称"
            value={title}
            onChange={(e) => setTitle(e.currentTarget.value)}
            required
          />
          <TextInput label="负责人" value={email} mt="md" readOnly />
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
          <div
            className="row"
            style={{ justifyContent: "flex-end", marginTop: 24 }}
          >
            <Button
              variant="default"
              type="button"
              onClick={() => setOpen(false)}
            >
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
