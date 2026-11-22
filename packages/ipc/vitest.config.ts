import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // @tauri-apps/api/mocks installs its fake IPC on `window`.
    environment: "jsdom",
  },
});
