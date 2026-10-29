import { expect, test, type Page } from "@playwright/test";

const PROJECT = "11111111-1111-1111-1111-111111111111";
const project = { id: PROJECT, title: "导航回归项目", status: "open", ended_at: null, ends_at: "2026-12-31T15:59:59Z", workspace_id: PROJECT };

async function authorize(page: Page) {
  await page.addInitScript(() => localStorage.setItem("kb.token", "navigation-test-token"));
}

test("项目列表失败显示重试而不是空列表", async ({ page }) => {
  let attempts = 0;
  await authorize(page);
  await page.route("**/api/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/me") return route.fulfill({ json: { id: "user", email: "test@local" } });
    if (path === "/api/v3/tender-projects") {
      attempts++;
      return route.fulfill(attempts === 1 ? { status: 503, json: { message: "temporarily unavailable" } } : { json: [project] });
    }
    return route.fulfill({ status: 404, json: {} });
  });
  await page.goto("/#/");
  await expect(page.getByRole("alert")).toContainText("项目读取失败");
  await expect(page.getByRole("heading", { name: "还没有项目" })).toHaveCount(0);
  await page.getByRole("button", { name: "重试读取项目" }).click();
  await expect(page.getByRole("cell", { name: project.title })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("临时登录验证失败保留token并允许原页重试", async ({ page }) => {
  let attempts = 0;
  await authorize(page);
  await page.route("**/api/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/me") {
      attempts++;
      return route.fulfill(attempts === 1 ? { status: 503, json: { message: "temporarily unavailable" } } : { json: { id: "user", email: "test@local" } });
    }
    if (path === "/api/v3/tender-projects") return route.fulfill({ json: [project] });
    return route.fulfill({ status: 404, json: {} });
  });
  await page.goto("/#/");
  await expect(page.getByRole("alert")).toContainText("登录信息已保留");
  expect(await page.evaluate(() => localStorage.getItem("kb.token"))).toBe("navigation-test-token");
  await expect(page.getByTestId("login-form")).toHaveCount(0);
  await page.getByRole("button", { name: "重试验证登录" }).click();
  await expect(page.getByRole("cell", { name: project.title })).toBeVisible();
});

test("明确401才清除登录并回到登录页", async ({ page }) => {
  await authorize(page);
  await page.route("**/api/v1/me", route => route.fulfill({ status: 401, json: { message: "expired" } }));
  await page.goto("/#/");
  await expect(page.getByTestId("login-form")).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("kb.token"))).toBeNull();
});

test("已验证会话遇到503时保留当前列表和输入", async ({ page }) => {
  let reads = 0;
  await authorize(page);
  await page.route("**/api/**", route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/me") {
      reads++;
      return route.fulfill(reads === 1 ? { json: { id: "user", email: "test@local" } } : { status: 503, json: { message: "unavailable" } });
    }
    if (path === "/api/v3/tender-projects") return route.fulfill({ json: [project] });
    return route.fulfill({ status: 404, json: {} });
  });
  await page.goto("/#/");
  const filter = page.getByPlaceholder("按项目名过滤…");
  await filter.fill("导航回归");
  await page.evaluate(() => { location.hash = "/?refresh=1"; });
  await expect(page.getByRole("alert")).toContainText("当前页面和登录信息已保留");
  await expect(filter).toHaveValue("导航回归");
  await expect(page.getByRole("cell", { name: project.title })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("kb.token"))).toBe("navigation-test-token");
});
