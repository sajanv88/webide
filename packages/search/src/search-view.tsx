import "./search-view.scss";
import { FileIcon, FilterIcon, RefreshIcon } from "@nuvui/icons";
import {
  Button,
  Field,
  FieldControl,
  FieldError,
  FieldLabel,
  Input,
  ScrollArea,
  Toggle,
  Tree,
  TreeItem,
  VisuallyHidden,
} from "@nuvui/react";
import type {
  AppCommandId,
  AppEvents,
  FileMatchesDto,
  SearchPort,
  TextSearchDto,
} from "@repo/contracts";
import type { CommandRegistry, EventBus } from "@repo/kernel";
import {
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { Highlighted, type Range } from "./highlight";

/** How long typing has to pause before a search starts. */
export const SEARCH_DELAY_MS = 250;
/** Matching lines drawn before a "Show more" row. */
export const RESULT_PAGE = 1000;

/** Values of rows that aren't lines. A path never contains NUL. */
const SEP = "\u0000";
const MORE = `${SEP}more`;

export interface SearchViewProps {
  /** Searches the open workspace. */
  readonly port: SearchPort;
  readonly registry: CommandRegistry<AppCommandId>;
  readonly bus: EventBus<AppEvents>;
  /** The shell view this is in; `search.findInFiles` shows it with `view.showView`. */
  readonly viewId: string;
  /** CSP nonce for nuvui ScrollArea's `<style>` element. */
  readonly styleNonce?: string | undefined;
}

interface Options {
  readonly matchCase: boolean;
  readonly wholeWord: boolean;
  readonly isRegex: boolean;
}

type Results =
  | { readonly status: "idle" }
  | { readonly status: "searching"; readonly previous: Done | null }
  | Done
  | { readonly status: "invalid"; readonly message: string }
  | { readonly status: "failed"; readonly message: string };

interface Done {
  readonly status: "done";
  /** Which search this was: a new one draws its tree afresh. */
  readonly id: number;
  readonly pattern: string;
  readonly found: TextSearchDto;
}

/** Globs typed as "src/**, *.ts": split at commas, blanks dropped. */
export function splitGlobs(text: string): string[] {
  return text
    .split(",")
    .map((glob) => glob.trim())
    .filter((glob) => glob !== "");
}

/** Find in files: a search form, and what it found as a tree of files and lines. */
export function SearchView({ port, registry, bus, viewId, styleNonce }: SearchViewProps) {
  const headingId = useId();
  const filtersId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [pattern, setPattern] = useState("");
  const [options, setOptions] = useState<Options>({
    matchCase: false,
    wholeWord: false,
    isRegex: false,
  });
  const [showFilters, setShowFilters] = useState(false);
  const [include, setInclude] = useState("");
  const [exclude, setExclude] = useState("");
  const [results, setResults] = useState<Results>({ status: "idle" });
  const [again, setAgain] = useState(0);
  const request = useRef(0);
  // Enter and the refresh button search at once, not after the typing pause.
  const immediate = useRef(false);

  useEffect(
    () =>
      registry.register({
        id: "search.findInFiles",
        title: "Find in Files",
        category: "Search",
        run: async () => {
          await registry.execute("view.showView", viewId);
          requestAnimationFrame(() => {
            inputRef.current?.focus();
            inputRef.current?.select();
          });
        },
      }),
    [registry, viewId],
  );

  // Search as the person types, once they pause; Enter searches at once.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `again` is how Enter and the refresh button ask for the same search again
  useEffect(() => {
    const id = ++request.current;
    if (pattern === "") {
      setResults({ status: "idle" });
      return;
    }
    const delay = immediate.current ? 0 : SEARCH_DELAY_MS;
    immediate.current = false;
    const timer = setTimeout(async () => {
      setResults((now) => ({
        status: "searching",
        previous: now.status === "done" ? now : now.status === "searching" ? now.previous : null,
      }));
      const result = await port.searchText({
        pattern,
        ...options,
        include: splitGlobs(include),
        exclude: splitGlobs(exclude),
      });
      // A newer search replaced this one.
      if (id !== request.current) return;
      if (result.ok) {
        if (!result.value.cancelled) {
          setResults({ status: "done", id, pattern, found: result.value });
        }
      } else if (result.error.code === "invalidQuery") {
        setResults({ status: "invalid", message: result.error.message });
      } else {
        setResults({ status: "failed", message: result.error.message });
      }
    }, delay);
    return () => clearTimeout(timer);
  }, [port, pattern, options, include, exclude, again]);

  const searchNow = () => {
    immediate.current = true;
    setAgain((count) => count + 1);
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    searchNow();
  };
  // Enter in any field searches at once.
  const onFieldKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    event.preventDefault();
    searchNow();
  };

  const shown =
    results.status === "done" ? results : results.status === "searching" ? results.previous : null;
  const invalid = results.status === "invalid" ? results.message : undefined;

  return (
    <section className="search-view" aria-labelledby={headingId}>
      <div className="search-view__header">
        <h2 id={headingId} className="search-view__title">
          Search
        </h2>
        <Button
          intent="ghost"
          size="sm"
          className="search-view__icon"
          aria-label="Search again"
          disabled={pattern === ""}
          onClick={searchNow}
        >
          <RefreshIcon size={16} />
        </Button>
      </div>
      {/* biome-ignore lint/a11y/useSemanticElements: a <search> element isn't announced as a landmark by every screen reader yet; role="search" on the form is */}
      <form
        className="search-view__form"
        role="search"
        aria-label="Find in files"
        onSubmit={submit}
      >
        <Field controlId="search-pattern">
          <FieldLabel className="search-view__label">Search</FieldLabel>
          <FieldControl>
            <Input
              ref={inputRef}
              value={pattern}
              onChange={(event) => setPattern(event.currentTarget.value)}
              placeholder="Text to find"
              autoComplete="off"
              spellCheck={false}
              onKeyDown={onFieldKey}
            />
          </FieldControl>
          <FieldError>{invalid}</FieldError>
        </Field>
        <div className="search-view__options">
          <Toggle
            size="sm"
            aria-label="Match case"
            title="Match case"
            pressed={options.matchCase}
            onPressedChange={(matchCase) => setOptions((now) => ({ ...now, matchCase }))}
          >
            <span className="search-view__glyph" aria-hidden="true">
              Aa
            </span>
          </Toggle>
          <Toggle
            size="sm"
            aria-label="Match whole word"
            title="Match whole word"
            pressed={options.wholeWord}
            onPressedChange={(wholeWord) => setOptions((now) => ({ ...now, wholeWord }))}
          >
            <span className="search-view__glyph search-view__glyph--word" aria-hidden="true">
              ab
            </span>
          </Toggle>
          <Toggle
            size="sm"
            aria-label="Use regular expression"
            title="Use regular expression"
            pressed={options.isRegex}
            onPressedChange={(isRegex) => setOptions((now) => ({ ...now, isRegex }))}
          >
            <span className="search-view__glyph" aria-hidden="true">
              .*
            </span>
          </Toggle>
          <Toggle
            size="sm"
            className="search-view__filters-button"
            aria-label="Files to include or exclude"
            title="Files to include or exclude"
            aria-controls={filtersId}
            pressed={showFilters}
            onPressedChange={setShowFilters}
          >
            <FilterIcon size={16} />
          </Toggle>
        </div>
        {/* `hidden` on the outer box: the inner one's grid would override it. */}
        <div id={filtersId} hidden={!showFilters}>
          <div className="search-view__filters">
            <Field controlId="search-include">
              <FieldLabel className="search-view__label">Files to include</FieldLabel>
              <FieldControl>
                <Input
                  value={include}
                  onChange={(event) => setInclude(event.currentTarget.value)}
                  placeholder="src/**, *.ts"
                  autoComplete="off"
                  spellCheck={false}
                  onKeyDown={onFieldKey}
                />
              </FieldControl>
            </Field>
            <Field controlId="search-exclude">
              <FieldLabel className="search-view__label">Files to exclude</FieldLabel>
              <FieldControl>
                <Input
                  value={exclude}
                  onChange={(event) => setExclude(event.currentTarget.value)}
                  placeholder="*.test.ts, dist/"
                  autoComplete="off"
                  spellCheck={false}
                  onKeyDown={onFieldKey}
                />
              </FieldControl>
            </Field>
          </div>
        </div>
      </form>
      <p className="search-view__status" role="status">
        {statusText(results)}
      </p>
      {shown && shown.found.files.length > 0 ? (
        <ResultTree
          // A new search starts from the top, with every file open.
          key={shown.id}
          found={shown.found}
          bus={bus}
          styleNonce={styleNonce}
        />
      ) : null}
    </section>
  );
}

