import { defineConfig, devices } from "@playwright/test";

if (!process.env.KB_LIVE_API_URL) {
  throw new Error("KB_LIVE_API_URL is required for live browser acceptance");
}

const liveUiUrl = process.env.KB_LIVE_UI_URL;

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*-live*.spec.ts",
  fullyParallel: false,
  forbidOnly: true,
  retries: 0,
  timeout: 8 * 60_000,
  reporter: [
    ["html", { outputFolder: "playwright-report-live", open: "never" }],
    ["list"],
  ],
  outputDir: "test-results-live",
  use: {
    baseURL: liveUiUrl ?? "http://127.0.0.1:4174",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    locale: "zh-CN",
  },
  webServer: liveUiUrl
    ? undefined
    : {
        command: "npm run preview -- --host 127.0.0.1 --port 4174",
        url: "http://127.0.0.1:4174",
        reuseExistingServer: false,
        timeout: 120_000,
      },
  projects: [{ name: "chromium-live", use: { ...devices["Desktop Chrome"] } }],
});
