// Breakpoints in a gutter, and the line the debugger is paused on.
import { type Extension, RangeSet, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  GutterMarker,
  gutter,
  keymap,
} from "@codemirror/view";

/** Sets the breakpoint lines (1-based) and the paused line of the document shown. */
export const setDebugMarks = StateEffect.define<{
  readonly breakpoints: readonly number[];
  readonly execution: number | null;
}>();

class BreakpointMarker extends GutterMarker {
  override toDOM() {
    const dot = document.createElement("span");
    dot.className = "editor-surface-breakpoint";
    return dot;
  }
}
const breakpoint = new BreakpointMarker();
const executionLine = Decoration.line({ class: "editor-surface-execution" });

interface Marks {
  readonly breakpoints: RangeSet<GutterMarker>;
  readonly execution: DecorationSet;
}

const marks = StateField.define<Marks>({
  create: () => ({ breakpoints: RangeSet.empty, execution: Decoration.none }),
  update(value, transaction) {
    let next: Marks = {
      breakpoints: value.breakpoints.map(transaction.changes),
      execution: value.execution.map(transaction.changes),
    };
    for (const effect of transaction.effects) {
      if (!effect.is(setDebugMarks)) continue;
      const doc = transaction.state.doc;
      const lines = [...new Set(effect.value.breakpoints)]
        .filter((line) => line >= 1 && line <= doc.lines)
        .sort((a, b) => a - b);
      const paused = effect.value.execution;
      next = {
        breakpoints: RangeSet.of(lines.map((line) => breakpoint.range(doc.line(line).from))),
        execution:
          paused !== null && paused >= 1 && paused <= doc.lines
            ? Decoration.set([executionLine.range(doc.line(paused).from)])
            : Decoration.none,
      };
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.execution),
});

/**
 * The breakpoint gutter (a press toggles the line's breakpoint), F9 (toggles
 * the cursor's line), and the paused line. `toggle()` is the document's own,
 * or nothing when it can't have breakpoints.
 */
export function debugExtensions(toggle: () => ((line: number) => void) | undefined): Extension[] {
  return [
    marks,
    gutter({
      class: "editor-surface-breakpoints",
      markers: (view) => view.state.field(marks).breakpoints,
      initialSpacer: () => breakpoint,
      domEventHandlers: {
        mousedown(view, block) {
          const onToggle = toggle();
          if (!onToggle) return false;
          onToggle(view.state.doc.lineAt(block.from).number);
          return true;
        },
      },
    }),
    keymap.of([
      {
        key: "F9",
        run(view) {
          const onToggle = toggle();
          if (!onToggle) return false;
          onToggle(view.state.doc.lineAt(view.state.selection.main.head).number);
          return true;
        },
      },
    ]),
  ];
}
