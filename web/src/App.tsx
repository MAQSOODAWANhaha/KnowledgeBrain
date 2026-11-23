import { useEffect, useState } from "react";
import { ApiError, api, setToken, token } from "./api";
import { Assets } from "./assets/Assets";
import { type BidProjectView } from "./bid/api";
import { tenderApi } from "./bid/api/workbench";
import { authoringHref } from "./bid/authoring/routes";
import { Workbench } from "./bid/Workbench";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Table } from "@astryxdesign/core/Table";
import { TextInput } from "@astryxdesign/core/TextInput";
import { go, parseAssetRoute, parseBidRoute, useHash } from "./hash";
import { LoginScreen } from "./login/LoginScreen";
import { Shell } from "./Shell";
import { PageHeader } from "./shell/PageHeader";
import { OpenProjects } from "./shell/ProjectNav";
import { Tab, TabList } from "@astryxdesign/core/TabList";

const bidApi = tenderApi;

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
      header={<PageHeader
        crumbs={[{ label: "投标项目" }]}
        title="投标项目"
        actions={<Button data-testid="new-bid" label="新建项目" variant="primary" onClick={() => setOpen(true)} />}
        tabs={<TabList value={filter} onChange={(value) => setFilter(value as typeof filter)} hasDivider={false}>
          <Tab value="all" label="全部" />
          <Tab value="open" label="在办" />
          <Tab value="ended" label="已结束" />
        </TabList>}
      />}
      nav={<OpenProjects rows={rows} />}
    >
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto p-6">
        {listError && <div role="alert"><Banner status="error" title="项目读取失败，已有项目不会因此被清空。" endContent={<Button size="sm" label="重试读取项目" onClick={() => setListRetry((value) => value + 1)} />} /></div>}
        {rows === null ? listError ? null : (
          <div className="panel space-y-3 p-4">
            <Skeleton height={48} />
            <Skeleton height={48} />
            <Skeleton height={48} />
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
              <TextInput label="过滤项目" isLabelHidden placeholder="按项目名称搜索" value={query} onChange={setQuery} />
            </div>
            <Table hasHover data={shown} columns={[
              { key: "title", header: "项目", renderCell: (p) => <a href={`#${authoringHref(p.id, "files")}`}>{p.title}</a> },
              { key: "ended_at", header: "结束时间", renderCell: (p) => p.ended_at ? p.ended_at.slice(0, 10) : "—" },
              { key: "status", header: "状态", renderCell: (p) => <Badge label={p.status === "ended" ? "已结束" : "在办"} /> },
            ]} />
          </div>
        )}
      </div>
      <Dialog isOpen={open} onOpenChange={setOpen}>
        <DialogHeader title="新建项目" onOpenChange={setOpen} />
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
            <TextInput id="bid-title" data-testid="bid-title" label="项目名称" isRequired value={title} onChange={setTitle} />
            <TextInput label="负责人" value={email} isReadOnly />
            {err ? <Banner status="error" title={err} /> : null}
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="secondary" type="button" label="取消" onClick={() => setOpen(false)} />
              <Button type="submit" variant="primary" label="创建" data-testid="bid-create" />
            </div>
          </form>
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
        <span className="grid h-7 w-7 place-items-center rounded-[6px] bg-sky text-[11px] font-semibold tracking-wide text-white">
          KB
        </span>
      </div>
    );
  }
  if (path === "/login") return <LoginScreen />;
  const retrySession = <Button size="sm" label="重试验证登录" onClick={() => setSessionRetry((value) => value + 1)} />;
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
