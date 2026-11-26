@echo off
rem The `webide` command: opens the folder or file it names (relative to the
rem terminal's folder) in webide, and leaves the terminal free. The installer
rem puts this folder on the PATH.
start "" "%~dp0..\webide.exe" %*
