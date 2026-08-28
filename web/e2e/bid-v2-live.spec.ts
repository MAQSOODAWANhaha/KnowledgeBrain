import { expect, test, type Locator, type Page } from "@playwright/test";

async function activateWithKeyboard(locator: Locator) {
  await expect(locator).toBeEnabled();
  await locator.focus();
  await locator.press("Enter");
}

async function login(page: Page) {
  await page.goto("/#/login");
  await page.getByTestId("login-email").fill(process.env.KB_LIVE_EMAIL ?? "");
  await page
    .getByTestId("login-password")
    .fill(process.env.KB_LIVE_PASSWORD ?? "");
  await activateWithKeyboard(page.getByTestId("login-submit"));
  await expect(page.getByTestId("new-bid")).toBeVisible();
}

test("live V2 workbench exposes files/authoring/export without Gate", async ({
  page,
}) => {
  await login(page);
  await activateWithKeyboard(page.getByTestId("new-bid"));
  await page.getByTestId("bid-title").fill(`V2 live ${Date.now()}`);
  await page.getByTestId("bid-ends").fill("2026-12-31");
  await activateWithKeyboard(page.getByTestId("bid-create"));
  await expect(page.getByTestId("wizard-files")).toBeVisible();
  await expect(page.getByTestId("wizard-authoring")).toBeVisible();
  await expect(page.getByTestId("wizard-export")).toBeVisible();
  await expect(page.getByTestId("gate-issues")).toHaveCount(0);
  await activateWithKeyboard(page.getByTestId("wizard-export"));
  await expect(page.getByTestId("export-docx")).toBeVisible();
});
