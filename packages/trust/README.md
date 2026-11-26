# @repo/trust

**Status:** after the roadmap.

## Purpose

Workspace trust, as in VS Code. Whether the person trusts the authors of the
open folder's files decides what may run the folder's own code. The backend
enforces it (`ide-ipc`); this package asks and shows it.

- `WorkspaceTrust`: the dialog. It opens by itself the first time an
  untrusted folder opens, and on `workspace.manageTrust` ("Manage Workspace
  Trust…", which it registers).
  - **Untrusted:** it says what stays off in Restricted Mode (`RESTRICTED`):
    source control, Run and Debug, and the language servers that load the
    project's own code. Editing, search, the terminal and the other
    language features work either way.
  - **Trust Folder** trusts the folder. With the checkbox, it trusts the
    folder it's in, and so every folder there. **Stay in Restricted Mode**,
    or closing the dialog, changes nothing.
  - **Trusted:** **Don't Trust** takes the trust back, and with it the
    trust of any parent folder that covered this one.
  - `onChange(trusted)` follows the backend's answer, so the app can read
    git again and restart the language servers.
- `RestrictedStatusItem`: "Restricted Mode" in the status bar while the
  folder isn't trusted. Pressing it opens the dialog.
- `parentOf(path)`: the folder a path is in, as the checkbox shows it.

Other modules point to `workspace.manageTrust` too: the source control
view's Restricted state, and a language server that's off until the folder
is trusted.

## Public API

`WorkspaceTrust` (`workspace`, `trusted`, `port: TrustPort`, `registry`,
`onChange`), `RestrictedStatusItem` (`trusted`, `registry`), `RESTRICTED`,
`parentOf`.

## May depend on

`@repo/contracts`, `@repo/kernel`, `react`, `@nuvui/react`, `@nuvui/icons`.
Never another feature package, `@repo/ipc` or `@tauri-apps/*`.
