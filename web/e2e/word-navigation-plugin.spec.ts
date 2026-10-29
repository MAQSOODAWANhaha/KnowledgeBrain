import { expect, test, type Frame, type Page } from "@playwright/test";
import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import type { WordNavigationResult } from "../src/bid/authoring/wordNavigation";

const APP = "http://127.0.0.1:4173";
const EDITOR = "http://127.0.0.1:4188";
const ATTACKER = "http://127.0.0.1:4189";
const BOOKMARK = "kb_c_" + "a".repeat(28);
const CHANNEL = "kb.word-navigation.v1";
type Binding = { nonce: string; editorKey: string; versionId: string };
type Adapter = ReturnType<typeof import("../src/bid/authoring/wordNavigation").connectWordNavigation>;
type HostWindow = Window & {
  WordNavigationTest: { connectWordNavigation: typeof import("../src/bid/authoring/wordNavigation").connectWordNavigation };
  harness: { adapter: Adapter | null; results: WordNavigationResult[]; messages: Record<string, unknown>[]; binding: Binding | null };
};
type PluginWindow = Window & {
  __sdkInit?: number; __attackerEval?: number; __calls?: string[]; __defer?: boolean;
  __pending?: (() => void)[]; __probes?: string[];
  Asc: { plugin: { _initInternal?: boolean } };
};
let adapterScript = "";

// The actual SDK evaluates its normal plugin_init payload. Only Office's
// command executor and document API are fixtures; guard/plugin code is unmodified.
const INIT = `
window.__sdkInit = (window.__sdkInit || 0) + 1;
window.__calls = []; window.__pending = []; window.__defer = false;
window.Api = { GetDocument: function () { return { GetBookmark: function () { return { GoTo: function () {} }; } }; } };
Asc.plugin.callCommand = function (command, close, recalculate, callback) {
  window.__calls.push(Asc.scope.kbBookmark);
  var result = command();
  if (window.__defer) window.__pending.push(function () { callback(result); });
  else callback(result);
};
Asc.plugin.init();`;

test.beforeAll(async () => {
  const result = await build({ entryPoints: [fileURLToPath(new URL("../src/bid/authoring/wordNavigation.ts", import.meta.url))],
    bundle: true, write: false, format: "iife", globalName: "WordNavigationTest", platform: "browser", target: "es2022" });
  adapterScript = result.outputFiles[0].text;
});

async function routes(page: Page) {
  await page.route("**/onlyoffice-navigation/*", async route => {
    const name = new URL(route.request().url()).pathname.split("/").pop()!;
    if (!["index.html", "config.json", "sdk-guard.js", "navigation.js", "onlyoffice-plugins-8.3.3.js"].includes(name)) return route.abort();
    const body = await readFile(new URL(`../public/onlyoffice-navigation/${name}`, import.meta.url));
    await route.fulfill({ body, contentType: name.endsWith(".js") ? "text/javascript" : name.endsWith(".json") ? "application/json" : "text/html" });
  });
  await page.route("**/test/navigation-host", route => route.fulfill({ contentType: "text/html", body: "<!doctype html><html><body></body></html>" }));
  await page.route("**/test/editor-frame*", route => {
    const auto = new URL(route.request().url()).searchParams.get("auto") !== "false";
    return route.fulfill({ contentType: "text/html", body: `<!doctype html><html><body><script>
      var child = document.createElement("iframe");
      child.src = ${JSON.stringify(APP + "/onlyoffice-navigation/index.html")};
      window.addEventListener("message", function (event) {
        if (event.source !== child.contentWindow || typeof event.data !== "string") return;
        var data; try { data = JSON.parse(event.data); } catch { return; }
        if (data.type === "initialize" && ${auto}) child.contentWindow.postMessage(${JSON.stringify(JSON.stringify({ type: "plugin_init", data: INIT }))}, ${JSON.stringify(APP)});
      });
      document.body.append(child);
      </script></body></html>` });
  });
  await page.route("**/test/sibling", route => route.fulfill({ contentType: "text/html", body: "<!doctype html><html><body>Sibling frame</body></html>" }));
}

