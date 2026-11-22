/**
 * The nonce Tauri gave this page's `<style>` elements, for components that add a
 * `<style>` element at runtime (nuvui `ScrollArea`). Tauri writes a fresh nonce on
 * every `<style>` in the served HTML; `index.html` keeps one empty `<style>` for this.
 * `undefined` when the page wasn't served by Tauri's asset protocol (the dev server).
 */
export function getStyleNonce(): string | undefined {
  const nonce = document.querySelector<HTMLStyleElement>("style[nonce]")?.nonce;
  return nonce ? nonce : undefined;
}
