import "./no-file-open.scss";
import { FileCodeIcon } from "@nuvui/icons";

/** The work area while a folder is open and no file is, after nuvui's app-shell page hint. */
export function NoFileOpen() {
  return (
    <div className="no-file-open">
      <div className="no-file-open__card">
        <span className="no-file-open__icon">
          <FileCodeIcon size={24} />
        </span>
        <h2 className="no-file-open__heading">No file open</h2>
        <p className="no-file-open__text">Choose a file in the explorer to open it here.</p>
      </div>
    </div>
  );
}
