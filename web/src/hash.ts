import { useEffect, useState } from "react";

export function useHash(): string {
  const [path, setPath] = useState(() => location.hash.replace(/^#/, "") || "/");
  useEffect(() => {
    const on = () => setPath(location.hash.replace(/^#/, "") || "/");
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return path;
}

export function go(path: string): void {
  location.hash = path;
}

export type BidRoute = {
  id: string;
  view: string;
  part: string;
  pane: "table" | "draft" | "detail";
  clause: string | null;
};

export function bidHref(
  id: string,
  view: string,
  extra?: { part?: string; pane?: "table" | "draft" | "detail"; clause?: string | null },
): string {
  const q = new URLSearchParams();
  if (view && view !== "commercial") q.set("view", view);
  if (extra?.part) q.set("part", extra.part);
  if (extra?.pane && extra.pane !== "table") q.set("pane", extra.pane);
  if (extra?.clause) q.set("clause", extra.clause);
  const qs = q.toString();
  return qs ? `/bids/${id}?${qs}` : `/bids/${id}`;
}

export function parseBidRoute(path: string): BidRoute | null {
  const [rawPath, qs] = path.split("?");
  const raw = (rawPath || "/").replace(/\/+$/, "") || "/";
  const preview = raw.match(/^\/bids\/([^/]+)\/preview$/);
  const picks = raw.match(/^\/bids\/([^/]+)\/picks$/);
  const bookletPath = raw.match(/^\/bids\/([^/]+)\/booklet\/([^/]+)$/);
  const bid = raw.match(/^\/bids\/([^/]+)$/);
  const id = preview?.[1] ?? picks?.[1] ?? bookletPath?.[1] ?? bid?.[1];
  if (!id) return null;
  const q = new URLSearchParams(qs || "");
  let view = q.get("view") || "commercial";
  let part = q.get("part") || "1";
  if (preview) {
    view = "booklet";
    part = q.get("part") || "1";
  }
  if (bookletPath) {
    view = "booklet";
    part = decodeURIComponent(bookletPath[2]);
  }
  const paneRaw = q.get("pane");
  const pane = paneRaw === "draft" || paneRaw === "detail" ? paneRaw : "table";
  return { id, view, part, pane, clause: q.get("clause") };
}
