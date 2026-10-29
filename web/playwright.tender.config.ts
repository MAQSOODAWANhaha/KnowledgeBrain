import { defineConfig, devices } from "@playwright/test";

const evidence = process.env.KB_TENDER_EVIDENCE_DIR ?? "/opt/workspace/kb-tender-evidence/frontend-workbench/playwright";
export default defineConfig({
  testDir: "./e2e",
  testMatch: ["tender-workbench.spec.ts", "tender-flow.spec.ts", "tender-execution.spec.ts", "docx-export.spec.ts", "app-navigation.spec.ts", "word-navigation-plugin.spec.ts", "outline-planner.spec.ts", "outline-recovery.spec.ts", "knowledge-selection.spec.ts", "analysis-review.spec.ts", "analysis-recovery.spec.ts", "analysis-structured.spec.ts", "chapter-revision.spec.ts", "saved-word-review.spec.ts", "download-state.spec.ts", "workspace-guidance.spec.ts", "task-progress.spec.ts", "workspace-density.spec.ts", "ui-state-races.spec.ts", "word-session-boundaries.spec.ts"],
  fullyParallel: false,
  retries: 0,
  reporter: [["list"]],
  outputDir: evidence,
  use: { baseURL: "http://127.0.0.1:4184", trace: "retain-on-failure", screenshot: "only-on-failure", locale: "zh-CN" },
  webServer: { command: "npm run preview -- --host 127.0.0.1 --port 4184 --strictPort", url: "http://127.0.0.1:4184", reuseExistingServer: false, timeout: 120_000 },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
