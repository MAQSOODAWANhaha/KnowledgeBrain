import { downloadCompositionReport } from "../api/compositionReport";
import { describe, expect, it } from "./harness";

describe("composition report", () => {
  it("report downloads use the exact encoded workspace and version without submitting work", async () => {
    const previous = globalThis.fetch; let sentPath: unknown; let sent: RequestInit | undefined;
    const report = JSON.stringify({ source_open_items: [] });
    globalThis.fetch = async (path, init) => { sentPath = path; sent = init; return new Response(report, { headers: { "Content-Type": "application/json" } }); };
    try {
      const blob = await downloadCompositionReport("workspace /?", "version/#?");
      expect(sentPath).toBe("/api/v2/submission-workspaces/workspace%20%2F%3F/docx/versions/version%2F%23%3F/composition-report");
      expect(sent?.method ?? "GET").toBe("GET");
      expect(new Headers(sent?.headers).get("Idempotency-Key")).toBe(null);
      expect(await blob.text()).toBe(report);
    } finally { globalThis.fetch = previous; }
  });
});
