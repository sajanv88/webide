/** A file that matches what was typed, with where each typed character matched. */
export interface FileMatch {
  readonly path: string;
  readonly score: number;
  /** Indices into `path`, ascending. */
  readonly positions: readonly number[];
}

const BOUNDARY = new Set(["/", ".", "-", "_", " "]);

/**
 * Matches `query`'s characters in order (not necessarily next to each other)
 * against `path`, ignoring case and spaces. Matches in the file's name rank
 * above matches spread over its folders; consecutive characters and the start
 * of a word count extra. `null` when it doesn't match.
 */
export function matchFile(path: string, query: string): FileMatch | null {
  const wanted = query.replace(/\s+/g, "").toLowerCase();
  if (wanted === "") return { path, score: 0, positions: [] };
  const lower = path.toLowerCase();
  const nameStart = path.lastIndexOf("/") + 1;
  // In the name alone first, then anywhere.
  const inName = positionsOf(path, lower, wanted, nameStart);
  const positions = inName ?? positionsOf(path, lower, wanted, 0);
  if (!positions) return null;

  let score = inName ? 1000 : 0;
  positions.forEach((at, index) => {
    score += 1;
    if (index > 0 && positions[index - 1] === at - 1) score += 10;
    const before = path[at - 1];
    if (at === 0 || at === nameStart || (before !== undefined && BOUNDARY.has(before))) {
      score += 8;
    } else if (before !== undefined && isLower(before) && isUpper(path[at] ?? "")) {
      score += 6;
    }
  });
  if (inName && path.slice(nameStart).toLowerCase().startsWith(wanted)) score += 50;
  return { path, score, positions };
}

/**
 * The best `limit` matches, best first; ties go to the shorter path, then in
 * path order. With nothing typed, the first `limit` files as given.
 */
export function matchFiles(files: readonly string[], query: string, limit: number): FileMatch[] {
  if (query.trim() === "") {
    return files.slice(0, limit).map((path) => ({ path, score: 0, positions: [] }));
  }
  const matches: FileMatch[] = [];
  for (const path of files) {
    const match = matchFile(path, query);
    if (match) matches.push(match);
  }
  matches.sort(
    (a, b) =>
      b.score - a.score ||
      a.path.length - b.path.length ||
      (a.path < b.path ? -1 : a.path > b.path ? 1 : 0),
  );
  return matches.slice(0, limit);
}

/**
 * Where each character of `wanted` matches in `lower` (the lowercased
 * `path`) from `from`, in order. Each takes its next place, or the next start
 * of a word when the rest still matches after it ("dtabs" in
 * "document-tabs": the t of "tabs", not of "document").
 */
function positionsOf(path: string, lower: string, wanted: string, from: number): number[] | null {
  const positions: number[] = [];
  let at = from;
  for (let index = 0; index < wanted.length; index += 1) {
    const char = wanted[index] as string;
    const next = lower.indexOf(char, at);
    if (next < 0) return null;
    let chosen = next;
    // Right after the previous character, it stays: a run beats a jump.
    if (next !== at && !startsWord(path, next, from)) {
      for (
        let word = lower.indexOf(char, next + 1);
        word >= 0;
        word = lower.indexOf(char, word + 1)
      ) {
        if (!startsWord(path, word, from)) continue;
        if (fits(lower, wanted.slice(index + 1), word + 1)) chosen = word;
        break;
      }
    }
    positions.push(chosen);
    at = chosen + 1;
  }
  return positions;
}

/** Whether `rest` matches, in order, in `text` from `from`. */
function fits(text: string, rest: string, from: number): boolean {
  let at = from;
  for (const char of rest) {
    const found = text.indexOf(char, at);
    if (found < 0) return false;
    at = found + 1;
  }
  return true;
}

/** Whether `path[at]` begins a word: the start, after a separator, or a capital after a small letter. */
function startsWord(path: string, at: number, start: number): boolean {
  if (at === start || at === 0) return true;
  const before = path[at - 1];
  if (before === undefined) return true;
  return BOUNDARY.has(before) || (isLower(before) && isUpper(path[at] ?? ""));
}

const isLower = (char: string): boolean => char !== char.toUpperCase();
const isUpper = (char: string): boolean =>
  char !== char.toLowerCase() && char === char.toUpperCase();
