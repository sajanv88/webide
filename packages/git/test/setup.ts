import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom has no ResizeObserver; Radix needs one to exist.
class NoopResizeObserver implements ResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
globalThis.ResizeObserver ??= NoopResizeObserver;

// jsdom doesn't lay out, so it has no scrollIntoView; cmdk calls it on the active item.
Element.prototype.scrollIntoView ??= () => {};

afterEach(() => {
  cleanup();
});
