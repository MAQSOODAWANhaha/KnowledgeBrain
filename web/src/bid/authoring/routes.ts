export const AUTHORING_STEPS = [
  { key: "files", n: "1", label: "文件" },
  { key: "authoring", n: "2", label: "编制" },
  { key: "export", n: "3", label: "导出" },
] as const;

export type AuthoringStep = (typeof AUTHORING_STEPS)[number]["key"];

export type AuthoringRoute = {
  projectId: string;
  step: AuthoringStep;
  nodeLineageId: string | null;
};

function isAuthoringStep(value: string): value is AuthoringStep {
  return AUTHORING_STEPS.some((step) => step.key === value);
}

export function parseAuthoringRoute(path: string): AuthoringRoute | null {
  const raw = (path.split("?")[0] || "/").replace(/\/+$/, "") || "/";
  const match = raw.match(/^\/bids\/([^/]+)(?:\/([^/]+))?(?:\/([^/]+))?$/);
  if (!match) return null;
  const projectId = match[1];
  const stepRaw = match[2] ?? "files";
  if (!isAuthoringStep(stepRaw)) return null;
  const nodeLineageId = stepRaw === "authoring" ? (match[3] ?? null) : null;
  if (stepRaw !== "authoring" && match[3]) return null;
  return { projectId, step: stepRaw, nodeLineageId };
}

export function authoringHref(
  projectId: string,
  step: AuthoringStep = "files",
  nodeLineageId?: string | null,
): string {
  if (step === "authoring" && nodeLineageId)
    return `/bids/${projectId}/authoring/${nodeLineageId}`;
  if (step === "files") return `/bids/${projectId}/files`;
  return `/bids/${projectId}/${step}`;
}
