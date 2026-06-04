import { defineConfig, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";
import { readFileSync } from "fs";

const packageJson = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf-8"),
) as { version: string };

// Proxy target: unix socket if SOCKET_PATH is set (default for dev.sh),
// otherwise TCP via SERVER_URL (legacy/CI).
//
// node-http-proxy accepts a target object with a `socketPath`; the host/port
// are required by the URL parser but ignored when socketPath is honoured.
const socketPath = process.env.SOCKET_PATH;
const serverUrl = process.env.SERVER_URL ?? "http://localhost:8000";

const apiProxy: ProxyOptions = socketPath
  ? {
      // node-http-proxy honours `target.socketPath` at runtime, but Vite's
      // ProxyOptions['target'] type (http-proxy's ProxyTargetUrl) doesn't
      // expose it — cast through unknown to keep the runtime behaviour.
      target: {
        socketPath,
        host: "localhost",
        protocol: "http:",
      } as unknown as ProxyOptions["target"],
      changeOrigin: false,
      ws: true,
    }
  : {
      target: serverUrl,
      changeOrigin: false,
      ws: true,
    };

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
      "/api": apiProxy,
    },
  },
  // `vite preview` (used by the Playwright webServer) needs its own proxy —
  // it does not inherit `server.proxy`. Without this the e2e sweep hits
  // /api on :4173 and 404s instead of reaching server-bin on :8000.
  preview: {
    proxy: {
      "/api": apiProxy,
    },
  },
});
