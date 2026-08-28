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
  return crypto.randomUUID();
}

export function clientRef(prefix: string): string {
  const ref = `${prefix}_${crypto.randomUUID().replace(/-/g, "")}`;
  if (!CLIENT_REF_RE.test(ref)) throw new Error(`invalid client ref: ${ref}`);
  return ref;
}

export async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(text),
  );
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

export async function fileSha256Hex(file: File): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    await file.arrayBuffer(),
  );
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}