function statusText(results: Results): string {
  switch (results.status) {
    case "idle":
      return "";
    case "searching":
      return "Searching…";
    case "invalid":
      return "The search isn't valid: see the message under it.";
    case "failed":
      return `Couldn't search: ${results.message}`;
    case "done": {
      const { files, truncated } = results.found;
      if (files.length === 0) return `No results for “${results.pattern}”.`;
      const lines = files.reduce((sum, file) => sum + file.lines.length, 0);
      const text = `${count(lines, "result")} in ${count(files.length, "file")}.`;
      return truncated
        ? `${text} The search stopped there: there may be more. Narrow it to see them.`
        : text;
    }
  }
}

const count = (n: number, word: string): string =>
  `${n.toLocaleString("en")} ${word}${n === 1 ? "" : "s"}`;

function ResultTree({
  found,
  bus,
  styleNonce,
}: {
  readonly found: TextSearchDto;
  readonly bus: EventBus<AppEvents>;
  readonly styleNonce?: string | undefined;
}) {
  const [expanded, setExpanded] = useState<string[]>(() => found.files.map((file) => file.path));
  const [selected, setSelected] = useState<string[]>([]);
  const [limit, setLimit] = useState(RESULT_PAGE);
  const byPath = useMemo(() => new Map(found.files.map((file) => [file.path, file])), [found]);

  const onValueChange = (value: string[]) => {
    const chosen = value[0];
    if (chosen === MORE) {
      setLimit((now) => now + RESULT_PAGE);
      return;
    }
    if (!chosen?.includes(SEP)) return; // a file row: it only opens and closes
    setSelected(value);
    const [path = "", index = ""] = chosen.split(SEP);
    const line = byPath.get(path)?.lines[Number(index)];
    if (line) bus.emit("file.openRequested", { path, line: line.line, column: line.column });
  };

  // Lines are drawn up to `limit`, counting every file's.
  const rows: ReactNode[] = [];
  let left = limit;
  let hidden = 0;
  for (const file of found.files) {
    if (left <= 0) {
      hidden += file.lines.length;
      continue;
    }
    const lines = file.lines.slice(0, left);
    left -= lines.length;
    hidden += file.lines.length - lines.length;
    rows.push(<FileRow key={file.path} file={file} lines={lines.length} />);
  }
  if (hidden > 0) {
    rows.push(
      <TreeItem
        key={MORE}
        value={MORE}
        label={`Show ${Math.min(RESULT_PAGE, hidden).toLocaleString("en")} more (${hidden.toLocaleString("en")} not shown)`}
      />,
    );
  }

  return (
    <div className="search-view__results">
      <ScrollArea
        className="search-view__scroll"
        aria-label="Search results"
        {...(styleNonce ? { nonce: styleNonce } : {})}
      >
        <Tree
          aria-label="Search results"
          className="search-view__tree"
          selectionMode="single"
          value={selected}
          onValueChange={onValueChange}
          expanded={expanded}
          onExpandedChange={setExpanded}
        >
          {rows}
        </Tree>
      </ScrollArea>
    </div>
  );
}

