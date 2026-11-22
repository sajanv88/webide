// CodeMirror extensions for language features from a language server:
// completion, hover, and go to definition. This package is the only one that
// may import CodeMirror.
import type { CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { EditorState, type Extension, Prec, type Text } from "@codemirror/state";
import { EditorView, hoverTooltip, keymap } from "@codemirror/view";
import type { SurfaceLanguage, SurfacePosition } from "./contract";

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
 * Completion, hover and go to definition, each asking `language()` (the
 * features of the document shown now, if any).
 */
export function languageExtensions(language: () => SurfaceLanguage | undefined): Extension[] {
  const complete = async (context: CompletionContext): Promise<CompletionResult | null> => {
    const features = language();
    if (!features) return null;
    const word = context.matchBefore(/[\w$]*/);
    const before = context.state.sliceDoc(Math.max(0, context.pos - 1), context.pos);
    // Ask after a word starts, after a dot, or when asked (Ctrl+Space).
    if (!context.explicit && (!word || word.from === word.to) && before !== ".") return null;
    const items = await features.completions(positionAt(context.state.doc, context.pos));
    if (!items || context.aborted) return null;
    return {
      from: word ? word.from : context.pos,
      options: items.map((item) => ({
        label: item.label,
        ...(item.kind && TYPES[item.kind] ? { type: TYPES[item.kind] } : {}),
        ...(item.detail ? { detail: item.detail } : {}),
        ...(item.insert && item.insert !== item.label ? { apply: item.insert } : {}),
      })),
      validFor: /^[\w$]*$/,
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
