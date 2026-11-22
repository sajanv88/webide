import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom lays nothing out. CodeMirror measures text through ranges, and Radix
// (nuvui's Select) expects a ResizeObserver to exist.
class NoopResizeObserver implements ResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
globalThis.ResizeObserver ??= NoopResizeObserver;
const emptyRects = (): DOMRectList =>
  Object.assign([], { item: () => null }) as unknown as DOMRectList;
Range.prototype.getClientRects ??= emptyRects;
Range.prototype.getBoundingClientRect ??= () => new DOMRect();

afterEach(() => {
  cleanup();
});
