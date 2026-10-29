import { expect, test } from "@playwright/test";

test("Office can read public navigation assets without enabling CORS on the app", async ({ request }) => {
  const headers = { Origin: "http://office.example.test" };
  const config = await request.get("/onlyoffice-navigation/config.json", { headers });
  expect(config.ok()).toBeTruthy();
  expect(config.headers()["access-control-allow-origin"]).toBe("*");
  expect((await config.json()).guid).toMatch(/^asc\./);
  const app = await request.get("/", { headers });
  expect(app.ok()).toBeTruthy();
  expect(app.headers()["access-control-allow-origin"]).toBeUndefined();
});
