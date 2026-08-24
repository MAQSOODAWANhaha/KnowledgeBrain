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

export type BidStep = "files" | "facts" | "matching" | "quote" | "parts";

export const BID_STEPS: { key: BidStep; n: string; label: string }[] = [
  { key: "files", n: "1", label: "文件" },
  { key: "facts", n: "2", label: "事实/条款" },
  { key: "matching", n: "3", label: "匹配/选择" },
  { key: "quote", n: "4", label: "报价/材料" },
  { key: "parts", n: "5", label: "成稿" },
];

export type BidRoute = {
  id: string;
  step: BidStep;
  view: string;
  part: string;
  pane: "table" | "draft";
  clause: string | null;
  doc: string | null;
};

function isBidStep(value: string | null): value is BidStep {
  return value === "files" || value === "facts" || value === "matching" || value === "quote" || value === "parts";
}

function normalizeStep(value: string | null): BidStep | null {
  if (isBidStep(value)) return value;
  return null;
}

export function bidHref(
  id: string,
  view: string,
  extra?: { step?: BidStep; part?: string; pane?: "table" | "draft"; clause?: string | null; doc?: string | null },
): string {
  const q = new URLSearchParams();
  const step =
    normalizeStep(extra?.step ?? null) ??
    (view === "files"
      ? "files"
      : view === "parts"
        ? "parts"
        : view === "quote" || view === "materials"
          ? "quote"
          : view === "matching"
            ? "matching"
            : view === "facts"
              ? "facts"
              : isBidStep(view)
                ? view
                : "facts");
  q.set("step", step);
  if (view && view !== step) q.set("view", view);
  if (extra?.part) q.set("part", extra.part);
  if (extra?.pane && extra.pane !== "table") q.set("pane", extra.pane);
  if (extra?.clause) q.set("clause", extra.clause);
  if (step === "files" && extra?.doc) q.set("doc", extra.doc);
  const qs = q.toString();
  return qs ? `/bids/${id}?${qs}` : `/bids/${id}`;
}

export type AssetRoute =
  | { kind: "company" }
  | { kind: "folder"; folderId: string; versionId?: string }
  | { kind: "lines" }
  | { kind: "line"; lineId: string }
  | { kind: "product"; lineId: string; productId: string }
  | { kind: "version"; lineId: string; productId: string; versionId: string }
  | { kind: "doc"; folderId: string; versionId: string; docId: string }
  | { kind: "doc"; lineId: string; productId: string; versionId: string; docId: string };

export function parseAssetRoute(path: string): AssetRoute | null {
  const raw = (path.split("?")[0] || "/").replace(/\/+$/, "") || "/";
  const lib = raw.match(/^\/library(?:\/([^/]+))?(?:\/([^/]+))?(?:\/([^/]+))?$/);
  if (lib) {
    if (!lib[1]) return { kind: "company" };
    if (lib[3]) return { kind: "doc", folderId: lib[1], versionId: lib[2], docId: lib[3] };
    return lib[2] ? { kind: "folder", folderId: lib[1], versionId: lib[2] } : { kind: "folder", folderId: lib[1] };
  }
  const prod = raw.match(/^\/products(?:\/([^/]+))?(?:\/([^/]+))?(?:\/([^/]+))?(?:\/([^/]+))?$/);
  if (!prod) return null;
  if (!prod[1]) return { kind: "lines" };
  if (!prod[2]) return { kind: "line", lineId: prod[1] };
  if (!prod[3]) return { kind: "product", lineId: prod[1], productId: prod[2] };
  if (prod[4]) return { kind: "doc", lineId: prod[1], productId: prod[2], versionId: prod[3], docId: prod[4] };
  return { kind: "version", lineId: prod[1], productId: prod[2], versionId: prod[3] };
}

export function assetDocHref(route: AssetRoute, versionId: string, docId: string): string {
  if (route.kind === "folder" || (route.kind === "doc" && "folderId" in route)) {
    const folderId = route.kind === "folder" ? route.folderId : route.folderId;
    return `/library/${folderId}/${versionId}/${docId}`;
  }
  if (route.kind === "line") return `/products/${route.lineId}`;
  const lineId = "lineId" in route ? route.lineId : "";
  const productId = "productId" in route ? route.productId : "";
  return `/products/${lineId}/${productId}/${versionId}/${docId}`;
}

export function assetVersionHref(route: AssetRoute, versionId?: string): string {
  if (route.kind === "folder" || (route.kind === "doc" && "folderId" in route)) {
    const folderId = route.kind === "folder" ? route.folderId : route.folderId;
    const vid = versionId ?? (route.kind === "folder" ? route.versionId : route.versionId);
    return vid ? `/library/${folderId}/${vid}` : `/library/${folderId}`;
  }
  if (route.kind === "version") return `/products/${route.lineId}/${route.productId}/${route.versionId}`;
  if (route.kind === "doc" && "lineId" in route) {
    return `/products/${route.lineId}/${route.productId}/${versionId ?? route.versionId}`;
  }
  if (route.kind === "product") return `/products/${route.lineId}/${route.productId}`;
  if (route.kind === "line") return `/products/${route.lineId}`;
  return "/library";
}

export function parseBidRoute(path: string): BidRoute | null {
  const [rawPath, qs] = path.split("?");
  const raw = (rawPath || "/").replace(/\/+$/, "") || "/";
  const bid = raw.match(/^\/bids\/([^/]+)$/);
  const id = bid?.[1];
  if (!id) return null;
  const q = new URLSearchParams(qs || "");
  let view = q.get("view") || "";
  const part = q.get("part") || "1";
  let step = normalizeStep(q.get("step"));
  if (!step) step = normalizeStep(view) ?? "files";
  if (step === "files") view = view || "files";
  if (step === "facts" && !view) view = "facts";
  if (step === "matching" && !view) view = "commercial";
  if (step === "quote" && !view) view = "quote";
  if (step === "parts") view = "parts";
  const pane = q.get("pane") === "draft" ? "draft" : "table";
  return { id, step, view, part, pane, clause: q.get("clause"), doc: q.get("doc") };
}
