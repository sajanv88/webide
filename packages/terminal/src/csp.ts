/**
 * Lets xterm's styles through the page's strict CSP (no `'unsafe-inline'`).
 *
 * xterm has no `nonce` option, and draws in two ways the policy turns away:
 *
 * - `<style>` elements it makes as it runs (the scrollbar, the theme, the cell
 *   size). Each `<style>` made with `document.createElement` gets the page's
 *   nonce, so it's allowed, as a `<style>` the page wrote would be.
 * - `setAttribute("style", …)` for 24-bit (truecolor) text and backgrounds.
 *   That's set through the style object instead (`style.cssText`), which the
 *   policy allows; the element ends up with the same `style` attribute.
 *
 * Neither lets markup in: the nonce goes only on elements made by script, and
 * script can already set any style through `element.style`. What the policy
 * still stops is a `<style>` or `style="…"` that arrives as HTML.
 *
 * Installed once, for the whole page, the first time a terminal is drawn.
 */
let installed = false;

export function allowTerminalStyles(nonce: string | undefined): void {
  if (installed || typeof document === "undefined") return;
  installed = true;

  if (nonce) {
    const create = document.createElement.bind(document);
    document.createElement = ((tagName: string, options?: ElementCreationOptions) => {
      const element = create(tagName, options);
      if (tagName.toLowerCase() === "style") element.setAttribute("nonce", nonce);
      return element;
    }) as typeof document.createElement;
  }

  const setAttribute = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (this: Element, name: string, value: string) {
    if (name.toLowerCase() === "style" && this instanceof HTMLElement) {
      this.style.cssText = value;
      return;
    }
    setAttribute.call(this, name, value);
  };
}

/** For tests: whether the shim is in place. */
export const terminalStylesAllowed = (): boolean => installed;
