import { digestSha256Hex } from "../../sha256";
import { randomUuid } from "../../uuid";

export const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const SHA256_RE = /^[0-9a-f]{64}$/;
export const CLIENT_REF_RE = /^[A-Za-z0-9_-]{1,128}$/;

export function isUuid(value: string): boolean {
  return UUID_RE.test(value);
}

export function isSha256(value: string): boolean {
  return SHA256_RE.test(value);
}

export function newUuid(): string {
  return randomUuid();
}

export function clientRef(prefix: string): string {
  const ref = `${prefix}_${randomUuid().replace(/-/g, "")}`;
  if (!CLIENT_REF_RE.test(ref)) throw new Error(`invalid client ref: ${ref}`);
  return ref;
}

export async function sha256Hex(text: string): Promise<string> {
  return digestSha256Hex(new TextEncoder().encode(text));
}

export async function fileSha256Hex(file: File): Promise<string> {
  return digestSha256Hex(new Uint8Array(await file.arrayBuffer()));
}
