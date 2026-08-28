import { useEffect, useState } from "react";
import {
  authoringHref,
  parseAuthoringRoute,
  type AuthoringRoute,
  type AuthoringStep,
} from "./bid/authoring/routes";

export function useHash(): string {
  const [path, setPath] = useState(
    () => location.hash.replace(/^#/, "") || "/",
  );
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

export type BidStep = AuthoringStep;
export type BidRoute = AuthoringRoute;
export { authoringHref as bidHref, parseAuthoringRoute as parseBidRoute };

export const BID_STEPS = [
  { key: "files", n: "1", label: "文件" },
  { key: "authoring", n: "2", label: "编制" },
  { key: "export", n: "3", label: "导出" },
] as const;

export type AssetRoute =
  | { kind: "company" }
  | { kind: "folder"; folderId: string; versionId?: string }
  | { kind: "lines" }
  | { kind: "line"; lineId: string }
  | { kind: "product"; lineId: string; productId: string }
  | { kind: "version"; lineId: string; productId: string; versionId: string }
  | { kind: "doc"; folderId: string; versionId: string; docId: string }
  | {
      kind: "doc";
      lineId: string;
      productId: string;
      versionId: string;
      docId: string;
    };

export function parseAssetRoute(path: string): AssetRoute | null {
  const raw = (path.split("?")[0] || "/").replace(/\/+$/, "") || "/";
  const lib = raw.match(
    /^\/library(?:\/([^/]+))?(?:\/([^/]+))?(?:\/([^/]+))?$/,
  );
  if (lib) {
    if (!lib[1]) return { kind: "company" };
    if (lib[3])
      return {
        kind: "doc",
        folderId: lib[1],
        versionId: lib[2],
        docId: lib[3],
      };
    return lib[2]
      ? { kind: "folder", folderId: lib[1], versionId: lib[2] }
      : { kind: "folder", folderId: lib[1] };
  }
  const prod = raw.match(
    /^\/products(?:\/([^/]+))?(?:\/([^/]+))?(?:\/([^/]+))?(?:\/([^/]+))?$/,
  );
  if (!prod) return null;
  if (!prod[1]) return { kind: "lines" };
  if (!prod[2]) return { kind: "line", lineId: prod[1] };
  if (!prod[3]) return { kind: "product", lineId: prod[1], productId: prod[2] };
  if (prod[4])
    return {
      kind: "doc",
      lineId: prod[1],
      productId: prod[2],
      versionId: prod[3],
      docId: prod[4],
    };
  return {
    kind: "version",
    lineId: prod[1],
    productId: prod[2],
    versionId: prod[3],
  };
}

export function assetDocHref(
  route: AssetRoute,
  versionId: string,
  docId: string,
): string {
  if (
    route.kind === "folder" ||
    (route.kind === "doc" && "folderId" in route)
  ) {
    const folderId = route.kind === "folder" ? route.folderId : route.folderId;
    return `/library/${folderId}/${versionId}/${docId}`;
  }
  if (route.kind === "line") return `/products/${route.lineId}`;
  const lineId = "lineId" in route ? route.lineId : "";
  const productId = "productId" in route ? route.productId : "";
  return `/products/${lineId}/${productId}/${versionId}/${docId}`;
}

export function assetVersionHref(
  route: AssetRoute,
  versionId?: string,
): string {
  if (
    route.kind === "folder" ||
    (route.kind === "doc" && "folderId" in route)
  ) {
    const folderId = route.kind === "folder" ? route.folderId : route.folderId;
    const vid =
      versionId ??
      (route.kind === "folder" ? route.versionId : route.versionId);
    return vid ? `/library/${folderId}/${vid}` : `/library/${folderId}`;
  }
  if (route.kind === "version")
    return `/products/${route.lineId}/${route.productId}/${route.versionId}`;
  if (route.kind === "doc" && "lineId" in route) {
    return `/products/${route.lineId}/${route.productId}/${versionId ?? route.versionId}`;
  }
  if (route.kind === "product")
    return `/products/${route.lineId}/${route.productId}`;
  if (route.kind === "line") return `/products/${route.lineId}`;
  return "/library";
}
