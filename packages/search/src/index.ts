// Public entry of @repo/search.
export { type FileMatch, matchFile, matchFiles } from "./fuzzy";
export { QUICK_OPEN_LIMIT, QuickOpen, type QuickOpenProps } from "./quick-open";
export {
  RESULT_PAGE,
  SEARCH_DELAY_MS,
  SearchView,
  type SearchViewProps,
  splitGlobs,
} from "./search-view";
