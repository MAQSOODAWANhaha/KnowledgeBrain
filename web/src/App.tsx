import { useEffect, useState } from "react";
import { ApiError, api, setToken, token } from "./api";
import { Assets } from "./assets/Assets";
import { createBidV2Client, type BidProjectView } from "./bid/api";
import { authoringHref } from "./bid/authoring/routes";
import { BidTree } from "./bid/BidTree";
import { Workbench } from "./bid/Workbench";
import { shanghaiEndOfDay } from "./bid/helpers";
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

const bidApi = createBidV2Client();

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
          placeholder="账号"
          value={email}
          onChange={(e) => setEmail(e.currentTarget.value)}
        />
        <div className="mt-4">
          <Label htmlFor="login-password">密码</Label>
          <Input
            id="login-password"
            data-testid="login-password"
            type="password"
            placeholder="密码"
            value={password}
            onChange={(e) => setPassword(e.currentTarget.value)}
          />
        </div>
        {err ? <p className="mt-3 text-sm text-stop">{err}</p> : null}
        <Button type="submit" className="mt-6 w-full" size="lg" disabled={busy} data-testid="login-submit">
          {busy ? "进入中…" : "进入"}
        </Button>
      </form>
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
        {rows === null ? (
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
                  <TableHead className="w-[140px]">招标结束</TableHead>
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
                      {p.ends_at ? p.ends_at.slice(0, 10) : "—"}
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
            <div>
              <Label htmlFor="bid-ends">招标结束日</Label>
              <Input
                id="bid-ends"
                data-testid="bid-ends"
                type="date"
                value={when}
                onChange={(e) => setWhen(e.currentTarget.value)}
                required
              />
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
      <div className="grid min-h-dvh place-items-center bg-canvas">
        <span className="grid h-8 w-8 place-items-center rounded-[10px] bg-[linear-gradient(140deg,#60a5fa,#2563eb_55%,#1d4ed8)] text-[13px] font-extrabold text-white shadow-[0_2px_8px_rgba(37,99,235,.35)]">
          KB
        </span>
      </div>
    );
  }
  if (path === "/login") return <Login />;
  const asset = parseAssetRoute(path);
  if (asset) return <Assets email={email} route={asset} />;
  if (parseBidRoute(path)) return <Workbench email={email} />;
  return <Bids email={email} />;
}
