import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const apiTarget = process.env.KB_LIVE_API_URL || "http://127.0.0.1:28080";

export default defineConfig({
  plugins: [react()],
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
