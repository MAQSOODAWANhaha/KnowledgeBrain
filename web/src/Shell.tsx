import { type ReactNode, useState } from "react";
import { setToken } from "./api";
import { go } from "./hash";

type Props = {
  root: "bids" | "assets";
  email: string;
  crumbs: string;
  title: string;
  extra?: ReactNode;
  lead?: ReactNode;
  tree?: ReactNode;
  inspector?: ReactNode;
  children: ReactNode;
  className?: string;
};

export function Shell({ root, email, crumbs, title, extra, lead, tree, inspector, children, className }: Props) {
  const [menu, setMenu] = useState(false);
  const initial = !email || email.startsWith("dev@") ? "张" : email.slice(0, 1).toUpperCase();
  return (
    <div className="app">
      <header className="pnav">
        <div className="pnav-left" style={{ position: "relative" }}>
          <div className="mark">KB</div>
          <button className="acct" type="button" onClick={() => setMenu((v) => !v)}>
            <em>{email || "dev@local"}</em>
            <svg viewBox="0 0 24 24">
              <path d="M6 9l6 6 6-6" />
            </svg>
          </button>
          {menu && (
            <div className="acct-menu">
              <button
                type="button"
                onClick={() => {
                  setToken(null);
                  go("/login");
                }}
              >
                退出
              </button>
            </div>
          )}
        </div>
        <div className="pnav-main">
          <nav className="ctx-nav">
            <a className={root === "bids" ? "on" : undefined} href="#/">
              <svg viewBox="0 0 24 24">
                <rect x="3" y="3" width="7" height="7" rx="1" />
                <rect x="14" y="3" width="7" height="7" rx="1" />
                <rect x="3" y="14" width="7" height="7" rx="1" />
                <rect x="14" y="14" width="7" height="7" rx="1" />
              </svg>
              投标项目
            </a>
            <a className={root === "assets" ? "on" : undefined} href="#/library">
              <svg viewBox="0 0 24 24">
                <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
              </svg>
              知识资产
            </a>
          </nav>
          <div className="spacer" />
          <span className="pnav-link">
            <svg viewBox="0 0 24 24">
              <circle cx="12" cy="12" r="9" />
              <path d="M9.6 9.2a2.4 2.4 0 1 1 3.2 2.2c-.7.4-1 .9-1 1.7V14" />
              <path d="M12 17.2h.01" />
            </svg>
            帮助
          </span>
          <div className="avatar">{initial}</div>
        </div>
      </header>
      <aside className="side">
        <div className="side-find">
          <svg viewBox="0 0 24 24">
            <circle cx="11" cy="11" r="7" />
            <path d="M20 20l-3-3" />
          </svg>
          <input placeholder="快速搜索…" />
          <kbd>⌘K</kbd>
        </div>
        {tree}
      </aside>
      <div className={`maincol ${className ?? ""}`}>
        <div className="pagehead">
          <div>
            <p className="crumbs">{crumbs}</p>
            <h1 className="h1">{title}</h1>
            {lead}
          </div>
          {extra && <div className="actions">{extra}</div>}
        </div>
        {inspector ? (
          <div className="bench">
            <div className="bench-main">{children}</div>
            <aside className="insp">{inspector}</aside>
          </div>
        ) : (
          children
        )}
      </div>
    </div>
  );
}

export function StageLine({
  derived,
}: {
  derived: {
    has_files: boolean;
    files_ready: boolean;
    extract_running: boolean;
    unconfirmed_drafts: number;
    match_running: boolean;
    has_picks: boolean;
  };
}) {
  let label = "待上传招标文件";
  if (derived.extract_running) label = "正在抽条款";
  else if (derived.match_running) label = "正在匹配产品";
  else if (derived.unconfirmed_drafts > 0) label = `${derived.unconfirmed_drafts} 条待确认`;
  else if (derived.has_files && !derived.files_ready) label = "文件解析中";
  else if (derived.has_picks) label = "已勾选产品，可预览";
  else if (derived.files_ready) label = "待审条款";
  return <span className="note" style={{ margin: 0 }}>{label}</span>;
}
