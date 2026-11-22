import "@nuvui/react/styles.css";
import "@nuvui/backgrounds/styles.css";
import "./styles/globals.scss";
import { getStyleNonce } from "@repo/ipc";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app";

// Radix's scroll lock (modal menus, dialogs) adds a <style> element through
// react-style-singleton, which reads its nonce from get-nonce's global fallback,
// `__webpack_nonce__`. Give it Tauri's per-load nonce so the strict CSP allows it.
const nonce = getStyleNonce();
if (nonce) {
  (globalThis as { __webpack_nonce__?: string }).__webpack_nonce__ = nonce;
}

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html has no #root element");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
