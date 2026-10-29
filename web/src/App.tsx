import { useEffect, useState } from "react";
import { ApiError, api, setToken, token } from "./api";
import { Assets } from "./assets/Assets";
import { type BidProjectView } from "./bid/api";
import { tenderApi } from "./bid/api/workbench";
import { authoringHref } from "./bid/authoring/routes";
import { BidTree } from "./bid/BidTree";
import { Workbench } from "./bid/Workbench";
import { Alert } from "./components/ui/alert";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Dialog, DialogContent } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { Skeleton } from "./components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "./components/ui/table";
import { Crumbs } from "./Crumbs";
import { go, parseAssetRoute, parseBidRoute, useHash } from "./hash";
import { cn } from "./lib/utils";
import { Shell } from "./Shell";

const bidApi = tenderApi;

function Login() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <div className="grid min-h-dvh place-items-center bg-[radial-gradient(circle_at_50%_36%,rgba(37,99,235,.08),transparent_240px),#fafafb] px-6 py-12">
      <form
        className="w-full max-w-[400px] rounded-[14px] border border-line bg-white px-9 py-10 shadow-[0_1px_2px_rgba(17,17,26,.04),0_6px_18px_rgba(17,17,26,.05)]"
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
        <div className="mb-7 flex items-center gap-2.5">
          <span className="grid h-8 w-8 place-items-center rounded-[10px] bg-[linear-gradient(140deg,#60a5fa,#2563eb_55%,#1d4ed8)] text-[13px] font-extrabold text-white shadow-[0_2px_8px_rgba(37,99,235,.35)]">
            KB
          </span>
          <strong className="text-base font-semibold">KnowledgeBrain</strong>
        </div>
        <h1 className="mb-6 text-[22px] font-semibold tracking-tight">登录</h1>
        <Label htmlFor="login-email">账号</Label>
        <Input
          id="login-email"
          data-testid="login-email"
          required
          autoComplete="username"
          placeholder="账号"
          value={email}
          onChange={(e) => setEmail(e.currentTarget.value)}
        />
        <div className="mt-4">
          <Label htmlFor="login-password">密码</Label>
          <Input
            id="login-password"
            data-testid="login-password"
            required
            autoComplete="current-password"
            type="password"
            placeholder="密码"
            value={password}
            onChange={(e) => setPassword(e.currentTarget.value)}
          />
        </div>
        {err ? <p className="mt-3 text-sm text-stop">{err}</p> : null}
        <Button type="submit" className="mt-6 w-full" size="lg" disabled={busy || !email.trim() || !password.trim()} data-testid="login-submit">
          {busy ? "进入中…" : "进入"}
        </Button>
      </form>
    </div>
  );
}

