# ide-cli

**Status:** after the roadmap.

## Purpose

The `webide` command, so developers can start the app from a terminal:

```sh
webide .                 # the folder you're in
webide ~/projects/app    # another folder
webide src/main.ts       # a file, in its folder
```

- `launch_target(args, cwd)` reads what the arguments ask to open: the first
  one that isn't a flag, relative to the terminal's folder. A folder opens
  as the workspace; a file opens in its folder, and the page opens it. A
  path that isn't there opens nothing, as if none was given.
- `detach_from_terminal()` (macOS and Linux, release builds): started from
  a terminal, the app starts again in its own process group with no
  terminal, and the first copy ends, so the terminal is free at once and
  closing it doesn't end the app. Windows needs none of this: the app has
  no console there.
- `LocalCommandLine` puts the command on the PATH, or takes it off, the way
  VS Code does for `code`:
  - **Windows:** the installer puts `bin\webide.cmd` beside the app (it
    runs `start "" webide.exe %*`), and its hooks run
    `webide.exe --install-command-line` after installing and
    `--uninstall-command-line` before uninstalling. Either adds `bin` to the
    user's own PATH, or takes it out, through Windows PowerShell. PowerShell
    reads the value as stored, so `%USERPROFILE%` and the like stay as they
    are, changes only that one entry, and tells running programs that the
    environment changed. Terminals opened after that see it. The script goes
    to PowerShell as `-EncodedCommand`, and the folder in an environment
    variable, so nothing in either is read as the command line's.
  - **macOS:** a link `/usr/local/bin/webide` to the app's program. When the
    folder isn't writable, macOS asks for an administrator's password. The
    paths go to AppleScript as arguments, quoted there for the shell. Not
    from the disk image, or the copy macOS runs before the app is moved to
    Applications.
  - **Linux:** a `.deb` or `.rpm` puts the program in `/usr/bin` itself.
    Otherwise there's a link `~/.local/bin/webide`, to the AppImage when the
    app is one (`$APPIMAGE`). When that folder isn't on the PATH, the status
    says so.
  - On macOS and Linux it never replaces a file that isn't a link (someone
    else's program), and uninstalling takes away only a link to this app.
- `run_flag(args, line)`: the installer's two flags. The program does what
  they ask and ends, without opening a window.

## Public API

- `COMMAND` (`"webide"`), `launch_target`, `detach_from_terminal`, `run_flag`.
- `LocalCommandLine::new(exe)`, which implements `ide_core::cli::CommandLine`;
  `with_link(exe, link)` (macOS and Linux, for tests).

## May depend on

`ide-core` only.