async function connect(page: Page, versionId = "version-1", replace = false) {
  await page.evaluate(({ versionId, replace }) => {
    const host = window as HostWindow;
    if (replace) { host.harness.adapter!.dispose(); host.harness.binding = null; }
    host.harness.adapter = host.WordNavigationTest.connectWordNavigation({ elementId: "editor", editorKey: "session-1", versionId,
      onResult: result => host.harness.results.push(result) });
  }, { versionId, replace });
}

async function harness(page: Page, { auto = true, bound = true } = {}) {
  await routes(page);
  await page.goto(APP + "/test/navigation-host");
  await page.addScriptTag({ content: adapterScript });
  await page.evaluate(() => {
    const host = window as HostWindow;
    host.harness = { adapter: null, results: [], messages: [], binding: null };
    host.addEventListener("message", event => {
      if (event.data?.channel !== "kb.word-navigation.v1") return;
      host.harness.messages.push(event.data);
      if (event.data.type === "ready") host.harness.binding = event.data;
    });
    const editor = document.createElement("iframe"); editor.id = "editor"; document.body.append(editor);
  });
  if (bound) await connect(page);
  await page.evaluate(({ editor, app, auto }) => {
    (document.getElementById("editor") as HTMLIFrameElement).src = editor + "/test/editor-frame?auto=" + auto;
    for (const [id, origin] of [["editor-sibling", editor], ["app-sibling", app]]) {
      const sibling = document.createElement("iframe"); sibling.id = id; sibling.src = origin + "/test/sibling"; document.body.append(sibling);
    }
  }, { editor: EDITOR, app: APP, auto });
  await expect.poll(() => page.frames().some(frame => frame.url() === APP + "/onlyoffice-navigation/index.html")).toBe(true);
  const plugin = page.frames().find(frame => frame.url() === APP + "/onlyoffice-navigation/index.html")!;
  await plugin.waitForFunction(() => (window as PluginWindow).Asc?.plugin?._initInternal === true);
  await plugin.evaluate(() => {
    const plugin = window as PluginWindow;
    plugin.__probes = [];
    plugin.addEventListener("message", event => { if (event.data?.testProbe) plugin.__probes!.push(event.data.testProbe); });
  });
  if (auto && bound) await page.waitForFunction(() => (window as HostWindow).harness.binding !== null);
  return plugin;
}

async function topSend(page: Page, message: Record<string, unknown>) {
  await page.evaluate(({ app, message }) => {
    (document.getElementById("editor") as HTMLIFrameElement).contentWindow!.frames[0].postMessage(message, app);
  }, { app: APP, message });
}

async function calls(plugin: Frame) { return plugin.evaluate(() => (window as PluginWindow).__calls ?? []); }

test("the real SDK queues initialization until the trusted host binds its editor", async ({ page }) => {
  const plugin = await harness(page, { bound: false });
  expect(await plugin.evaluate(() => (window as PluginWindow).__sdkInit)).toBeUndefined();
  await connect(page);
  await plugin.evaluate(({ channel, app }) => window.top!.postMessage({ channel, type: "hello" }, app), { channel: CHANNEL, app: APP });
  await page.waitForFunction(() => (window as HostWindow).harness.binding !== null);
  expect(await plugin.evaluate(() => (window as PluginWindow).__sdkInit)).toBe(1);
});

