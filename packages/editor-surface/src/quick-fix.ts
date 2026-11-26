// The quick fix menu: the fixes a language server offers for the problem at
// the cursor, in a menu drawn there (Ctrl+. or Cmd+., through the app's
// command). Picking one makes its edits; Escape, Tab or a click elsewhere
// closes it and gives the editor back its focus.
import { type EditorState, StateEffect, StateField, type Text } from "@codemirror/state";
import { type EditorView, showTooltip, type Tooltip } from "@codemirror/view";
import type { SurfaceFix, SurfaceLanguage } from "./contract";
import { editChanges, overlapping } from "./edits";
import { positionAt } from "./language";

interface FixMenu {
  /** Where it's drawn. */
  readonly pos: number;
  /** The fixes, the preferred first; none to say there aren't any. */
  readonly fixes: readonly SurfaceFix[];
  /** The text they were made for: changed, they no longer fit. */
  readonly doc: Text;
}

const setFixMenu = StateEffect.define<FixMenu | null>();

/** The fixes, the one the language prefers first. */
export function orderFixes(fixes: readonly SurfaceFix[]): SurfaceFix[] {
  return [...fixes].sort((a, b) => Number(b.preferred) - Number(a.preferred));
}

/** Makes a fix's edits, if the text is still the one it was made for. */
export function applyFix(view: EditorView, fix: SurfaceFix, doc: Text): boolean {
  if (view.state.doc !== doc) return false;
  const changes = editChanges(doc, fix.edits);
  if (changes.length === 0 || overlapping(changes)) return false;
  view.dispatch({ changes, userEvent: "input.fix", scrollIntoView: true });
  return true;
}

function menuDom(view: EditorView, menu: FixMenu): HTMLElement {
  const close = () => {
    view.dispatch({ effects: setFixMenu.of(null) });
    view.focus();
  };
  if (menu.fixes.length === 0) {
    const note = document.createElement("div");
    note.className = "editor-surface-fixes editor-surface-fixes--empty";
    note.setAttribute("role", "status");
    note.textContent = "No quick fixes here";
    return note;
  }
  const root = document.createElement("div");
  root.className = "editor-surface-fixes";
  root.setAttribute("role", "menu");
  root.setAttribute("aria-label", "Quick fixes");
  const items = menu.fixes.map((fix) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "editor-surface-fixes__item";
    item.setAttribute("role", "menuitem");
    item.tabIndex = -1;
    item.textContent = fix.title;
    item.addEventListener("click", () => {
      applyFix(view, fix, menu.doc);
      close();
    });
    return item;
  });
  root.append(...items);
  root.addEventListener("keydown", (event) => {
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const move = (to: number) => {
      event.preventDefault();
      items[(to + items.length) % items.length]?.focus();
    };
    if (event.key === "ArrowDown") move(at + 1);
    else if (event.key === "ArrowUp") move(at - 1);
    else if (event.key === "Home") move(0);
    else if (event.key === "End") move(items.length - 1);
    else if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  });
  // Focus leaving the menu (a click in the text, another window) closes it.
  // Not at once: focus also leaves when the menu is taken away, inside an
  // update of the editor, which can't start another.
  root.addEventListener("focusout", (event) => {
    if (root.contains(event.relatedTarget as Node | null)) return;
    setTimeout(() => {
      if (fixMenuOpen(view.state)) view.dispatch({ effects: setFixMenu.of(null) });
    });
  });
  return root;
}

const fixMenu = StateField.define<FixMenu | null>({
  create: () => null,
  update(value, transaction) {
    for (const effect of transaction.effects) if (effect.is(setFixMenu)) return effect.value;
    // Typing or moving away closes it: its fixes were for that text and place.
    if (value && (transaction.docChanged || transaction.selection)) return null;
    return value;
  },
  provide: (field) =>
    showTooltip.from(field, (menu): Tooltip | null =>
      menu
        ? {
            pos: menu.pos,
            above: false,
            create: (view) => {
              const dom = menuDom(view, menu);
              return {
                dom,
                // Into the menu, so the arrow keys move through it.
                mount: () => dom.querySelector<HTMLElement>('[role="menuitem"]')?.focus(),
              };
            },
          }
        : null,
    ),
});

/** The quick fix menu's state: one of the editor's extensions. */
export const fixMenuExtension = [fixMenu];

/** Whether the menu (or the note that there are no fixes) is open. */
export const fixMenuOpen = (state: EditorState): boolean => state.field(fixMenu, false) != null;

/**
 * Asks for the fixes at the cursor (or the selection) and shows them there.
 * A note says so when there are none; it closes with the next key or move.
 */
export async function showFixes(
  view: EditorView,
  language: SurfaceLanguage | undefined,
): Promise<void> {
  const { from, to, head } = view.state.selection.main;
  const doc = view.state.doc;
  const fixes = language?.fixes
    ? await language.fixes(positionAt(doc, from), positionAt(doc, to))
    : null;
  // Typed on meanwhile: those fixes are for text that's gone.
  if (view.state.doc !== doc) return;
  view.dispatch({ effects: setFixMenu.of({ pos: head, fixes: orderFixes(fixes ?? []), doc }) });
  if (!fixes?.length) {
    // The note goes with the next key, wherever focus is.
    const dismiss = () => {
      if (fixMenuOpen(view.state)) view.dispatch({ effects: setFixMenu.of(null) });
    };
    view.contentDOM.addEventListener("keydown", dismiss, { once: true });
    view.contentDOM.addEventListener("blur", dismiss, { once: true });
  }
}
