import "./editor-surface-loading.scss";
import { Spinner } from "@nuvui/react";
import { lazy, Suspense } from "react";
import type { EditorSurfaceProps } from "./contract";

// CodeMirror is most of the editor's weight, and only a screen with a file
// open needs it, so it's loaded then: the first time a file is shown.
const Surface = lazy(() =>
  import("./editor-surface").then((module) => ({ default: module.EditorSurface })),
);

/** The editor, loaded the first time it's drawn. */
export function EditorSurface(props: EditorSurfaceProps) {
  return (
    <Suspense
      fallback={
        <div className="editor-surface-loading">
          <Spinner label="Loading the editor" />
        </div>
      }
    >
      <Surface {...props} />
    </Suspense>
  );
}
