//! Windows: `bin\webide.cmd` beside the app (the installer puts it there),
//! with `bin` in the user's own PATH.
//!
//! The PATH is changed by Windows PowerShell, which every Windows 10 and 11
//! has: it reads the value as stored (`%USERPROFILE%` and the like left as
//! they are), adds or takes out only our folder, and tells running programs
//! the environment changed, so new terminals see it.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ide_core::IdeError;
use ide_core::cli::CommandLineStatus;

use crate::LocalCommandLine;

/// No console window flashes up from the app.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `WEBIDE_PATH_ACTION` is `status`, `install` or `uninstall`; `WEBIDE_BIN` the folder.
const SCRIPT: &str = r"
$ErrorActionPreference = 'Stop'
$dir = $env:WEBIDE_BIN
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$names = $key.GetValueNames()
$old = ''
$kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
if ($names -contains 'Path') {
  $old = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
  $kind = $key.GetValueKind('Path')
}
$parts = @($old -split ';' | Where-Object { $_ -ne '' })
$ours = { param($entry) $entry.TrimEnd('\') -ieq $dir.TrimEnd('\') }
$present = @($parts | Where-Object { & $ours $_ }).Count -gt 0
switch ($env:WEBIDE_PATH_ACTION) {
  'status' { if ($present) { 'present' } else { 'absent' }; exit 0 }
  'install' { if ($present) { 'present'; exit 0 }; $new = @($parts + $dir) }
  'uninstall' { if (-not $present) { 'absent'; exit 0 }; $new = @($parts | Where-Object { -not (& $ours $_) }) }
}
$key.SetValue('Path', ($new -join ';'), $kind)
# Setting and clearing a variable tells running programs (Explorer, new
# terminals) that the environment changed.
[Environment]::SetEnvironmentVariable('WEBIDE_PATH_CHANGED', '1', 'User')
[Environment]::SetEnvironmentVariable('WEBIDE_PATH_CHANGED', $null, 'User')
'changed'
";

/// The folder with `webide.cmd`, if this copy of the app has one.
fn bin_folder(exe: &Path) -> Result<PathBuf, CommandLineStatus> {
    let bin = exe.parent().map(|folder| folder.join("bin"));
    match bin {
        Some(bin) if bin.join("webide.cmd").is_file() => Ok(bin),
        _ => Err(CommandLineStatus::Unavailable {
            reason: "This copy of the app has no bin\\webide.cmd: install it with its installer."
                .to_owned(),
        }),
    }
}

pub fn status(line: &LocalCommandLine) -> CommandLineStatus {
    let bin = match bin_folder(&line.exe) {
        Ok(bin) => bin,
        Err(status) => return status,
    };
    match run("status", &bin) {
        Ok(answer) if answer == "present" => installed(&bin),
        _ => CommandLineStatus::NotInstalled,
    }
}

pub fn install(line: &LocalCommandLine) -> Result<CommandLineStatus, IdeError> {
    let bin = match bin_folder(&line.exe) {
        Ok(bin) => bin,
        Err(status) => return Ok(status),
    };
    run("install", &bin)?;
    Ok(installed(&bin))
}

pub fn uninstall(line: &LocalCommandLine) -> Result<CommandLineStatus, IdeError> {
    let bin = match bin_folder(&line.exe) {
        Ok(bin) => bin,
        Err(status) => return Ok(status),
    };
    run("uninstall", &bin)?;
    Ok(CommandLineStatus::NotInstalled)
}

fn installed(bin: &Path) -> CommandLineStatus {
    CommandLineStatus::Installed {
        path: bin.display().to_string(),
        hint: Some(
            "Open a new terminal: those open already keep the PATH they started with.".to_owned(),
        ),
    }
}

/// Runs the script for `action`; its last line of output.
fn run(action: &str, bin: &Path) -> Result<String, IdeError> {
    // By its full path, so nothing named powershell.exe elsewhere runs instead.
    let system =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let powershell = system
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let output = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded(SCRIPT),
        ])
        .env("WEBIDE_PATH_ACTION", action)
        .env("WEBIDE_BIN", bin)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(IdeError::Io(format!("couldn't change the PATH: {why}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_owned())
}

/// `script` as `-EncodedCommand` takes it: UTF-16LE in Base64, so no quote
/// or line in it is read as the command line's.
pub fn encoded(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
