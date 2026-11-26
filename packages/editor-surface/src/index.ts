// Public entry of @repo/editor-surface: the EditorSurface contract and its
// implementation. Which editor that is stays inside this package.
export type {
  DiffSurfaceProps,
  EditorSurfaceHandle,
  EditorSurfaceProps,
  SurfaceActionResult,
  SurfaceCompletion,
  SurfaceDiagnostic,
  SurfaceDisplay,
  SurfaceEdit,
  SurfaceFix,
  SurfaceLanguage,
  SurfacePosition,
  SurfaceSourceAction,
} from "./contract";
export { DiffSurface, EditorSurface } from "./lazy-surface";
