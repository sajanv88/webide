import { toast } from "@nuvui/react";
import type {
  AppEvents,
  CompletionDto,
  EditorProblem,
  LanguagePort,
  ProblemDto,
  SourceActionDto,
  TextEditDto,
} from "@repo/contracts";
import type { EventBus } from "@repo/kernel";

/** How long typing pauses before the server hears the new text. */
export const CHANGE_DELAY_MS = 150;
/** How long changes settle before problems are asked for again. */
export const PROBLEMS_DELAY_MS = 500;
/** At most this many open files have their problems refreshed after a change. */
const REFRESH_LIMIT = 20;

/** A place in a text: 0-based line, and column in UTF-16 units. */
export interface Position {
  readonly line: number;
  readonly character: number;
}

/** A change to the text: what's between `from` and `to` becomes `insert`. */
export interface Edit {
  readonly from: Position;
  readonly to: Position;
  readonly insert: string;
}

/** One completion, as the editor shows it. */
export interface CompletionItem {
  readonly label: string;
  readonly kind?: string;
  readonly detail?: string;
  readonly insert?: string;
  /** Where what's inserted starts, when not where the word typed does. */
  readonly replaceFrom?: Position;
  /** The module it would be imported from. */
  readonly source?: string;
  /** Other changes made with it, such as an import. */
  readonly edits?: readonly Edit[];
  /** Asked when it's picked: the import it adds. */
  readonly resolve?: () => Promise<readonly Edit[]>;
}

/** A fix the server offers for a problem. */
export interface Fix {
  readonly title: string;
  readonly preferred: boolean;
  readonly edits: readonly Edit[];
}

/** What the editor asks about the file it shows (the editor surface's `SurfaceLanguage`). */
export interface LanguageFeatures {
  completions(at: Position): Promise<readonly CompletionItem[] | null>;
  hover(at: Position): Promise<string | null>;
  goToDefinition(at: Position): void;
  /** The fixes for the problems between `from` and `to`. */
  fixes(from: Position, to: Position): Promise<readonly Fix[] | null>;
  /** The edits that organize the imports (or another source action); `null` when not offered. */
  sourceAction(action: SourceActionDto): Promise<readonly Edit[] | null>;
}

export interface LanguageClient {
  /** Whether the language server handles this file. */
  handles(path: string): boolean;
  /** The features of one file, for its editor. The same object each time. */
  features(path: string): LanguageFeatures;
  /** The latest problems of an open file. */
  problems(path: string): readonly EditorProblem[];
  /** Stops the server and starts it again with the open files. */
  restart(): Promise<void>;
  dispose(): void;
}

/**
 * The files a language server here handles: scripts (TypeScript), JSON, CSS,
 * SCSS and Less, HTML, YAML. The backend decides which server.
 */
const HANDLED = /\.(?:[mc]?[jt]s|[jt]sx|jsonc?|css|scss|less|html?|ya?ml|vue|svelte)$/i;

/** Character offsets of each line's start, in UTF-16 units (as JavaScript counts). */
function lineStarts(text: string): number[] {
  const starts = [0];
  for (let at = 0; at < text.length; at += 1) {
    if (text.charCodeAt(at) === 10) starts.push(at + 1);
  }
  return starts;
}

/** Problems from the server, placed in `text` by offset. */
export function placeProblems(text: string, problems: readonly ProblemDto[]): EditorProblem[] {
  const starts = lineStarts(text);
  const offset = (line: number, character: number) => {
    const start = starts[Math.min(line, starts.length - 1)] ?? 0;
    const end = starts[line + 1] ?? text.length + 1;
    return Math.min(start + character, end - 1, text.length);
  };
  return problems.map((problem) => {
    const from = offset(problem.range.start.line, problem.range.start.character);
    const to = Math.max(from, offset(problem.range.end.line, problem.range.end.character));
    return {
      from,
      to,
      severity: problem.severity,
      message: problem.message,
      ...(problem.source ? { source: problem.source } : {}),
      line: problem.range.start.line + 1,
      column: problem.range.start.character + 1,
    };
  });
}

const kindOf = (item: CompletionDto): string => item.kind;

/** A server's edit, as the editor takes it. */
export const toEdit = (edit: TextEditDto): Edit => ({
  from: edit.range.start,
  to: edit.range.end,
  insert: edit.newText,
});

/**
 * Keeps the workspace's language server in step with the open files (from
 * the `document.*` events), asks it for problems as files change (sent as
 * `language.problems`), and answers the editor's questions.
 */