test("an untrusted top parent cannot bind or evaluate SDK initialization", async ({ page }) => {
  await routes(page);
  await page.goto(ATTACKER + "/test/editor-frame?auto=true");
  await expect.poll(() => page.frames().some(frame => frame.url() === APP + "/onlyoffice-navigation/index.html")).toBe(true);
  const plugin = page.frames().find(frame => frame.url() === APP + "/onlyoffice-navigation/index.html")!;
  await plugin.waitForFunction(() => (window as PluginWindow).Asc?.plugin?._initInternal === true);
  await plugin.evaluate(() => {
    (window as PluginWindow).__probes = [];
    window.addEventListener("message", event => { if (event.data?.testProbe) (window as PluginWindow).__probes!.push(event.data.testProbe); });
  });
  await page.evaluate(({ channel, app, attacker }) => {
    const target = document.querySelector("iframe")!.contentWindow!;
    target.postMessage({ channel, type: "bind", editorOrigin: attacker, nonce: "evil", editorKey: "evil", versionId: "evil" }, app);
    target.postMessage(JSON.stringify({ type: "plugin_init", data: "window.__attackerEval=1" }), app);
    target.postMessage({ testProbe: "hostile-parent-done" }, app);
  }, { channel: CHANNEL, app: APP, attacker: ATTACKER });
  await plugin.waitForFunction(() => (window as PluginWindow).__probes?.includes("hostile-parent-done"));
  expect(await plugin.evaluate(() => (window as PluginWindow).__sdkInit)).toBeUndefined();
  expect(await plugin.evaluate(() => (window as PluginWindow).__attackerEval)).toBeUndefined();
});

test("SDK eval rejects same-origin top and editor-origin sibling sources", async ({ page }) => {
  const plugin = await harness(page);
  await page.evaluate(({ app }) => {
    const target = (document.getElementById("editor") as HTMLIFrameElement).contentWindow!.frames[0];
    target.postMessage(JSON.stringify({ type: "plugin_init", data: "window.__attackerEval=1" }), app);
    target.postMessage({ testProbe: "top-done" }, app);
  }, { app: APP });
  const sibling = page.frames().find(frame => frame.url() === EDITOR + "/test/sibling")!;
  await sibling.evaluate(({ app }) => {
    const target = window.top!.frames[0].frames[0];
    target.postMessage(JSON.stringify({ type: "plugin_init", data: "window.__attackerEval=2" }), app);
    target.postMessage({ channel: "kb.word-navigation.v1", type: "bind", nonce: "evil", editorKey: "evil", versionId: "evil" }, app);
    target.postMessage({ testProbe: "sibling-done" }, app);
  }, { app: APP });
  await plugin.waitForFunction(() => (window as PluginWindow).__probes?.includes("top-done") && (window as PluginWindow).__probes?.includes("sibling-done"));
  expect(await plugin.evaluate(() => (window as PluginWindow).__attackerEval)).toBeUndefined();
  expect(await plugin.evaluate(() => (window as PluginWindow).__sdkInit)).toBe(1);
  await page.evaluate(({ bookmark }) => (window as HostWindow).harness.adapter!.navigate({ id: "after-attack", versionId: "version-1", bookmark }), { bookmark: BOOKMARK });
  await expect.poll(() => calls(plugin)).toEqual([BOOKMARK]);
});

test("navigation rejects wrong nonce, session, version and bookmark", async ({ page }) => {
  const plugin = await harness(page);
  const binding = await page.evaluate(() => (window as HostWindow).harness.binding!);
  for (const invalid of [{ nonce: "wrong" }, { editorKey: "wrong" }, { versionId: "version-old" }, { bookmark: "javascript:alert(1)" }]) {
    await topSend(page, { channel: CHANNEL, type: "navigate", ...binding, id: "invalid", bookmark: BOOKMARK, ...invalid });
  }
  await page.evaluate(({ bookmark }) => (window as HostWindow).harness.adapter!.navigate({ id: "valid", versionId: "version-1", bookmark }), { bookmark: BOOKMARK });
  await page.waitForFunction(() => (window as HostWindow).harness.results.some(result => result.id === "valid"));
  expect(await calls(plugin)).toEqual([BOOKMARK]);
});

