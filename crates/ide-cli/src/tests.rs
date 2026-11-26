use std::ffi::OsString;

use ide_core::IdeError;
use ide_core::cli::{CommandLine, CommandLineStatus, Launch};

use crate::{launch_target, run_flag};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn a_folder_opens_as_the_workspace_and_a_file_in_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("app");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src").join("main.ts"), "").unwrap();

    // `webide .` in the project.
    assert_eq!(
        launch_target(args(&["."]), &project),
        Some(Launch {
            folder: std::path::absolute(project.join(".")).unwrap(),
            file: None
        })
    );
    // A path relative to where the terminal is, after a flag.
    let launch = launch_target(args(&["-psn_0_1", "src/main.ts"]), &project).unwrap();
    assert_eq!(launch.folder, project.join("src"));
    assert_eq!(launch.file, Some(project.join("src").join("main.ts")));
    // An absolute one.
    let launch = launch_target(args(&[&project.display().to_string()]), dir.path()).unwrap();
    assert_eq!(launch.folder, project);
}

#[test]
fn nothing_named_or_nothing_there_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(launch_target(args(&[]), dir.path()), None);
    assert_eq!(launch_target(args(&["--verbose"]), dir.path()), None);
    assert_eq!(launch_target(args(&["missing"]), dir.path()), None);
}

/// Counts what the installer's flags ask of it.
#[derive(Default)]
struct Counting {
    installs: std::sync::atomic::AtomicUsize,
    uninstalls: std::sync::atomic::AtomicUsize,
}

impl CommandLine for Counting {
    fn status(&self) -> CommandLineStatus {
        CommandLineStatus::NotInstalled
    }
    fn install(&self) -> Result<CommandLineStatus, IdeError> {
        self.installs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(CommandLineStatus::NotInstalled)
    }
    fn uninstall(&self) -> Result<CommandLineStatus, IdeError> {
        self.uninstalls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err(IdeError::PermissionDenied)
    }
}

#[test]
fn the_installers_flags_install_or_uninstall_and_nothing_else_does() {
    let line = Counting::default();
    assert_eq!(
        run_flag(args(&["--install-command-line"]), &line),
        Some(std::process::ExitCode::SUCCESS)
    );
    assert_eq!(
        run_flag(args(&["--uninstall-command-line"]), &line),
        Some(std::process::ExitCode::FAILURE)
    );
    assert_eq!(run_flag(args(&["."]), &line), None);
    assert_eq!(run_flag(args(&[]), &line), None);
    assert_eq!(line.installs.into_inner(), 1);
    assert_eq!(line.uninstalls.into_inner(), 1);
}

#[cfg(windows)]
#[test]
fn powershell_gets_the_script_in_utf16_base64() {
    // "ab" is 61 00 62 00.
    assert_eq!(crate::windows::encoded("ab"), "YQBiAA==");
    assert_eq!(crate::windows::encoded("a"), "YQA=");
    assert_eq!(crate::windows::encoded("abc"), "YQBiAGMA");
}

#[cfg(unix)]
mod links {
    use super::*;
    use crate::LocalCommandLine;

    #[test]
    fn it_links_to_the_program_and_takes_away_only_its_own_link() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app").join("webide");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        let link = dir.path().join("bin").join("webide");
        let line = LocalCommandLine::with_link(exe.clone(), link.clone());

        assert_eq!(line.status(), CommandLineStatus::NotInstalled);
        assert!(matches!(
            line.install().unwrap(),
            CommandLineStatus::Installed { .. }
        ));
        assert_eq!(std::fs::read_link(&link).unwrap(), exe);
        assert!(matches!(line.status(), CommandLineStatus::Installed { .. }));
        // Again: nothing changes.
        assert!(matches!(
            line.install().unwrap(),
            CommandLineStatus::Installed { .. }
        ));

        assert_eq!(line.uninstall().unwrap(), CommandLineStatus::NotInstalled);
        assert!(std::fs::symlink_metadata(&link).is_err());

        // A link to another program is replaced on install, and left on uninstall.
        let other = dir.path().join("other");
        std::fs::write(&other, "").unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();
        line.uninstall().unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), other);
        line.install().unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), exe);
    }

    #[test]
    fn another_program_in_its_place_isnt_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("webide-app");
        std::fs::write(&exe, "").unwrap();
        let link = dir.path().join("webide");
        std::fs::write(&link, "someone else's").unwrap();
        let line = LocalCommandLine::with_link(exe, link.clone());
        assert!(line.install().is_err());
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "someone else's");
    }
}
