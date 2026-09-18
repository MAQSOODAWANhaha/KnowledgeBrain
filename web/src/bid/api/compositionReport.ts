import { v2Blob } from "./http";

export function downloadCompositionReport(workspace: string, version: string): Promise<Blob> {
  return v2Blob(`/api/v2/submission-workspaces/${encodeURIComponent(workspace)}/docx/versions/${encodeURIComponent(version)}/composition-report`);
}
