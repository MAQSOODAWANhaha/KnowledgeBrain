import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "tender-word-live.spec.ts",
  fullyParallel: false,
  retries: 0,
  reporter: [["list"]],
  outputDir: process.env.KB_TENDER_EVIDENCE_DIR ?? "/opt/workspace/kb-tender-evidence/frontend-workbench/real-word",
  use: { baseURL: "http://127.0.0.1:4175", ignoreHTTPSErrors: true, locale: "zh-CN", viewport: { width: 1600, height: 1100 }, trace: "retain-on-failure", screenshot: "only-on-failure" },
  webServer: { command: "npm run preview -- --host 127.0.0.1 --port 4175 --strictPort", url: "http://127.0.0.1:4175", reuseExistingServer: false, timeout: 120_000 },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1600, height: 1000 } } }],
});
