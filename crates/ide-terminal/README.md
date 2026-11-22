# ide-terminal

**Status:** Milestone 4.

## Purpose

Terminals: the person's shell on the system's pseudo-terminal (ConPTY on
Windows, a pty on macOS and Linux), through `portable-pty`.

- **The shell** is the person's default: PowerShell 7 (`pwsh`) when it's
  installed, else Windows PowerShell, on Windows; `$SHELL` (or `/bin/sh`)
  elsewhere, as a login shell on macOS (an app started from the Dock gets a
  bare `PATH`; a login shell reads the person's profile, as Terminal.app
  does). It starts in the workspace folder, with the app's environment plus
  `TERM=xterm-256color` and `COLORTERM=truecolor`.
- **The webview never chooses** the program, its arguments or its folder.
- **Output** is read on a thread of its own, 64 KiB at a time, and decoded as
  UTF-8 without splitting a character between two reads
  (`ide_core::terminal::Utf8Stream`). Bytes that aren't UTF-8 become U+FFFD.
- **The exit** is reported after the last output: when the shell ends, its
  output has 300 ms to arrive, then the terminal is closed (ConPTY keeps its
  pipe open until then), then `Exited` with the exit code.
- **Dropping a session** ends the shell (and what runs in its foreground).
- On Windows, ConPTY starts by asking the terminal where its cursor is
  (`ESC[6n`) and waits for the answer; xterm.js answers.

### Known limits

- The shell can't be chosen yet; nor its arguments or environment.
- A shell's child processes in the background may outlive it.

## Public API

- `LocalTerminals` (implements `ide_core::TerminalHost`), with
  `LocalTerminals::with_shell(program, args)` for tests.
- `default_shell()` → the program and its arguments.

## May depend on

`ide-core`, `portable-pty`. Never another workspace crate, Tauri or an async
runtime. Checked by `cargo xtask check-boundaries`.
