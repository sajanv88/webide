import { join } from "node:path";
import process from "node:process";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

/**
 * Tauri freezes `Object.prototype` (`freezePrototype`, against prototype
 * pollution). Then assigning `toString` to a plain object throws, since the
 * frozen one it inherits isn't writable, and xterm's bundled VS Code code does
 * that once, as it loads (the `KeyCodeUtils` namespace). This defines it as
 * the object's own property instead, which is what the assignment meant. It
 * fails the build if the code changes, so an xterm upgrade is checked again.
 */
function xtermUnderFrozenPrototype(): Plugin {
  const file = /[\\/]@xterm[\\/]xterm[\\/]lib[\\/]xterm\.mjs$/;
  const assignment = /\b([A-Za-z_$][\w$]*)\.toString=([A-Za-z_$][\w$]*);/g;
  return {
    name: "xterm-under-frozen-prototype",
    enforce: "pre",
    transform(code, id) {
      if (!file.test(id)) return null;
      const found = code.match(assignment)?.length ?? 0;
      if (found !== 1) {
        throw new Error(
          `xterm-under-frozen-prototype: expected one \`x.toString=y;\` in ${id}, found ${found}. Check the new xterm under freezePrototype.`,
        );
      }
      return {
        code: code.replace(
          assignment,
          'Object.defineProperty($1,"toString",{value:$2,writable:true,configurable:true});',
        ),
        map: null,
      };
    },
  };
}

const host = process.env.TAURI_DEV_HOST;
const isWindows = process.env.TAURI_ENV_PLATFORM === "windows";
const isDebug = Boolean(process.env.TAURI_ENV_DEBUG);

export default defineConfig({
  plugins: [react(), xtermUnderFrozenPrototype()],
  // Served as is in dev too, so the plugin above sees it.
  optimizeDeps: {
    exclude: ["@xterm/xterm"],
  },
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
