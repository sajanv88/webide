// How the text's lines are drawn: indent guides, a ruler, and marks for
// spaces and tabs. The cursor line's look is a class on the surface (see
// editor-surface.scss). Nothing here sets a style attribute, which the CSP
// forbids: the guides and the ruler are layer markers, which CodeMirror
// places through the CSSOM, and the marks are classes.
import { type Extension, RangeSetBuilder, type Text } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  type EditorView,
  layer,
  RectangleMarker,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import type { SurfaceDisplay } from "./contract";

/** What the surface shows when the app says nothing. */
export const DEFAULT_DISPLAY: SurfaceDisplay = {
  indentGuides: true,
  rulerColumn: null,
  whitespace: "selection",
  lineHighlight: "border",
};

/** How far a blank line looks, up and down, for the indentation around it. */
const BLANK_REACH = 100;
/** How many lines are read to find the document's indentation step. */
const STEP_SAMPLE = 2000;

/** The width, in columns, of a line's leading spaces and tabs; `null` for a blank line. */
export function indentWidth(text: string, tabSize: number): number | null {
  let width = 0;
  for (const char of text) {
    if (char === " ") width += 1;
    else if (char === "\t") width += tabSize - (width % tabSize);
    else return width;
  }
  return null;
}

const steps = new WeakMap<Text, number>();

/**
 * The document's indentation step in columns: the smallest indentation
 * among its first lines, from 1 to 8, or the tab size when nothing is
 * indented. So a file indented by 4 gets a guide every 4 columns, whatever
 * the tab size.
 */
export function indentStep(doc: Text, tabSize: number): number {
  const known = steps.get(doc);
  if (known !== undefined) return known;
  let smallest = Number.POSITIVE_INFINITY;
  const last = Math.min(doc.lines, STEP_SAMPLE);
  for (let number = 1; number <= last; number++) {
    const width = indentWidth(doc.line(number).text, tabSize);
    if (width !== null && width > 0 && width < smallest) smallest = width;
  }
  const step = Number.isFinite(smallest) ? Math.min(smallest, 8) : Math.max(1, tabSize);
  steps.set(doc, step);
  return step;
}

/**
 * How many guides line `number` (from 1) gets. A blank line takes the
 * smaller indentation of the lines around it, so guides run through it.
 */
export function guideLevel(doc: Text, number: number, tabSize: number, step: number): number {
  const width = indentWidth(doc.line(number).text, tabSize);
  if (width !== null) return Math.ceil(width / step);
  const around = (direction: 1 | -1): number => {
    for (let at = number + direction, n = 0; n < BLANK_REACH; at += direction, n++) {
      if (at < 1 || at > doc.lines) return 0;
      const found = indentWidth(doc.line(at).text, tabSize);
      if (found !== null) return found;
    }
    return 0;
  };
  return Math.ceil(Math.min(around(-1), around(1)) / step);
}

/** Where the layer's markers are measured from: the scroller's top left, scrolled. */
function base(view: EditorView): { readonly left: number; readonly top: number } {
  const rect = view.scrollDOM.getBoundingClientRect();
  return {
    left: rect.left - view.scrollDOM.scrollLeft * view.scaleX,
    top: rect.top - view.scrollDOM.scrollTop * view.scaleY,
  };
}

const redraws = (update: ViewUpdate): boolean =>
  update.docChanged ||
  update.viewportChanged ||
  update.geometryChanged ||
  update.startState.tabSize !== update.state.tabSize;

const indentGuides = layer({
  above: false,
  class: "editor-surface-guides",
  update: redraws,
  markers(view) {
    const { doc, tabSize } = view.state;
    const step = indentStep(doc, tabSize);
    const origin = base(view);
    const width = view.defaultCharacterWidth;
    const markers: RectangleMarker[] = [];
    for (const block of view.viewportLineBlocks) {
      const line = doc.lineAt(block.from);
      const level = guideLevel(doc, line.number, tabSize, step);
      if (level === 0) continue;
      const start = view.coordsAtPos(line.from, 1);
      if (!start) continue;
      const left = (start.left - origin.left) / view.scaleX;
      const top = view.documentTop - origin.top + block.top;
      for (let guide = 0; guide < level; guide++) {
        markers.push(
          new RectangleMarker(
            "editor-surface-guide",
            left + guide * step * width,
            top,
            null,
            block.height,
          ),
        );
      }
    }
    return markers;
  },
});

