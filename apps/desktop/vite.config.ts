import { join } from "node:path";
import process from "node:process";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;
const isWindows = process.env.TAURI_ENV_PLATFORM === "windows";
const isDebug = Boolean(process.env.TAURI_ENV_DEBUG);

export default defineConfig({
  plugins: [react()],
  // Keep Rust errors visible in `tauri dev`.
  clearScreen: false,
  resolve: {
    dedupe: ["react", "react-dom"],
  },
  css: {
    preprocessorOptions: {
      scss: {
        loadPaths: [join(import.meta.dirname, "src", "styles")],
      },
    },
  },
  server: {
    // Must match build.devUrl in src-tauri/tauri.conf.json.
    port: 1420,
    strictPort: true,
    host: host || false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // nuvui needs Chromium 111 / Safari 16.4 (cascade layers, oklch, color-mix).
    target: isWindows ? "chrome111" : "safari16.4",
    minify: !isDebug,
    sourcemap: isDebug,
  },
});
