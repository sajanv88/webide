// CodeMirror extensions for language features from a language server:
// completion, hover, and go to definition. This package is the only one that
// may import CodeMirror.
import {
  type Completion,
  type CompletionContext,
  type CompletionResult,
  pickedCompletion,
} from "@codemirror/autocomplete";
import { EditorState, type Extension, Prec, type Text } from "@codemirror/state";
import { EditorView, hoverTooltip, keymap } from "@codemirror/view";
import type { SurfaceCompletion, SurfaceLanguage, SurfacePosition } from "./contract";
import { editChanges, offsetAt, overlapping } from "./edits";

/** A character offset as a line and a UTF-16 column (CodeMirror counts UTF-16). */
export function positionAt(doc: Text, offset: number): SurfacePosition {
  const at = Math.max(0, Math.min(offset, doc.length));
  const line = doc.lineAt(at);
  return { line: line.number - 1, character: at - line.from };
}

/** The kinds a server names, as CodeMirror's completion types (its icons). */
const TYPES: Record<string, string> = {
  method: "method",
  function: "function",
  constructor: "class",
  field: "property",
  property: "property",
  variable: "variable",
  constant: "constant",
  class: "class",
  interface: "interface",
  enum: "enum",
  enumMember: "enum",
  module: "namespace",
  keyword: "keyword",
  typeParameter: "type",
  text: "text",
};

/**
 * What starts a completion before a word does: a member (`.`), a JSON key or
 * value (`"`, `:`), an HTML tag or a closing one (`<`, `/`), a CSS at-rule (`@`).
 */
const TRIGGERS = [".", '"', ":", "<", "/", "@"];

/**
 * Markdown from a hover as plain DOM: fenced code as `<pre>`, the rest as
 * paragraphs. Text only (never HTML), so nothing from a server is run.
 */
export function hoverContent(markdown: string): HTMLElement {
  const root = document.createElement("div");
  root.className = "editor-surface-hover";
  const parts = markdown.split(/```[\w-]*\n?/);
  parts.forEach((part, index) => {
    const text = part.trim();
    if (!text) return;
    // Odd parts were inside a fence.
    const element = document.createElement(index % 2 === 1 ? "pre" : "p");
    element.className =
      index % 2 === 1 ? "editor-surface-hover__code" : "editor-surface-hover__text";
    element.textContent = text;
    root.append(element);
  });
  return root;
}

/**
 * Picking `item`: its text replaces `from`–`to`, with its other edits (an
 * import) in the same change. Edits that come only once it's picked
 * (`resolve`) follow in a change of their own, when the text before them is
 * still as it was: their places were counted in that text.
 */
export function applyCompletion(item: SurfaceCompletion) {
  return (view: EditorView, completion: Completion, wordFrom: number, to: number): void => {
    const before = view.state.doc;
    // Where the server's own edit starts, if it's on this line before the cursor.
    const start = item.replaceFrom ? offsetAt(before, item.replaceFrom) : wordFrom;
    const from =
      start <= to && before.lineAt(start).number === before.lineAt(to).number ? start : wordFrom;
    const insert = item.insert ?? item.label;
    const extra = editChanges(before, item.edits ?? []).filter(
      (change) => change.to <= from || change.from >= to,
    );
    const changes = view.state.changes([{ from, to, insert }, ...extra]);
    view.dispatch({
      changes,
      selection: { anchor: changes.mapPos(to, 1) },
      annotations: pickedCompletion.of(completion),
      userEvent: "input.complete",
      scrollIntoView: true,
    });
    if (!item.resolve || (item.edits?.length ?? 0) > 0) return;
    const after = view.state.doc;
    void item.resolve().then((edits) => {
      const later = editChanges(before, edits)
        .filter((change) => change.to <= from || change.from >= to)
        .map((change) => ({
          from: changes.mapPos(change.from, -1),
          to: changes.mapPos(change.to, 1),
          insert: change.insert,
        }));
      if (later.length === 0 || overlapping(later)) return;
      // Places before `end` are still right if nothing before it was typed since.
      const end = Math.max(...later.map((change) => change.to));
      if (view.state.doc.sliceString(0, end) !== after.sliceString(0, end)) return;
      view.dispatch({ changes: later, userEvent: "input.complete" });
    });
  };
}

/**
 * Completion, hover and go to definition, each asking `language()` (the
 * features of the document shown now, if any).
 */
export function languageExtensions(language: () => SurfaceLanguage | undefined): Extension[] {
  const complete = async (context: CompletionContext): Promise<CompletionResult | null> => {
    const features = language();
    if (!features) return null;
    // A word, with CSS's and HTML's hyphens in it.
    const word = context.matchBefore(/[\w$-]*/);
    const before = context.state.sliceDoc(Math.max(0, context.pos - 1), context.pos);
    // Ask after a word starts, after one of TRIGGERS, or when asked (Ctrl+Space).
    if (!context.explicit && (!word || word.from === word.to) && !TRIGGERS.includes(before)) {
      return null;
    }
    const items = await features.completions(positionAt(context.state.doc, context.pos));
    if (!items || context.aborted) return null;
    return {
      from: word ? word.from : context.pos,
      options: items.map((item) => {
        const detail = item.detail ?? item.source;
        const changes = item.resolve || item.replaceFrom || (item.edits?.length ?? 0) > 0;
        return {
          label: item.label,
          ...(item.kind && TYPES[item.kind] ? { type: TYPES[item.kind] } : {}),
          ...(detail ? { detail } : {}),
          ...(changes
            ? { apply: applyCompletion(item) }
            : item.insert && item.insert !== item.label
              ? { apply: item.insert }
              : {}),
        };
      }),
      validFor: /^[\w$-]*$/,
    };
  };

  const goToDefinition = (view: EditorView, offset: number) => {
    const features = language();
    if (!features) return false;
    features.goToDefinition(positionAt(view.state.doc, offset));
    return true;
  };

  return [
    // Added to what the language itself suggests.
    EditorState.languageData.of(() => [{ autocomplete: complete }]),
    hoverTooltip(async (view, offset) => {
      const features = language();
      if (!features) return null;
      const markdown = await features.hover(positionAt(view.state.doc, offset));
      if (!markdown) return null;
      return { pos: offset, above: true, create: () => ({ dom: hoverContent(markdown) }) };
    }),
    Prec.high(
      keymap.of([
        { key: "F12", run: (view) => goToDefinition(view, view.state.selection.main.head) },
      ]),
    ),
    // Ctrl (Cmd on macOS) and a click: go to the definition, not another cursor.
    Prec.high(
      EditorView.domEventHandlers({
        mousedown(event, view) {
          const mod = /Mac/.test(navigator.userAgent) ? event.metaKey : event.ctrlKey;
          if (!mod || event.button !== 0 || !language()) return false;
          const offset = view.posAtCoords({ x: event.clientX, y: event.clientY });
          if (offset === null) return false;
          event.preventDefault();
          return goToDefinition(view, offset);
        },
      }),
    ),
  ];
}
