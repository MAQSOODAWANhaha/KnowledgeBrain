import { expect, test, type Page } from "@playwright/test";
import { fixture, project } from "./tender.fixture";

const productId = "22222222-2222-4222-8222-222222222222";
const libraryId = "33333333-3333-4333-8333-333333333333";
const productVersion = "44444444-4444-4444-8444-444444444444";
const libraryVersion = "55555555-5555-4555-8555-555555555555";

async function catalog(page: Page) {
  const state = { fail: false };
  await page.route("**/api/v1/workspaces", route => route.fulfill({ status: state.fail ? 503 : 200, contentType: "application/json", body: JSON.stringify(state.fail ? { error: { message: "资料目录暂不可读取" } } : [
    { id: "line", name: "设备产品线", slug: "line", kind: "product_line" },
    { id: "company", name: "公司资料", slug: "company", kind: "company" },
  ]) }));
  await page.route("**/api/v1/workspaces/*/products", route => route.fulfill({ contentType: "application/json", body: JSON.stringify(route.request().url().includes("/line/") ? [
    { id: productId, workspace_id: "line", kind: "product", name: "设备甲", slug: "device", current_version_id: productVersion },
  ] : [{ id: libraryId, workspace_id: "company", kind: "library", name: "资质证照", slug: "qualification", current_version_id: libraryVersion }]) }));
  await page.route("**/api/v1/products/*/versions", route => route.fulfill({ contentType: "application/json", body: JSON.stringify(route.request().url().includes(productId) ? [
    { id: productVersion, product_id: productId, label: "2026版", status: "active", current: true },
    { id: "66666666-6666-4666-8666-666666666666", product_id: productId, label: "归档旧版", status: "archived", current: false },
    { id: "current", product_id: productId, label: "漂移引用", status: "active", current: true },
  ] : [{ id: libraryVersion, product_id: libraryId, label: "2026资质", status: "active", current: true }]) }));
  return state;
}

test("reference versions require explicit selection and keep product and library UUIDs fixed", async ({ page }) => {
  const f = await fixture(page); await catalog(page);
  await page.goto(`/#/bids/${project}/files`);
  const section = page.getByRole("region", { name: "编制参考资料版本", exact: true });
  await expect(section).toBeVisible();
  await expect(section.getByRole("listitem")).toHaveCount(0);
  const select = section.getByLabel("选择资料版本", { exact: true });
  await expect(select.locator("option")).toHaveCount(3);
  await expect(select).toHaveValue("");
  await select.selectOption(`${productId}:${productVersion}`);
  await section.getByRole("button", { name: "添加资料版本", exact: true }).click();
  await expect(select.locator(`option[value="${productId}:${productVersion}"]`)).toBeDisabled();
  await select.selectOption(`${libraryId}:${libraryVersion}`);
  await section.getByRole("button", { name: "添加资料版本", exact: true }).click();
  await expect(section.getByRole("listitem")).toHaveCount(2);
  await expect(section).toContainText("设备甲");
  await expect(section).toContainText("资质证照");
  await page.getByRole("navigation", { name: "项目分区" }).getByRole("link", { name: "标书编制" }).click();
  await page.getByRole("tab", { name: "大纲编排", exact: true }).click();
  await page.getByRole("button", { name: "按修订 1 生成正文", exact: true }).click();
  await expect.poll(() => f.state.starts.length).toBe(1);
  expect(f.state.starts[0].body).toMatchObject({ knowledge_selections: [
    { product_id: productId, version_id: productVersion }, { product_id: libraryId, version_id: libraryVersion },
  ] });
});

test("catalog refresh failure retains selected versions and provides a retry", async ({ page }) => {
  await fixture(page); const state = await catalog(page);
  await page.goto(`/#/bids/${project}/files`);
  const section = page.getByRole("region", { name: "编制参考资料版本", exact: true });
  await section.getByLabel("选择资料版本", { exact: true }).selectOption(`${productId}:${productVersion}`);
  await section.getByRole("button", { name: "添加资料版本", exact: true }).click();
  state.fail = true;
  await section.getByRole("button", { name: "刷新资料目录", exact: true }).click();
  await expect(section.getByRole("alert")).toContainText("资料目录暂不可读取");
  await expect(section.getByRole("listitem")).toContainText("设备甲");
  state.fail = false;
  await section.getByRole("button", { name: "重试资料目录", exact: true }).click();
  await expect(section.getByRole("alert")).toHaveCount(0);
  await expect(section.getByRole("listitem")).toHaveCount(1);
  await section.getByRole("button", { name: /移除.*设备甲/ }).click();
  await expect(section.getByRole("listitem")).toHaveCount(0);
});

test("read-only project state disables selection controls while retaining selected versions", async ({ page }) => {
  await page.clock.install(); const f = await fixture(page); await catalog(page);
  await page.goto(`/#/bids/${project}/files`);
  const section = page.getByRole("region", { name: "编制参考资料版本", exact: true });
  await section.getByLabel("选择资料版本", { exact: true }).selectOption(`${productId}:${productVersion}`);
  await section.getByRole("button", { name: "添加资料版本", exact: true }).click();
  f.view.project.status = "ended";
  await page.clock.runFor(5100);
  await expect(section.getByLabel("选择资料版本", { exact: true })).toBeDisabled();
  await expect(section.getByRole("button", { name: /移除.*设备甲/ })).toBeDisabled();
  await expect(section.getByRole("listitem")).toContainText("设备甲");
  f.view.project.status = "open";
  await page.clock.runFor(5100);
  await expect(section.getByRole("button", { name: /移除.*设备甲/ })).toBeEnabled();
  await expect(section.getByRole("listitem")).toHaveCount(1);
});
