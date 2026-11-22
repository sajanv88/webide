// Public entry of @repo/editor-surface: the EditorSurface contract and its
// implementation. Which editor that is stays inside this package.
export type {
  DiffSurfaceProps,
  EditorSurfaceHandle,
  EditorSurfaceProps,
  SurfaceDiagnostic,
} from "./contract";
export { DiffSurface, EditorSurface } from "./lazy-surface";