const ruler = (column: number): Extension =>
  layer({
    above: false,
    class: "editor-surface-rulers",
    update: redraws,
    markers(view) {
      const start = view.coordsAtPos(view.viewport.from, 1);
      if (!start) return [];
      const origin = base(view);
      const left = (start.left - origin.left) / view.scaleX;
      return [
        new RectangleMarker(
          "editor-surface-ruler",
          left + column * view.defaultCharacterWidth,
          view.documentTop - origin.top,
          null,
          view.contentHeight,
        ),
      ];
    },
  });

const tabMark = Decoration.mark({ class: "editor-surface-tab" });
const spaceMarks = new Map<number, Decoration>();
/** One mark for a run of spaces, drawn as as many dots (`data-marks`). */
const spaceMark = (count: number): Decoration => {
  let mark = spaceMarks.get(count);
  if (!mark) {
    mark = Decoration.mark({
      class: "editor-surface-space",
      attributes: { "data-marks": "·".repeat(count) },
    });
    spaceMarks.set(count, mark);
  }
  return mark;
};

/** The parts of the text to mark: all that's shown, or only what's selected in it. */
function markedRanges(
  view: EditorView,
  where: "selection" | "all",
): readonly { from: number; to: number }[] {
  if (where === "all") return view.visibleRanges;
  const ranges: { from: number; to: number }[] = [];
  for (const selected of view.state.selection.ranges) {
    if (selected.empty) continue;
    for (const visible of view.visibleRanges) {
      const from = Math.max(selected.from, visible.from);
      const to = Math.min(selected.to, visible.to);
      if (from < to) ranges.push({ from, to });
    }
  }
  return ranges.sort((a, b) => a.from - b.from);
}

function whitespaceMarks(view: EditorView, where: "selection" | "all"): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const runs = /\t| +/g;
  for (const { from, to } of markedRanges(view, where)) {
    const text = view.state.sliceDoc(from, to);
    for (const match of text.matchAll(runs)) {
      const start = from + match.index;
      const run = match[0];
      builder.add(start, start + run.length, run === "\t" ? tabMark : spaceMark(run.length));
    }
  }
  return builder.finish();
}

const whitespace = (where: "selection" | "all"): Extension =>
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = whitespaceMarks(view, where);
      }
      update(update: ViewUpdate) {
        if (
          update.docChanged ||
          update.viewportChanged ||
          (where === "selection" && update.selectionSet)
        ) {
          this.decorations = whitespaceMarks(update.view, where);
        }
      }
    },
    { decorations: (plugin) => plugin.decorations },
  );

const cursorLine = Decoration.line({ class: "editor-surface-cursor-line" });

function cursorLines(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let last = -1;
  for (const range of view.state.selection.ranges) {
    const line = view.state.doc.lineAt(range.head);
    if (line.from > last) builder.add(line.from, line.from, cursorLine);
    last = line.from;
  }
  return builder.finish();
}

/** Outlines each line with a cursor; the stylesheet shows it while the editor has focus. */
const outlineCursorLine = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = cursorLines(view);
    }
    update(update: ViewUpdate) {
      if (update.docChanged || update.selectionSet) this.decorations = cursorLines(update.view);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/**
 * The extensions that draw `display`. nuvui tints the cursor's line itself;
 * the surface's class takes the tint away for "none" and "border".
 */
export function displayExtensions(display: SurfaceDisplay): Extension[] {
  const outlined = display.lineHighlight === "border" || display.lineHighlight === "both";
  return [
    outlined ? outlineCursorLine : [],
    display.indentGuides ? indentGuides : [],
    display.rulerColumn === null ? [] : ruler(display.rulerColumn),
    display.whitespace === "none" ? [] : whitespace(display.whitespace),
  ];
}