/** A file with its first `lines` matching lines under it. */
function FileRow({ file, lines }: { readonly file: FileMatchesDto; readonly lines: number }) {
  const slash = file.path.lastIndexOf("/");
  const name = file.path.slice(slash + 1);
  const folder = slash > 0 ? file.path.slice(0, slash) : "";
  return (
    <TreeItem
      value={file.path}
      textValue={name}
      label={
        <span className="search-file">
          <FileIcon size={16} className="search-file__icon" />
          <span className="search-file__name">{name}</span>
          {folder ? <span className="search-file__folder">{folder}</span> : null}
          <span className="search-file__count">
            {file.lines.length}
            <VisuallyHidden>{file.lines.length === 1 ? " match" : " matches"}</VisuallyHidden>
          </span>
        </span>
      }
    >
      {file.lines.slice(0, lines).map((line, index) => (
        <TreeItem
          key={line.line}
          value={`${file.path}${SEP}${index}`}
          textValue={line.preview}
          label={
            <span className="search-line">
              <span className="search-line__number">{line.line}</span>
              <span className="search-line__text">
                <Highlighted
                  text={line.preview}
                  ranges={line.spans.map(({ start, end }): Range => [start, end])}
                />
              </span>
            </span>
          }
        />
      ))}
    </TreeItem>
  );
}