export function createLanguageClient(port: LanguagePort, bus: EventBus<AppEvents>): LanguageClient {
  const texts = new Map<string, string>();
  const sent = new Map<string, string>();
  const pending = new Map<string, ReturnType<typeof setTimeout>>();
  const found = new Map<string, readonly EditorProblem[]>();
  const features = new Map<string, LanguageFeatures>();
  let problemsTimer: ReturnType<typeof setTimeout> | undefined;
  let live = true;
  const handles = (path: string) => HANDLED.test(path);

  const refresh = async (path: string) => {
    const result = await port.problems(path);
    const text = texts.get(path);
    if (!live || !result.ok || text === undefined) return;
    const problems = placeProblems(text, result.value);
    found.set(path, problems);
    bus.emit("language.problems", { path, problems });
  };
  // A change in one file can change another's problems: refresh the open ones.
  const refreshSoon = () => {
    clearTimeout(problemsTimer);
    problemsTimer = setTimeout(() => {
      for (const path of [...texts.keys()].slice(0, REFRESH_LIMIT)) void refresh(path);
    }, PROBLEMS_DELAY_MS);
  };

  /** Sends a file's latest text now, if the server hasn't had it. */
  const flush = async (path: string) => {
    clearTimeout(pending.get(path));
    pending.delete(path);
    const text = texts.get(path);
    if (text === undefined || sent.get(path) === text) return;
    sent.set(path, text);
    await port.change(path, text);
  };

  const open = async (path: string, text: string) => {
    sent.set(path, text);
    const result = await port.open(path, text);
    if (result.ok) await refresh(path);
  };

  const stops = [
    // A server found problems by itself (its project loaded, it pushed new ones).
    bus.on("language.problemsChanged", () => refreshSoon()),
    bus.on("document.opened", ({ path, text }) => {
      if (!handles(path)) return;
      texts.set(path, text);
      void open(path, text);
    }),
    bus.on("document.changed", ({ path, text }) => {
      if (!handles(path) || !texts.has(path)) return;
      texts.set(path, text);
      clearTimeout(pending.get(path));
      pending.set(
        path,
        setTimeout(() => {
          void flush(path).then(refreshSoon);
        }, CHANGE_DELAY_MS),
      );
    }),
    bus.on("document.closed", ({ path }) => {
      if (!texts.has(path)) return;
      clearTimeout(pending.get(path));
      pending.delete(path);
      texts.delete(path);
      sent.delete(path);
      found.delete(path);
      features.delete(path);
      bus.emit("language.problems", { path, problems: [] });
      void port.close(path);
    }),
  ];

  const featuresFor = (path: string): LanguageFeatures => ({
    async completions(at) {
      await flush(path);
      const result = await port.completions(path, at.line, at.character);
      if (!result.ok) return null;
      return result.value.items.map((item): CompletionItem => {
        const token = item.resolve;
        return {
          label: item.label,
          kind: kindOf(item),
          ...(item.detail ? { detail: item.detail } : {}),
          ...(item.insert ? { insert: item.insert } : {}),
          ...(item.replaceFrom ? { replaceFrom: item.replaceFrom } : {}),
          ...(item.source ? { source: item.source } : {}),
          ...(item.edits.length > 0 ? { edits: item.edits.map(toEdit) } : {}),
          ...(token
            ? {
                resolve: async () => {
                  const resolved = await port.resolveCompletion(path, token);
                  return resolved.ok ? resolved.value.edits.map(toEdit) : [];
                },
              }
            : {}),
        };
      });
    },
    async fixes(from, to) {
      await flush(path);
      const result = await port.fixes(path, from, to);
      if (!result.ok) return null;
      return result.value.map((fix) => ({
        title: fix.title,
        preferred: fix.preferred,
        edits: fix.edits.map(toEdit),
      }));
    },
    async sourceAction(action) {
      await flush(path);
      const result = await port.sourceAction(path, action);
      if (!result.ok) {
        toast("The language server couldn't do that", {
          intent: "danger",
          description: result.error.message,
        });
        return null;
      }
      return result.value ? result.value.map(toEdit) : null;
    },
    async hover(at) {
      await flush(path);
      const result = await port.hover(path, at.line, at.character);
      return result.ok && result.value ? result.value.markdown : null;
    },
    goToDefinition(at) {
      void (async () => {
        await flush(path);
        const result = await port.definition(path, at.line, at.character);
        if (!result.ok) {
          toast("Couldn't find the definition", {
            intent: "danger",
            description: result.error.message,
          });
          return;
        }
        const [target] = result.value;
        if (!target) {
          toast("No definition found here");
          return;
        }
        if (target.kind === "workspace") {
          bus.emit("file.openRequested", {
            path: target.path,
            line: target.range.start.line + 1,
            column: target.range.start.character + 1,
          });
        } else {
          const name = target.path.slice(target.path.lastIndexOf("/") + 1);
          toast(`Defined in ${name}`, {
            description:
              "That file is outside this folder (the language's or a library's own types).",
          });
        }
      })();
    },
  });

  return {
    handles,
    features(path) {
      let existing = features.get(path);
      if (!existing) {
        existing = featuresFor(path);
        features.set(path, existing);
      }
      return existing;
    },
    problems: (path) => found.get(path) ?? [],
    async restart() {
      await port.restart();
      // The new server starts with the open files.
      for (const [path, text] of texts) await open(path, text);
    },
    dispose() {
      live = false;
      clearTimeout(problemsTimer);
      for (const timer of pending.values()) clearTimeout(timer);
      for (const stop of stops) stop();
    },
  };
}