test("save rebind ignores old replies and a same-origin sibling cannot forge a result", async ({ page }) => {
  const plugin = await harness(page);
  await plugin.evaluate(() => { (window as PluginWindow).__defer = true; });
  await page.evaluate(({ bookmark }) => (window as HostWindow).harness.adapter!.navigate({ id: "old-request", versionId: "version-1", bookmark }), { bookmark: BOOKMARK });
  await expect.poll(() => calls(plugin)).toHaveLength(1);
  await connect(page, "version-2", true);
  await page.waitForFunction(() => (window as HostWindow).harness.binding?.versionId === "version-2");
  await plugin.evaluate(() => { (window as PluginWindow).__pending!.shift()!(); });
  await page.evaluate(({ bookmark }) => (window as HostWindow).harness.adapter!.navigate({ id: "new-request", versionId: "version-2", bookmark }), { bookmark: BOOKMARK });
  await expect.poll(() => calls(plugin)).toHaveLength(2);
  const binding = await page.evaluate(() => (window as HostWindow).harness.binding!);
  const sibling = page.frames().find(frame => frame.url() === APP + "/test/sibling")!;
  await sibling.evaluate(({ app, channel, binding }) => {
    window.top!.postMessage({ channel, ...binding, type: "hello" }, app);
    window.top!.postMessage({ channel, ...binding, type: "result", id: "new-request", ok: true }, app);
  }, { app: APP, channel: CHANNEL, binding });
  await plugin.evaluate(() => { (window as PluginWindow).__pending!.shift()!(); });
  await page.waitForFunction(() => (window as HostWindow).harness.results.length > 0);
  expect(await page.evaluate(() => (window as HostWindow).harness.results)).toEqual([{ id: "new-request", ok: true }]);
  expect(await plugin.evaluate(() => (window as PluginWindow).__sdkInit)).toBe(1);
});

test("a late adapter rebind recovers after the first hello was missed", async ({ page }) => {
  const plugin = await harness(page);
  await page.evaluate(() => {
    const host = window as HostWindow;
    host.harness.messages = []; host.harness.binding = null; host.harness.adapter!.dispose();
  });
  await page.waitForFunction(() => (window as HostWindow).harness.messages.some(message => message.type === "hello"));
  await connect(page, "version-2");
  await expect.poll(() => page.evaluate(() => (window as HostWindow).harness.binding?.versionId), { timeout: 1500 }).toBe("version-2");
  expect(await plugin.evaluate(() => (window as PluginWindow).__sdkInit)).toBe(1);
});

test("rapid navigation queues the latest valid request until the current Office command completes", async ({ page }) => {
  const plugin = await harness(page);
  await plugin.evaluate(() => { (window as PluginWindow).__defer = true; });
  await page.evaluate(({ bookmark }) => {
    const adapter = (window as HostWindow).harness.adapter!;
    adapter.navigate({ id: "first", versionId: "version-1", bookmark });
  }, { bookmark: BOOKMARK });
  await expect.poll(() => calls(plugin)).toHaveLength(1);
  await page.evaluate(({ bookmark }) => {
    const adapter = (window as HostWindow).harness.adapter!;
    adapter.navigate({ id: "second", versionId: "version-1", bookmark });
    adapter.navigate({ id: "latest", versionId: "version-1", bookmark: bookmark.replace(/a/g, "b") });
  }, { bookmark: BOOKMARK });
  await topSend(page, { channel: CHANNEL, ...(await page.evaluate(() => (window as HostWindow).harness.binding!)),
    type: "navigate", nonce: "stale", id: "forged", bookmark: "kb_c_" + "c".repeat(28) });
  await plugin.evaluate(() => { (window as PluginWindow).__pending!.shift()!(); });
  await expect.poll(() => calls(plugin), { timeout: 1500 }).toEqual([BOOKMARK, "kb_c_" + "b".repeat(28)]);
  await plugin.evaluate(() => { (window as PluginWindow).__pending!.shift()!(); });
  await page.waitForFunction(() => (window as HostWindow).harness.results.some(result => result.id === "latest"));
  expect(await page.evaluate(() => (window as HostWindow).harness.results.find(result => result.id === "latest"))).toEqual({ id: "latest", ok: true });
});
