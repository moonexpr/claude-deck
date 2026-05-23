import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";
import { readFileSync } from "fs";

const packageJson = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf-8"),
) as { version: string };

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    APP_VERSION: JSON.stringify(packageJson.version),
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    allowedHosts: true,
    proxy: {
      "/api": {
        target: "http://localhost:8000",
        changeOrigin: false,
        ws: true,
      },
    },
  },
  // `vite preview` (used by the Playwright webServer) needs its own proxy —
  // it does not inherit `server.proxy`. Without this the e2e sweep hits
  // /api on :4173 and 404s instead of reaching server-bin on :8000.
  preview: {
    proxy: {
      "/api": {
        target: "http://localhost:8000",
        changeOrigin: false,
        ws: true,
      },
    },
  },
});
