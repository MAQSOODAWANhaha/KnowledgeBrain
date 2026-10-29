import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Connect } from "vite";

const apiTarget =
  (globalThis as { process?: { env?: Record<string, string | undefined> } })
    .process?.env?.KB_LIVE_API_URL || "http://127.0.0.1:28080";

const navigationCors: Connect.NextHandleFunction = (request, response, next) => {
  if (["GET", "HEAD"].includes(request.method ?? "") &&
      request.url?.startsWith("/onlyoffice-navigation/")) {
    response.setHeader("Access-Control-Allow-Origin", "*");
  }
  next();
};

export default defineConfig({
  plugins: [react(), tailwindcss(), {
    name: "navigation-assets-cors",
    configureServer: server => { server.middlewares.use(navigationCors); },
    configurePreviewServer: server => { server.middlewares.use(navigationCors); },
  }],
  resolve: {
    alias: { "@": new URL("./src", import.meta.url).pathname },
  },
  server: {
    port: 5174,
    proxy: {
      "/api": apiTarget,
      "/health": apiTarget,
    },
  },
  preview: {
    proxy: {
      "/api": apiTarget,
      "/health": apiTarget,
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