function Bids({ email }: { email: string }) {
  const [rows, setRows] = useState<BidProjectView[] | null>(null);
  const [listError, setListError] = useState(false);
  const [listRetry, setListRetry] = useState(0);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [err, setErr] = useState("");
  const [filter, setFilter] = useState<"all" | "open" | "ended">("all");
  const [query, setQuery] = useState("");
  useEffect(() => {
    let canceled = false;
    setListError(false);
    bidApi
      .listProjects()
      .then((value) => { if (!canceled) setRows(value); })
      .catch(() => { if (!canceled) setListError(true); });
    return () => { canceled = true; };
  }, [listRetry]);
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
      crumbs={<Crumbs items={[{ label: "投标项目" }, { label: "在办项目" }]} />}
      title="在办项目"
      extra={
        <Button data-testid="new-bid" onClick={() => setOpen(true)}>
          新建项目
        </Button>
      }
      tree={<BidTree rows={rows} />}
    >
      <div className="wrap stack">
        {listError && <Alert role="alert">
          项目读取失败，已有项目不会因此被清空。
          <Button size="sm" className="ml-3" onClick={() => setListRetry((value) => value + 1)}>重试读取项目</Button>
        </Alert>}
        {rows === null ? listError ? null : (
          <div className="panel space-y-3 p-4">
            <Skeleton className="h-12" />
            <Skeleton className="h-12" />
            <Skeleton className="h-12" />
          </div>
        ) : rows.length === 0 ? (
          <div className="panel">
            <div className="empty">
              <h2>还没有项目</h2>
            </div>
          </div>
        ) : (
          <div className="panel">
            <div className="toolbar">
              <Input
                placeholder="按项目名过滤…"
                value={query}
                onChange={(event) => setQuery(event.currentTarget.value)}
                className="flex-1"
              />
              <div className="flex rounded-[6px] border border-line p-0.5">
                {(
                  [
                    ["all", "全部"],
                    ["open", "在办"],
                    ["ended", "已结束"],
                  ] as const
                ).map(([value, label]) => (
                  <button
                    key={value}
                    type="button"
                    className={cn(
                      "h-7 rounded-[4px] px-2.5 text-[12px] font-medium",
                      filter === value ? "bg-sky text-white" : "text-quiet hover:text-ink",
                    )}
                    onClick={() => setFilter(value)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>项目</TableHead>
                  <TableHead className="w-[140px]">结束时间</TableHead>
                  <TableHead className="w-[88px]">状态</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {shown.map((p) => (
                  <TableRow
                    key={p.id}
                    className="cursor-pointer"
                    onClick={() => go(authoringHref(p.id, "files"))}
                  >
                    <TableCell className="font-semibold">{p.title}</TableCell>
                    <TableCell className="text-quiet">
                      {p.ended_at ? p.ended_at.slice(0, 10) : "—"}
                    </TableCell>
                    <TableCell>
                      <Badge tone={p.status === "ended" ? "gray" : "sky"}>
                        {p.status === "ended" ? "已结束" : "在办"}
                      </Badge>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </div>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent title="新建项目">
          <form
            data-testid="create-bid-form"
            className="space-y-4"
            onSubmit={async (e) => {
              e.preventDefault();
              if (!title.trim()) {
                setErr("先写项目名称");
                return;
              }
              try {
                const p = await bidApi.createProject({
                  title: title.trim(),
                });
                go(authoringHref(p.id, "files"));
              } catch (ex) {
                setErr(ex instanceof Error ? ex.message : "创建失败");
              }
            }}
          >
            <div>
              <Label htmlFor="bid-title">项目名称</Label>
              <Input
                id="bid-title"
                data-testid="bid-title"
                value={title}
                onChange={(e) => setTitle(e.currentTarget.value)}
                required
              />
            </div>
            <div>
              <Label>负责人</Label>
              <Input value={email} readOnly />
            </div>
            {err ? <Alert>{err}</Alert> : null}
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="outline" type="button" onClick={() => setOpen(false)}>
                取消
              </Button>
              <Button type="submit" data-testid="bid-create">
                创建
              </Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>
    </Shell>
  );
}

export function App() {
  const path = useHash();
  const [email, setEmail] = useState<string | null>(null);
  const [verifiedToken, setVerifiedToken] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [sessionError, setSessionError] = useState(false);
  const [sessionRetry, setSessionRetry] = useState(0);

  useEffect(() => {
    let canceled = false;
    if (path === "/login") {
      setReady(true);
      return;
    }
    if (!token()) {
      go("/login");
      setReady(true);
      return;
    }
    const requestToken = token();
    setSessionError(false);
    api
      .me()
      .then((m) => {
        if (canceled || token() !== requestToken) return;
        setEmail(m.email);
        setVerifiedToken(requestToken);
        setReady(true);
      })
      .catch((error: unknown) => {
        if (canceled || token() !== requestToken) return;
        if (error instanceof ApiError && error.status === 401) {
          setToken(null);
          setEmail(null);
          go("/login");
        } else setSessionError(true);
        setReady(true);
      });
    return () => { canceled = true; };
  }, [path, sessionRetry]);

  if (!ready) {
    return (
      <div className="grid min-h-dvh place-items-center bg-canvas">
        <span className="grid h-8 w-8 place-items-center rounded-[10px] bg-[linear-gradient(140deg,#60a5fa,#2563eb_55%,#1d4ed8)] text-[13px] font-extrabold text-white shadow-[0_2px_8px_rgba(37,99,235,.35)]">
          KB
        </span>
      </div>
    );
  }
  if (path === "/login") return <Login />;
  const retrySession = <Button size="sm" onClick={() => setSessionRetry((value) => value + 1)}>重试验证登录</Button>;
  if (email === null || verifiedToken !== token()) return <div className="grid min-h-dvh place-items-center bg-canvas p-6">
    <div role={sessionError ? "alert" : "status"} className="panel max-w-md space-y-4 p-6">
      <p>{sessionError ? "暂时无法验证登录状态，登录信息已保留。" : "正在验证登录状态…"}</p>
      {sessionError && retrySession}
    </div>
  </div>;
  const asset = parseAssetRoute(path);
  return <>
    {sessionError && <div role="alert" className="fixed right-4 top-16 z-50 flex max-w-lg items-center gap-3 rounded-md border border-line bg-white p-3 text-sm shadow-lg">
      暂时无法验证登录状态，当前页面和登录信息已保留。{retrySession}
    </div>}
    {asset ? <Assets email={email} route={asset} /> : parseBidRoute(path) ? <Workbench email={email} /> : <Bids email={email} />}
  </>;
}
