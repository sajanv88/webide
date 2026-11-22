import "./highlight.scss";
import type { ReactNode } from "react";

/** `[start, end)` in UTF-16 units, as JavaScript indexes strings. */
export type Range = readonly [number, number];

/**
 * `text` with each range in a `<mark>`, which screen readers can announce and
 * which is bold as well as tinted, so it isn't told by color alone.
 * Ranges may overlap or be out of order.
 */
export function Highlighted({
  text,
  ranges,
}: {
  readonly text: string;
  readonly ranges: readonly Range[];
}) {
  const merged = merge(ranges, text.length);
  const parts: ReactNode[] = [];
  let at = 0;
  for (const [start, end] of merged) {
    if (start > at) parts.push(text.slice(at, start));
    parts.push(
      <mark key={start} className="search-highlight">
        {text.slice(start, end)}
      </mark>,
    );
    at = end;
  }
  if (at < text.length) parts.push(text.slice(at));
  return <>{parts}</>;
}

/** Sorted, clamped to `length`, overlaps joined, empty ranges dropped. */
export function merge(ranges: readonly Range[], length: number): Range[] {
  const sorted = ranges
    .map(([start, end]): Range => [Math.max(0, start), Math.min(length, end)])
    .filter(([start, end]) => start < end)
    .sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const [start, end] of sorted) {
    const last = merged[merged.length - 1];
    if (last && start <= last[1]) last[1] = Math.max(last[1], end);
    else merged.push([start, end]);
  }
  return merged;
}

/** Single positions (from file matching) as ranges, neighbours joined. */
export function positionsToRanges(positions: readonly number[]): Range[] {
  return merge(
    positions.map((at): Range => [at, at + 1]),
    Number.POSITIVE_INFINITY,
  );
}
