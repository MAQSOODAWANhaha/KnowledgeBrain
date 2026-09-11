import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const apiTarget =
  (globalThis as { process?: { env?: Record<string, string | undefined> } })
    .process?.env?.KB_LIVE_API_URL || "http://127.0.0.1:28080";

export default defineConfig({
  plugins: [react(), tailwindcss()],
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
